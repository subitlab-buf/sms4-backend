use std::{collections::HashSet, ops::RangeInclusive};

use axum::{
    extract::{Path, Query, State},
    Json,
};
use dmds::{IoHandle, StreamExt};
use serde::{Deserialize, Serialize};
use sms4_backend::{
    account::{Permission, Tag},
    post::{Post, Priority, Status},
    Error,
};
use time::{Date, OffsetDateTime};

use crate::{Auth, Global};

#[derive(Deserialize)]
pub struct NewPostReq {
    pub title: String,
    pub notes: String,
    pub time: RangeInclusive<time::Date>,
    pub resources: Box<[u64]>,
    pub grouped: bool,
    pub priority: Priority,
}

pub async fn new_post<Io: IoHandle>(
    auth: Auth,
    State(Global { worlds, .. }): State<Global<Io>>,
    Json(NewPostReq {
        title,
        notes,
        time,
        resources,
        grouped,
        priority,
    }): Json<NewPostReq>,
) -> Result<(), Error> {
    let select = sd!(worlds.account, auth.account);
    va!(auth, select => Post);

    let mut validated = 0;
    let mut select = worlds
        .resource
        .select(0, *resources.first().ok_or(Error::PostResourceEmpty)?)
        .hints(resources.iter().copied());
    for id in resources.iter().copied() {
        select = select.plus(0, id)
    }
    let mut iter = select.iter();
    while let Some(Ok(mut lazy)) = iter.next().await {
        if resources.contains(&lazy.id()) {
            if let Ok(val) = lazy.get().await {
                if val.owner() == auth.account {
                    if let Ok(val) = lazy.get_mut().await {
                        val.block()?;
                        lazy.close().await?;
                        validated += 1;
                    }
                }
            }
        }
        if validated >= resources.len() {
            break;
        }
    }
    if validated < resources.len() {
        return Err(Error::PermissionDenied);
    }

    worlds
        .post
        .try_insert(Post::new(
            title,
            notes,
            time,
            resources,
            auth.account,
            grouped,
            priority,
        )?)
        .await
        .map_err(|_| Error::PermissionDenied)
}

#[derive(Deserialize)]
pub struct FilterPostsParams {
    /// Specify posts after this id.
    #[serde(default)]
    pub after: Option<u64>,
    /// Max posts to return.
    #[serde(default = "FilterPostsParams::DEFAULT_LIMIT")]
    pub limit: usize,

    /// Specify posts creator.
    #[serde(default)]
    pub creator: Option<u64>,
    /// Specify posts status.
    #[serde(default)]
    pub status: Option<sms4_backend::post::Status>,

    /// Specify posts available time.
    #[serde(default)]
    pub on: Option<Date>,
}

impl FilterPostsParams {
    const DEFAULT_LIMIT: fn() -> usize = || 64;
}

#[derive(Serialize)]
pub struct FilterPostsRes {
    pub posts: Vec<u64>,
}

pub async fn filter_posts<Io: IoHandle>(
    Query(FilterPostsParams {
        after,
        limit,
        creator,
        status,
        on,
    }): Query<FilterPostsParams>,
    auth: Auth,
    State(Global { worlds, .. }): State<Global<Io>>,
) -> Result<Json<FilterPostsRes>, Error> {
    let select = sd!(worlds.account, auth.account);
    let lazy_this = va!(auth, select);
    let permitted_review = lazy_this
        .get()
        .await?
        .tags()
        .contains_permission(&Tag::Permission(Permission::ReviewPost));
    let permitted_get_pub = lazy_this
        .get()
        .await?
        .tags()
        .contains_permission(&Tag::Permission(Permission::GetPubPost));

    let mut select = worlds.post.select_all();
    if let Some(after) = after {
        select = select.and(0, after..);
    }
    if let Some(creator) = creator {
        select = select.and(2, creator);
    }
    if let Some(status) = status {
        if matches!(status, sms4_backend::post::Status::Approved) {
            select = select.and(3, 1);
        } else {
            select = select.and(3, 0);
        }
    }
    if let Some(on) = on {
        let end_o = (on + Post::MAX_DUR).ordinal();
        let start_o = (on - Post::MAX_DUR).ordinal();
        if start_o > end_o {
            select = select.and(1, start_o as u64..).plus(1, ..=end_o as u64);
        } else {
            select = select.and(1, start_o as u64..=end_o as u64);
        }
    }

    let mut iter = select.iter();
    let mut posts = Vec::new();
    while let Some(Ok(lazy)) = iter.next().await {
        if after.is_some_and(|a| lazy.id() <= a) {
            continue;
        }
        if let Ok(val) = lazy.get().await {
            if creator.is_some_and(|c| val.creator() != c)
                || status.is_some_and(|s| val.state().status() != s)
                || on.is_some_and(|d| !val.time().contains(&d))
                || (val.creator() != auth.account
                    && !if matches!(val.state().status(), sms4_backend::post::Status::Approved) {
                        permitted_get_pub
                    } else {
                        permitted_review
                    })
            {
                continue;
            }
            posts.push(val.id());
            if posts.len() == limit {
                break;
            }
        }
    }
    Ok(Json(FilterPostsRes { posts }))
}

#[derive(Serialize)]
#[serde(tag = "type")]
pub enum Info {
    Simple {
        id: u64,
        title: String,
        creator: u64,
        /// List of resource ids this post used.
        resources: Box<[u64]>,
        grouped: bool,
        priority: Priority,
    },
    Full {
        id: u64,
        creator: u64,
        #[serde(flatten)]
        inner: Post,
    },
}

impl Info {
    #[inline]
    fn from_simple(post: &Post) -> Self {
        Self::Simple {
            id: post.id(),
            title: post.title().to_owned(),
            creator: post.creator(),
            resources: post.resources().to_owned().into_boxed_slice(),
            grouped: post.is_grouped(),
            priority: post.priority(),
        }
    }

    #[inline]
    fn from_full(post: &Post) -> Self {
        Self::Full {
            id: post.id(),
            creator: post.creator(),
            inner: post.clone(),
        }
    }
}

pub async fn get_info<Io: IoHandle>(
    Path(id): Path<u64>,
    auth: Auth,
    State(Global { worlds, .. }): State<Global<Io>>,
) -> Result<Json<Info>, Error> {
    let select = sd!(worlds.account, auth.account);
    let this_lazy = va!(auth, select);
    let permitted_review = this_lazy
        .get()
        .await?
        .tags()
        .contains_permission(&Tag::Permission(Permission::ReviewPost));
    let permitted_get_pub = this_lazy
        .get()
        .await?
        .tags()
        .contains_permission(&Tag::Permission(Permission::GetPubPost));
    let now = OffsetDateTime::now_utc().date();
    let select = sd!(worlds.post, id);
    let lazy = gd!(select, id).ok_or(Error::PostNotFound(id))?;
    let val = lazy.get().await?;

    if val.creator() == auth.account || permitted_review {
        return Ok(Json(Info::from_full(&val)));
    } else if permitted_get_pub
        && matches!(val.state().status(), sms4_backend::post::Status::Approved)
        && val.time().contains(&now)
    {
        return Ok(Json(Info::from_simple(val)));
    } else {
        Err(Error::PostNotFound(id))
    }
}

#[derive(Deserialize)]
pub struct BulkGetInfoReq {
    pub posts: Vec<u64>,
}

#[derive(Serialize)]
pub struct BulkGetInfoRes {
    pub posts: Vec<Info>,
}

pub async fn bulk_get_info<Io: IoHandle>(
    auth: Auth,
    State(Global { worlds, .. }): State<Global<Io>>,
    Json(BulkGetInfoReq { posts }): Json<BulkGetInfoReq>,
) -> Result<Json<BulkGetInfoRes>, Error> {
    let select = sd!(worlds.account, auth.account);
    let this_lazy = va!(auth, select);
    let permitted_review = this_lazy
        .get()
        .await?
        .tags()
        .contains_permission(&Tag::Permission(Permission::ReviewPost));
    let permitted_get_pub = this_lazy
        .get()
        .await?
        .tags()
        .contains_permission(&Tag::Permission(Permission::GetPubPost));

    let Some(first) = posts.get(0).copied() else {
        return Ok(Json(BulkGetInfoRes { posts: vec![] }));
    };
    let mut select = worlds.post.select(0, first).hints(posts.iter().copied());
    for id in posts[1..].iter().copied() {
        select = select.plus(0, id);
    }
    let mut iter = select.iter();
    let mut res = Vec::with_capacity(posts.len().max(64));
    let now = OffsetDateTime::now_utc().date();
    while let Some(Ok(lazy)) = iter.next().await {
        if posts.contains(&lazy.id()) {
            if let Ok(val) = lazy.get().await {
                if val.creator() == auth.account || permitted_review {
                    res.push(Info::from_full(val));
                } else if permitted_get_pub
                    && matches!(val.state().status(), sms4_backend::post::Status::Approved)
                    && val.time().contains(&now)
                {
                    res.push(Info::from_simple(val));
                }
            }
        }
    }
    Ok(Json(BulkGetInfoRes { posts: res }))
}

#[derive(Deserialize)]
pub struct ModifyReq {
    /// Modifies the title.
    #[serde(default)]
    pub title: Option<String>,
    /// Appends notes to the post,
    /// without modification.
    #[serde(default)]
    pub notes: Option<String>,

    #[serde(default)]
    pub time: Option<RangeInclusive<time::Date>>,
    /// Overrides the linked post resources
    /// with given ones.
    #[serde(default)]
    pub resources: Option<Box<[u64]>>,

    #[serde(default)]
    pub grouped: Option<bool>,
}

pub async fn modify<Io: IoHandle>(
    Query(id): Query<u64>,
    auth: Auth,
    State(Global { worlds, .. }): State<Global<Io>>,
    Json(mut req): Json<ModifyReq>,
) -> Result<(), Error> {
    let select = sd!(worlds.account, auth.account);
    va!(auth, select => Post);
    let select = sd!(worlds.post, id);
    let mut lazy = gd!(select, id).ok_or(Error::PostNotFound(id))?;
    let post = lazy.get_mut().await?;
    if post.creator() != auth.account {
        return Err(Error::PostNotFound(id));
    }

    macro_rules! modify {
        ($($i:ident => $m:ident),*$(,)?) => { $(if let Some(v) = req.$i.take() { post.$m(v) })* };
    }
    modify! {
        title => set_title,
        grouped => set_is_grouped,
    }
    if let Some(time) = req.time.take() {
        post.set_time(time)?
    }
    if let Some(new_res) = req
        .resources
        .take()
        .map(|s| s.into_iter().copied().collect::<HashSet<_>>())
    {
        let old_res = post
            .resources()
            .into_iter()
            .copied()
            .collect::<HashSet<_>>();
        let new_diff = new_res
            .difference(&old_res)
            .copied()
            .collect::<HashSet<_>>();
        let old_diff = old_res
            .difference(&new_res)
            .copied()
            .collect::<HashSet<_>>();
        let mut result = new_res.intersection(&old_res).copied().collect::<Vec<_>>();

        if !(new_diff.is_empty() && old_diff.is_empty()) {
            let mut select = worlds
                .resource
                .select(
                    0,
                    new_diff
                        .iter()
                        .copied()
                        .next()
                        .unwrap_or_else(|| old_diff.iter().copied().next().unwrap()),
                )
                .hints(new_diff.iter().copied())
                .hints(old_diff.iter().copied());
            for id in old_diff.iter().copied().chain(new_diff.iter().copied()) {
                select = select.plus(0, id)
            }
            let mut iter = select.iter();

            while let Some(Ok(mut lazy)) = iter.next().await {
                if old_diff.contains(&lazy.id()) {
                    lazy.destroy().await?;
                } else if new_diff.contains(&lazy.id()) {
                    let res = lazy.get_mut().await?;
                    res.block()?;
                    result.push(lazy.id());
                    lazy.close().await?;
                } else {
                    continue;
                }
            }
        }

        post.set_resources(result.into_boxed_slice());
    }
    post.pust_state(sms4_backend::post::State::new(
        sms4_backend::post::Status::Pending,
        auth.account,
        req.notes.unwrap_or_default(),
    ))?;
    lazy.close().await.map_err(From::from)
}

#[derive(Deserialize)]
pub struct ReviewReq {
    pub status: Status,

    #[serde(default)]
    pub message: Option<String>,
}

pub async fn review<Io: IoHandle>(
    Query(id): Query<u64>,
    auth: Auth,
    State(Global { worlds, .. }): State<Global<Io>>,
    Json(ReviewReq { status, message }): Json<ReviewReq>,
) -> Result<(), Error> {
    let select = sd!(worlds.account, auth.account);
    va!(auth, select => ReviewPost);
    let select = sd!(worlds.post, id);
    let mut lazy = gd!(select, id).ok_or(Error::PostNotFound(id))?;
    let post = lazy.get_mut().await?;
    post.pust_state(sms4_backend::post::State::new(
        status,
        auth.account,
        message.unwrap_or_default(),
    ))?;
    lazy.close().await.map_err(From::from)
}

pub async fn remove<Io: IoHandle>(
    Query(id): Query<u64>,
    auth: Auth,
    State(Global { worlds, .. }): State<Global<Io>>,
) -> Result<(), Error> {
    let select = sd!(worlds.account, auth.account);
    let this_lazy = va!(auth, select => Post);

    let select = sd!(worlds.post, id);
    let lazy = gd!(select, id).ok_or(Error::PostNotFound(id))?;

    let post = lazy.get().await?;
    if post.creator() != auth.account
        && !this_lazy
            .get()
            .await?
            .tags()
            .contains_permission(&Tag::Permission(Permission::RemovePost))
    {
        return Err(Error::PostNotFound(id));
    }

    if let Some(first) = post.resources().first().copied() {
        let resources = post.resources();
        let mut select = worlds
            .resource
            .select(0, first)
            .hints(resources.iter().copied());
        for id in resources.iter().copied() {
            select = select.plus(0, id)
        }
        let mut iter = select.iter();
        while let Some(Ok(lazy)) = iter.next().await {
            if resources.contains(&lazy.id()) {
                lazy.destroy().await?;
            }
        }
    }
    lazy.destroy().await.map_err(From::from)
}

#[derive(Deserialize)]
pub struct BulkRemoveReq {
    pub posts: Vec<u64>,
}

pub async fn bulk_remove<Io: IoHandle>(
    auth: Auth,
    State(Global { worlds, .. }): State<Global<Io>>,
    Json(BulkRemoveReq { posts }): Json<BulkRemoveReq>,
) -> Result<(), Error> {
    let select = sd!(worlds.account, auth.account);
    let this_lazy = va!(auth, select => Post);
    let permitted_rm = this_lazy
        .get()
        .await?
        .tags()
        .contains_permission(&Tag::Permission(Permission::RemovePost));

    let Some(first) = posts.get(0).copied() else {
        return Ok(());
    };
    let mut select = worlds.post.select(0, first).hints(posts.iter().copied());
    for id in posts[1..].iter().copied() {
        select = select.plus(0, id);
    }
    let mut iter = select.iter();

    let mut resources_rm = vec![];

    while let Some(Ok(lazy)) = iter.next().await {
        if posts.contains(&lazy.id()) {
            let post = lazy.get().await?;
            if post.creator() != auth.account && !permitted_rm {
                continue;
            }
            resources_rm.extend_from_slice(post.resources());
            lazy.destroy().await?;
        }
    }

    if let Some(first) = resources_rm.first().copied() {
        let mut select = worlds
            .resource
            .select(0, first)
            .hints(resources_rm.iter().copied());
        for id in resources_rm.iter().copied() {
            select = select.plus(0, id)
        }
        let mut iter = select.iter();
        while let Some(Ok(lazy)) = iter.next().await {
            if resources_rm.contains(&lazy.id()) {
                lazy.destroy().await?;
            }
        }
    }

    Ok(())
}

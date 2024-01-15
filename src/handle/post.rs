use std::{
    collections::{HashMap, HashSet},
    ops::RangeInclusive,
};

use axum::{
    extract::{Path, Query, State},
    Json,
};
use dmds::{IoHandle, StreamExt};
use serde::{Deserialize, Serialize};
use sms4_backend::{
    account::{Permission, Tag},
    post::{Post, Priority, Status},
    Error, Id,
};
use time::{Date, OffsetDateTime};

use crate::{Auth, Global};

/// Request body for creating a new post.
///
/// # Examples
///
/// ```json
/// {
///     "title": "Test Post",
///     "notes": "This is a test post.",
///     "time": {
///         "start": "2021-09-01",
///         "end": "2021-09-05",
///     },
///     "resources": [1, 2, 3],
///     "grouped": true,
///     "priority": "Normal",
/// }
/// ```
#[derive(Deserialize)]
pub struct NewPostReq {
    /// Title of the post.
    pub title: String,
    /// Notes of the post.
    ///
    /// The notes will be stored as the notes of
    /// initialize state of the post.
    pub notes: String,
    /// Time range of the post.
    pub time: RangeInclusive<time::Date>,
    /// List of resource ids this post used.
    pub resources: Box<[Id]>,
    /// Whether this post should be played as
    /// a full sequence.
    pub grouped: bool,
    /// Priority of the post.
    pub priority: Priority,
}

/// Response body for creating a new post.
///
/// # Examples
///
/// ```json
/// {
///     "id": 12,
/// }
/// ```
#[derive(Serialize)]
pub struct NewPostRes {
    /// Id of the new post.
    pub id: Id,
}

/// Creates a new post.
///
/// # Request
///
/// The request body is declared as [`NewPostReq`].
///
/// # Authorization
///
/// The request must be authorized with [`Permission::Post`].
///
/// # Response
///
/// The response body is declared as [`NewPostRes`].
///
/// # Errors
///
/// The request will returns an error if:
///
/// - The given resources are not owned by the
/// creator of this post, or there is no any
/// resource in the given list.
/// - The given time range is longer than **one week**.
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
) -> Result<Json<NewPostRes>, Error> {
    let select = sd!(worlds.account, auth.account);
    va!(auth, select => Post);

    let mut validated = 0;
    let mut select = worlds
        .resource
        .select(0, resources.first().ok_or(Error::PostResourceEmpty)?.0)
        .hints(resources.iter().copied().map(From::from));
    for id in resources.iter().copied() {
        select = select.plus(0, id.0)
    }
    let mut iter = select.iter();
    while let Some(Ok(mut lazy)) = iter.next().await {
        if resources.contains(&Id(lazy.id())) {
            if let Ok(val) = lazy.get().await {
                if val.owner() == Id(auth.account) {
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

    let post = Post::new(
        title,
        notes,
        time,
        resources,
        auth.account,
        grouped,
        priority,
    )?;
    let id = post.id();

    worlds
        .post
        .try_insert(post)
        .await
        .map_err(|_| Error::PermissionDenied)?;

    Ok(Json(NewPostRes { id: id.into() }))
}

/// Request URL query parameters for filtering posts.
///
/// # Examples
///
/// ```json
/// {
///     "limit": 16,
///     "creator": 345,
/// }
/// ```
#[derive(Deserialize)]
pub struct FilterPostsParams {
    /// Filter posts after this id.\
    /// The field can be omitted.
    #[serde(default)]
    pub from: Option<Id>,
    /// Max posts to return.\
    /// The field can be omitted,
    /// and the default value is **64**.
    #[serde(default = "FilterPostsParams::DEFAULT_LIMIT")]
    pub limit: usize,

    /// Filter posts creator.\
    /// The field can be omitted.
    #[serde(default)]
    pub creator: Option<Id>,
    /// Filter with post status.\
    /// The field can be omitted.
    #[serde(default)]
    pub status: Option<sms4_backend::post::Status>,

    /// Filter with post available time.\
    /// The field can be omitted.
    #[serde(default)]
    pub on: Option<Date>,

    /// Filter with screen id.\
    /// The field can be omitted.
    #[serde(default)]
    pub screen: Option<usize>,
}

impl FilterPostsParams {
    const DEFAULT_LIMIT: fn() -> usize = || 64;
}

/// Response body for filtering posts.
///
/// # Examples
///
/// ```json
/// {
///     "posts": [1, 2, 3],
/// }
/// ```
#[derive(Serialize)]
pub struct FilterPostsRes {
    /// List of post ids.
    pub posts: Box<[Id]>,
}

/// Filters posts with given filter options.
///
/// # Request
///
/// The request **query parameters** is declared as [`FilterPostsParams`].
///
/// # Authorization
///
/// The request must be authorized.
pub async fn filter<Io: IoHandle>(
    Query(FilterPostsParams {
        from,
        limit,
        creator,
        status,
        on,
        screen,
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
    if let Some(from) = from {
        select = select.and(0, from.0..);
    }
    if let Some(creator) = creator {
        select = select.and(2, creator.0);
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
        if from.is_some_and(|a| lazy.id() <= a.0)
            || screen.is_some_and(|s| lazy.id() % (s + 1) as u64 != 0)
        {
            continue;
        }
        if let Ok(val) = lazy.get().await {
            if creator.is_some_and(|c| val.creator() != c)
                || status.is_some_and(|s| val.state().status() != s)
                || on.is_some_and(|d| !val.time().contains(&d))
                || (val.creator() != Id(auth.account)
                    && !if matches!(val.state().status(), sms4_backend::post::Status::Approved) {
                        permitted_get_pub
                    } else {
                        permitted_review
                    })
            {
                continue;
            }
            posts.push(Id(val.id()));
            if posts.len() == limit {
                break;
            }
        }
    }
    Ok(Json(FilterPostsRes {
        posts: posts.into_boxed_slice(),
    }))
}

/// Represents information of a post.
#[derive(Serialize)]
#[serde(tag = "type")]
pub enum Info {
    /// Simple information of a post.
    Simple {
        /// Title of the post.
        title: String,
        /// Creator of this post.
        creator: Id,
        /// List of resource ids this post used.
        resources: Box<[Id]>,
        /// Whether this post should be played as
        /// a full sequence.
        grouped: bool,
        /// Priority of the post.
        priority: Priority,
    },
    /// Full information of a post.
    ///
    /// This variant is only available for
    /// the creator of this post, or the
    /// user with [`Permission::ReviewPost`].
    ///
    /// This variant can only be returned
    /// by [`get_info`].
    Full {
        /// The post.
        ///
        /// This field is flattened
        /// in the data structure.
        #[serde(flatten)]
        inner: Post,
    },
}

impl Info {
    #[inline]
    fn from_simple(post: &Post) -> Self {
        Self::Simple {
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
            inner: post.clone(),
        }
    }
}

pub async fn get_info<Io: IoHandle>(
    Path(Id(id)): Path<Id>,
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

    if val.creator() == Id(auth.account) || permitted_review {
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
    pub posts: Box<[Id]>,
}

#[derive(Serialize)]
pub struct BulkGetInfoRes {
    pub posts: Vec<Info>,
}

pub async fn bulk_get_info<Io: IoHandle>(
    auth: Auth,
    State(Global { worlds, .. }): State<Global<Io>>,
    Json(BulkGetInfoReq { posts }): Json<BulkGetInfoReq>,
) -> Result<Json<HashMap<u64, Info>>, Error> {
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
        return Ok(Json(HashMap::new()));
    };
    let mut select = worlds
        .post
        .select(0, first.0)
        .hints(posts.iter().copied().map(From::from));
    for id in posts[1..].iter().copied() {
        select = select.plus(0, id.0);
    }
    let mut iter = select.iter();
    let mut res = HashMap::with_capacity(posts.len().max(64));
    let now = OffsetDateTime::now_utc().date();
    while let Some(Ok(lazy)) = iter.next().await {
        if posts.contains(&Id(lazy.id())) {
            if let Ok(val) = lazy.get().await {
                if val.creator() == Id(auth.account) || permitted_review {
                    res.insert(val.id(), Info::from_full(val));
                } else if permitted_get_pub
                    && matches!(val.state().status(), sms4_backend::post::Status::Approved)
                    && val.time().contains(&now)
                {
                    res.insert(val.id(), Info::from_simple(val));
                }
            }
        }
    }
    Ok(Json(res))
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
    pub resources: Option<Box<[Id]>>,

    #[serde(default)]
    pub grouped: Option<bool>,
}

pub async fn modify<Io: IoHandle>(
    Path(id): Path<Id>,
    auth: Auth,
    State(Global { worlds, .. }): State<Global<Io>>,
    Json(mut req): Json<ModifyReq>,
) -> Result<(), Error> {
    let select = sd!(worlds.account, auth.account);
    va!(auth, select => Post);
    let select = sd!(worlds.post, id.0);
    let mut lazy = gd!(select, id.0).ok_or(Error::PostNotFound(id.0))?;
    let post = lazy.get_mut().await?;
    if post.creator() != Id(auth.account) {
        return Err(Error::PostNotFound(id.0));
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
                        .map_or_else(|| old_diff.iter().copied().next().unwrap().0, |i| i.0),
                )
                .hints(new_diff.iter().copied().map(From::from))
                .hints(old_diff.iter().copied().map(From::from));
            for id in old_diff.iter().copied().chain(new_diff.iter().copied()) {
                select = select.plus(0, id.0)
            }
            let mut iter = select.iter();

            while let Some(Ok(mut lazy)) = iter.next().await {
                if old_diff.contains(&Id(lazy.id())) {
                    lazy.destroy().await?;
                } else if new_diff.contains(&Id(lazy.id())) {
                    let res = lazy.get_mut().await?;
                    res.block()?;
                    result.push(Id(lazy.id()));
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
    Path(id): Path<Id>,
    auth: Auth,
    State(Global { worlds, .. }): State<Global<Io>>,
    Json(ReviewReq { status, message }): Json<ReviewReq>,
) -> Result<(), Error> {
    let select = sd!(worlds.account, auth.account);
    va!(auth, select => ReviewPost);
    if !matches!(status, Status::Approved | Status::Rejected) {
        return Err(Error::InvalidPostStatus);
    }
    let select = sd!(worlds.post, id.0);
    let mut lazy = gd!(select, id.0).ok_or(Error::PostNotFound(id.0))?;
    let post = lazy.get_mut().await?;
    post.pust_state(sms4_backend::post::State::new(
        status,
        auth.account,
        message.unwrap_or_default(),
    ))?;
    lazy.close().await.map_err(From::from)
}

pub async fn remove<Io: IoHandle>(
    Path(id): Path<Id>,
    auth: Auth,
    State(Global { worlds, .. }): State<Global<Io>>,
) -> Result<(), Error> {
    let select = sd!(worlds.account, auth.account);
    let this_lazy = va!(auth, select => Post);

    let select = sd!(worlds.post, id.0);
    let lazy = gd!(select, id.0).ok_or(Error::PostNotFound(id.0))?;

    let post = lazy.get().await?;
    if post.creator() != Id(auth.account)
        && !this_lazy
            .get()
            .await?
            .tags()
            .contains_permission(&Tag::Permission(Permission::RemovePost))
    {
        return Err(Error::PostNotFound(id.0));
    }

    if let Some(first) = post.resources().first().copied() {
        let resources = post.resources();
        let mut select = worlds
            .resource
            .select(0, first.0)
            .and(1, 1)
            .hints(resources.iter().copied().map(From::from));
        for id in resources.iter().copied() {
            select = select.plus(0, id.0)
        }
        let mut iter = select.iter();
        while let Some(Ok(lazy)) = iter.next().await {
            if resources.contains(&Id(lazy.id())) {
                lazy.destroy().await?;
            }
        }
    }
    lazy.destroy().await.map_err(From::from)
}

#[derive(Deserialize)]
#[serde(tag = "type")]
pub enum BulkRemoveReq {
    Posts { posts: Box<[Id]> },
    Unused,
}

pub async fn bulk_remove<Io: IoHandle>(
    auth: Auth,
    State(Global { worlds, .. }): State<Global<Io>>,
    Json(req): Json<BulkRemoveReq>,
) -> Result<(), Error> {
    let select = sd!(worlds.account, auth.account);
    let this_lazy = va!(auth, select => Post);
    let permitted_rm = this_lazy
        .get()
        .await?
        .tags()
        .contains_permission(&Tag::Permission(Permission::RemovePost));
    let mut resources_rm = vec![];

    match req {
        BulkRemoveReq::Posts { posts } => {
            let Some(first) = posts.get(0).copied() else {
                return Ok(());
            };
            let mut select = worlds
                .post
                .select(0, first.0)
                .hints(posts.iter().copied().map(From::from));
            for id in posts[1..].iter().copied() {
                select = select.plus(0, id.0);
            }
            let mut iter = select.iter();

            while let Some(Ok(lazy)) = iter.next().await {
                if posts.contains(&Id(lazy.id())) {
                    let post = lazy.get().await?;
                    if post.creator() != Id(auth.account) && !permitted_rm {
                        continue;
                    }
                    resources_rm.extend_from_slice(post.resources());
                    lazy.destroy().await?;
                }
            }
        }
        BulkRemoveReq::Unused => {
            if !this_lazy
                .get()
                .await?
                .tags()
                .contains_permission(&Tag::Permission(Permission::Maintain))
            {
                return Err(Error::PermissionDenied);
            }

            let now = OffsetDateTime::now_utc();
            let select = worlds.post.select_all();
            let mut iter = select.iter();
            while let Some(Ok(lazy)) = iter.next().await {
                if let Ok(post) = lazy.get().await {
                    if post.time().end() < &now.date() {
                        resources_rm.extend_from_slice(post.resources());
                        lazy.destroy().await?;
                    }
                }
            }
        }
    }

    if let Some(first) = resources_rm.first().copied() {
        let mut select = worlds
            .resource
            .select(0, first.0)
            .and(1, 1)
            .hints(resources_rm.iter().copied().map(From::from));
        for id in resources_rm.iter().copied() {
            select = select.plus(0, id.0)
        }
        let mut iter = select.iter();
        while let Some(Ok(lazy)) = iter.next().await {
            if resources_rm.contains(&Id(lazy.id())) {
                lazy.destroy().await?;
            }
        }
    }

    Ok(())
}

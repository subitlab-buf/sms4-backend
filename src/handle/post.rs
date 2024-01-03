use std::ops::RangeInclusive;

use axum::{
    extract::{Path, Query, State},
    Json,
};
use dmds::{IoHandle, StreamExt};
use serde::{Deserialize, Serialize};
use sms4_backend::{
    account::{Permission, Tag},
    post::Post,
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
}

pub async fn new_post<Io: IoHandle>(
    auth: Auth,
    State(Global { worlds, .. }): State<Global<Io>>,
    Json(NewPostReq {
        title,
        notes,
        time,
        resources,
    }): Json<NewPostReq>,
) -> Result<(), Error> {
    let select = sa!(worlds.account, auth.account);
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
                        val.block();
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
        .try_insert(Post::new(title, notes, time, resources, auth.account))
        .await
        .map_err(|_| Error::PermissionDenied)
}

const fn default_limit() -> usize {
    64
}

#[derive(Deserialize)]
pub struct FilterPostsParams {
    /// Specify posts after this id.
    #[serde(default)]
    pub after: Option<u64>,
    /// Max posts to return.
    #[serde(default = "default_limit")]
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
    let select = sa!(worlds.account, auth.account);
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
    let select = sa!(worlds.account, auth.account);
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
    let select = worlds.post.select(0, id).hint(id);
    let mut iter = select.iter();
    while let Some(Ok(lazy)) = iter.next().await {
        if lazy.id() == id {
            if let Ok(val) = lazy.get().await {
                if val.creator() == auth.account || permitted_review {
                    return Ok(Json(Info::from_full(&val)));
                } else if permitted_get_pub
                    && matches!(val.state().status(), sms4_backend::post::Status::Approved)
                    && val.time().contains(&now)
                {
                    return Ok(Json(Info::from_simple(val)));
                }
            }
        }
    }
    Err(Error::PostNotFound(id))
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
    let select = sa!(worlds.account, auth.account);
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

use std::collections::HashMap;

use axum::{
    extract::{Path, Query, State},
    Json,
};
use dmds::{IoHandle, StreamExt};
use serde::{Deserialize, Serialize};
use sms4_backend::{
    account::{Permission, Tag},
    notification::Notification,
    Id,
};
use time::{Date, Duration, OffsetDateTime};

use crate::{Auth, Error, Global};

/// Request body for creating a new notification.
///
/// # Examples
///
/// ```json
/// {
///     "title": "教务通知",
///     "body": "教务通知",
///     "time": 1620000000,
/// }
/// ```
#[derive(Deserialize)]
pub struct NotifyReq {
    /// Title of the notification.
    ///
    /// # Examples
    ///
    /// ```txt
    /// 教务通知
    /// ```
    ///
    /// ```txt
    /// 教务公告
    /// ```
    pub title: String,
    /// Body of the notification.
    pub body: String,

    /// Start time of the notification.
    #[serde(with = "time::serde::timestamp")]
    pub time: OffsetDateTime,
}

/// Response body for creating a new notification.
///
/// # Examples
///
/// ```json
/// {
///     "id": 19,
/// }
/// ```
#[derive(Serialize)]
pub struct NotifyRes {
    /// Id of the notification.
    pub id: Id,
}

/// Creates a new notification.
///
/// # Request
///
/// The request body is declared as [`NotifyReq`].
///
/// # Authorization
///
/// The request must be authorized with [`Permission::ManageNotifications`].
///
/// # Response
///
/// The response body is declared as [`NotifyRes`].
pub async fn notify<Io: IoHandle>(
    auth: Auth,
    State(Global { worlds, .. }): State<Global<Io>>,
    Json(NotifyReq { title, body, time }): Json<NotifyReq>,
) -> Result<Json<NotifyRes>, Error> {
    let select = sd!(worlds.account, auth.account);
    va!(auth, select => ManageNotifications);

    let notification = Notification::new(title, body, time, auth.account);
    let id = Id(notification.id());
    worlds.notification.insert(notification).await?;
    Ok(Json(NotifyRes { id }))
}

/// Request URL query parameters  for filtering notifications.
#[derive(Deserialize)]
pub struct FilterNotificationParams {
    /// Filter notifications from this date.\
    /// The field can be omitted.
    #[serde(default)]
    pub after: Option<Date>,
    /// Filter notifications until this date.\
    /// The field can be omitted.
    #[serde(default)]
    pub before: Option<Date>,

    /// Filter notifications after this id.\
    /// The field can be omitted.
    #[serde(default)]
    pub from: Option<Id>,
    /// Max notifications to return.\
    /// The field can be omitted,
    /// and the default value is **16**.
    #[serde(default = "FilterNotificationParams::DEFAULT_LIMIT")]
    pub limit: usize,

    /// Filter notifications from this account.\
    /// The field can be omitted.
    ///
    /// This only works with the permission [`Permission::ManageNotifications`].
    #[serde(default)]
    pub sender: Option<Id>,
}

impl FilterNotificationParams {
    const DEFAULT_LIMIT: fn() -> usize = || 16;
}

/// Response body for filtering notifications.
#[derive(Serialize)]
pub struct FilterNotificationRes {
    /// Notifications ids.
    pub notifications: Box<[Id]>,
}

/// Filters notifications.
pub async fn filter<Io: IoHandle>(
    Query(FilterNotificationParams {
        after,
        before,
        from,
        limit,
        sender,
    }): Query<FilterNotificationParams>,
    auth: Auth,
    State(Global { worlds, .. }): State<Global<Io>>,
) -> Result<Json<FilterNotificationRes>, Error> {
    let select = sd!(worlds.account, auth.account);
    let lazy_this = va!(auth, select => GetPubNotifications);
    let permitted_manage = lazy_this
        .get()
        .await?
        .tags()
        .contains_permission(&Tag::Permission(Permission::ManageNotifications));

    let mut select = worlds.notification.select_all();
    if let Some(from) = from {
        select = select.and(0, from.0..);
    }
    if let (Some(before), Some(after)) = (before, after) {
        if after + Duration::days(365) > before {
            if after.year() == before.year() {
                select = select.and(1, after.ordinal() as u64..=before.ordinal() as u64);
            } else {
                select = select
                    .and(1, ..=before.ordinal() as u64)
                    .plus(1, after.ordinal() as u64..);
            }
        }
    }

    let mut iter = select.iter();
    let mut notifications = Vec::new();
    let now = OffsetDateTime::now_utc();
    while let Some(Ok(lazy)) = iter.next().await {
        if from.is_some_and(|a| lazy.id() <= a.0) {
            continue;
        }
        if let Ok(val) = lazy.get().await {
            if sender.is_some_and(|c| Id(val.sender()) != c && permitted_manage)
                || after.is_some_and(|d| val.time().date() >= d)
                || before.is_some_and(|d| val.time().date() <= d)
                || (!permitted_manage && val.time() > now)
            {
                continue;
            }
            notifications.push(Id(val.id()));
            if notifications.len() == limit {
                break;
            }
        }
    }
    Ok(Json(FilterNotificationRes {
        notifications: notifications.into_boxed_slice(),
    }))
}

#[derive(Serialize)]
pub enum Info {
    Simple {
        title: String,
        body: String,
    },
    Full {
        #[serde(flatten)]
        inner: Notification,
    },
}

impl Info {
    fn from_simple(notification: &Notification) -> Self {
        Self::Simple {
            title: notification.title.to_owned(),
            body: notification.body.to_owned(),
        }
    }

    #[inline]
    fn from_full(notification: &Notification) -> Self {
        Self::Full {
            inner: notification.clone(),
        }
    }
}

/// Gets a notification.
pub async fn get_info<Io: IoHandle>(
    Path(Id(id)): Path<Id>,
    auth: Auth,
    State(Global { worlds, .. }): State<Global<Io>>,
) -> Result<Json<Info>, Error> {
    let select = sd!(worlds.account, auth.account);
    let lazy_this = va!(auth, select => GetPubNotifications);
    let permitted_manage = lazy_this
        .get()
        .await?
        .tags()
        .contains_permission(&Tag::Permission(Permission::ManageNotifications));
    let select = sd!(worlds.notification, id);
    let lazy = gd!(select, id).ok_or(Error::NotificationNotFound(id))?;
    let notification = lazy.get().await?;
    if notification.time() > OffsetDateTime::now_utc() && !permitted_manage {
        return Err(Error::NotificationNotFound(id));
    }
    Ok(Json(if permitted_manage {
        Info::from_full(notification)
    } else {
        Info::from_simple(notification)
    }))
}

#[derive(Deserialize)]
pub struct BulkGetInfoReq {
    pub notifications: Box<[Id]>,
}

pub async fn bulk_get_info<Io: IoHandle>(
    auth: Auth,
    State(Global { worlds, .. }): State<Global<Io>>,
    Json(BulkGetInfoReq { notifications }): Json<BulkGetInfoReq>,
) -> Result<Json<HashMap<u64, Info>>, Error> {
    let select = sd!(worlds.account, auth.account);
    let this_lazy = va!(auth, select => GetPubPost);
    let permitted_manage = this_lazy
        .get()
        .await?
        .tags()
        .contains_permission(&Tag::Permission(Permission::ManageNotifications));

    let Some(first) = notifications.first().copied() else {
        return Ok(Json(HashMap::new()));
    };
    let mut select = worlds
        .notification
        .select(0, first.0)
        .hints(notifications.iter().copied().map(From::from));
    for id in notifications[1..].iter().copied() {
        select = select.plus(0, id.0);
    }
    let mut iter = select.iter();
    let mut res = HashMap::with_capacity(notifications.len().max(64));
    let now = OffsetDateTime::now_utc();
    while let Some(Ok(lazy)) = iter.next().await {
        if notifications.contains(&Id(lazy.id())) {
            if let Ok(val) = lazy.get().await {
                if val.time() <= now && !permitted_manage {
                    continue;
                }
                if permitted_manage {
                    res.insert(val.id(), Info::from_full(val));
                } else {
                    res.insert(val.id(), Info::from_simple(val));
                }
            }
        }
    }
    Ok(Json(res))
}

/// Removes a notification.
///
/// # Authorization
///
/// The request must be authorized with [`Permission::ManageNotifications`].
pub async fn remove<Io: IoHandle>(
    Path(id): Path<Id>,
    auth: Auth,
    State(Global { worlds, .. }): State<Global<Io>>,
) -> Result<(), Error> {
    let select = sd!(worlds.account, auth.account);
    va!(auth, select => ManageNotifications);

    let select = sd!(worlds.notification, id.0);
    gd!(select, id.0)
        .ok_or(Error::NotificationNotFound(id.0))?
        .destroy()
        .await?;

    Ok(())
}

/// Request body for bulk removing notifications.
#[derive(Deserialize)]
pub struct BulkRemoveReq {
    pub notifications: Box<[Id]>,
}

/// Bulk removes notifications.
///
/// # Authorization
///
/// The request must be authorized with [`Permission::ManageNotifications`].
///
/// # Request
///
/// The request body is declared as [`BulkRemoveReq`].
pub async fn bulk_remove<Io: IoHandle>(
    auth: Auth,
    State(Global { worlds, .. }): State<Global<Io>>,
    Json(BulkRemoveReq { notifications }): Json<BulkRemoveReq>,
) -> Result<(), Error> {
    let select = sd!(worlds.account, auth.account);
    va!(auth, select => ManageNotifications);

    if let Some(first) = notifications.first().copied() {
        let mut select = worlds
            .notification
            .select(0, first.0)
            .hints(notifications.iter().copied().map(From::from));
        for id in notifications[1..].iter().copied() {
            select = select.plus(0, id.0);
        }

        let mut iter = select.iter();
        while let Some(Ok(lazy)) = iter.next().await {
            if notifications.contains(&Id(lazy.id())) {
                lazy.destroy().await?;
            }
        }
    }

    Ok(())
}

/// Request body for modifying a notification.
#[derive(Deserialize)]
pub struct ModifyReq {
    pub title: Option<String>,
    pub body: Option<String>,

    /// Modifies the start time of the notification.
    pub time: Option<OffsetDateTime>,
}

/// Modifies a notification.
///
/// # Authorization
///
/// The request must be authorized with [`Permission::ManageNotifications`].
///
/// # Request
///
/// The request body is declared as [`ModifyReq`].
pub async fn modify<Io: IoHandle>(
    Path(id): Path<Id>,
    auth: Auth,
    State(Global { worlds, .. }): State<Global<Io>>,
    Json(ModifyReq { title, body, time }): Json<ModifyReq>,
) -> Result<(), Error> {
    let select = sd!(worlds.account, auth.account);
    va!(auth, select => ManageNotifications);
    let select = sd!(worlds.notification, id.0);
    let mut lazy = gd!(select, id.0).ok_or(Error::NotificationNotFound(id.0))?;
    let val = lazy.get_mut().await?;

    if let Some(title) = title {
        val.title = title;
    }
    if let Some(body) = body {
        val.body = body;
    }

    if let Some(time) = time {
        val.set_time(time);
    }

    Ok(())
}

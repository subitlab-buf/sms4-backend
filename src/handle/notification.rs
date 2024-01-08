use axum::{
    extract::{Path, Query, State},
    Json,
};
use dmds::{IoHandle, StreamExt};
use serde::{Deserialize, Serialize};
use sms4_backend::{
    account::{Permission, Tag},
    notification::Notification,
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
pub struct NotifyRes {
    /// Id of the notification.
    pub id: u64,
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
) -> Result<NotifyRes, Error> {
    let select = sd!(worlds.account, auth.account);
    va!(auth, select => ManageNotifications);

    let notification = Notification::new(title, body, time, auth.account);
    let id = notification.id();
    worlds.notification.insert(notification).await?;
    Ok(NotifyRes { id })
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
    pub from: Option<u64>,
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
    pub sender: Option<u64>,
}

impl FilterNotificationParams {
    const DEFAULT_LIMIT: fn() -> usize = || 16;
}

/// Response body for filtering notifications.
#[derive(Serialize)]
pub struct FilterNotificationRes {
    /// Notifications ids.
    pub notifications: Vec<u64>,
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
        select = select.and(0, from..);
    }
    if let Some((before, after)) = after.zip(before) {
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
        if from.is_some_and(|a| lazy.id() <= a) {
            continue;
        }
        if let Ok(val) = lazy.get().await {
            if sender.is_some_and(|c| val.sender() != c && permitted_manage)
                || after.is_some_and(|d| val.time().date() >= d)
                || before.is_some_and(|d| val.time().date() <= d)
                || (!permitted_manage && val.time() > now)
            {
                continue;
            }
            notifications.push(val.id());
            if notifications.len() == limit {
                break;
            }
        }
    }
    Ok(Json(FilterNotificationRes { notifications }))
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
    Path(id): Path<u64>,
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

pub struct BulkGetInfoReq {
    pub ids: Box<[u64]>,
}

pub async fn bulk_get_info<Io: IoHandle>() {
    unimplemented!()
}

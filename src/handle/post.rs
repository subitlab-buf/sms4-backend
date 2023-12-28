use std::ops::RangeInclusive;

use axum::{extract::State, Json};
use dmds::IoHandle;
use serde::Deserialize;
use sms4_backend::{post::Post, Error};

use crate::{Auth, Global};

#[derive(Deserialize)]
pub struct NewPostReq {
    pub title: String,
    pub notes: String,
    pub time: RangeInclusive<time::Date>,
    pub resources: Box<[u64]>,
}

async fn new_post<Io: IoHandle>(
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

    worlds
        .post
        .try_insert(Post::new(title, notes, time, resources, auth.account))
        .await
        .map_err(|_| Error::PermissionDenied)
}

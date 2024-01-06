use std::collections::HashMap;

use axum::{
    body::Body,
    extract::{Path, State},
    Json,
};
use dmds::{IoHandle, StreamExt};
use serde::{Deserialize, Serialize};

#[allow(unused_imports)]
use sms4_backend::account::Permission;

use sms4_backend::resource::{Resource, Variant};
use tokio::{fs::File, io::BufReader};

use crate::{Auth, Error, Global};

/// Request body for [`new_session`].
///
/// # Examples
///
/// ```json
/// {
///     "variant": {
///         "type": "Video",
///         "duration": 60,
///     }
/// }
/// ```
#[derive(Deserialize)]
pub struct NewSessionReq {
    /// Variant of the resource to upload.
    pub variant: Variant,
}

/// Response body for [`new_session`].
///
/// # Examples
///
/// ```json
/// {
///     "id": 1234567890,
/// }
/// ```
#[derive(Serialize)]
pub struct NewSessionRes {
    /// Id of the upload session.\
    /// This is **not** id of the resource.
    pub id: u64,
}

/// Creates a new upload session.
///
/// # Request
///
/// The request body is declared as [`NewSessionReq`].
///
/// # Authorization
///
/// The request must be authorized with [`Permission::UploadResource`].
///
/// # Response
///
/// The response body is declared as [`NewSessionRes`].
pub async fn new_session<Io: IoHandle>(
    auth: Auth,
    State(Global {
        worlds,
        resource_sessions,
        ..
    }): State<Global<Io>>,
    Json(NewSessionReq { variant }): Json<NewSessionReq>,
) -> Result<Json<NewSessionRes>, Error> {
    let select = sd!(worlds.account, auth.account);
    va!(auth, select => UploadResource);

    let resource = Resource::new(variant, auth.account);
    let id = resource.id();
    resource_sessions.lock().await.insert(resource);
    Ok(Json(NewSessionRes { id }))
}

/// Response body for [`upload`].
pub struct UploadRes {
    /// Id of the resource.
    pub id: u64,
}

/// Uploads a resource within the given session.
///
/// # Request
///
/// The request body is the raw bytes of the resource.
///
/// # Authorization
///
/// The request must be authorized with [`Permission::UploadResource`].
///
/// # Response
///
/// The response body is declared as [`UploadRes`].
pub async fn upload<Io: IoHandle>(
    Path(id): Path<u64>,
    auth: Auth,
    State(Global {
        worlds,
        resource_sessions,
        config,
        ..
    }): State<Global<Io>>,
    payload: axum::body::Bytes,
) -> Result<Json<UploadRes>, Error> {
    let select = sd!(worlds.account, auth.account);
    va!(auth, select => UploadResource);
    let resource = resource_sessions
        .lock()
        .await
        .accept(id, &payload, auth.account)?;
    let id = resource.id();
    let path = config.resource_path.join(resource.file_name());
    if let Err(err) = tokio::fs::write(path, payload).await {
        tracing::error!("failed to write resource file {resource:?}: {err}");
        return Err(Error::PermissionDenied);
    }

    worlds
        .resource
        .try_insert(resource)
        .await
        .map_err(|_| Error::PermissionDenied)?;
    Ok(Json(UploadRes { id }))
}

/// Gets payload of a resource.
///
/// # Authorization
///
/// The request must be authorized with [`Permission::GetPubPost`].
///
/// # Response
///
/// The response body is the raw bytes of the resource.
///
/// # Errors
///
/// - [`Error::ResourceNotFound`] if the resource with the given id does not exist.
/// - [`Error::PermissionDenied`] if the resource is not blocked **and** is not owned by the authorized account.
pub async fn get_payload<Io: IoHandle>(
    Path(id): Path<u64>,
    auth: Auth,
    State(Global { worlds, config, .. }): State<Global<Io>>,
) -> Result<Body, Error> {
    let select = sd!(worlds.account, auth.account);
    va!(auth, select => GetPubPost);
    let select = sd!(worlds.resource, id);
    let lazy = gd!(select, id).ok_or(Error::ResourceNotFound(id))?;
    let resource = lazy.get().await?;
    if resource.owner() != auth.account && !resource.is_blocked() {
        return Err(Error::PermissionDenied);
    }

    // Read the file and response it asynchronously.
    let path = config.resource_path.join(resource.file_name());
    Ok(Body::from_stream(tokio_util::io::ReaderStream::new(
        BufReader::new(File::open(path).await.map_err(|_| Error::Unknown)?),
    )))
}

/// Information of a resource.
#[derive(Serialize)]
pub struct Info {
    /// The resource variant.
    pub variant: Variant,
}

pub async fn get_info<Io: IoHandle>(
    Path(id): Path<u64>,
    auth: Auth,
    State(Global { worlds, .. }): State<Global<Io>>,
) -> Result<Json<Info>, Error> {
    let select = sd!(worlds.account, auth.account);
    va!(auth, select => GetPubPost);
    let select = sd!(worlds.resource, id).and(1, 1);
    let lazy = gd!(select, id).ok_or(Error::ResourceNotFound(id))?;
    let resource = lazy.get().await?;
    if resource.owner() != auth.account && !resource.is_blocked() {
        return Err(Error::PermissionDenied);
    }
    Ok(Json(Info {
        variant: resource.variant().clone(),
    }))
}

/// Request body for [`bulk_get_info`].
#[derive(Deserialize)]
pub struct BulkGetInfoReq {
    /// Ids of the resources.
    pub ids: Box<[u64]>,
}

pub async fn bulk_get_info<Io: IoHandle>(
    auth: Auth,
    State(Global { worlds, .. }): State<Global<Io>>,
    Json(BulkGetInfoReq { ids }): Json<BulkGetInfoReq>,
) -> Result<Json<HashMap<u64, Info>>, Error> {
    let select = sd!(worlds.account, auth.account);
    va!(auth, select => GetPubPost);

    let mut infos = HashMap::with_capacity(ids.len());
    let mut select = worlds.resource.select(1, 1).hints(ids.iter().copied());
    for &id in &*ids {
        select = select.and(0, id);
    }
    let mut iter = select.iter();
    while let Some(Ok(lazy)) = iter.next().await {
        if ids.contains(&lazy.id()) {
            if let Ok(resource) = lazy.get().await {
                if resource.owner() != auth.account && !resource.is_blocked() {
                    return Err(Error::PermissionDenied);
                }
                infos.insert(
                    resource.id(),
                    Info {
                        variant: resource.variant().clone(),
                    },
                );
            }
        }
    }
    Ok(Json(infos))
}

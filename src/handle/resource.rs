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

use sms4_backend::{
    resource::{Resource, Variant},
    Id,
};
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
    pub id: Id,
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

    let resource = Resource::new(variant, Id(auth.account));
    let id = resource.id();
    resource_sessions.lock().await.insert(resource);
    Ok(Json(NewSessionRes { id: Id(id) }))
}

/// Response body for [`upload`].
#[derive(Serialize)]
pub struct UploadRes {
    /// Id of the resource.
    pub id: Id,
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
    Path(Id(id)): Path<Id>,
    auth: Auth,
    State(Global {
        worlds,
        resource_sessions,
        config,
        ..
    }): State<Global<Io>>,
    payload: axum::body::Body,
) -> Result<Json<UploadRes>, Error> {
    let select = sd!(worlds.account, auth.account);
    va!(auth, select => UploadResource);

    let mut hasher = highway::PortableHash::default();
    let buf_path = config.resource_path.join(
        resource_sessions
            .lock()
            .await
            .buf_name(id)
            .ok_or(Error::ResourceUploadSessionNotFound(id))?,
    );
    let mut file = tokio::fs::File::create(&buf_path)
        .await
        .map_err(|_| Error::ResourceSaveFailed)?;
    let mut stream = http_body_util::BodyStream::new(payload);

    const MAX_PAYLOAD_LEN: usize = 50 * 1024 * 1024;

    let mut len = 0_usize;
    while let Some(chunk) = stream
        .try_next()
        .await
        .map_err(|_| Error::ResourceSaveFailed)?
    {
        let chunk = chunk.into_data().map_err(|_| Error::ResourceSaveFailed)?;
        len += chunk.len();
        highway::HighwayHash::append(&mut hasher, &chunk);
        tokio::io::AsyncWriteExt::write_all(&mut file, &chunk)
            .await
            .map_err(|_| Error::ResourceSaveFailed)?;
    }
    if len > MAX_PAYLOAD_LEN {
        drop(file);
        let _ = tokio::fs::remove_file(buf_path).await;
        return Err(Error::PayloadTooLarge {
            max: MAX_PAYLOAD_LEN,
        });
    }

    tokio::io::AsyncWriteExt::flush(&mut file)
        .await
        .map_err(|_| Error::ResourceSaveFailed)?;
    file.sync_data()
        .await
        .map_err(|_| Error::ResourceSaveFailed)?;

    let resource = resource_sessions
        .lock()
        .await
        .accept(Id(id), hasher, Id(auth.account))?;
    let id = Id(resource.id());
    let path = config.resource_path.join(resource.file_name());
    tokio::fs::rename(buf_path, path)
        .await
        .map_err(|_| Error::ResourceSaveFailed)?;

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
    Path(Id(id)): Path<Id>,
    auth: Auth,
    State(Global { worlds, config, .. }): State<Global<Io>>,
) -> Result<Body, Error> {
    let select = sd!(worlds.account, auth.account);
    va!(auth, select => GetPubPost);
    let select = sd!(worlds.resource, id);
    let lazy = gd!(select, id).ok_or(Error::ResourceNotFound(id))?;
    let resource = lazy.get().await?;
    if resource.owner() != Id(auth.account) && !resource.is_blocked() {
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
    Path(Id(id)): Path<Id>,
    auth: Auth,
    State(Global { worlds, .. }): State<Global<Io>>,
) -> Result<Json<Info>, Error> {
    let select = sd!(worlds.account, auth.account);
    va!(auth, select => GetPubPost);
    let select = sd!(worlds.resource, id).and(1, 1);
    let lazy = gd!(select, id).ok_or(Error::ResourceNotFound(id))?;
    let resource = lazy.get().await?;
    if resource.owner() != Id(auth.account) && !resource.is_blocked() {
        return Err(Error::PermissionDenied);
    }
    Ok(Json(Info {
        variant: resource.variant(),
    }))
}

/// Request body for [`bulk_get_info`].
#[derive(Deserialize)]
pub struct BulkGetInfoReq {
    /// Ids of the resources.
    pub ids: Box<[Id]>,
}

pub async fn bulk_get_info<Io: IoHandle>(
    auth: Auth,
    State(Global { worlds, .. }): State<Global<Io>>,
    Json(BulkGetInfoReq { ids }): Json<BulkGetInfoReq>,
) -> Result<Json<HashMap<u64, Info>>, Error> {
    let select = sd!(worlds.account, auth.account);
    va!(auth, select => GetPubPost);

    let mut infos = HashMap::with_capacity(ids.len());
    let mut select = worlds
        .resource
        .select(1, 1)
        .hints(ids.iter().copied().map(From::from));
    for &id in &*ids {
        select = select.and(0, id.0);
    }
    let mut iter = select.iter();
    while let Some(Ok(lazy)) = iter.next().await {
        if ids.contains(&Id(lazy.id())) {
            if let Ok(resource) = lazy.get().await {
                if resource.owner() != Id(auth.account) && !resource.is_blocked() {
                    return Err(Error::PermissionDenied);
                }
                infos.insert(
                    resource.id(),
                    Info {
                        variant: resource.variant(),
                    },
                );
            }
        }
    }
    Ok(Json(infos))
}

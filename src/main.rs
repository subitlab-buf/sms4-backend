use std::{sync::Arc, time::Duration};

use axum::Router;
use dmds::{world, IoHandle, World};
use dmds_tokio_fs::FsHandle;
use lettre::AsyncSmtpTransport;
use sms4_backend::{account::Account, config::Config, resource, Error};
use tokio::{net::TcpListener, sync::Mutex};

macro_rules! ipc {
    ($c:literal) => {
        ((u64::MAX as u128 + 1) / $c)
    };
}

#[tokio::main]
async fn main() {
    let config: Config = {
        const CFG_PATH: &'static str = "config.json";
        let mut config_file = std::fs::File::open(CFG_PATH).expect("failed to open config file");
        serde_json::from_reader(&mut config_file).expect("failed to parse config file")
    };
    macro_rules! dpath {
        ($l:expr) => {{
            let mut p = config.db_path.clone();
            p.push($l);
            p
        }};
    }
    let state = Global {
        smtp_transport: Arc::new(config.smtp.to_transport().unwrap()),
        worlds: Arc::new(crate::Worlds {
            account: Arc::new(world!(FsHandle::new(dpath!("accounts"),false),ipc!(16)=> ..)),
            unverified_account: Arc::new(
                world!(FsHandle::new(dpath!("unverified_accounts"),false),ipc!(4)=> ..),
            ),
            post: Arc::new(
                world!(FsHandle::new(dpath!("posts"),false),ipc!(16)=> ..,368/4=> ..=367,ipc!(16)=> ..,1=> ..2),
            ),
            resource: Arc::new(
                world!(FsHandle::new(dpath!("resources"),false),ipc!(256)=> ..,1=> ..2),
            ),
            notification: Arc::new(
                world! {FsHandle::new(dpath!("notifications"),false),ipc!(32)=> ..,368/4=> ..=367},
            ),
        }),
        config: Arc::new(config),
        test_cx: Default::default(),
        resource_sessions: Arc::new(Mutex::new(sms4_backend::resource::UploadSessions::new())),
    };

    macro_rules! daemon {
        ($($i:ident => $s:expr),*$(,)?) => {
            $(tokio::spawn(dmds_tokio_fs::daemon(state.worlds.$i.clone(), Duration::from_secs($s)));)*
        };
    }

    daemon! {
        account => 300,
        unverified_account => 900,
        post => 120,
        resource => 60,
        notification => 120,
    }

    let app: Router<()> = routing(axum::Router::new()).with_state(state);
    let addr = std::net::SocketAddr::from(([127, 0, 0, 1], 8080));
    axum::serve(TcpListener::bind(addr).await.unwrap(), app)
        .await
        .unwrap();
}

pub mod routes {
    pub const SEND_CAPTCHA: &str = "/account/send-captcha";
    pub const REGISTER: &str = "/account/register";
    pub const LOGIN: &str = "/account/login";
    pub const SEND_RESET_PASSWORD_CAPTCHA: &str = "/account/send-reset-password-captcha";
    pub const RESET_PASSWORD: &str = "/account/reset-password";
    pub const MODIFY_ACCOUNT: &str = "/account/modify";
    pub const LOGOUT: &str = "/account/logout";
    pub const SET_PERMISSIONS: &str = "/account/set-permissions";
    pub const GET_ACCOUNT_INFO: &str = "/account/get/:id";
    pub const BULK_GET_ACCOUNT_INFO: &str = "/account/bulk-get";

    pub const NEW_POST: &str = "/post/new";
    pub const FILTER_POSTS: &str = "/post/filter";
    pub const GET_POST: &str = "/post/get/:id";
    pub const GET_POSTS: &str = "/post/bulk-get";
    pub const MODIFY_POST: &str = "/post/modify/:id";
    pub const REVIEW_POST: &str = "/post/review/:id";
    pub const DELETE_POST: &str = "/post/delete/:id";
    pub const BULK_DELETE_POST: &str = "/post/bulk-delete";

    pub const NEW_UPLOAD_SESSION: &str = "/resource/new-session";
    pub const UPLOAD_RESOURCE: &str = "/resource/upload/:id";
    pub const GET_RESOURCE_PAYLOAD: &str = "/resource/payload/:id";
    pub const GET_RESOURCE_INFO: &str = "/resource/get/:id";
    pub const BULK_GET_RESOURCE_INFO: &str = "/resource/bulk-get";

    pub const NOTIFY: &str = "/notification/new";
    pub const FILTER_NOTIFICATIONS: &str = "/notification/filter";
    pub const GET_NOTIFICATION: &str = "/notification/get/:id";
    pub const BULK_GET_NOTIFICATION: &str = "/notification/bulk-get";
    pub const DELETE_NOTIFICATION: &str = "/notification/delete/:id";
    pub const BULK_DELETE_NOTIFICATION: &str = "/notification/bulk-delete";
    pub const MODIFY_NOTIFICATION: &str = "/notification/modify/:id";
}

#[derive(Debug)]
pub struct Global<Io: IoHandle> {
    pub smtp_transport: Arc<AsyncSmtpTransport<lettre::Tokio1Executor>>,
    pub worlds: Arc<Worlds<Io>>,
    pub resource_sessions: Arc<Mutex<resource::UploadSessions>>,
    pub config: Arc<Config>,

    pub test_cx: Arc<sms4_backend::TestCx>,
}

impl<Io: IoHandle> Clone for Global<Io> {
    #[inline]
    fn clone(&self) -> Self {
        Self {
            smtp_transport: self.smtp_transport.clone(),
            worlds: self.worlds.clone(),
            config: self.config.clone(),
            test_cx: self.test_cx.clone(),
            resource_sessions: self.resource_sessions.clone(),
        }
    }
}

type AccountWorld<Io> = World<Account, 1, Io>;
type UnverifiedAccountWorld<Io> = World<sms4_backend::account::Unverified, 1, Io>;
type PostWorld<Io> = World<sms4_backend::post::Post, 4, Io>;
type ResourceWorld<Io> = World<sms4_backend::resource::Resource, 2, Io>;
type NotificationWorld<Io> = World<sms4_backend::notification::Notification, 2, Io>;

#[derive(Debug)]
pub struct Worlds<Io: IoHandle> {
    account: Arc<AccountWorld<Io>>,
    unverified_account: Arc<UnverifiedAccountWorld<Io>>,
    post: Arc<PostWorld<Io>>,
    resource: Arc<ResourceWorld<Io>>,
    notification: Arc<NotificationWorld<Io>>,
}

mod handle;

#[derive(Debug)]
pub struct Auth {
    account: u64,
    token: String,
}

impl Auth {
    #[inline]
    pub fn new(account: u64, token: String) -> Self {
        Self { account, token }
    }

    const KEY: &'static str = "Authorization";

    #[cfg(test)]
    pub fn append_to_req_builder(&self, builder: &mut Option<axum::http::request::Builder>) {
        *builder = Some(
            builder
                .take()
                .expect("request builder should not be None")
                .header(Self::KEY, format!("{}:{}", self.account, self.token)),
        );
    }
}

#[async_trait::async_trait]
impl<Io: IoHandle> axum::extract::FromRequestParts<Global<Io>> for Auth {
    type Rejection = Error;

    async fn from_request_parts(
        parts: &mut axum::http::request::Parts,
        _state: &Global<Io>,
    ) -> Result<Self, Self::Rejection> {
        let raw = parts.headers.remove(Self::KEY).ok_or(Error::NotLoggedIn)?;
        let (account, token) = raw
            .to_str()?
            .split_once(':')
            .ok_or(Error::InvalidAuthHeader)?;
        Ok(Self {
            account: account.parse().map_err(|_| Error::InvalidAuthHeader)?,
            token: token.to_owned(),
        })
    }
}

fn routing<Io: IoHandle + 'static>(router: Router<Global<Io>>) -> Router<Global<Io>> {
    use axum::routing::{delete, get, patch, post, put};
    use routes::*;

    router
        // account services
        .route(SEND_CAPTCHA, post(handle::account::send_captcha))
        .route(REGISTER, put(handle::account::register))
        .route(LOGIN, post(handle::account::login))
        .route(GET_ACCOUNT_INFO, get(handle::account::get_info))
        .route(
            SEND_RESET_PASSWORD_CAPTCHA,
            post(handle::account::send_reset_password_captcha),
        )
        .route(RESET_PASSWORD, patch(handle::account::reset_password))
        .route(MODIFY_ACCOUNT, patch(handle::account::modify))
        .route(LOGOUT, post(handle::account::logout))
        .route(SET_PERMISSIONS, patch(handle::account::set_permissions))
        .route(BULK_GET_ACCOUNT_INFO, post(handle::account::bulk_get_info))
        // post services
        .route(NEW_POST, put(handle::post::new_post))
        .route(FILTER_POSTS, get(handle::post::filter))
        .route(GET_POST, get(handle::post::get_info))
        .route(GET_POSTS, post(handle::post::bulk_get_info))
        .route(MODIFY_POST, patch(handle::post::modify))
        .route(REVIEW_POST, patch(handle::post::review))
        .route(DELETE_POST, delete(handle::post::remove))
        .route(BULK_DELETE_POST, delete(handle::post::bulk_remove))
        // resource services
        .route(NEW_UPLOAD_SESSION, put(handle::resource::new_session))
        .route(UPLOAD_RESOURCE, put(handle::resource::upload))
        .route(GET_RESOURCE_PAYLOAD, get(handle::resource::get_payload))
        .route(GET_RESOURCE_INFO, get(handle::resource::get_info))
        .route(
            BULK_GET_RESOURCE_INFO,
            post(handle::resource::bulk_get_info),
        )
        // notification services
        .route(NOTIFY, put(handle::notification::notify))
        .route(FILTER_NOTIFICATIONS, get(handle::notification::filter))
        .route(GET_NOTIFICATION, get(handle::notification::get_info))
        .route(
            BULK_GET_NOTIFICATION,
            post(handle::notification::bulk_get_info),
        )
        .route(DELETE_NOTIFICATION, delete(handle::notification::remove))
        .route(
            BULK_DELETE_NOTIFICATION,
            delete(handle::notification::bulk_remove),
        )
        .route(MODIFY_NOTIFICATION, patch(handle::notification::modify))
}

#[cfg(test)]
mod tests;

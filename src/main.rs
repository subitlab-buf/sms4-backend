use std::sync::Arc;

use dmds::{IoHandle, World};
use lettre::AsyncSmtpTransport;
use sms4_backend::{account::Account, config::Config, Error};

fn main() {}

mod routes {
    pub const SEND_CAPTCHA: &str = "/account/send-captcha";
    pub const REGISTER: &str = "/account/register";
    pub const LOGIN: &str = "/account/login";
    pub const SEND_RESET_PASSWORD_CAPTCHA: &str = "/account/send-reset-password-captcha";
    pub const RESET_PASSWORD: &str = "/account/reset-password";
    pub const SELF_ACCOUNT_INFO: &str = "/account/self-info";
    pub const MODIFY_ACCOUNT: &str = "/account/modify";
    pub const LOGOUT: &str = "/account/logout";
    pub const SET_PERMISSIONS: &str = "/account/set-permissions";
    pub const GET_INFO: &str = "/account/get-info/:id";
}

#[derive(Debug)]
pub struct Global<Io: IoHandle> {
    pub smtp_transport: Arc<AsyncSmtpTransport<lettre::Tokio1Executor>>,
    pub worlds: Arc<Worlds<Io>>,
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
        }
    }
}

type AccountWorld<Io> = World<Account, 1, Io>;
type UnverifiedAccountWorld<Io> = World<sms4_backend::account::Unverified, 1, Io>;
type PostWorld<Io> = World<sms4_backend::post::Post, 4, Io>;

#[derive(Debug)]
pub struct Worlds<Io: IoHandle> {
    account: AccountWorld<Io>,
    unverified_account: UnverifiedAccountWorld<Io>,
    post: PostWorld<Io>,
}

mod handle {
    /// Selects an account.
    #[macro_export]
    macro_rules! sa {
        ($w:expr, $id:expr) => {
            $w.select(0, $id).hint($id)
        };
    }

    /// Gets an account from selection.
    #[macro_export]
    macro_rules! ga {
        ($s:expr, $id:expr) => {{
            let mut iter = $s.iter();
            let mut lazy = None;
            while let Some(Ok(l)) = dmds::StreamExt::next(&mut iter).await {
                if l.id() == $id {
                    lazy = Some(l);
                }
            }
            lazy
        }};
    }

    /// Validates an account.
    #[macro_export]
    macro_rules! va {
        ($a:expr, $s:expr => $($p:ident),*$(,)?) => {{
            let lazy = ga!($s, $a.account).ok_or(crate::Error::PermissionDenied)?;
            let a = lazy.get().await?;
            if a.is_token_valid(&$a.token) {
                let _tags = a.tags();
                if !($(_tags.contains_permission(&sms4_backend::account::Tag::Permission(sms4_backend::account::Permission::$p)) &&)* true) {
                    return Err($crate::Error::PermissionDenied);
                }
            } else {
                return Err($crate::Error::LibAccount(libaccount::Error::InvalidToken));
            }
            lazy
        }};
        ($a:expr, $s:expr) => {
            va!($a, $s =>)
        }
    }

    pub mod account;
    pub mod post;
}

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

    const KEY: &str = "Authorization";

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

#[cfg(test)]
mod tests;

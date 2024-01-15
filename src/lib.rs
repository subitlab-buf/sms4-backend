use std::sync::atomic::AtomicBool;

use account::verify::VerifyVariant;
use axum::{http::StatusCode, response::IntoResponse};
use lettre::transport::smtp;
use serde::{Deserialize, Serialize};
use time::Duration;

pub mod config;

pub mod account;
pub mod notification;
pub mod post;

pub mod resource;

pub static IS_TEST: AtomicBool = AtomicBool::new(false);

#[derive(Debug, Default)]
pub struct TestCx {
    pub captcha: tokio::sync::Mutex<Option<account::verify::Captcha>>,
}

#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    #[error("account error: {0}")]
    LibAccount(libaccount::Error),
    #[error("verify session \"{0}\" not found")]
    VerifySessionNotFound(VerifyVariant),
    #[error("permission denied")]
    PermissionDenied,
    #[error("unverified account not found")]
    UnverifiedAccountNotFound,
    #[error("username or password incorrect")]
    UsernameOrPasswordIncorrect,
    #[error("target operation account not found")]
    AccountNotFound,

    #[error("captcha incorrect")]
    CaptchaIncorrect,
    #[error("request too frequent, try after {0}")]
    ReqTooFrequent(time::Duration),

    #[error("address error: {0}")]
    EmailAddress(lettre::address::AddressError),
    #[error("email message error: {0}")]
    Lettre(lettre::error::Error),
    #[error("failed to send email")]
    Smtp(smtp::Error),

    #[error("resource upload session {0} not found")]
    ResourceUploadSessionNotFound(u64),

    #[error("not logged in")]
    NotLoggedIn,
    #[error("non-ascii header value: {0}")]
    HeaderNonAscii(axum::http::header::ToStrError),
    #[error("auth header is not in {{account}}:{{token}} syntax")]
    InvalidAuthHeader,

    #[error("the given post resources list is empty")]
    PostResourceEmpty,
    #[error("post with given post id {0} not found")]
    PostNotFound(u64),
    #[error(
        "post time range out of bound: given duration: {0}, expected: <= {}",
        post::Post::MAX_DUR
    )]
    PostTimeRangeOutOfBound(Duration),
    #[error("given post end time is earlier than now")]
    PostTimeEnded,
    #[error("invalid review result status")]
    InvalidPostStatus,

    #[error("resource {0} has already be used")]
    ResourceUsed(u64),
    #[error("resource save failed")]
    ResourceSaveFailed,
    #[error("resource {0} not found")]
    ResourceNotFound(u64),

    #[error("notification {0} not found")]
    NotificationNotFound(u64),

    #[error("database errored")]
    Database(dmds::Error),

    #[error("unknown")]
    Unknown,
}

impl Error {
    pub fn to_status_code(&self) -> StatusCode {
        match self {
            Error::VerifySessionNotFound(_)
            | Error::ResourceUploadSessionNotFound(_)
            | Error::AccountNotFound
            | Error::UnverifiedAccountNotFound
            | Error::ResourceNotFound(_)
            | Error::NotificationNotFound(_) => StatusCode::NOT_FOUND,
            Error::ReqTooFrequent(_) => StatusCode::TOO_MANY_REQUESTS,
            Error::EmailAddress(_) => StatusCode::BAD_REQUEST,
            Error::Lettre(_) | Error::Smtp(_) => StatusCode::INTERNAL_SERVER_ERROR,
            Error::NotLoggedIn => StatusCode::UNAUTHORIZED,
            Error::HeaderNonAscii(_) | Error::InvalidAuthHeader => StatusCode::BAD_REQUEST,
            Error::ResourceUsed(_) => StatusCode::CONFLICT,
            Error::Database(_) | Error::Unknown | Error::ResourceSaveFailed => {
                StatusCode::INTERNAL_SERVER_ERROR
            }
            _ => StatusCode::FORBIDDEN,
        }
    }
}

impl IntoResponse for Error {
    #[inline]
    fn into_response(self) -> axum::response::Response {
        #[derive(Serialize)]
        struct ErrorInfo {
            error: String,
        }
        (
            self.to_status_code(),
            axum::Json(ErrorInfo {
                error: self.to_string(),
            }),
        )
            .into_response()
    }
}

/// Implements `From<T>` for [`Error`].
macro_rules! impl_from {
    ($($t:ty => $v:ident),* $(,)?) => {
        $(
            impl From<$t> for $crate::Error {
                #[inline]
                fn from(err: $t) -> Self {
                    Self::$v(err)
                }
            }
        )*
    };
}

impl_from! {
    libaccount::Error => LibAccount,
    lettre::address::AddressError => EmailAddress,
    lettre::error::Error => Lettre,
    smtp::Error => Smtp,
    axum::http::header::ToStrError => HeaderNonAscii,
    dmds::Error => Database,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct Id(pub u64);

impl Serialize for Id {
    #[inline]
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.0.to_string())
    }
}

impl<'de> Deserialize<'de> for Id {
    #[inline]
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Repr<'a> {
            Num(u64),
            Str(&'a str),
        }

        match Repr::deserialize(deserializer)? {
            Repr::Num(n) => Ok(Self(n)),
            Repr::Str(s) => s.parse().map_err(|_| {
                serde::de::Error::invalid_value(
                    serde::de::Unexpected::Str(s),
                    &"number as a string",
                )
            }),
        }
    }
}

impl std::str::FromStr for Id {
    type Err = std::num::ParseIntError;

    #[inline]
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        s.parse().map(Self)
    }
}

impl From<u64> for Id {
    #[inline]
    fn from(value: u64) -> Self {
        Self(value)
    }
}

impl From<Id> for u64 {
    #[inline]
    fn from(value: Id) -> Self {
        value.0
    }
}

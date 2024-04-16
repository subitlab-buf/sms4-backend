//! The configuration of the server.

use std::path::PathBuf;

use lettre::{transport::smtp, AsyncSmtpTransport};
use serde::{Deserialize, Serialize};

/// The configuration of the server.
#[derive(Debug, Serialize, Deserialize)]
pub struct Config {
    /// SMTP configuration.
    pub smtp: SMTP,

    /// The root path of the database.
    pub db_path: PathBuf,
    /// The port of HTTP server.
    pub port: u16,

    /// The root path of resource files.
    pub resource_path: PathBuf,

    /// Numbers of public screens.
    pub screens: usize,
}

/// SMTP mailing configuration.
#[derive(Debug, Serialize, Deserialize)]
pub struct SMTP {
    /// The SMTP Server address.
    pub server: String,
    /// The SMTP Server port.
    #[serde(default)]
    pub port: Option<u16>,
    /// The encryption type of target SMTP server.
    #[serde(default)]
    pub encrypt: SmtpEncryption,

    /// The email address.
    pub address: lettre::Address,

    /// The username.
    pub username: String,
    /// The password.
    pub password: String,

    /// The auth mechanism.
    ///
    /// Serialized and deserialized as `PascalCase`.
    pub auth: Vec<smtp::authentication::Mechanism>,
}

/// The encryption type of target SMTP server.
#[derive(Debug, Serialize, Deserialize, Default)]
pub enum SmtpEncryption {
    /// Use TLS.
    #[default]
    Tls,
    /// Use STARTTLS.
    StartTls,
}

impl SMTP {
    /// Make this configuration to an async smtp transport.
    pub fn to_transport<E>(&self) -> Result<AsyncSmtpTransport<E>, smtp::Error>
    where
        E: lettre::Executor,
    {
        let mut builder = if matches!(self.encrypt, SmtpEncryption::StartTls) {
            AsyncSmtpTransport::<E>::starttls_relay(&self.server)?
        } else {
            AsyncSmtpTransport::<E>::relay(&self.server)?
        };
        builder = builder
            .credentials(smtp::authentication::Credentials::new(
                self.username.to_owned(),
                self.password.to_owned(),
            ))
            .authentication(self.auth.clone());
        if let Some(port) = self.port {
            builder = builder.port(port);
        }
        Ok(builder.build())
    }
}

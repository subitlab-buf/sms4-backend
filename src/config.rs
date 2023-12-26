use std::path::PathBuf;

use lettre::{transport::smtp, AsyncSmtpTransport};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct Config {
    /// SMTP configuration.
    pub smtp: SMTP,

    /// The root path of database.
    pub db_path: PathBuf,
    /// The port of HTTP server.
    pub port: u16,
}

/// SMTP mailing configuration.
#[derive(Debug, Serialize, Deserialize)]
pub struct SMTP {
    /// The SMTP Server address.
    pub server: String,
    /// The SMTP Server port.
    #[serde(default)]
    pub port: Option<u16>,
    #[serde(default)]
    pub encrypt: SmtpEncryption,

    /// The email address.
    pub address: lettre::Address,

    pub username: String,
    pub password: String,

    /// The auth mechanism.
    ///
    /// Serialized and deserialized as `PascalCase`.
    pub auth: Vec<smtp::authentication::Mechanism>,
}

/// The encryption type of target SMTP server.
#[derive(Debug, Serialize, Deserialize, Default)]
pub enum SmtpEncryption {
    #[default]
    Tls,
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

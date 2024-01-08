use std::{
    hash::{Hash, Hasher},
    time::SystemTime,
};

use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

/// Notification sent by admins, and displayed
/// by the screens.
///
/// # dmds Dimensions
///
/// ```txt
/// 0 -> id
/// 1 -> start date day of the year
/// ```
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Notification {
    /// Id of the notification.
    #[serde(skip)]
    id: u64,

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
    time: OffsetDateTime,
    /// Sender's account id of the notification.
    sender: u64,
}

impl Notification {
    /// Creates a new notification.
    ///
    /// The **id** of the notification is generated
    /// from the body, time and current time.
    pub fn new(title: String, body: String, time: OffsetDateTime, sender: u64) -> Self {
        let mut hasher = siphasher::sip::SipHasher24::new();
        body.hash(&mut hasher);
        time.hash(&mut hasher);
        SystemTime::now().hash(&mut hasher);

        Self {
            id: hasher.finish(),
            title,
            body,
            time,
            sender,
        }
    }

    /// Returns the id of the notification.
    #[inline]
    pub fn id(&self) -> u64 {
        self.id
    }
}

impl dmds::Data for Notification {
    const DIMS: usize = 2;
    const VERSION: u32 = 1;

    #[inline]
    fn dim(&self, dim: usize) -> u64 {
        match dim {
            0 => self.id,
            1 => self.time.date().ordinal() as u64,
            _ => unreachable!(),
        }
    }

    fn decode<B: bytes::Buf>(version: u32, dims: &[u64], buf: B) -> std::io::Result<Self> {
        match version {
            1 => bincode::deserialize_from(buf.reader())
                .map(|mut n: Self| {
                    n.id = dims[0];
                    n
                })
                .map_err(|err| std::io::Error::new(std::io::ErrorKind::Other, err)),
            _ => unreachable!("unsupported data version {version}"),
        }
    }

    #[inline]
    fn encode<B: bytes::BufMut>(&self, buf: B) -> std::io::Result<()> {
        bincode::serialize_into(buf.writer(), self)
            .map_err(|err| std::io::Error::new(std::io::ErrorKind::Other, err))
    }
}

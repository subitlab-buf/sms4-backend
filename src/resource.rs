use std::{
    collections::HashMap,
    hash::{Hash, Hasher},
    time::SystemTime,
};

use serde::{Deserialize, Serialize};
use time::Instant;

use crate::Error;

/// Reference and metadata of a resource file.
///
/// # dmds Dimensions
///
/// ```txt
/// 0 -> id
/// 1 -> used (false -> 0, true -> 1)
/// ```
#[derive(Debug, Serialize, Deserialize)]
pub struct Resource {
    #[serde(skip)]
    id: u64,
    variant: Variant,
    owner: u64,

    #[serde(skip)]
    used: bool,
}

impl Resource {
    /// Creates a new resource with given variant and user.
    ///
    /// The id will be generated randomly based on the
    /// time and account.
    pub fn new(variant: Variant, account: u64) -> Self {
        let mut hasher = siphasher::sip::SipHasher24::new();
        SystemTime::now().hash(&mut hasher);
        account.hash(&mut hasher);
        rand::random::<i32>().hash(&mut hasher);

        Self {
            id: hasher.finish(),
            variant,
            owner: account,
            used: false,
        }
    }

    /// Id of this resource.
    #[inline]
    pub fn id(&self) -> u64 {
        self.id
    }

    #[inline]
    pub fn owner(&self) -> u64 {
        self.owner
    }

    /// Variant of this resource.
    #[inline]
    pub fn variant(&self) -> Variant {
        self.variant
    }

    /// Marks this resource as used.
    #[inline]
    pub fn block(&mut self) -> Result<(), Error> {
        if self.used {
            return Err(Error::ResourceUsed(self.id));
        }
        self.used = true;
        Ok(())
    }

    /// Marks this resource as unused.
    #[inline]
    pub fn unblock(&mut self) {
        self.used = false
    }

    /// Whether this resource is currently
    /// used by some data.
    #[inline]
    pub fn is_blocked(&self) -> bool {
        self.used
    }

    const FILE_PREFIX: &'static str = "r_";

    /// File name of this resource.
    pub fn file_name(&self) -> String {
        format!("{}{}", Self::FILE_PREFIX, self.id)
    }

    const BUF_PREFIX: &'static str = "buf_";

    /// Buffer file name of this resource.
    pub fn buf_name(&self) -> String {
        format!("{}{}", Self::BUF_PREFIX, self.id)
    }
}

impl dmds::Data for Resource {
    const DIMS: usize = 2;
    const VERSION: u32 = 1;

    #[inline]
    fn dim(&self, dim: usize) -> u64 {
        match dim {
            0 => self.id,
            1 => self.used as u64,
            _ => unreachable!(),
        }
    }

    fn decode<B: bytes::Buf>(version: u32, dims: &[u64], buf: B) -> std::io::Result<Self> {
        match version {
            0 => {
                let mut this: Self = bincode::deserialize_from(buf.reader())
                    .map_err(|err| std::io::Error::new(std::io::ErrorKind::Other, err))?;
                this.id = dims[0];
                this.used = dims[1] != 0;
                Ok(this)
            }
            _ => unreachable!(),
        }
    }

    #[inline]
    fn encode<B: bytes::BufMut>(&self, buf: B) -> std::io::Result<()> {
        bincode::serialize_into(buf.writer(), self)
            .map_err(|err| std::io::Error::new(std::io::ErrorKind::Other, err))
    }
}

/// A resource uploading session.
#[derive(Debug)]
struct UploadSession {
    resource: Resource,
    instant: Instant,
}

impl UploadSession {
    /// Creates a new session.
    #[inline]
    fn new(resource: Resource) -> Self {
        Self {
            resource,
            instant: Instant::now(),
        }
    }

    /// Expire duration of a session.
    const EXPIRE_DUR: time::Duration = time::Duration::seconds(15);

    /// Whether this session is expired.
    #[inline]
    fn is_expired(&self) -> bool {
        self.instant.elapsed() > Self::EXPIRE_DUR
    }
}

impl From<Resource> for UploadSession {
    #[inline]
    fn from(value: Resource) -> Self {
        Self::new(value)
    }
}

impl From<UploadSession> for Resource {
    #[inline]
    fn from(value: UploadSession) -> Self {
        value.resource
    }
}

/// Storage of resource upload sessions.
#[derive(Debug, Default)]
pub struct UploadSessions {
    /// Id => Session.
    inner: HashMap<u64, UploadSession>,
}

impl UploadSessions {
    #[inline]
    pub fn new() -> Self {
        Default::default()
    }

    #[inline]
    fn cleanup(&mut self) {
        self.inner.retain(|_, v| !v.is_expired());
    }

    /// Inserts a new session.
    pub fn insert(&mut self, res: Resource) {
        self.cleanup();
        self.inner.insert(res.id, res.into());
    }

    /// Accepts the body of a resource with given id,
    /// and returns the resource.
    ///
    /// **Id of the resource will be changed**, so you have to
    /// tell the new id to the frontend.
    pub fn accept<H: Hasher>(
        &mut self,
        id: u64,
        mut hasher: H,
        user: u64,
    ) -> Result<Resource, Error> {
        self.cleanup();
        let res = &self
            .inner
            .get(&id)
            .ok_or(Error::ResourceUploadSessionNotFound(id))?
            .resource;
        if res.owner != user {
            return Err(Error::PermissionDenied);
        }

        let mut res = self.inner.remove(&id).unwrap().resource;
        SystemTime::now().hash(&mut hasher);
        user.hash(&mut hasher);
        res.id = hasher.finish();
        Ok(res)
    }

    /// Gets filesystem buffer name of a resource session.
    #[inline]
    pub fn buf_name(&self, id: u64) -> Option<String> {
        self.inner.get(&id).map(|s| s.resource.buf_name())
    }
}

/// Type of a [`Resource`].
#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq, Eq)]
#[serde(tag = "type")]
pub enum Variant {
    Image,
    Pdf {
        /// Number of pages.
        pages: u16,
    },
    Video {
        /// Video duration, as seconds.
        duration: u32,
    },
}

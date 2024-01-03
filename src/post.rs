use std::{
    hash::{Hash, Hasher},
    ops::RangeInclusive,
    time::SystemTime,
};

use serde::{Deserialize, Serialize};
use time::{Date, Duration, OffsetDateTime};

/// A post.
///
/// # dmds Dimensions
///
/// ```txt
/// 0 -> id
/// 1 -> start date day of the year
/// 2 -> creator uid
/// 3 -> is approved
/// ```
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Post {
    #[serde(skip)]
    id: u64,
    title: String,
    /// On-screen time range.
    time: RangeInclusive<Date>,

    /// List of resource ids this post used.
    resources: Box<[u64]>,

    /// Post states in time order.\
    /// There should be at least one state in a post.
    states: Vec<State>,
}

impl Post {
    pub const MAX_DUR: Duration = Duration::WEEK;

    pub fn new(
        title: String,
        notes: String,
        time: RangeInclusive<time::Date>,
        resources: Box<[u64]>,
        account: u64,
    ) -> Self {
        let mut hasher = siphasher::sip::SipHasher24::new();
        title.hash(&mut hasher);
        account.hash(&mut hasher);
        time.hash(&mut hasher);
        SystemTime::now().hash(&mut hasher);

        Self {
            id: hasher.finish(),
            title,
            time,
            resources,
            states: vec![State::new(Status::Pending, account, notes)],
        }
    }

    /// Gets id of this post.
    #[inline]
    pub fn id(&self) -> u64 {
        self.id
    }

    /// Gets the overall states of this post.
    #[inline]
    pub fn states(&self) -> &[State] {
        &self.states
    }

    /// The current state of this post.
    #[inline]
    pub fn state(&self) -> &State {
        self.states
            .last()
            .expect("there should be at least one state in a post")
    }

    /// Creator of this post.
    #[inline]
    pub fn creator(&self) -> u64 {
        self.states
            .first()
            .expect("there should be at least one state in a post")
            .operator
    }

    /// Gets the time range of this post.
    #[inline]
    pub fn time(&self) -> &RangeInclusive<Date> {
        &self.time
    }

    /// Gets the title of this post.
    #[inline]
    pub fn title(&self) -> &str {
        &self.title
    }

    /// Gets the resources used by this post.
    #[inline]
    pub fn resources(&self) -> &[u64] {
        &self.resources
    }
}

impl dmds::Data for Post {
    const DIMS: usize = 4;
    const VERSION: u32 = 1;

    #[inline]
    fn dim(&self, dim: usize) -> u64 {
        match dim {
            0 => self.id,
            1 => self.time.start().ordinal() as u64,
            2 => self.creator(),
            3 => self
                .states
                .last()
                .is_some_and(|s| matches!(s.status, Status::Approved)) as u64,
            _ => unreachable!(),
        }
    }

    fn decode<B: bytes::Buf>(version: u32, dims: &[u64], buf: B) -> std::io::Result<Self> {
        match version {
            1 => bincode::deserialize_from(buf.reader())
                .map(|mut p: Self| {
                    p.id = dims[0];
                    p
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

/// State of a [`Post`].
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct State {
    status: Status,
    #[serde(with = "time::serde::timestamp")]
    time: OffsetDateTime,
    operator: u64,

    /// Description of this state.
    message: String,
}

impl State {
    #[inline]
    pub fn new(status: Status, account: u64, message: String) -> Self {
        Self {
            status,
            time: OffsetDateTime::now_utc(),
            operator: account,
            message,
        }
    }

    /// [`Status`] of this state.
    #[inline]
    pub fn status(&self) -> Status {
        self.status
    }

    /// Creation time of this state.
    #[inline]
    pub fn time(&self) -> OffsetDateTime {
        self.time
    }

    /// Creator of this state.
    #[inline]
    pub fn operator(&self) -> u64 {
        self.operator
    }

    /// Description of this state, written by operators.
    #[inline]
    pub fn message(&self) -> &str {
        &self.message
    }
}

/// Status of a post.
#[derive(Debug, PartialEq, Eq, Clone, Copy, Serialize, Deserialize)]
pub enum Status {
    Pending,
    Approved,
    Rejected,
}

/// Deploy priority of a post.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
#[repr(u8)]
pub enum Priority {
    /// Blocks all other non-blocking posts while
    /// in play time.
    Block = 255_u8,

    High = 3,
    #[default]
    Normal = 2,
    Low = 1,
}

use std::{
    hash::{Hash, Hasher},
    ops::RangeInclusive,
    time::SystemTime,
};

use serde::{Deserialize, Serialize};
use time::{Date, Duration, OffsetDateTime};

use crate::{Error, Id};

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
    resources: Box<[Id]>,

    /// Post states in time order.\
    /// There should be at least one state in a post.
    states: Vec<State>,

    /// Whether this post should be played as
    /// a full sequence.
    grouped: bool,
    priority: Priority,
}

pub fn validate_time(time: &RangeInclusive<Date>) -> Result<(), Error> {
    let dur = Duration::days(
        u32::try_from(
            time.end()
                .to_julian_day()
                .checked_sub(time.start().to_julian_day())
                .ok_or_else(|| Error::PostTimeRangeOutOfBound(Duration::MAX))?,
        )
        .map_err(|_| Error::PostTimeRangeOutOfBound(Duration::MAX))? as i64,
    );
    if dur > Post::MAX_DUR {
        Err(Error::PostTimeRangeOutOfBound(dur))
    } else if *time.end() < OffsetDateTime::now_utc().date() {
        Err(Error::PostTimeEnded)
    } else {
        Ok(())
    }
}

impl Post {
    pub const MAX_DUR: Duration = Duration::WEEK;

    pub fn new(
        title: String,
        notes: String,
        time: RangeInclusive<time::Date>,
        resources: Box<[Id]>,
        account: u64,
        grouped: bool,
        priority: Priority,
    ) -> Result<Self, Error> {
        validate_time(&time)?;

        let mut hasher = siphasher::sip::SipHasher24::new();
        title.hash(&mut hasher);
        account.hash(&mut hasher);
        time.hash(&mut hasher);
        SystemTime::now().hash(&mut hasher);

        Ok(Self {
            id: hasher.finish(),
            title,
            time,
            resources,
            states: vec![State::new(Status::Pending, account, notes)],
            grouped,
            priority,
        })
    }

    /// Gets id of this post.
    #[inline]
    pub fn id(&self) -> u64 {
        self.id
    }

    /// Gets the title of this post.
    #[inline]
    pub fn title(&self) -> &str {
        &self.title
    }

    /// Sets the title of this post.
    #[inline]
    pub fn set_title(&mut self, title: String) {
        self.title = title;
    }

    /// Gets the overall states of this post.
    #[inline]
    pub fn states(&self) -> &[State] {
        &self.states
    }

    /// Pushes a state into this post.
    pub fn pust_state(&mut self, state: State) -> Result<(), Error> {
        if self.state().status() == state.status()
            && matches!(state.status(), Status::Approved | Status::Rejected)
        {
            return Err(Error::InvalidPostStatus);
        }
        self.states.push(state);
        Ok(())
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
    pub fn creator(&self) -> Id {
        Id(self
            .states
            .first()
            .expect("there should be at least one state in a post")
            .operator)
    }

    #[inline]
    pub fn priority(&self) -> Priority {
        self.priority
    }

    /// Gets the time range of this post.
    #[inline]
    pub fn time(&self) -> &RangeInclusive<Date> {
        &self.time
    }

    /// Sets the time range of this post.
    #[inline]
    pub fn set_time(&mut self, time: RangeInclusive<Date>) -> Result<(), Error> {
        validate_time(&time)?;
        self.time = time;
        Ok(())
    }

    /// Gets the resources used by this post.
    #[inline]
    pub fn resources(&self) -> &[Id] {
        &self.resources
    }

    #[inline]
    pub fn set_resources(&mut self, resources: Box<[Id]>) {
        self.resources = resources
    }

    #[inline]
    pub fn is_grouped(&self) -> bool {
        self.grouped
    }

    #[inline]
    pub fn set_is_grouped(&mut self, grouped: bool) {
        self.grouped = grouped
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
            2 => self.creator().0,
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

/// Status of a [`Post`].
#[derive(Debug, PartialEq, Eq, Clone, Copy, Serialize, Deserialize)]
pub enum Status {
    /// Pending for review.
    Pending,
    /// Approved.
    Approved,
    /// Rejected.
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

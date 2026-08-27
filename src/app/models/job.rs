//! Persistent jobs: the unit of retryable work for the outbound
//! (upload) and inbound (forward-back) legs. The `JobQueue` port
//! lives in `ports`; persistence lives in `db`.

/// Which leg a job belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JobKind {
    /// Object-store upload of a received, de-identified instance.
    Upload,
    /// Forward a fetched, re-identified result to its destination AE.
    Forward,
}

impl JobKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Upload => "upload",
            Self::Forward => "forward",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "upload" => Some(Self::Upload),
            "forward" => Some(Self::Forward),
            _ => None,
        }
    }
}

/// A job to persist for later processing.
pub struct NewJob {
    pub kind: JobKind,
    /// Storage key for uploads, `bucket/key` for forwards (tracing).
    pub key: String,
    pub payload: Vec<u8>,
}

/// A claimed job handed to a worker.
#[derive(Debug, Clone)]
pub struct Job {
    pub id: i64,
    pub kind: JobKind,
    pub key: String,
    pub payload: Vec<u8>,
    pub attempts: i32,
}

/// Retry backoff: 2^attempts seconds, capped at 15 minutes.
pub fn backoff_seconds(attempts: i32) -> i64 {
    (1_i64 << attempts.clamp(0, 20)).min(900)
}

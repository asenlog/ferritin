//! Queue workers: the retrying consumers behind the two legs. One
//! worker per `JobKind` — claim a job, hand it to the leg's
//! processor, complete on success, fail (with backoff) on error.
//!
//! A worker is a thin loop over `tick`; the legs' real logic lives in
//! the services and adapters handed in as the processor closure.

use crate::app::models::job::{Job, JobKind};
use crate::app::ports::JobQueue;
use std::time::Duration;

/// How long a worker sleeps after an empty or failed round.
const IDLE_SLEEP: Duration = Duration::from_secs(5);

pub struct QueueWorker<Q> {
    queue: Q,
    kind: JobKind,
}

impl<Q: JobQueue> QueueWorker<Q> {
    pub fn new(queue: Q, kind: JobKind) -> Self {
        Self { queue, kind }
    }

    /// Claim and process jobs forever. Strand recovery runs once at
    /// startup: jobs left `running` by a crash go back to pending
    /// (single-node assumption — one worker per kind per node).
    pub fn run(&self, process: impl Fn(&Job) -> anyhow::Result<()>) -> ! {
        match self.queue.recover_running(self.kind) {
            Ok(0) => {}
            Ok(n) => tracing::warn!(kind = self.kind.as_str(), "recovered {n} stranded jobs"),
            Err(e) => tracing::error!(kind = self.kind.as_str(), "recovery failed: {e:#}"),
        }
        loop {
            match self.tick(&process) {
                Ok(true) => {} // work done: look for more immediately
                Ok(false) => std::thread::sleep(IDLE_SLEEP),
                Err(e) => {
                    tracing::error!(kind = self.kind.as_str(), "worker round failed: {e:#}");
                    std::thread::sleep(IDLE_SLEEP);
                }
            }
        }
    }

    /// One claim-process-complete/fail round. Returns whether a job
    /// was claimed — `false` means the queue had nothing due.
    pub fn tick(&self, process: impl Fn(&Job) -> anyhow::Result<()>) -> anyhow::Result<bool> {
        let Some(job) = self.queue.claim(self.kind)? else {
            return Ok(false);
        };
        match process(&job) {
            Ok(()) => {
                self.queue.complete(job.id)?;
                tracing::info!(
                    kind = self.kind.as_str(),
                    key = job.key,
                    "job {} done",
                    job.id
                );
            }
            Err(e) => {
                tracing::warn!(
                    kind = self.kind.as_str(),
                    key = job.key,
                    "job {} failed (attempt {}): {e:#}",
                    job.id,
                    job.attempts
                );
                self.queue.fail(job.id, &format!("{e:#}"))?;
            }
        }
        Ok(true)
    }
}

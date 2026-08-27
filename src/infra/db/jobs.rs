//! Repository for the `jobs` table: the persistent, retrying queues
//! behind the upload and forward legs. The in-memory sibling below
//! serves tests.

use super::PgStore;
use crate::app::models::job::{backoff_seconds, Job, JobKind, NewJob};
use crate::app::ports::JobQueue;
use anyhow::Context;
use chrono::{DateTime, Utc};
use std::sync::Mutex;

/// The `jobs` row as persisted; converted to the domain `Job` on
/// claim (rows are only ever read through `claim`).
#[derive(sqlx::FromRow)]
struct JobRow {
    id: i64,
    kind: String,
    key: Option<String>,
    payload: Vec<u8>,
    attempts: i32,
}

impl TryFrom<JobRow> for Job {
    type Error = anyhow::Error;

    fn try_from(row: JobRow) -> anyhow::Result<Self> {
        Ok(Self {
            id: row.id,
            kind: JobKind::parse(&row.kind)
                .with_context(|| format!("unknown job kind {:?} in jobs table", row.kind))?,
            key: row.key.unwrap_or_default(),
            payload: row.payload,
            attempts: row.attempts,
        })
    }
}

impl JobQueue for PgStore {
    fn enqueue(&self, job: NewJob) -> anyhow::Result<()> {
        self.runtime.block_on(async {
            sqlx::query("INSERT INTO jobs (kind, key, payload) VALUES ($1, $2, $3)")
                .bind(job.kind.as_str())
                .bind(&job.key)
                .bind(&job.payload)
                .execute(&self.pool)
                .await
                .context("failed to enqueue job")?;
            anyhow::Ok(())
        })
    }

    fn claim(&self, kind: JobKind) -> anyhow::Result<Option<Job>> {
        self.runtime.block_on(async {
            // atomic claim: oldest due job, skipped by competing
            // workers; the status flip and the attempt count happen
            // in the same statement
            let row = sqlx::query_as::<_, JobRow>(
                "UPDATE jobs SET status = 'running', attempts = attempts + 1
                 WHERE id = (
                     SELECT id FROM jobs
                     WHERE kind = $1 AND status = 'pending' AND next_run_at <= now()
                       AND deleted_at IS NULL
                     ORDER BY id
                     FOR UPDATE SKIP LOCKED
                     LIMIT 1
                 )
                 RETURNING id, kind, key, payload, attempts",
            )
            .bind(kind.as_str())
            .fetch_optional(&self.pool)
            .await
            .context("failed to claim job")?;
            row.map(TryInto::try_into).transpose()
        })
    }

    fn complete(&self, id: i64) -> anyhow::Result<()> {
        self.runtime.block_on(async {
            sqlx::query("UPDATE jobs SET status = 'done' WHERE id = $1")
                .bind(id)
                .execute(&self.pool)
                .await
                .context("failed to complete job")?;
            anyhow::Ok(())
        })
    }

    fn fail(&self, id: i64, error: &str) -> anyhow::Result<()> {
        self.runtime.block_on(async {
            // attempts was already incremented at claim; exhausting
            // the budget marks the job dead (the DLQ), anything else
            // goes back to pending with an exponential backoff
            sqlx::query(
                "UPDATE jobs SET
                     status = CASE WHEN attempts >= max_attempts THEN 'dead' ELSE 'pending' END,
                     last_error = $2,
                     next_run_at = CASE
                         WHEN attempts >= max_attempts THEN next_run_at
                         ELSE now() + (LEAST(POW(2, attempts), 900)::int || ' seconds')::interval
                     END
                 WHERE id = $1",
            )
            .bind(id)
            .bind(error)
            .execute(&self.pool)
            .await
            .context("failed to mark job failed")?;
            anyhow::Ok(())
        })
    }

    fn recover_running(&self, kind: JobKind) -> anyhow::Result<u64> {
        self.runtime.block_on(async {
            let result = sqlx::query(
                "UPDATE jobs SET status = 'pending' WHERE kind = $1 AND status = 'running'",
            )
            .bind(kind.as_str())
            .execute(&self.pool)
            .await
            .context("failed to recover running jobs")?;
            anyhow::Ok(result.rows_affected())
        })
    }
}

/// Test adapter: the same state machine in memory, no database.
/// Clone-cheap (shared interior) so a test can hand one instance to
/// the intake and keep one to drain with a worker.
#[derive(Clone, Default)]
pub struct InMemoryJobQueue {
    inner: std::sync::Arc<Mutex<Vec<InMemoryJob>>>,
    next_id: std::sync::Arc<Mutex<i64>>,
}

#[derive(Clone)]
struct InMemoryJob {
    id: i64,
    kind: JobKind,
    key: String,
    payload: Vec<u8>,
    status: &'static str,
    attempts: i32,
    max_attempts: i32,
    next_run_at: DateTime<Utc>,
}

impl JobQueue for InMemoryJobQueue {
    fn enqueue(&self, job: NewJob) -> anyhow::Result<()> {
        let mut id = self.next_id.lock().expect("job queue poisoned");
        *id += 1;
        self.inner
            .lock()
            .expect("job queue poisoned")
            .push(InMemoryJob {
                id: *id,
                kind: job.kind,
                key: job.key,
                payload: job.payload,
                status: "pending",
                attempts: 0,
                max_attempts: 8,
                next_run_at: Utc::now(),
            });
        Ok(())
    }

    fn claim(&self, kind: JobKind) -> anyhow::Result<Option<Job>> {
        let mut guard = self.inner.lock().expect("job queue poisoned");
        let now = Utc::now();
        let entry = guard
            .iter_mut()
            .find(|job| job.kind == kind && job.status == "pending" && job.next_run_at <= now);
        Ok(entry.map(|job| {
            job.status = "running";
            job.attempts += 1;
            Job {
                id: job.id,
                kind: job.kind,
                key: job.key.clone(),
                payload: job.payload.clone(),
                attempts: job.attempts,
            }
        }))
    }

    fn complete(&self, id: i64) -> anyhow::Result<()> {
        let mut guard = self.inner.lock().expect("job queue poisoned");
        if let Some(job) = guard.iter_mut().find(|job| job.id == id) {
            job.status = "done";
        }
        Ok(())
    }

    fn fail(&self, id: i64, _error: &str) -> anyhow::Result<()> {
        let mut guard = self.inner.lock().expect("job queue poisoned");
        if let Some(job) = guard.iter_mut().find(|job| job.id == id) {
            if job.attempts >= job.max_attempts {
                job.status = "dead";
            } else {
                job.status = "pending";
                job.next_run_at =
                    Utc::now() + chrono::Duration::seconds(backoff_seconds(job.attempts));
            }
        }
        Ok(())
    }

    fn recover_running(&self, kind: JobKind) -> anyhow::Result<u64> {
        let mut guard = self.inner.lock().expect("job queue poisoned");
        let mut recovered = 0;
        for job in guard
            .iter_mut()
            .filter(|job| job.kind == kind && job.status == "running")
        {
            job.status = "pending";
            recovered += 1;
        }
        Ok(recovered)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn upload_job() -> NewJob {
        NewJob {
            kind: JobKind::Upload,
            key: "1.2.3/4.5/6.7.dcm".to_string(),
            payload: b"bytes".to_vec(),
        }
    }

    #[test]
    fn enqueue_claim_complete_lifecycle() {
        let queue = InMemoryJobQueue::default();
        queue.enqueue(upload_job()).unwrap();

        let job = queue.claim(JobKind::Upload).unwrap().unwrap();
        assert_eq!(job.attempts, 1);
        assert_eq!(job.key, "1.2.3/4.5/6.7.dcm");
        // already claimed: nothing else is due
        assert!(queue.claim(JobKind::Upload).unwrap().is_none());

        queue.complete(job.id).unwrap();
        assert!(queue.claim(JobKind::Upload).unwrap().is_none());
    }

    #[test]
    fn kinds_are_independent() {
        let queue = InMemoryJobQueue::default();
        queue.enqueue(upload_job()).unwrap();

        assert!(queue.claim(JobKind::Forward).unwrap().is_none());
        assert!(queue.claim(JobKind::Upload).unwrap().is_some());
    }

    #[test]
    fn failed_job_backs_off_then_dead_lettered() {
        let queue = InMemoryJobQueue::default();
        queue.enqueue(upload_job()).unwrap();

        let job = queue.claim(JobKind::Upload).unwrap().unwrap();
        queue.fail(job.id, "boom").unwrap();
        // backoff: pending but not yet due
        assert!(queue.claim(JobKind::Upload).unwrap().is_none());

        // burn the attempt budget; the backoff has to be wound
        // forward between rounds (same module, so the test can)
        for _ in 0..7 {
            force_due(&queue);
            let job = queue.claim(JobKind::Upload).unwrap().unwrap();
            queue.fail(job.id, "boom").unwrap();
        }

        // budget exhausted: dead, never claimable again
        force_due(&queue);
        assert!(queue.claim(JobKind::Upload).unwrap().is_none());
    }

    fn force_due(queue: &InMemoryJobQueue) {
        let mut guard = queue.inner.lock().expect("job queue poisoned");
        for job in guard.iter_mut() {
            job.next_run_at = Utc::now() - chrono::Duration::seconds(1);
        }
    }

    #[test]
    fn recover_running_requeues_stranded_jobs() {
        let queue = InMemoryJobQueue::default();
        queue.enqueue(upload_job()).unwrap();
        let _job = queue.claim(JobKind::Upload).unwrap().unwrap();

        assert_eq!(queue.recover_running(JobKind::Upload).unwrap(), 1);
        assert!(queue.claim(JobKind::Upload).unwrap().is_some());
    }
}

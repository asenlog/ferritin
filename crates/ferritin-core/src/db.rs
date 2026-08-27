//! Database-backed ports: the per-study pseudonym mapping and the
//! authorized-caller directory, both on Postgres, plus the in-memory
//! adapters used in tests.
//!
//! These tables hold user-managed domain data — the things a frontend
//! edits — as opposed to deployment config, which lives in env vars.
//! The mapping ties the original patient identity of a study to the
//! pseudonym the de-identification step replaces it with; the
//! re-identification leg reads the same rows to restore results.

use crate::auth::{AuthorizedCaller, CallerDirectory};
use anyhow::Context;
use chrono::{DateTime, Utc};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// The de-identification record for one study.
#[derive(Debug, Clone, PartialEq)]
pub struct StudyMapping {
    pub study_instance_uid: String,
    pub patient_id: String,
    pub patient_name: String,
    pub anon_patient_id: String,
    pub anon_patient_name: String,
    pub created_at: DateTime<Utc>,
}

/// Where per-study pseudonym mappings are kept.
pub trait MappingStore {
    /// Return the mapping for `study_instance_uid`, creating it on
    /// first sight. Pseudonyms derive deterministically from the
    /// study UID, so a repeated study always maps the same way.
    fn mapping_for(
        &self,
        study_instance_uid: &str,
        patient_id: &str,
        patient_name: &str,
    ) -> anyhow::Result<StudyMapping>;

    /// Look up the mapping for a study without creating one — the
    /// re-identification leg must not fabricate mappings for studies
    /// it has never seen.
    fn find(&self, study_instance_uid: &str) -> anyhow::Result<Option<StudyMapping>>;
}

/// Pseudonyms are a deterministic function of the study UID: any
/// component can regenerate them, and two studies never collide
/// unless their UIDs do.
fn pseudonyms(study_instance_uid: &str) -> (String, String) {
    let hex = format!("{:x}", Sha256::digest(study_instance_uid.as_bytes()));
    let id = format!("ANON-{}", &hex[..12]);
    let name = format!("ANON^{}", &hex[..8].to_uppercase());
    (id, name)
}

const MAPPING_SELECT: &str = "SELECT study_instance_uid, patient_id, patient_name,
        anon_patient_id, anon_patient_name, created_at
     FROM study_mappings WHERE study_instance_uid = $1";

fn mapping_from_row(row: &sqlx::postgres::PgRow) -> anyhow::Result<StudyMapping> {
    use sqlx::Row;
    Ok(StudyMapping {
        study_instance_uid: row.try_get("study_instance_uid")?,
        patient_id: row.try_get("patient_id")?,
        patient_name: row.try_get("patient_name")?,
        anon_patient_id: row.try_get("anon_patient_id")?,
        anon_patient_name: row.try_get("anon_patient_name")?,
        created_at: row.try_get("created_at")?,
    })
}

/// Postgres adapter for both database-backed ports. Owns a private
/// tokio runtime (behind an `Arc`, so clones share it) so the sync
/// ports can sit underneath the (blocking) SCP without forcing async
/// into the association loop.
#[derive(Clone)]
pub struct PgStore {
    pool: sqlx::PgPool,
    runtime: Arc<tokio::runtime::Runtime>,
}

impl PgStore {
    /// Connect and bring the schema up to date. Migrations run under
    /// a Postgres advisory lock, so concurrent first boots serialize
    /// instead of racing.
    pub fn connect(database_url: &str) -> anyhow::Result<Self> {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .context("failed to build tokio runtime for Postgres store")?;
        let pool = runtime.block_on(async {
            let pool = sqlx::PgPool::connect(database_url)
                .await
                .context("failed to connect to Postgres")?;
            // resolved relative to the crate, so this is the
            // workspace-root `migrations/` directory
            sqlx::migrate!("../../migrations")
                .run(&pool)
                .await
                .context("failed to run migrations")?;
            anyhow::Ok(pool)
        })?;
        Ok(Self {
            pool,
            runtime: Arc::new(runtime),
        })
    }
}

impl MappingStore for PgStore {
    fn mapping_for(
        &self,
        study_instance_uid: &str,
        patient_id: &str,
        patient_name: &str,
    ) -> anyhow::Result<StudyMapping> {
        let (anon_id, anon_name) = pseudonyms(study_instance_uid);
        self.runtime.block_on(async {
            // lose the race harmlessly: first writer wins, then read back
            sqlx::query(
                "INSERT INTO study_mappings
                 (study_instance_uid, patient_id, patient_name, anon_patient_id, anon_patient_name)
                 VALUES ($1, $2, $3, $4, $5)
                 ON CONFLICT (study_instance_uid) DO NOTHING",
            )
            .bind(study_instance_uid)
            .bind(patient_id)
            .bind(patient_name)
            .bind(&anon_id)
            .bind(&anon_name)
            .execute(&self.pool)
            .await
            .context("failed to insert study mapping")?;

            let row = sqlx::query(MAPPING_SELECT)
                .bind(study_instance_uid)
                .fetch_one(&self.pool)
                .await
                .context("failed to read back study mapping")?;
            mapping_from_row(&row)
        })
    }

    fn find(&self, study_instance_uid: &str) -> anyhow::Result<Option<StudyMapping>> {
        self.runtime.block_on(async {
            let row = sqlx::query(MAPPING_SELECT)
                .bind(study_instance_uid)
                .fetch_optional(&self.pool)
                .await
                .context("failed to look up study mapping")?;
            row.as_ref().map(mapping_from_row).transpose()
        })
    }
}

impl CallerDirectory for PgStore {
    fn authorized_callers(&self) -> anyhow::Result<Vec<AuthorizedCaller>> {
        use sqlx::Row;

        self.runtime.block_on(async {
            let rows = sqlx::query("SELECT ae_title, network FROM authorized_callers")
                .fetch_all(&self.pool)
                .await
                .context("failed to load authorized callers")?;

            // a malformed row authorizes no one (fail closed on that
            // row) but must not lock out every other caller
            let callers = rows
                .iter()
                .filter_map(|row| {
                    let ae_title: String = row.try_get("ae_title").ok()?;
                    let network: String = row.try_get("network").ok()?;
                    format!("{ae_title}@{network}")
                        .parse::<AuthorizedCaller>()
                        .map_err(|e| {
                            tracing::warn!("ignoring malformed authorized_callers row: {e:#}");
                            e
                        })
                        .ok()
                })
                .collect();
            Ok(callers)
        })
    }
}

/// Test adapter: same get-or-create semantics, no database.
/// Clone-cheap (shared interior) so tests can hand one instance to
/// both the intake and the forwarding side.
#[derive(Clone, Default)]
pub struct InMemoryMappingStore {
    inner: Arc<Mutex<HashMap<String, StudyMapping>>>,
}

impl MappingStore for InMemoryMappingStore {
    fn mapping_for(
        &self,
        study_instance_uid: &str,
        patient_id: &str,
        patient_name: &str,
    ) -> anyhow::Result<StudyMapping> {
        let mut guard = self.inner.lock().expect("mapping store poisoned");
        Ok(guard
            .entry(study_instance_uid.to_string())
            .or_insert_with(|| {
                let (anon_id, anon_name) = pseudonyms(study_instance_uid);
                StudyMapping {
                    study_instance_uid: study_instance_uid.to_string(),
                    patient_id: patient_id.to_string(),
                    patient_name: patient_name.to_string(),
                    anon_patient_id: anon_id,
                    anon_patient_name: anon_name,
                    created_at: Utc::now(),
                }
            })
            .clone())
    }

    fn find(&self, study_instance_uid: &str) -> anyhow::Result<Option<StudyMapping>> {
        let guard = self.inner.lock().expect("mapping store poisoned");
        Ok(guard.get(study_instance_uid).cloned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_sight_creates_then_returns_same_mapping() {
        let store = InMemoryMappingStore::default();

        let first = store.mapping_for("1.2.3", "PAT-1", "Doe^John").unwrap();
        let second = store.mapping_for("1.2.3", "PAT-1", "Doe^John").unwrap();

        assert_eq!(first, second);
        assert!(first.anon_patient_id.starts_with("ANON-"));
        assert!(first.anon_patient_name.starts_with("ANON^"));
    }

    #[test]
    fn distinct_studies_get_distinct_pseudonyms() {
        let store = InMemoryMappingStore::default();

        let a = store.mapping_for("1.2.3", "PAT-1", "Doe^John").unwrap();
        let b = store.mapping_for("9.9.9", "PAT-1", "Doe^John").unwrap();

        assert_ne!(a.anon_patient_id, b.anon_patient_id);
    }

    #[test]
    fn find_returns_none_for_unknown_study() {
        let store = InMemoryMappingStore::default();

        assert!(store.find("1.2.3").unwrap().is_none());

        let created = store.mapping_for("1.2.3", "PAT-1", "Doe^John").unwrap();
        assert_eq!(store.find("1.2.3").unwrap(), Some(created));
    }
}

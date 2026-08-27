//! Repository for the `study_mappings` table.
//!
//! Owns the row model and the row ↔ domain conversion: SQL never
//! leaks past this module, and the domain (`mappings::StudyMapping`)
//! never learns SQL exists. The in-memory sibling below serves tests.

use super::PgStore;
use crate::app::models::mappings::StudyMapping;
use crate::app::ports::MappingStore;
use anyhow::Context;
use chrono::Utc;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// The `study_mappings` row as persisted. Field-for-field the domain
/// record today, but a separate type on purpose: schema changes stop
/// here instead of rippling into the domain.
#[derive(sqlx::FromRow)]
struct StudyMappingRow {
    study_instance_uid: String,
    patient_id: String,
    patient_name: String,
    anon_patient_id: String,
    anon_patient_name: String,
    created_at: chrono::DateTime<Utc>,
}

impl From<StudyMappingRow> for StudyMapping {
    fn from(row: StudyMappingRow) -> Self {
        Self {
            study_instance_uid: row.study_instance_uid,
            patient_id: row.patient_id,
            patient_name: row.patient_name,
            anon_patient_id: row.anon_patient_id,
            anon_patient_name: row.anon_patient_name,
            created_at: row.created_at,
        }
    }
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

            let row = sqlx::query_as::<_, StudyMappingRow>(MAPPING_SELECT)
                .bind(study_instance_uid)
                .fetch_one(&self.pool)
                .await
                .context("failed to read back study mapping")?;
            Ok(row.into())
        })
    }

    fn find(&self, study_instance_uid: &str) -> anyhow::Result<Option<StudyMapping>> {
        self.runtime.block_on(async {
            let row = sqlx::query_as::<_, StudyMappingRow>(MAPPING_SELECT)
                .bind(study_instance_uid)
                .fetch_optional(&self.pool)
                .await
                .context("failed to look up study mapping")?;
            Ok(row.map(Into::into))
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

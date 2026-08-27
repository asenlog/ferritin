//! Per-study pseudonym mappings — the domain type and its port.
//!
//! `StudyMapping` ties the original patient identity of a study to
//! the pseudonym the de-identification step replaces it with; the
//! re-identification leg reads the same record to restore results.
//! Persistence lives in `db` (the `study_mappings` repository);
//! nothing here knows SQL exists.

use chrono::{DateTime, Utc};

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

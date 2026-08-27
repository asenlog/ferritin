//! Per-study pseudonym mappings — the domain type.
//!
//! `StudyMapping` ties the original patient identity of a study to
//! the pseudonym the de-identification step replaces it with; the
//! re-identification leg reads the same record to restore results.
//! The `MappingStore` port lives in `ports`; persistence lives in
//! `db` (the `study_mappings` repository); nothing here knows SQL
//! exists.

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

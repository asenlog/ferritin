//! Ports: every interface the application core depends on, in one module
//!
//! Implementations live at the edges (`db` repositories, `store` and
//! `ferritin-cloud` adapters) or in `fixtures` for tests. The domain
//! holds types only; this module holds traits only; nothing here has
//! a body beyond a signature.

use crate::app::models::mappings::StudyMapping;

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

/// Where the authorized-caller list is kept. Read fresh per
/// association so changes take effect without a restart.
pub trait CallerDirectory {
    fn authorized_callers(&self)
        -> anyhow::Result<Vec<crate::app::models::auth::AuthorizedCaller>>;
}

/// Where the forwarding-rule list is kept. Read fresh per result so
/// changes take effect without a restart.
pub trait RuleDirectory {
    fn forwarding_rules(&self) -> anyhow::Result<Vec<crate::app::models::rules::ForwardingRule>>;
}

/// Object persistence and retrieval (the fetch leg: results listener
/// → fetch → re-identification).
pub trait ObjectStore {
    /// Store `bytes` under `key`. Keys are `/`-separated relative paths
    /// (`{study}/{series}/{sop}.dcm`); implementations map them onto
    /// their native addressing (filesystem paths, S3 keys, ...).
    fn put(&self, key: &str, bytes: &[u8]) -> anyhow::Result<()>;

    /// Fetch the object stored under `key`. Fails if the key does not
    /// resolve or no object exists under it.
    fn get(&self, key: &str) -> anyhow::Result<Vec<u8>>;
}

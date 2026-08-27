//! Intake pipeline for received DICOM instances: parse the dataset,
//! filter it against the intake policy, de-identify it against the
//! per-study pseudonym mapping, wrap it into a proper Part-10 file,
//! and enqueue the upload through the `JobQueue` port.
//!
//! A C-STORE success means "durably queued", not "uploaded": the
//! upload worker (kind `Upload`) performs the object-store put with
//! retries, so a store outage or a crash never loses an accepted
//! instance.
//!
//! Knows nothing about sockets, PDUs, or DIMSE statuses — the SCP
//! adapter maps `IntakeError` onto wire statuses.

use crate::app::dicom::anonymize;
use crate::app::models::filter;
use crate::app::models::job::{JobKind, NewJob};
use crate::app::ports::{FilterDirectory, JobQueue, MappingStore};
use dicom_dictionary_std::tags;
use dicom_object::{FileMetaTableBuilder, InMemDicomObject};
use dicom_transfer_syntax_registry::{TransferSyntaxIndex, TransferSyntaxRegistry};

pub struct IntakeService<Q, M, F> {
    queue: Q,
    mappings: M,
    filter: F,
}

#[derive(Debug, thiserror::Error)]
pub enum IntakeError {
    #[error("unsupported transfer syntax: {0}")]
    UnsupportedTransferSyntax(String),

    #[error("malformed dataset: {0}")]
    MalformedDataset(String),

    #[error("rejected by intake filter: {0}")]
    Filtered(String),

    #[error("failed to resolve pseudonym mapping: {0}")]
    Mapping(String),

    #[error("failed to persist object: {0}")]
    Store(String),
}

impl<Q: JobQueue, M: MappingStore, F: FilterDirectory> IntakeService<Q, M, F> {
    pub fn new(queue: Q, mappings: M, filter: F) -> Self {
        Self {
            queue,
            mappings,
            filter,
        }
    }

    /// Parse a received dataset encoded in `ts_uid`, de-identify it,
    /// and queue the upload. Returns the storage key it will be
    /// written under.
    pub fn store_instance(&self, ts_uid: &str, dataset: &[u8]) -> Result<String, IntakeError> {
        let ts = TransferSyntaxRegistry
            .get(ts_uid)
            .ok_or_else(|| IntakeError::UnsupportedTransferSyntax(ts_uid.to_string()))?;

        let mut obj = InMemDicomObject::read_dataset_with_ts(dataset, ts)
            .map_err(|e| IntakeError::MalformedDataset(e.to_string()))?;

        let sop_instance_uid = uid_of(&obj, tags::SOP_INSTANCE_UID)
            .ok_or_else(|| IntakeError::MalformedDataset("missing SOP Instance UID".into()))?;
        let study_uid = uid_of(&obj, tags::STUDY_INSTANCE_UID).unwrap_or("unknown".into());
        let series_uid = uid_of(&obj, tags::SERIES_INSTANCE_UID).unwrap_or("unknown".into());

        // Policy first: reject before any mapping or storage work.
        // The policy is read fresh per instance so frontend edits
        // apply to the very next object. A policy we cannot load
        // fails closed — better to refuse than to let everything
        // through while the filter DB is down.
        let policy = self
            .filter
            .filter_policy()
            .map_err(|e| IntakeError::Filtered(format!("policy unavailable: {e}")))?;
        filter::evaluate(
            &policy,
            uid_of(&obj, tags::MODALITY).as_deref(),
            uid_of(&obj, tags::SOP_CLASS_UID).as_deref(),
            uid_of(&obj, tags::MANUFACTURER).as_deref(),
        )
        .map_err(|rejection| IntakeError::Filtered(rejection.to_string()))?;

        // De-identify before anything hits disk. A study without a UID
        // cannot be mapped; it is stored as-is (fail-open), matching
        // the rest of the intake's missing-metadata policy. A mapping
        // lookup that fails stops the store (fail-closed): identified
        // data must never reach the store because the mapping DB is
        // unavailable.
        if study_uid != "unknown" {
            let patient_id = uid_of(&obj, tags::PATIENT_ID).unwrap_or_default();
            let patient_name = uid_of(&obj, tags::PATIENT_NAME).unwrap_or_default();
            let mapping = self
                .mappings
                .mapping_for(&study_uid, &patient_id, &patient_name)
                .map_err(|e| IntakeError::Mapping(e.to_string()))?;
            anonymize::anonymize(&mut obj, &mapping);
        }

        // Over the wire only the dataset travels; rebuild the Part-10
        // file meta so the stored object is a valid .dcm file.
        // with_meta picks Media Storage SOP Class/Instance UIDs up from
        // the dataset itself.
        let file_obj = obj
            .with_meta(FileMetaTableBuilder::new().transfer_syntax(ts_uid))
            .map_err(|e| IntakeError::MalformedDataset(e.to_string()))?;

        let mut bytes = Vec::new();
        file_obj
            .write_all(&mut bytes)
            .map_err(|e| IntakeError::Store(e.to_string()))?;

        let key = format!("{study_uid}/{series_uid}/{sop_instance_uid}.dcm");
        self.queue
            .enqueue(NewJob {
                kind: JobKind::Upload,
                key: key.clone(),
                payload: bytes,
            })
            .map_err(|e| IntakeError::Store(e.to_string()))?;

        Ok(key)
    }
}

/// Read a UI element trimmed of the padding DICOM uses to reach even
/// length (NUL or space) — the raw value is unusable as a file key.
fn uid_of(obj: &InMemDicomObject, tag: dicom_core::Tag) -> Option<String> {
    let value = obj.element(tag).ok()?.to_str().ok()?;
    let trimmed = value.trim_end_matches(['\0', ' ']);
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::ports::ObjectStore;
    use crate::app::service::worker::QueueWorker;
    use crate::infra::db::{InMemoryJobQueue, InMemoryMappingStore};
    use crate::infra::store::FsObjectStore;
    use dicom_core::{dicom_value, DataElement, VR};
    use dicom_dictionary_std::uids;
    use dicom_transfer_syntax_registry::entries;
    use std::path::Path;

    const CT_IMAGE_STORAGE: &str = uids::CT_IMAGE_STORAGE;

    fn dataset_bytes() -> Vec<u8> {
        let obj = InMemDicomObject::from_element_iter([
            DataElement::new(
                tags::SOP_CLASS_UID,
                VR::UI,
                dicom_value!(Str, CT_IMAGE_STORAGE),
            ),
            DataElement::new(
                tags::SOP_INSTANCE_UID,
                VR::UI,
                dicom_value!(Str, "1.2.3.4.5.6.7.8.9"),
            ),
            DataElement::new(
                tags::STUDY_INSTANCE_UID,
                VR::UI,
                dicom_value!(Str, "1.2.3.4.5"),
            ),
            DataElement::new(
                tags::SERIES_INSTANCE_UID,
                VR::UI,
                dicom_value!(Str, "1.2.3.4.5.6"),
            ),
            DataElement::new(tags::MODALITY, VR::CS, dicom_value!(Str, "CT")),
            DataElement::new(tags::PATIENT_NAME, VR::PN, dicom_value!(Str, "Doe^John")),
            DataElement::new(tags::PATIENT_ID, VR::LO, dicom_value!(Str, "PAT-1")),
        ]);
        let ts = entries::IMPLICIT_VR_LITTLE_ENDIAN.erased();
        let mut bytes = Vec::new();
        obj.write_dataset_with_ts(&mut bytes, &ts).unwrap();
        bytes
    }

    struct StaticFilter(filter::FilterPolicy);

    impl StaticFilter {
        fn allow_all() -> Self {
            Self(filter::FilterPolicy::default())
        }
    }

    impl FilterDirectory for StaticFilter {
        fn filter_policy(&self) -> anyhow::Result<filter::FilterPolicy> {
            Ok(self.0.clone())
        }
    }

    /// An intake with a clonable in-memory queue, so tests can keep a
    /// handle and drain the upload leg themselves.
    fn rig() -> (
        IntakeService<InMemoryJobQueue, InMemoryMappingStore, StaticFilter>,
        InMemoryJobQueue,
    ) {
        let queue = InMemoryJobQueue::default();
        let intake = IntakeService::new(
            queue.clone(),
            InMemoryMappingStore::default(),
            StaticFilter::allow_all(),
        );
        (intake, queue)
    }

    /// Drive upload rounds against the filesystem until the queue is
    /// empty — what the production worker does on its thread.
    fn drain_uploads(queue: &InMemoryJobQueue, dir: &Path) {
        let store = FsObjectStore::new(dir);
        let worker = QueueWorker::new(queue.clone(), JobKind::Upload);
        while worker
            .tick(|job| store.put(&job.key, &job.payload))
            .unwrap()
        {}
    }

    #[test]
    fn stores_instance_under_deterministic_key() {
        let dir = tempfile::tempdir().unwrap();
        let (intake, queue) = rig();

        let key = intake
            .store_instance(uids::IMPLICIT_VR_LITTLE_ENDIAN, &dataset_bytes())
            .unwrap();

        assert_eq!(key, "1.2.3.4.5/1.2.3.4.5.6/1.2.3.4.5.6.7.8.9.dcm");
        // durably queued, not yet on disk
        assert!(!dir.path().join(&key).exists());

        drain_uploads(&queue, dir.path());
        let written = dir.path().join(&key);
        assert!(written.exists());
        let bytes = std::fs::read(&written).unwrap();
        assert_eq!(&bytes[128..132], b"DICM");
    }

    #[test]
    fn stored_instance_is_deidentified() {
        let dir = tempfile::tempdir().unwrap();
        let (intake, queue) = rig();

        let key = intake
            .store_instance(uids::IMPLICIT_VR_LITTLE_ENDIAN, &dataset_bytes())
            .unwrap();
        drain_uploads(&queue, dir.path());

        let stored = dicom_object::open_file(dir.path().join(&key)).unwrap();
        let patient_name = stored
            .element(tags::PATIENT_NAME)
            .unwrap()
            .to_str()
            .unwrap();
        let patient_id = stored.element(tags::PATIENT_ID).unwrap().to_str().unwrap();
        assert!(patient_name.starts_with("ANON^"), "got {patient_name}");
        assert!(patient_id.starts_with("ANON-"), "got {patient_id}");
    }

    #[test]
    fn unknown_transfer_syntax_is_rejected() {
        let (intake, _queue) = rig();

        let err = intake
            .store_instance("9.9.9", &dataset_bytes())
            .unwrap_err();
        assert!(matches!(err, IntakeError::UnsupportedTransferSyntax(_)));
    }

    #[test]
    fn garbage_dataset_is_rejected() {
        let (intake, _queue) = rig();

        let err = intake
            .store_instance(uids::IMPLICIT_VR_LITTLE_ENDIAN, b"not a dicom dataset")
            .unwrap_err();
        assert!(matches!(err, IntakeError::MalformedDataset(_)));
    }

    #[test]
    fn policy_rejection_stops_the_store() {
        let queue = InMemoryJobQueue::default();
        let intake = IntakeService::new(
            queue.clone(),
            InMemoryMappingStore::default(),
            StaticFilter(filter::FilterPolicy {
                // the fixture dataset is CT — MG-only policy rejects it
                allow_modalities: vec![crate::app::models::modality::ModalityType::MG],
                ..Default::default()
            }),
        );

        let err = intake
            .store_instance(uids::IMPLICIT_VR_LITTLE_ENDIAN, &dataset_bytes())
            .unwrap_err();

        assert!(matches!(err, IntakeError::Filtered(_)));
        // nothing was queued
        assert!(queue.claim(JobKind::Upload).unwrap().is_none());
    }
}

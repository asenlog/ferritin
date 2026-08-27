//! Inbound results pipeline: parse a fetched result object, restore
//! its original identity, resolve its destination, and forward it.
//!
//! Fed by the results-queue listener; knows nothing about SQS or S3.
//! A failure here leaves the queue message in place (the listener
//! deletes only on success), so results are never lost silently.

use crate::anonymize::deanonymize;
use crate::db::MappingStore;
use crate::rules::{self, RuleDirectory};
use crate::scu::ScuClient;
use dicom_dictionary_std::tags;
use dicom_object::InMemDicomObject;

pub struct ForwardingService<M, R> {
    mappings: M,
    rules: R,
    scu: ScuClient,
}

#[derive(Debug, thiserror::Error)]
pub enum ForwardError {
    #[error("malformed result object: {0}")]
    MalformedResult(String),

    #[error("no pseudonym mapping for study {0}")]
    UnknownStudy(String),

    #[error("no forwarding rule for {modality} / {sop_class_uid}")]
    NoRoute { modality: String, sop_class_uid: String },

    #[error("mapping lookup failed: {0}")]
    Mapping(String),

    #[error("failed to load forwarding rules: {0}")]
    Rules(String),

    #[error("forward to destination failed: {0}")]
    Forward(String),
}

impl<M: MappingStore, R: RuleDirectory> ForwardingService<M, R> {
    pub fn new(mappings: M, rules: R, scu: ScuClient) -> Self {
        Self {
            mappings,
            rules,
            scu,
        }
    }

    /// Re-identify a fetched result object (a Part-10 file, as stored
    /// by the intake leg) and C-STORE it to its resolved destination.
    pub fn forward_result(&self, bytes: &[u8]) -> Result<(), ForwardError> {
        let mut obj = dicom_object::from_reader(bytes)
            .map_err(|e| ForwardError::MalformedResult(e.to_string()))?;

        let study_uid = text_of(&obj, tags::STUDY_INSTANCE_UID)
            .ok_or_else(|| ForwardError::MalformedResult("missing Study Instance UID".into()))?;
        let mapping = self
            .mappings
            .find(&study_uid)
            .map_err(|e| ForwardError::Mapping(e.to_string()))?
            .ok_or_else(|| ForwardError::UnknownStudy(study_uid.clone()))?;
        deanonymize(&mut obj, &mapping);

        let modality = text_of(&obj, tags::MODALITY).unwrap_or_default();
        let sop_class_uid = text_of(&obj, tags::SOP_CLASS_UID)
            .ok_or_else(|| ForwardError::MalformedResult("missing SOP Class UID".into()))?;
        // rules are read fresh per result so directory changes (e.g.
        // from the frontend) apply without a restart
        let rules = self
            .rules
            .forwarding_rules()
            .map_err(|e| ForwardError::Rules(e.to_string()))?;
        let dest = rules::resolve(&rules, &modality, &sop_class_uid).ok_or_else(|| {
            ForwardError::NoRoute {
                modality: modality.clone(),
                sop_class_uid: sop_class_uid.clone(),
            }
        })?;

        self.scu
            .cstore(&dest, &obj)
            .map_err(|e| ForwardError::Forward(e.to_string()))
    }
}

/// Read an element as plain text, trimmed of DICOM padding.
fn text_of(obj: &InMemDicomObject, tag: dicom_core::Tag) -> Option<String> {
    let value = obj.element(tag).ok()?.to_str().ok()?;
    let trimmed = value.trim_end_matches(['\0', ' ']);
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

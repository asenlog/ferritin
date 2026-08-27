//! Per-study de-identification: a Replace/Keep tag transform driven
//! by the study's `StudyMapping`.
//!
//! Tags in the replace set take the study pseudonym; tags in the
//! empty set are blanked; everything else is kept untouched. UIDs are
//! deliberately kept — the study must stay linkable so processed
//! results can be matched back for re-identification. Full PS3.15
//! Annex E profile coverage lands with the hardening phase.

use crate::db::StudyMapping;
use dicom_core::{dicom_value, DataElement, Tag};
use dicom_dictionary_std::tags;
use dicom_object::InMemDicomObject;

/// Direct identifiers blanked outright rather than pseudonymized.
const EMPTY_TAGS: &[Tag] = &[
    tags::PATIENT_BIRTH_DATE,
    tags::PATIENT_ADDRESS,
    tags::PATIENT_TELEPHONE_NUMBERS,
    tags::REFERRING_PHYSICIAN_NAME,
];

/// Apply the Replace/Keep transform in place. Elements that are
/// absent stay absent — anonymization never fabricates metadata.
pub fn anonymize(obj: &mut InMemDicomObject, mapping: &StudyMapping) {
    if let Ok(elem) = obj.element(tags::PATIENT_NAME) {
        let vr = elem.vr();
        obj.put(DataElement::new(
            tags::PATIENT_NAME,
            vr,
            dicom_value!(Str, mapping.anon_patient_name.as_str()),
        ));
    }
    if let Ok(elem) = obj.element(tags::PATIENT_ID) {
        let vr = elem.vr();
        obj.put(DataElement::new(
            tags::PATIENT_ID,
            vr,
            dicom_value!(Str, mapping.anon_patient_id.as_str()),
        ));
    }
    for &tag in EMPTY_TAGS {
        if let Ok(elem) = obj.element(tag) {
            let vr = elem.vr();
            obj.put(DataElement::new(tag, vr, dicom_value!(Str, "")));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dicom_core::VR;

    fn mapping() -> StudyMapping {
        StudyMapping {
            study_instance_uid: "1.2.3".to_string(),
            patient_id: "PAT-1".to_string(),
            patient_name: "Doe^John".to_string(),
            anon_patient_id: "ANON-abcdef123456".to_string(),
            anon_patient_name: "ANON^ABCDEF12".to_string(),
            created_at: chrono::Utc::now(),
        }
    }

    fn identified_obj() -> InMemDicomObject {
        InMemDicomObject::from_element_iter([
            DataElement::new(tags::PATIENT_NAME, VR::PN, dicom_value!(Str, "Doe^John")),
            DataElement::new(tags::PATIENT_ID, VR::LO, dicom_value!(Str, "PAT-1")),
            DataElement::new(
                tags::PATIENT_BIRTH_DATE,
                VR::DA,
                dicom_value!(Str, "19700101"),
            ),
            DataElement::new(
                tags::REFERRING_PHYSICIAN_NAME,
                VR::PN,
                dicom_value!(Str, "Smith^Alice"),
            ),
            DataElement::new(tags::MODALITY, VR::CS, dicom_value!(Str, "CT")),
        ])
    }

    fn text_of(obj: &InMemDicomObject, tag: Tag) -> String {
        obj.element(tag).unwrap().to_str().unwrap().to_string()
    }

    #[test]
    fn patient_identity_is_replaced_with_pseudonym() {
        let mut obj = identified_obj();
        anonymize(&mut obj, &mapping());

        assert_eq!(text_of(&obj, tags::PATIENT_NAME), "ANON^ABCDEF12");
        assert_eq!(text_of(&obj, tags::PATIENT_ID), "ANON-abcdef123456");
    }

    #[test]
    fn direct_identifiers_are_blanked() {
        let mut obj = identified_obj();
        anonymize(&mut obj, &mapping());

        assert_eq!(text_of(&obj, tags::PATIENT_BIRTH_DATE), "");
        assert_eq!(text_of(&obj, tags::REFERRING_PHYSICIAN_NAME), "");
    }

    #[test]
    fn other_tags_are_kept() {
        let mut obj = identified_obj();
        anonymize(&mut obj, &mapping());

        assert_eq!(text_of(&obj, tags::MODALITY), "CT");
    }

    #[test]
    fn absent_elements_stay_absent() {
        let mut obj = InMemDicomObject::from_element_iter([DataElement::new(
            tags::MODALITY,
            VR::CS,
            dicom_value!(Str, "CT"),
        )]);
        anonymize(&mut obj, &mapping());

        assert!(obj.element(tags::PATIENT_NAME).is_err());
        assert!(obj.element(tags::PATIENT_BIRTH_DATE).is_err());
    }
}

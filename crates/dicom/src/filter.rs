use crate::models::{self};
use serde::{Deserialize};

#[derive(Debug, Clone, Deserialize)]
pub struct SopClassFilter {
    allowed_modality: models::ModalityType,
    allowed_sop_classes: Vec<String>,
    vendor_blocklist: Vec<String>,
}
impl SopClassFilter {
    pub fn new(
        allowed_modality: models::ModalityType,
        allowed_sop_classes: Vec<String>,
        vendor_blocklist: Vec<String>,
    ) -> Self {
        Self {
            allowed_modality,
            allowed_sop_classes,
            vendor_blocklist,
        }
    }
}

#[derive(Debug, Clone)]
pub struct InstanceInfo {
    modality: models::ModalityType,
    sop_class_uid: String,
    manufacturer: String,
}

pub fn validate(filter: &SopClassFilter, info: &InstanceInfo) -> bool {
    if info.modality != filter.allowed_modality {
        return false;
    }

    if !info.sop_class_uid.is_empty() && !filter.allowed_sop_classes.contains(&info.sop_class_uid) {
            return false;
    }

    if !info.manufacturer.is_empty() {
        let manufacturer = info.manufacturer.to_lowercase();
        let is_blocked = filter
            .vendor_blocklist
            .iter()
            .any(|blocked_vendor| manufacturer.contains(&blocked_vendor.to_lowercase()));

        if is_blocked {
            return false;
        }
    }

    true
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn matching_mammo_instance_passes_the_filter() {
        let filter_config = SopClassFilter {
            allowed_modality: models::ModalityType::MG,
            allowed_sop_classes: vec!["1.2.840.10008.5.1.4.1.1.1.2".to_string()],
            vendor_blocklist: vec!["lunit".to_string(), "screenpoint".to_string()],
        };
        let info = InstanceInfo {
            modality: models::ModalityType::MG,
            sop_class_uid: "1.2.840.10008.5.1.4.1.1.1.2".to_string(),
            manufacturer: "FUJIFILM Corporation".to_string(),
        };

        assert!(validate(&filter_config, &info));
    }

    /// Two allowed SOP classes on purpose — mammo 2D + tomosynthesis —
    /// so tests can prove the WHOLE list is honored, not just entry [0].
    fn mg_filter() -> SopClassFilter {
        SopClassFilter {
            allowed_modality: models::ModalityType::MG,
            allowed_sop_classes: vec![
                "1.2.840.10008.5.1.4.1.1.1.2".to_string(),
                "1.2.840.10008.5.1.4.1.1.13.1.3".to_string(),
            ],
            vendor_blocklist: vec!["lunit".to_string(), "screenpoint".to_string()],
        }
    }

    fn mg_info(
        modality: models::ModalityType,
        sop_class_uid: &str,
        manufacturer: &str,
    ) -> InstanceInfo {
        InstanceInfo {
            modality,
            sop_class_uid: sop_class_uid.to_string(),
            manufacturer: manufacturer.to_string(),
        }
    }

    #[test]
    fn allowed_sop_class_beyond_the_first_passes() {
        let info = mg_info(
            models::ModalityType::MG,
            "1.2.840.10008.5.1.4.1.1.13.1.3",
            "Hologic",
        );
        assert!(validate(&mg_filter(), &info));
    }

    #[test]
    fn empty_metadata_fails_open() {
        let info = mg_info(models::ModalityType::MG, "", "");
        assert!(validate(&mg_filter(), &info));
    }

    #[test]
    fn wrong_modality_is_rejected() {
        let info = mg_info(
            models::ModalityType::CT,
            "1.2.840.10008.5.1.4.1.1.1.2",
            "FUJIFILM Corporation",
        );
        assert!(!validate(&mg_filter(), &info));
    }

    #[test]
    fn disallowed_sop_class_is_rejected() {
        // Secondary Capture — the SOP class AI vendors stamp on result objects
        let info = mg_info(
            models::ModalityType::MG,
            "1.2.840.10008.5.1.4.1.1.7",
            "FUJIFILM Corporation",
        );
        assert!(!validate(&mg_filter(), &info));
    }

    #[test]
    fn blocklisted_vendor_is_rejected() {
        let info = mg_info(
            models::ModalityType::MG,
            "1.2.840.10008.5.1.4.1.1.1.2",
            "Lunit Inc.",
        );
        assert!(!validate(&mg_filter(), &info));
    }
}

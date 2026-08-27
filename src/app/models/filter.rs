//! Intake filtering: which studies this node accepts, by modality,
//! SOP class, and vendor — the policy types and the verdict logic.
//! The `FilterDirectory` port lives in `ports`; the policy itself is
//! user-managed data served from the database.

use crate::app::models::modality::ModalityType;

/// The intake policy: what to let through. An empty allowlist means
/// that dimension is unfiltered (allow all) — an unset list must
/// never silently block traffic.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct FilterPolicy {
    pub allow_modalities: Vec<ModalityType>,
    pub allow_sop_classes: Vec<String>,
    pub block_vendors: Vec<String>,
}

/// Why an instance was rejected. The reason string is for logs; the
/// SCP maps the rejection onto a DIMSE refusal status.
#[derive(Debug, Clone, PartialEq)]
pub struct FilterRejection(pub String);

impl std::fmt::Display for FilterRejection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// Evaluate an instance against the policy. Missing metadata fails
/// open — a study without a Modality, SOP Class, or Manufacturer tag
/// is let through rather than dropped. An explicit vendor block
/// always wins over the allowlists.
pub fn evaluate(
    policy: &FilterPolicy,
    modality: Option<&str>,
    sop_class_uid: Option<&str>,
    manufacturer: Option<&str>,
) -> Result<(), FilterRejection> {
    if let Some(vendor) = manufacturer {
        if policy.block_vendors.iter().any(|blocked| blocked == vendor) {
            return Err(FilterRejection(format!("vendor {vendor} is blocklisted")));
        }
    }
    if let Some(modality) = modality {
        if !policy.allow_modalities.is_empty()
            && !policy
                .allow_modalities
                .contains(&ModalityType::from(modality.to_string()))
        {
            return Err(FilterRejection(format!(
                "modality {modality} is not allowlisted"
            )));
        }
    }
    if let Some(sop_class) = sop_class_uid {
        if !policy.allow_sop_classes.is_empty()
            && !policy.allow_sop_classes.iter().any(|uid| uid == sop_class)
        {
            return Err(FilterRejection(format!(
                "SOP class {sop_class} is not allowlisted"
            )));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const CT: &str = "1.2.840.10008.5.1.4.1.1.2";
    const TOMO: &str = "1.2.840.10008.5.1.4.1.1.13.1.3";

    fn policy() -> FilterPolicy {
        FilterPolicy {
            allow_modalities: vec![ModalityType::MG],
            allow_sop_classes: vec![TOMO.to_string()],
            block_vendors: vec!["EvilCorp".to_string()],
        }
    }

    #[test]
    fn empty_policy_allows_everything() {
        let empty = FilterPolicy::default();
        assert!(evaluate(&empty, Some("CT"), Some(CT), Some("EvilCorp")).is_ok());
        assert!(evaluate(&empty, None, None, None).is_ok());
    }

    #[test]
    fn matching_policy_allows() {
        assert!(evaluate(&policy(), Some("MG"), Some(TOMO), Some("Canon")).is_ok());
    }

    #[test]
    fn unlisted_modality_is_rejected() {
        let err = evaluate(&policy(), Some("CT"), Some(TOMO), Some("Canon")).unwrap_err();
        assert!(err.0.contains("modality CT"));
    }

    #[test]
    fn unlisted_sop_class_is_rejected() {
        let err = evaluate(&policy(), Some("MG"), Some(CT), Some("Canon")).unwrap_err();
        assert!(err.0.contains("SOP class"));
    }

    #[test]
    fn blocklisted_vendor_is_rejected() {
        let err = evaluate(&policy(), Some("MG"), Some(TOMO), Some("EvilCorp")).unwrap_err();
        assert!(err.0.contains("EvilCorp"));
    }

    #[test]
    fn missing_metadata_fails_open() {
        assert!(evaluate(&policy(), None, None, None).is_ok());
        assert!(evaluate(&policy(), None, Some(TOMO), None).is_ok());
        assert!(evaluate(&policy(), Some("MG"), None, None).is_ok());
    }
}

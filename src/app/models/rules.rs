//! Forwarding rules: which destination AE a re-identified result is
//! routed to, parsed from the `DICOM_RULES` env config.
//!
//! Rule format: `{MODALITY} - {SOP_CLASS_UID} - {AE}@{HOST}:{PORT}`,
//! e.g. `MG - 1.2.840.10008.5.1.4.1.1.13.1.3 - PACS@192.168.1.10:104`.
//! These move into the database with the frontend-managed tables work.

use crate::app::models::modality::ModalityType;
use anyhow::{anyhow, Context};
use std::str::FromStr;

/// A DICOM node results are forwarded to.
#[derive(Debug, Clone, PartialEq)]
pub struct Destination {
    pub ae_title: String,
    pub host: String,
    pub port: u16,
}

/// One routing entry: studies of this modality and SOP class go to
/// this destination.
#[derive(Debug, Clone, PartialEq)]
pub struct ForwardingRule {
    pub modality: ModalityType,
    pub sop_class_uid: String,
    pub destination: Destination,
}

impl FromStr for ForwardingRule {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> anyhow::Result<Self> {
        let parts: Vec<&str> = s.split(" - ").map(str::trim).collect();
        let [modality, sop_class_uid, destination] = parts.as_slice() else {
            return Err(anyhow!(
                "expected MODALITY - SOP_CLASS_UID - AE@HOST:PORT, got {s:?}"
            ));
        };
        if sop_class_uid.is_empty() {
            return Err(anyhow!("empty SOP class UID in {s:?}"));
        }

        let (ae_title, address) = destination
            .split_once('@')
            .with_context(|| format!("expected AE@HOST:PORT in {s:?}"))?;
        let (host, port) = address
            .rsplit_once(':')
            .with_context(|| format!("missing port in {s:?}"))?;
        if ae_title.is_empty() || host.is_empty() {
            return Err(anyhow!("empty AE title or host in {s:?}"));
        }

        Ok(Self {
            modality: ModalityType::from(modality.to_string()),
            sop_class_uid: sop_class_uid.to_string(),
            destination: Destination {
                ae_title: ae_title.to_string(),
                host: host.to_string(),
                port: port
                    .parse()
                    .with_context(|| format!("invalid port in {s:?}"))?,
            },
        })
    }
}

/// Resolve the destination for a study: first matching rule wins.
pub fn resolve(
    rules: &[ForwardingRule],
    modality: &str,
    sop_class_uid: &str,
) -> Option<Destination> {
    let modality = ModalityType::from(modality.to_string());
    rules
        .iter()
        .find(|rule| rule.modality == modality && rule.sop_class_uid == sop_class_uid)
        .map(|rule| rule.destination.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    const TOMO: &str = "1.2.840.10008.5.1.4.1.1.13.1.3";
    const CT: &str = "1.2.840.10008.5.1.4.1.1.2";

    fn rules() -> Vec<ForwardingRule> {
        vec![
            format!("MG - {TOMO} - PACS@192.168.1.10:104")
                .parse()
                .unwrap(),
            format!("CT - {CT} - BACKUP@10.0.0.2:11112")
                .parse()
                .unwrap(),
        ]
    }

    #[test]
    fn parses_a_rule() {
        let rule = format!("MG - {TOMO} - PACS@192.168.1.10:104")
            .parse::<ForwardingRule>()
            .unwrap();

        assert_eq!(rule.modality, ModalityType::MG);
        assert_eq!(rule.sop_class_uid, TOMO);
        assert_eq!(
            rule.destination,
            Destination {
                ae_title: "PACS".to_string(),
                host: "192.168.1.10".to_string(),
                port: 104,
            }
        );
    }

    #[test]
    fn rejects_malformed_rules() {
        for bad in [
            "",
            "MG",
            "MG - PACS@192.168.1.10:104",
            "MG -  - PACS@192.168.1.10:104",
            "MG - 1.2.3 - NOPORT@192.168.1.10",
            "MG - 1.2.3 - @192.168.1.10:104",
            "MG - 1.2.3 - PACS@192.168.1.10:notaport",
        ] {
            assert!(
                bad.parse::<ForwardingRule>().is_err(),
                "expected rejection: {bad:?}"
            );
        }
    }

    #[test]
    fn resolves_first_matching_rule() {
        let rules = rules();

        let dest = resolve(&rules, "MG", TOMO).unwrap();
        assert_eq!(dest.ae_title, "PACS");
        let dest = resolve(&rules, "CT", CT).unwrap();
        assert_eq!(dest.host, "10.0.0.2");
    }

    #[test]
    fn unknown_combinations_resolve_to_nothing() {
        let rules = rules();

        assert!(resolve(&rules, "MR", TOMO).is_none());
        assert!(resolve(&rules, "MG", CT).is_none());
    }
}

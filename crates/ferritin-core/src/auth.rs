//! Node authorization: which remote DICOM nodes may open an association.
//!
//! Pure matching logic plus the dicom-ul `AccessControl` adapter — the
//! SCP builds one per accepted TCP connection, so the peer address is
//! known by the time the association request arrives. Unit-testable
//! without sockets.

use anyhow::{anyhow, Context};
use dicom_ul::association::server::AccessControl;
use dicom_ul::pdu::{AssociationRJServiceUserReason, UserIdentity};
use ipnet::IpNet;
use std::net::IpAddr;
use std::str::FromStr;

/// A remote node allowed to push to this SCP: an AE title that must be
/// presented from within a network range (`CT_SCANNER@10.0.0.5` or
/// `ORTHANC@192.168.1.0/24`). A bare IP means exactly that host.
#[derive(Debug, Clone, PartialEq)]
pub struct AuthorizedCaller {
    pub ae_title: String,
    pub network: IpNet,
}

impl FromStr for AuthorizedCaller {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> anyhow::Result<Self> {
        let (ae_title, network) = s
            .split_once('@')
            .with_context(|| format!("expected AE@NETWORK, got {s:?}"))?;
        let ae_title = ae_title.trim();
        if ae_title.is_empty() {
            return Err(anyhow!("empty AE title in {s:?}"));
        }
        let network = network.trim();
        let network = match network.parse::<IpNet>() {
            Ok(net) => net,
            Err(_) => network
                .parse::<IpAddr>()
                .map(IpNet::from)
                .with_context(|| format!("invalid network {network:?} in {s:?}"))?,
        };
        Ok(Self {
            ae_title: ae_title.to_string(),
            network,
        })
    }
}

/// Where the authorized-caller list is kept. The Postgres adapter
/// serves the server; the `Vec` impl serves tests and fixtures.
pub trait CallerDirectory {
    /// Snapshot of the currently authorized callers. Read fresh per
    /// association so changes take effect without a restart.
    fn authorized_callers(&self) -> anyhow::Result<Vec<AuthorizedCaller>>;
}

impl CallerDirectory for Vec<AuthorizedCaller> {
    fn authorized_callers(&self) -> anyhow::Result<Vec<AuthorizedCaller>> {
        Ok(self.clone())
    }
}

/// dicom-ul access-control policy enforcing the configured callers.
/// Rejects at the association level (A-ASSOCIATE-RJ), before any
/// DIMSE traffic: a wrong called AE, an unknown calling AE, or a
/// known AE calling from outside its network all get turned away.
pub struct NodeAccessControl<'a> {
    peer_addr: IpAddr,
    callers: &'a [AuthorizedCaller],
}

impl<'a> NodeAccessControl<'a> {
    pub fn new(peer_addr: IpAddr, callers: &'a [AuthorizedCaller]) -> Self {
        Self {
            peer_addr,
            callers,
        }
    }
}

/// AE titles arrive space-padded to 16 chars; compare trimmed.
fn ae_eq(a: &str, b: &str) -> bool {
    a.trim_end() == b.trim_end()
}

impl AccessControl for NodeAccessControl<'_> {
    fn check_access(
        &self,
        this_ae_title: &str,
        calling_ae_title: &str,
        called_ae_title: &str,
        _user_identity: Option<&UserIdentity>,
    ) -> Result<(), AssociationRJServiceUserReason> {
        if !ae_eq(called_ae_title, this_ae_title) {
            return Err(AssociationRJServiceUserReason::CalledAETitleNotRecognized);
        }

        let authorized = self.callers.iter().any(|caller| {
            ae_eq(&caller.ae_title, calling_ae_title) && caller.network.contains(&self.peer_addr)
        });
        if !authorized {
            return Err(AssociationRJServiceUserReason::CallingAETitleNotRecognized);
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn callers() -> Vec<AuthorizedCaller> {
        vec![
            "CT_SCANNER@10.0.0.5".parse().unwrap(),
            "ORTHANC@192.168.1.0/24".parse().unwrap(),
        ]
    }

    fn check(callers: &[AuthorizedCaller], peer: &str, calling: &str, called: &str) -> bool {
        NodeAccessControl::new(peer.parse().unwrap(), callers)
            .check_access("SYN_PROXY", calling, called, None)
            .is_ok()
    }

    #[test]
    fn parses_host_and_cidr_callers() {
        let host = "CT_SCANNER@10.0.0.5".parse::<AuthorizedCaller>().unwrap();
        assert_eq!(host.ae_title, "CT_SCANNER");
        assert_eq!(host.network, "10.0.0.5/32".parse::<IpNet>().unwrap());

        let cidr = "ORTHANC@192.168.1.0/24".parse::<AuthorizedCaller>().unwrap();
        assert_eq!(cidr.network, "192.168.1.0/24".parse::<IpNet>().unwrap());
    }

    #[test]
    fn rejects_malformed_entries() {
        assert!("NO_NETWORK".parse::<AuthorizedCaller>().is_err());
        assert!("@10.0.0.5".parse::<AuthorizedCaller>().is_err());
        assert!("AE@999.1.1.1".parse::<AuthorizedCaller>().is_err());
    }

    #[test]
    fn accepts_authorized_caller_from_its_network() {
        assert!(check(&callers(), "10.0.0.5", "CT_SCANNER", "SYN_PROXY"));
        assert!(check(&callers(), "192.168.1.77", "ORTHANC", "SYN_PROXY"));
        // space-padded AE titles, as they arrive on the wire
        assert!(check(&callers(), "10.0.0.5", "CT_SCANNER      ", "SYN_PROXY"));
    }

    #[test]
    fn rejects_known_ae_from_foreign_network() {
        assert!(!check(&callers(), "10.0.0.6", "CT_SCANNER", "SYN_PROXY"));
        assert!(!check(&callers(), "192.168.2.77", "ORTHANC", "SYN_PROXY"));
    }

    #[test]
    fn rejects_unknown_calling_ae() {
        assert!(!check(&callers(), "10.0.0.5", "STRANGER", "SYN_PROXY"));
    }

    #[test]
    fn rejects_wrong_called_ae() {
        assert!(!check(&callers(), "10.0.0.5", "CT_SCANNER", "SOMEONE_ELSE"));
    }

    #[test]
    fn empty_caller_list_rejects_everyone() {
        assert!(!check(&[], "127.0.0.1", "CT_SCANNER", "SYN_PROXY"));
    }
}

//! Node authorization vocabulary: the `AuthorizedCaller` type.
//! The `CallerDirectory` port lives in `ports`; the dicom-ul access
//! control adapter lives in `scp`.

use anyhow::{anyhow, Context};
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_host_and_cidr_callers() {
        let host = "CT_SCANNER@10.0.0.5".parse::<AuthorizedCaller>().unwrap();
        assert_eq!(host.ae_title, "CT_SCANNER");
        assert_eq!(host.network, "10.0.0.5/32".parse::<IpNet>().unwrap());

        let cidr = "ORTHANC@192.168.1.0/24"
            .parse::<AuthorizedCaller>()
            .unwrap();
        assert_eq!(cidr.network, "192.168.1.0/24".parse::<IpNet>().unwrap());
    }

    #[test]
    fn rejects_malformed_entries() {
        assert!("NO_NETWORK".parse::<AuthorizedCaller>().is_err());
        assert!("@10.0.0.5".parse::<AuthorizedCaller>().is_err());
        assert!("AE@999.1.1.1".parse::<AuthorizedCaller>().is_err());
    }
}

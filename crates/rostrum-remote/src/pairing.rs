//! Pairing: the one-time offer the desktop shows, and what it is exchanged for.
//!
//! The desktop page renders a [`PairingOffer`] as a `rostrum://pair?…` link and
//! as a QR code of the same link. Opening it on the phone — by tapping it in
//! the phone's browser, or by scanning the QR with the camera app — hands the
//! offer to the app, which posts the code in a [`PairRequest`] over a
//! connection pinned to the offer's fingerprint. The [`PairResponse`] carries
//! the device token every later request presents and, when the desktop has
//! one, a GitHub token so the phone needs no sign-in of its own.

use std::fmt;

use serde::{Deserialize, Serialize};
use url::Url;

use crate::{
    api::MachineInfo,
    code::{PairingCode, PairingCodeError},
    fingerprint::{CertFingerprint, FingerprintError},
    host::{Host, HostError},
    secret::{DeviceId, DeviceToken, GitHubToken},
};

/// The version of the link format, independent of [`crate::API_VERSION`].
const URI_VERSION: u32 = 1;
const URI_SCHEME: &str = "rostrum";
const URI_HOST: &str = "pair";

/// Where the desktop's API can be reached, and the certificate it must present.
///
/// Always at least one host; construction is the only way to get one.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "EndpointWire", into = "EndpointWire")]
pub struct Endpoint {
    hosts: Vec<Host>,
    port: u16,
    fingerprint: CertFingerprint,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum EndpointError {
    #[error("an endpoint needs at least one host")]
    NoHosts,
    #[error("port 0 is not a port")]
    Port,
}

impl Endpoint {
    pub fn new(
        hosts: Vec<Host>,
        port: u16,
        fingerprint: CertFingerprint,
    ) -> Result<Self, EndpointError> {
        if hosts.is_empty() {
            return Err(EndpointError::NoHosts);
        }
        if port == 0 {
            return Err(EndpointError::Port);
        }
        let mut unique = Vec::with_capacity(hosts.len());
        for host in hosts {
            if !unique.contains(&host) {
                unique.push(host);
            }
        }
        Ok(Self {
            hosts: unique,
            port,
            fingerprint,
        })
    }

    /// In the order they should be tried. Never empty.
    pub fn hosts(&self) -> &[Host] {
        &self.hosts
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    pub fn fingerprint(&self) -> CertFingerprint {
        self.fingerprint
    }
}

#[derive(Serialize, Deserialize)]
struct EndpointWire {
    hosts: Vec<Host>,
    port: u16,
    fingerprint: CertFingerprint,
}

impl TryFrom<EndpointWire> for Endpoint {
    type Error = EndpointError;

    fn try_from(wire: EndpointWire) -> Result<Self, Self::Error> {
        Self::new(wire.hosts, wire.port, wire.fingerprint)
    }
}

impl From<Endpoint> for EndpointWire {
    fn from(endpoint: Endpoint) -> Self {
        Self {
            hosts: endpoint.hosts,
            port: endpoint.port,
            fingerprint: endpoint.fingerprint,
        }
    }
}

/// Everything a phone needs to pair: whom, where, and the one-time code.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PairingOffer {
    pub machine: String,
    pub endpoint: Endpoint,
    pub code: PairingCode,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PairingUriError {
    #[error("not a link: {0}")]
    NotAUri(String),
    #[error("not a rostrum pairing link")]
    WrongKind,
    #[error("this pairing link is version {0}; update Rostrum to use it")]
    UnsupportedVersion(u32),
    #[error("the pairing link is missing `{0}`")]
    Missing(&'static str),
    #[error("the pairing link's port is not a number")]
    Port,
    #[error(transparent)]
    Host(#[from] HostError),
    #[error(transparent)]
    Code(#[from] PairingCodeError),
    #[error(transparent)]
    Fingerprint(#[from] FingerprintError),
    #[error(transparent)]
    Endpoint(#[from] EndpointError),
}

impl PairingOffer {
    /// `rostrum://pair?v=1&m=<machine>&h=<host>,<host>&p=<port>&c=<code>&fp=<fingerprint>`
    pub fn to_uri(&self) -> String {
        let hosts = self
            .endpoint
            .hosts
            .iter()
            .map(Host::to_string)
            .collect::<Vec<_>>()
            .join(",");
        let mut url = Url::parse(&format!("{URI_SCHEME}://{URI_HOST}"))
            .unwrap_or_else(|_| unreachable!("a constant URL parses"));
        url.query_pairs_mut()
            .append_pair("v", &URI_VERSION.to_string())
            .append_pair("m", &self.machine)
            .append_pair("h", &hosts)
            .append_pair("p", &self.endpoint.port.to_string())
            .append_pair("c", self.code.as_str())
            .append_pair("fp", &self.endpoint.fingerprint.to_base64url());
        url.to_string()
    }

    pub fn from_uri(text: &str) -> Result<Self, PairingUriError> {
        let url =
            Url::parse(text.trim()).map_err(|err| PairingUriError::NotAUri(err.to_string()))?;
        if url.scheme() != URI_SCHEME || url.host_str() != Some(URI_HOST) {
            return Err(PairingUriError::WrongKind);
        }
        let param = |key: &'static str| {
            url.query_pairs()
                .find(|(k, _)| k == key)
                .map(|(_, v)| v.into_owned())
                .ok_or(PairingUriError::Missing(key))
        };

        let version: u32 = param("v")?
            .parse()
            .map_err(|_| PairingUriError::Missing("v"))?;
        if version != URI_VERSION {
            return Err(PairingUriError::UnsupportedVersion(version));
        }
        let machine = param("m")?;
        let hosts = param("h")?
            .split(',')
            .filter(|part| !part.is_empty())
            .map(str::parse)
            .collect::<Result<Vec<Host>, _>>()?;
        let port = param("p")?.parse().map_err(|_| PairingUriError::Port)?;
        let code = PairingCode::parse(&param("c")?)?;
        let fingerprint = CertFingerprint::from_base64url(&param("fp")?)?;
        Ok(Self {
            machine,
            endpoint: Endpoint::new(hosts, port, fingerprint)?,
            code,
        })
    }
}

/// `GET /api/v1/hello`: who is answering, and which protocol it speaks.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Hello {
    pub machine: String,
    pub api_version: u32,
}

/// `POST /api/v1/pair`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PairRequest {
    pub code: PairingCode,
    /// Shown on the desktop page next to the revoke button, e.g. "Pixel 8".
    pub device_name: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PairResponse {
    pub device: DeviceId,
    pub token: DeviceToken,
    pub machine: MachineInfo,
    /// The desktop's GitHub token, when it has one. Absent, the phone falls
    /// back to asking for a personal access token.
    pub github: Option<GitHubHandover>,
}

/// A GitHub token passed from the desktop, and where it came from.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GitHubHandover {
    pub token: GitHubToken,
    /// `github.com`, or a GitHub Enterprise host.
    pub host: String,
    /// Human-readable provenance, e.g. "gh auth token on nymph-desk".
    pub source: String,
}

impl fmt::Debug for GitHubHandover {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("GitHubHandover")
            .field("token", &self.token)
            .field("host", &self.host)
            .field("source", &self.source)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn offer() -> PairingOffer {
        PairingOffer {
            machine: "nymph desk (home)".into(),
            endpoint: Endpoint::new(
                vec![
                    "192.168.1.20".parse().expect("ip"),
                    "fd7a:115c:a1e0::1".parse().expect("ip"),
                    "nymph-desk.tail1234.ts.net".parse().expect("name"),
                ],
                8485,
                CertFingerprint::of_der(b"cert"),
            )
            .expect("endpoint"),
            code: PairingCode::parse("K7QX-M2PD").expect("code"),
        }
    }

    #[test]
    fn an_offer_round_trips_through_its_link() {
        let offer = offer();
        let uri = offer.to_uri();
        assert!(uri.starts_with("rostrum://pair?v=1&"), "{uri}");
        assert_eq!(PairingOffer::from_uri(&uri), Ok(offer));
    }

    #[test]
    fn the_link_carries_the_compact_code_and_every_host() {
        let uri = offer().to_uri();
        assert!(uri.contains("c=K7QXM2PD"));
        assert!(uri.contains("p=8485"));
        assert!(uri.contains("nymph-desk.tail1234.ts.net"));
    }

    #[test]
    fn other_links_are_rejected() {
        assert_eq!(
            PairingOffer::from_uri("https://pair?v=1"),
            Err(PairingUriError::WrongKind)
        );
        assert_eq!(
            PairingOffer::from_uri("rostrum://other?v=1"),
            Err(PairingUriError::WrongKind)
        );
        assert!(matches!(
            PairingOffer::from_uri("not a link"),
            Err(PairingUriError::NotAUri(_))
        ));
    }

    #[test]
    fn a_newer_link_version_says_to_update() {
        let uri = offer().to_uri().replace("v=1", "v=2");
        assert_eq!(
            PairingOffer::from_uri(&uri),
            Err(PairingUriError::UnsupportedVersion(2))
        );
    }

    #[test]
    fn missing_or_bad_fields_are_named() {
        let uri = offer().to_uri();
        let without = |key: &str| {
            let (base, query) = uri.split_once('?').expect("query");
            let kept: Vec<&str> = query
                .split('&')
                .filter(|pair| !pair.starts_with(&format!("{key}=")))
                .collect();
            format!("{base}?{}", kept.join("&"))
        };
        assert_eq!(
            PairingOffer::from_uri(&without("fp")),
            Err(PairingUriError::Missing("fp"))
        );
        assert_eq!(
            PairingOffer::from_uri(&without("c")),
            Err(PairingUriError::Missing("c"))
        );
        let empty_hosts = uri.replace(
            &uri[uri.find("h=").expect("h")..uri.find("&p=").expect("p")],
            "h=",
        );
        assert_eq!(
            PairingOffer::from_uri(&empty_hosts),
            Err(PairingUriError::Endpoint(EndpointError::NoHosts))
        );
        let bad_port = uri.replace("p=8485", "p=eighty");
        assert_eq!(
            PairingOffer::from_uri(&bad_port),
            Err(PairingUriError::Port)
        );
    }

    #[test]
    fn endpoints_need_a_host_and_a_port_and_drop_duplicates() {
        let fp = CertFingerprint::of_der(b"c");
        assert_eq!(Endpoint::new(vec![], 1, fp), Err(EndpointError::NoHosts));
        let host: Host = "desk".parse().expect("name");
        assert_eq!(
            Endpoint::new(vec![host.clone()], 0, fp),
            Err(EndpointError::Port)
        );
        let endpoint = Endpoint::new(vec![host.clone(), host.clone()], 1, fp).expect("ok");
        assert_eq!(endpoint.hosts(), &[host]);
    }

    #[test]
    fn an_endpoint_with_no_hosts_does_not_deserialise() {
        let json = format!(
            "{{\"hosts\":[],\"port\":8485,\"fingerprint\":\"{}\"}}",
            CertFingerprint::of_der(b"c").to_base64url()
        );
        assert!(serde_json::from_str::<Endpoint>(&json).is_err());
    }

    #[test]
    fn the_github_token_is_redacted_in_debug_output() {
        let handover = GitHubHandover {
            token: GitHubToken::new("gho_abc123"),
            host: "github.com".into(),
            source: "gh auth token".into(),
        };
        assert!(!format!("{handover:?}").contains("gho_abc123"));
    }
}

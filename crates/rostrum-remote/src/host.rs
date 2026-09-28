//! An address the desktop can be reached at: an IP or a DNS name.
//!
//! A desktop typically has several — a LAN address, a tailnet address, a
//! MagicDNS name — and the phone tries them in order, so a pairing made at home
//! keeps working over the tailnet.

use std::{fmt, net::IpAddr, str::FromStr};

use serde::{Deserialize, Deserializer, Serialize, Serializer};

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Host {
    Ip(IpAddr),
    Name(String),
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("`{0}` is not an IP address or a DNS name")]
pub struct HostError(pub String);

impl Host {
    /// `host:port`, with an IPv6 address bracketed as a URL requires.
    pub fn authority(&self, port: u16) -> String {
        match self {
            Self::Ip(IpAddr::V6(v6)) => format!("[{v6}]:{port}"),
            Self::Ip(IpAddr::V4(v4)) => format!("{v4}:{port}"),
            Self::Name(name) => format!("{name}:{port}"),
        }
    }
}

impl FromStr for Host {
    type Err = HostError;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let trimmed = text.trim().trim_start_matches('[').trim_end_matches(']');
        if let Ok(ip) = trimmed.parse::<IpAddr>() {
            return Ok(Self::Ip(ip));
        }
        let name = trimmed.trim_end_matches('.').to_ascii_lowercase();
        let valid_label = |label: &str| {
            !label.is_empty()
                && label.len() <= 63
                && !label.starts_with('-')
                && !label.ends_with('-')
                && label
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-')
        };
        if name.is_empty() || name.len() > 253 || !name.split('.').all(valid_label) {
            return Err(HostError(text.to_string()));
        }
        Ok(Self::Name(name))
    }
}

impl fmt::Display for Host {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Ip(ip) => write!(f, "{ip}"),
            Self::Name(name) => f.write_str(name),
        }
    }
}

impl Serialize for Host {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for Host {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        text.parse().map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_ipv4_ipv6_and_names() {
        assert_eq!(
            "192.168.1.20".parse::<Host>(),
            Ok(Host::Ip("192.168.1.20".parse().expect("ip")))
        );
        assert_eq!(
            "fd7a:115c:a1e0::1".parse::<Host>(),
            Ok(Host::Ip("fd7a:115c:a1e0::1".parse().expect("ip")))
        );
        assert_eq!(
            "[fd7a:115c:a1e0::1]".parse::<Host>(),
            Ok(Host::Ip("fd7a:115c:a1e0::1".parse().expect("ip")))
        );
        assert_eq!(
            "Nymph-Desk.tail1234.ts.net.".parse::<Host>(),
            Ok(Host::Name("nymph-desk.tail1234.ts.net".into()))
        );
    }

    #[test]
    fn rejects_what_is_neither() {
        for bad in [
            "",
            "a b",
            "-leading.example",
            "trailing-.example",
            "a..b",
            "x_y",
        ] {
            assert!(bad.parse::<Host>().is_err(), "{bad:?} should be rejected");
        }
        assert!("a".repeat(254).parse::<Host>().is_err());
    }

    #[test]
    fn authority_brackets_ipv6_only() {
        let v6: Host = "fd7a::1".parse().expect("ip");
        let v4: Host = "10.0.0.2".parse().expect("ip");
        let name: Host = "desk".parse().expect("name");
        assert_eq!(v6.authority(8485), "[fd7a::1]:8485");
        assert_eq!(v4.authority(8485), "10.0.0.2:8485");
        assert_eq!(name.authority(8485), "desk:8485");
    }

    #[test]
    fn serde_uses_the_display_form() {
        let host: Host = "100.101.102.103".parse().expect("ip");
        let json = serde_json::to_string(&host).expect("serialises");
        assert_eq!(json, "\"100.101.102.103\"");
        assert_eq!(serde_json::from_str::<Host>(&json).expect("parses"), host);
    }
}

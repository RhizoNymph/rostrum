//! `Host` and `Origin` checks for the page server's privileged routes.
//!
//! The source-address gate ([`super::ClientClass`]) says the *connection*
//! comes from this computer or the tailnet. Two browser attacks get a
//! connection from there without the user meaning it:
//!
//! - **Cross-site requests.** A page on another site posts to
//!   `http://127.0.0.1:8484/pairing-codes`. The browser sends an `Origin`
//!   naming that other site, which does not match the request's `Host`.
//! - **DNS rebinding.** A page on `evil.example` re-points its own name at
//!   `127.0.0.1` and then talks to `http://evil.example:8484` "same-origin",
//!   so `Origin` and `Host` agree. What gives it away is the name: the page
//!   server only answers privileged requests addressed to an IP literal,
//!   `localhost`, this computer's hostname, or its tailnet name.

use rostrum_remote::Host;

/// The authority a request was addressed to, from its `Host` header.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RequestAuthority {
    pub host: Host,
    pub port: Option<u16>,
}

impl RequestAuthority {
    /// Parse a `Host` header value: `name`, `name:port`, `1.2.3.4:port`,
    /// `[v6]` or `[v6]:port`.
    pub fn parse(value: &str) -> Option<Self> {
        let value = value.trim();
        if value.is_empty() {
            return None;
        }
        if let Some(rest) = value.strip_prefix('[') {
            let (inside, after) = rest.split_once(']')?;
            let host = Host::Ip(inside.parse().ok()?);
            let port = match after {
                "" => None,
                other => Some(other.strip_prefix(':')?.parse().ok()?),
            };
            return Some(Self { host, port });
        }
        match value.rsplit_once(':') {
            // A bare IPv6 address has several colons and no brackets.
            Some((head, _)) if head.contains(':') => Some(Self {
                host: Host::Ip(value.parse().ok()?),
                port: None,
            }),
            Some((head, port)) => Some(Self {
                host: head.parse().ok()?,
                port: Some(port.parse().ok()?),
            }),
            None => Some(Self {
                host: value.parse().ok()?,
                port: None,
            }),
        }
    }
}

/// The names the page server answers privileged requests on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TrustedNames {
    names: Vec<String>,
}

impl TrustedNames {
    /// `localhost`, the hostname, `<hostname>.local`, and the tailnet's
    /// MagicDNS name for this computer when there is one.
    pub fn new(hostname: &str, tailnet_name: Option<&str>) -> Self {
        let normalise = |name: &str| name.trim().trim_end_matches('.').to_ascii_lowercase();
        let hostname = normalise(hostname);
        let mut names = vec!["localhost".to_string()];
        if !hostname.is_empty() {
            names.push(format!("{hostname}.local"));
            names.push(hostname);
        }
        if let Some(name) = tailnet_name.map(normalise).filter(|name| !name.is_empty()) {
            names.push(name);
        }
        Self { names }
    }

    /// An IP literal always; a name only if it is one of ours.
    pub fn admits(&self, authority: &RequestAuthority) -> bool {
        match &authority.host {
            Host::Ip(_) => true,
            Host::Name(name) => self.names.iter().any(|ours| ours == name),
        }
    }
}

/// Whether an `Origin` header names the same `http` origin as the request's
/// `Host`. `"null"`, another scheme, another host or another port is not.
pub fn origin_matches(origin: &str, authority: &RequestAuthority) -> bool {
    const HTTP_DEFAULT_PORT: u16 = 80;
    let Ok(url) = url::Url::parse(origin.trim()) else {
        return false;
    };
    if url.scheme() != "http" {
        return false;
    }
    let host = match url.host() {
        Some(url::Host::Domain(name)) => match name.parse::<Host>() {
            Ok(host) => host,
            Err(_) => return false,
        },
        Some(url::Host::Ipv4(v4)) => Host::Ip(v4.into()),
        Some(url::Host::Ipv6(v6)) => Host::Ip(v6.into()),
        None => return false,
    };
    let origin_port = url.port_or_known_default().unwrap_or(HTTP_DEFAULT_PORT);
    let host_port = authority.port.unwrap_or(HTTP_DEFAULT_PORT);
    same_host(&host, &authority.host) && origin_port == host_port
}

fn same_host(a: &Host, b: &Host) -> bool {
    match (a, b) {
        (Host::Ip(a), Host::Ip(b)) => a.to_canonical() == b.to_canonical(),
        (Host::Name(a), Host::Name(b)) => a == b,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn authority(value: &str) -> RequestAuthority {
        RequestAuthority::parse(value).expect("parses")
    }

    #[test]
    fn host_headers_parse_in_every_shape() {
        assert_eq!(
            authority("127.0.0.1:8484"),
            RequestAuthority {
                host: Host::Ip("127.0.0.1".parse().expect("ip")),
                port: Some(8484)
            }
        );
        assert_eq!(authority("[::1]:8484").port, Some(8484));
        assert_eq!(authority("[::1]").port, None);
        assert_eq!(
            authority("[fd7a:115c:a1e0::a]:8484").host,
            Host::Ip("fd7a:115c:a1e0::a".parse().expect("ip"))
        );
        assert_eq!(
            authority("Framework:8484").host,
            Host::Name("framework".into())
        );
        assert_eq!(authority("framework").port, None);
        assert_eq!(authority("::1").host, Host::Ip("::1".parse().expect("ip")));
    }

    #[test]
    fn malformed_host_headers_do_not_parse() {
        for bad in [
            "",
            "host:port",
            "[::1",
            "[::1]x",
            "[nope]:80",
            "a b:80",
            "x_y",
            "1.2.3.4:99999",
        ] {
            assert_eq!(RequestAuthority::parse(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn ip_literals_and_our_own_names_are_trusted() {
        let names = TrustedNames::new("framework", Some("framework-rttjig0y.ts.s8.gay."));
        for ok in [
            "127.0.0.1:8484",
            "[::1]:8484",
            "100.64.0.10:8484",
            "192.168.0.111:8484",
            "localhost:8484",
            "framework:8484",
            "FRAMEWORK.local:8484",
            "framework-rttjig0y.ts.s8.gay:8484",
        ] {
            assert!(names.admits(&authority(ok)), "{ok} should be trusted");
        }
    }

    #[test]
    fn a_rebound_name_is_not_trusted() {
        let names = TrustedNames::new("framework", None);
        for bad in [
            "evil.example:8484",
            "framework.evil.example:8484",
            "localhost.evil.example",
            "framework-rttjig0y.ts.s8.gay:8484",
        ] {
            assert!(!names.admits(&authority(bad)), "{bad} must not be trusted");
        }
    }

    #[test]
    fn an_origin_matches_only_the_same_http_host_and_port() {
        let host = authority("127.0.0.1:8484");
        assert!(origin_matches("http://127.0.0.1:8484", &host));
        assert!(!origin_matches("http://127.0.0.1:8485", &host));
        assert!(!origin_matches("https://127.0.0.1:8484", &host));
        assert!(!origin_matches("http://evil.example:8484", &host));
        assert!(!origin_matches("null", &host));
        assert!(!origin_matches("", &host));

        let named = authority("Framework:8484");
        assert!(origin_matches("http://framework:8484", &named));

        let v6 = authority("[::1]:8484");
        assert!(origin_matches("http://[::1]:8484", &v6));
        assert!(!origin_matches("http://127.0.0.1:8484", &v6));
    }

    #[test]
    fn default_ports_are_filled_in_on_both_sides() {
        assert!(origin_matches("http://localhost", &authority("localhost")));
        assert!(origin_matches(
            "http://localhost",
            &authority("localhost:80")
        ));
        assert!(origin_matches(
            "http://localhost:80",
            &authority("localhost")
        ));
    }
}

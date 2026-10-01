//! Who is asking, by source address.
//!
//! The owner's rule: only this computer (loopback) and the tailnet may
//! generate pairing codes, list paired devices, or revoke one. Anyone else on
//! the LAN may download the APK and nothing more. A pairing code is the key to
//! the user's clones and GitHub token, and the LAN is shared with whoever else
//! is on the Wi-Fi.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

/// Where a connection comes from, as far as the gate is concerned.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClientClass {
    /// `127.0.0.0/8` or `::1`: this computer.
    Loopback,
    /// Tailscale's ranges: `100.64.0.0/10` (CGNAT) and `fd7a:115c:a1e0::/48`.
    Tailnet,
    /// Everything else, including this computer's own LAN address.
    Other,
}

/// `fd7a:115c:a1e0::/48`, Tailscale's IPv6 ULA prefix.
const TAILNET_V6_PREFIX: [u16; 3] = [0xfd7a, 0x115c, 0xa1e0];

impl ClientClass {
    /// Classify a peer address. An IPv4-mapped IPv6 address (`::ffff:a.b.c.d`,
    /// what a dual-stack socket reports for an IPv4 peer) is classified as the
    /// IPv4 address it carries.
    pub fn of(ip: IpAddr) -> Self {
        match ip.to_canonical() {
            IpAddr::V4(v4) => Self::of_v4(v4),
            IpAddr::V6(v6) => Self::of_v6(v6),
        }
    }

    fn of_v4(ip: Ipv4Addr) -> Self {
        let [a, b, _, _] = ip.octets();
        if a == 127 {
            Self::Loopback
        } else if a == 100 && (b & 0xc0) == 64 {
            // 100.64.0.0/10: the top two bits of the second octet are 01.
            Self::Tailnet
        } else {
            Self::Other
        }
    }

    fn of_v6(ip: Ipv6Addr) -> Self {
        if ip == Ipv6Addr::LOCALHOST {
            Self::Loopback
        } else if ip.segments()[..3] == TAILNET_V6_PREFIX {
            Self::Tailnet
        } else {
            Self::Other
        }
    }

    /// Whether a peer from here may generate codes, list devices, or revoke.
    pub fn may_administer(self) -> bool {
        matches!(self, Self::Loopback | Self::Tailnet)
    }
}

/// The key the per-address pairing throttle counts failures under: the
/// canonical IPv4 address, or an IPv6 address's `/64` — one host commonly
/// holds a whole `/64`, so counting single IPv6 addresses would throttle
/// nothing.
pub fn throttle_key(ip: IpAddr) -> IpAddr {
    match ip.to_canonical() {
        IpAddr::V4(v4) => IpAddr::V4(v4),
        IpAddr::V6(v6) => {
            let s = v6.segments();
            IpAddr::V6(Ipv6Addr::new(s[0], s[1], s[2], s[3], 0, 0, 0, 0))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn class(text: &str) -> ClientClass {
        ClientClass::of(text.parse().expect("ip"))
    }

    #[test]
    fn loopback_is_all_of_127_slash_8_and_v6_localhost() {
        for ip in [
            "127.0.0.1",
            "127.0.0.0",
            "127.255.255.255",
            "127.1.2.3",
            "::1",
        ] {
            assert_eq!(class(ip), ClientClass::Loopback, "{ip}");
        }
    }

    #[test]
    fn the_tailnet_is_100_64_slash_10_and_tailscales_v6_prefix() {
        for ip in [
            "100.64.0.0",
            "100.64.0.10",
            "100.100.100.100",
            "100.127.255.255",
            "fd7a:115c:a1e0::",
            "fd7a:115c:a1e0::a",
            "fd7a:115c:a1e0:ffff:ffff:ffff:ffff:ffff",
        ] {
            assert_eq!(class(ip), ClientClass::Tailnet, "{ip}");
        }
    }

    #[test]
    fn the_edges_of_the_tailnet_ranges_are_other() {
        for ip in [
            "100.63.255.255",
            "100.128.0.0",
            "99.64.0.1",
            "101.64.0.1",
            "fd7a:115c:a1e1::1",
            "fd7a:115c:a1df:ffff::1",
            "fd7b:115c:a1e0::1",
        ] {
            assert_eq!(class(ip), ClientClass::Other, "{ip}");
        }
    }

    #[test]
    fn lan_link_local_unspecified_and_public_addresses_are_other() {
        for ip in [
            "192.168.0.111",
            "10.0.0.2",
            "172.17.0.1",
            "169.254.1.1",
            "0.0.0.0",
            "8.8.8.8",
            "::",
            "fe80::1",
            "2001:db8::1",
        ] {
            assert_eq!(class(ip), ClientClass::Other, "{ip}");
        }
    }

    #[test]
    fn ipv4_mapped_ipv6_is_classified_as_its_ipv4() {
        assert_eq!(class("::ffff:127.0.0.1"), ClientClass::Loopback);
        assert_eq!(class("::ffff:100.64.0.10"), ClientClass::Tailnet);
        assert_eq!(class("::ffff:192.168.0.5"), ClientClass::Other);
        assert_eq!(class("::ffff:100.128.0.1"), ClientClass::Other);
    }

    #[test]
    fn deprecated_ipv4_compatible_addresses_are_not_mapped() {
        // `::127.0.0.1` is not `::ffff:127.0.0.1`; nothing produces it, and it
        // must not be mistaken for loopback.
        assert_eq!(class("::127.0.0.1"), ClientClass::Other);
    }

    #[test]
    fn only_loopback_and_the_tailnet_may_administer() {
        assert!(ClientClass::Loopback.may_administer());
        assert!(ClientClass::Tailnet.may_administer());
        assert!(!ClientClass::Other.may_administer());
    }

    #[test]
    fn the_throttle_counts_ipv4_hosts_and_ipv6_slash_64s() {
        let ip = |text: &str| text.parse::<IpAddr>().expect("ip");
        assert_eq!(throttle_key(ip("192.168.0.5")), ip("192.168.0.5"));
        assert_eq!(throttle_key(ip("::ffff:192.168.0.5")), ip("192.168.0.5"));
        assert_eq!(
            throttle_key(ip("2001:db8:1:2:aaaa::1")),
            throttle_key(ip("2001:db8:1:2:bbbb::9"))
        );
        assert_ne!(
            throttle_key(ip("2001:db8:1:2::1")),
            throttle_key(ip("2001:db8:1:3::1"))
        );
    }
}

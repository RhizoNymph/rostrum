//! The LAN addresses a pairing link advertises.
//!
//! A developer machine has a dozen interfaces and most are not a way in:
//! Docker's `docker0` and `br-*` bridges, their `veth*` pairs, libvirt's
//! `virbr*`. Advertising those would have the phone time out on each one in
//! turn before reaching the address that works. Only IPv4 on a physical-looking
//! interface that is up is kept; the tailnet addresses come from `tailscale`
//! itself (see [`super::tailscale`]), so `tailscale0` is skipped here too.

use std::net::{IpAddr, Ipv4Addr};

/// One address on one interface, as the probe saw it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Interface {
    pub name: String,
    pub ip: IpAddr,
    /// False only when the kernel says the link is down; "unknown" counts as
    /// up, because several drivers never report anything else.
    pub up: bool,
}

/// Name prefixes of interfaces that are never a LAN a phone is on.
const VIRTUAL_PREFIXES: &[&str] = &[
    "lo",
    "docker",
    "br-",
    "veth",
    "virbr",
    "tailscale",
    "cni",
    "flannel",
    "podman",
    "lxcbr",
    "lxdbr",
    "vboxnet",
    "vmnet",
    "tun",
    "tap",
];

/// Whether an interface name is a bridge, container, VM, loopback or tunnel
/// device rather than a physical network.
pub fn is_virtual(name: &str) -> bool {
    VIRTUAL_PREFIXES
        .iter()
        .any(|prefix| name.starts_with(prefix))
}

/// The IPv4 addresses worth advertising, in interface order, without
/// duplicates.
pub fn lan_ipv4s(interfaces: &[Interface]) -> Vec<Ipv4Addr> {
    let mut out = Vec::new();
    for interface in interfaces {
        let IpAddr::V4(ip) = interface.ip else {
            continue;
        };
        let keep = interface.up
            && !is_virtual(&interface.name)
            && !ip.is_loopback()
            && !ip.is_link_local()
            && !ip.is_unspecified()
            && !ip.is_multicast()
            && !ip.is_broadcast();
        if keep && !out.contains(&ip) {
            out.push(ip);
        }
    }
    out
}

/// Read the interfaces from the kernel. A failure is logged and degrades to
/// no LAN addresses: the tailnet may still carry the pairing.
pub fn probe() -> Vec<Interface> {
    match if_addrs::get_if_addrs() {
        Ok(found) => found
            .into_iter()
            .map(|interface| Interface {
                up: !matches!(
                    interface.oper_status,
                    if_addrs::IfOperStatus::Down
                        | if_addrs::IfOperStatus::NotPresent
                        | if_addrs::IfOperStatus::LowerLayerDown
                ),
                ip: interface.ip(),
                name: interface.name,
            })
            .collect(),
        Err(error) => {
            tracing::warn!(%error, "could not list network interfaces; advertising no LAN address");
            Vec::new()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn iface(name: &str, ip: &str, up: bool) -> Interface {
        Interface {
            name: name.into(),
            ip: ip.parse().expect("ip"),
            up,
        }
    }

    /// The interfaces of the machine this was written on, near enough.
    fn framework() -> Vec<Interface> {
        vec![
            iface("lo", "127.0.0.1", true),
            iface("lo", "::1", true),
            iface("wlp1s0", "192.168.0.111", true),
            iface("wlp1s0", "fe80::6927:6d6f:5383:c06", true),
            iface("tailscale0", "100.64.0.10", true),
            iface("tailscale0", "fd7a:115c:a1e0::a", true),
            iface("br-14aa92d34a62", "10.201.3.1", false),
            iface("br-518e0acdba0c", "10.201.0.1", true),
            iface("br-71eb85d9f84f", "172.23.0.1", true),
            iface("docker0", "10.200.0.1", false),
            iface("veth1a2b3c", "fe80::1", true),
            iface("virbr0", "192.168.122.1", true),
        ]
    }

    #[test]
    fn only_the_physical_lan_ipv4_survives() {
        assert_eq!(
            lan_ipv4s(&framework()),
            vec!["192.168.0.111".parse::<Ipv4Addr>().expect("ip")]
        );
    }

    #[test]
    fn several_physical_interfaces_keep_their_order() {
        let interfaces = vec![
            iface("enp2s0", "10.0.0.5", true),
            iface("wlp1s0", "192.168.0.111", true),
            iface("enp2s0", "10.0.0.5", true),
        ];
        assert_eq!(
            lan_ipv4s(&interfaces),
            vec![
                "10.0.0.5".parse::<Ipv4Addr>().expect("ip"),
                "192.168.0.111".parse().expect("ip")
            ]
        );
    }

    #[test]
    fn down_link_local_and_ipv6_addresses_are_dropped() {
        let interfaces = vec![
            iface("enp2s0", "10.0.0.5", false),
            iface("enp3s0", "169.254.10.20", true),
            iface("enp4s0", "2001:db8::5", true),
            iface("enp5s0", "0.0.0.0", true),
        ];
        assert!(lan_ipv4s(&interfaces).is_empty());
    }

    #[test]
    fn virtual_names_are_recognised() {
        for name in [
            "lo",
            "docker0",
            "br-abc",
            "veth9",
            "virbr0",
            "tailscale0",
            "cni0",
            "podman1",
        ] {
            assert!(is_virtual(name), "{name}");
        }
        for name in ["wlp1s0", "enp2s0", "eth0", "wlan0", "en0"] {
            assert!(!is_virtual(name), "{name}");
        }
    }
}

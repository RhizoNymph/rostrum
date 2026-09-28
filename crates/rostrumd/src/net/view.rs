//! What this computer can be reached at, kept current by a background task.
//!
//! Addresses change — Wi-Fi roams, DHCP renews, Tailscale logs in — so a
//! watcher re-probes on an interval and publishes the latest [`NetworkView`]
//! over a `watch` channel. Readers take the current value without waiting on
//! a subprocess, and nothing else holds this state.

use std::{net::Ipv4Addr, time::Duration};

use rostrum_remote::Host;
use tokio::sync::watch;

use super::{
    interfaces::{self, Interface},
    tailscale::{self, Tailnet},
};

/// How often the watcher re-probes.
pub const REFRESH: Duration = Duration::from_secs(30);

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct NetworkView {
    /// Physical LAN IPv4 addresses, see [`interfaces::lan_ipv4s`].
    pub lan: Vec<Ipv4Addr>,
    pub tailnet: Option<Tailnet>,
}

impl NetworkView {
    pub fn new(interfaces: &[Interface], tailnet: Option<Tailnet>) -> Self {
        Self {
            lan: interfaces::lan_ipv4s(interfaces),
            tailnet,
        }
    }

    /// The `h` list of a pairing link, in the order a phone should try it:
    /// LAN IPv4 (fastest at home), then the tailnet's IPv4 and IPv6, then its
    /// MagicDNS name (last, because it needs the phone's tailnet DNS to
    /// resolve). No duplicates.
    pub fn advertised_hosts(&self) -> Vec<Host> {
        let mut hosts: Vec<Host> = self.lan.iter().map(|ip| Host::Ip((*ip).into())).collect();
        if let Some(tailnet) = &self.tailnet {
            hosts.extend(tailnet.ipv4.iter().map(|ip| Host::Ip((*ip).into())));
            hosts.extend(tailnet.ipv6.iter().map(|ip| Host::Ip((*ip).into())));
            hosts.extend(tailnet.dns_name.iter().map(|name| Host::Name(name.clone())));
        }
        let mut unique = Vec::with_capacity(hosts.len());
        for host in hosts {
            if !unique.contains(&host) {
                unique.push(host);
            }
        }
        unique
    }

    /// This page over the tailnet: by MagicDNS name, then by IPv4.
    pub fn tailnet_page_urls(&self, port: u16) -> Vec<String> {
        let Some(tailnet) = &self.tailnet else {
            return Vec::new();
        };
        tailnet
            .dns_name
            .iter()
            .map(|name| format!("http://{}/", Host::Name(name.clone()).authority(port)))
            .chain(
                tailnet
                    .ipv4
                    .iter()
                    .map(|ip| format!("http://{}/", Host::Ip((*ip).into()).authority(port))),
            )
            .collect()
    }

    pub fn tailnet_name(&self) -> Option<&str> {
        self.tailnet.as_ref()?.dns_name.as_deref()
    }
}

/// Probe interfaces and the tailnet once.
pub async fn probe() -> NetworkView {
    let tailnet = tailscale::probe(tailscale::TIMEOUT).await;
    NetworkView::new(&interfaces::probe(), tailnet)
}

/// Publish `initial`, then re-probe every `every` until every receiver is
/// gone.
pub fn spawn_watcher(initial: NetworkView, every: Duration) -> watch::Receiver<NetworkView> {
    let (tx, rx) = watch::channel(initial);
    tokio::spawn(async move {
        loop {
            tokio::select! {
                () = tx.closed() => return,
                () = tokio::time::sleep(every) => {}
            }
            let fresh = probe().await;
            tx.send_if_modified(|current| {
                if *current == fresh {
                    return false;
                }
                tracing::info!(
                    lan = ?fresh.lan,
                    tailnet = ?fresh.tailnet,
                    "the addresses this computer can be reached at changed"
                );
                *current = fresh;
                true
            });
        }
    });
    rx
}

#[cfg(test)]
mod tests {
    use super::*;

    fn view() -> NetworkView {
        NetworkView {
            lan: vec!["192.168.0.111".parse().expect("ip")],
            tailnet: Some(Tailnet {
                ipv4: vec!["100.64.0.10".parse().expect("ip")],
                ipv6: vec!["fd7a:115c:a1e0::a".parse().expect("ip")],
                dns_name: Some("framework-rttjig0y.ts.s8.gay".into()),
            }),
        }
    }

    #[test]
    fn hosts_are_lan_then_tailnet_v4_v6_then_the_name() {
        let hosts: Vec<String> = view()
            .advertised_hosts()
            .iter()
            .map(Host::to_string)
            .collect();
        assert_eq!(
            hosts,
            vec![
                "192.168.0.111",
                "100.64.0.10",
                "fd7a:115c:a1e0::a",
                "framework-rttjig0y.ts.s8.gay"
            ]
        );
    }

    #[test]
    fn without_a_tailnet_only_the_lan_is_advertised() {
        let view = NetworkView {
            tailnet: None,
            ..view()
        };
        assert_eq!(view.advertised_hosts().len(), 1);
        assert!(view.tailnet_page_urls(8484).is_empty());
        assert_eq!(view.tailnet_name(), None);
    }

    #[test]
    fn nothing_to_advertise_is_an_empty_list() {
        assert!(NetworkView::default().advertised_hosts().is_empty());
    }

    #[test]
    fn the_same_address_on_two_sources_is_advertised_once() {
        let mut view = view();
        view.lan.push("100.64.0.10".parse().expect("ip"));
        let hosts = view.advertised_hosts();
        assert_eq!(hosts.len(), 4);
    }

    #[test]
    fn the_page_is_offered_over_the_tailnet_by_name_then_address() {
        assert_eq!(
            view().tailnet_page_urls(8484),
            vec![
                "http://framework-rttjig0y.ts.s8.gay:8484/".to_string(),
                "http://100.64.0.10:8484/".to_string()
            ]
        );
    }

    #[test]
    fn a_view_is_built_from_a_raw_interface_list() {
        let interfaces = vec![
            Interface {
                name: "docker0".into(),
                ip: "172.17.0.1".parse().expect("ip"),
                up: true,
            },
            Interface {
                name: "wlp1s0".into(),
                ip: "192.168.0.111".parse().expect("ip"),
                up: true,
            },
        ];
        let view = NetworkView::new(&interfaces, None);
        assert_eq!(
            view.lan,
            vec!["192.168.0.111".parse::<Ipv4Addr>().expect("ip")]
        );
    }
}

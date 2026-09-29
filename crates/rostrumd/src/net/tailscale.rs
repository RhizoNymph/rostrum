//! This computer on the tailnet, from the `tailscale` CLI.
//!
//! One `tailscale status --json` gives everything: the backend state, this
//! node's tailnet addresses, and its MagicDNS name (which on a Headscale
//! tailnet is not under `ts.net`, so nothing here assumes a suffix). The CLI
//! is bounded by a timeout and every failure — not installed, not logged in,
//! daemon down — degrades to "no tailnet" rather than an error.

use std::{
    net::{IpAddr, Ipv4Addr, Ipv6Addr},
    process::Stdio,
    time::Duration,
};

use rostrum_remote::Host;
use serde::Deserialize;

/// How long to give the CLI. It asks a local daemon over a socket; anything
/// slower is a wedged daemon.
pub const TIMEOUT: Duration = Duration::from_secs(3);

/// This node on the tailnet.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Tailnet {
    pub ipv4: Vec<Ipv4Addr>,
    pub ipv6: Vec<Ipv6Addr>,
    /// The MagicDNS name, lowercase, without the trailing dot.
    pub dns_name: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum TailscaleError {
    #[error("could not run `tailscale`: {0}")]
    Spawn(#[source] std::io::Error),
    #[error("`tailscale status` timed out")]
    Timeout,
    #[error("`tailscale status` failed: {0}")]
    Failed(String),
    #[error("could not read `tailscale status --json`: {0}")]
    Parse(#[source] serde_json::Error),
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct Status {
    backend_state: Option<String>,
    #[serde(rename = "Self")]
    this: Option<Node>,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct Node {
    #[serde(rename = "DNSName")]
    dns_name: Option<String>,
    #[serde(rename = "TailscaleIPs")]
    tailscale_ips: Option<Vec<IpAddr>>,
}

/// Read `tailscale status --json`. `None` when the backend is not running
/// (logged out, stopped) or reports no node.
pub fn parse_status(json: &[u8]) -> Result<Option<Tailnet>, TailscaleError> {
    let status: Status = serde_json::from_slice(json).map_err(TailscaleError::Parse)?;
    if status.backend_state.as_deref() != Some("Running") {
        return Ok(None);
    }
    let Some(node) = status.this else {
        return Ok(None);
    };
    let mut tailnet = Tailnet::default();
    for ip in node.tailscale_ips.unwrap_or_default() {
        match ip {
            IpAddr::V4(v4) if !tailnet.ipv4.contains(&v4) => tailnet.ipv4.push(v4),
            IpAddr::V6(v6) if !tailnet.ipv6.contains(&v6) => tailnet.ipv6.push(v6),
            _ => {}
        }
    }
    tailnet.dns_name = node
        .dns_name
        .and_then(|name| name.parse::<Host>().ok())
        .and_then(|host| match host {
            Host::Name(name) => Some(name),
            Host::Ip(_) => None,
        });
    if tailnet.ipv4.is_empty() && tailnet.ipv6.is_empty() && tailnet.dns_name.is_none() {
        return Ok(None);
    }
    Ok(Some(tailnet))
}

/// Ask the CLI.
pub async fn status(timeout: Duration) -> Result<Option<Tailnet>, TailscaleError> {
    let child = tokio::process::Command::new("tailscale")
        .args(["status", "--json"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .map_err(TailscaleError::Spawn)?;
    let output = tokio::time::timeout(timeout, child.wait_with_output())
        .await
        .map_err(|_| TailscaleError::Timeout)?
        .map_err(TailscaleError::Spawn)?;
    if !output.status.success() {
        return Err(TailscaleError::Failed(
            String::from_utf8_lossy(&output.stderr).trim().to_string(),
        ));
    }
    parse_status(&output.stdout)
}

/// [`status`], with every failure logged at debug and treated as no tailnet.
pub async fn probe(timeout: Duration) -> Option<Tailnet> {
    match status(timeout).await {
        Ok(tailnet) => tailnet,
        Err(error) => {
            tracing::debug!(%error, "no tailnet addresses to advertise");
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RUNNING: &str = r#"{
        "Version": "1.90.0",
        "BackendState": "Running",
        "Self": {
            "ID": "n1",
            "HostName": "framework",
            "DNSName": "framework-rttjig0y.ts.s8.gay.",
            "TailscaleIPs": ["100.64.0.10", "fd7a:115c:a1e0::a"],
            "Online": true
        },
        "MagicDNSSuffix": "ts.s8.gay",
        "Peer": {}
    }"#;

    #[test]
    fn a_running_node_yields_its_addresses_and_name() {
        let tailnet = parse_status(RUNNING.as_bytes())
            .expect("parses")
            .expect("running");
        assert_eq!(
            tailnet.ipv4,
            vec!["100.64.0.10".parse::<Ipv4Addr>().expect("ip")]
        );
        assert_eq!(
            tailnet.ipv6,
            vec!["fd7a:115c:a1e0::a".parse::<Ipv6Addr>().expect("ip")]
        );
        assert_eq!(
            tailnet.dns_name.as_deref(),
            Some("framework-rttjig0y.ts.s8.gay")
        );
    }

    #[test]
    fn a_stopped_or_logged_out_backend_is_no_tailnet() {
        for state in ["Stopped", "NeedsLogin", "Starting"] {
            let json = RUNNING.replace("\"Running\"", &format!("\"{state}\""));
            assert_eq!(parse_status(json.as_bytes()).expect("parses"), None);
        }
    }

    #[test]
    fn missing_fields_degrade_rather_than_fail() {
        let json = r#"{"BackendState": "Running", "Self": {"TailscaleIPs": ["100.64.0.10"]}}"#;
        let tailnet = parse_status(json.as_bytes())
            .expect("parses")
            .expect("running");
        assert_eq!(tailnet.dns_name, None);
        assert!(tailnet.ipv6.is_empty());

        let no_self = r#"{"BackendState": "Running"}"#;
        assert_eq!(parse_status(no_self.as_bytes()).expect("parses"), None);

        let empty = r#"{"BackendState": "Running", "Self": {"DNSName": ""}}"#;
        assert_eq!(parse_status(empty.as_bytes()).expect("parses"), None);
    }

    #[test]
    fn garbage_is_a_parse_error() {
        assert!(matches!(
            parse_status(b"Failed to connect to local Tailscale daemon"),
            Err(TailscaleError::Parse(_))
        ));
    }
}

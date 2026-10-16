//! Finds this device and its tailnet peers with `tailscale status --json`.

use super::Device;
use serde_json::Value;
use std::io;
use std::net::{IpAddr, Ipv4Addr};
use std::process::{Command, Output};

/// Tailscale's command line, looked up on PATH.
const BINARY: &str = "tailscale";
/// The macOS app keeps its command line inside the bundle. Tried when `tailscale` is not on PATH.
const MAC_BINARY: &str = "/Applications/Tailscale.app/Contents/MacOS/Tailscale";

/// One peer as Tailscale reports it.
#[derive(Clone, Debug, PartialEq)]
pub struct Peer {
    pub device: Device,
    pub online: bool,
    /// e.g. "linux", "windows", "macOS"
    pub os: String,
}

/// This device and the peers Tailscale reports for it.
#[derive(Clone, Debug, PartialEq)]
pub struct Tailnet {
    pub me: Device,
    pub peers: Vec<Peer>,
}

/// Runs `tailscale status --json` and parses it. Errors with a short message when Tailscale is
/// not installed, not running, or logged out.
pub fn discover() -> Result<Tailnet, String> {
    let output = match status(BINARY) {
        Err(e) if e.kind() == io::ErrorKind::NotFound => match status(MAC_BINARY) {
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                return Err("Tailscale is not installed".to_string());
            }
            other => other,
        },
        other => other,
    }
    .map_err(|e| format!("Could not run Tailscale: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(if stderr.is_empty() { "Tailscale is not running".to_string() } else { stderr });
    }
    parse(&String::from_utf8_lossy(&output.stdout))
}

/// Pure parser for the JSON, so tests need no Tailscale.
pub fn parse(json: &str) -> Result<Tailnet, String> {
    let root: Value =
        serde_json::from_str(json).map_err(|e| format!("Could not read Tailscale status: {}", e))?;

    let state = text(&root, "BackendState");
    if state != "Running" {
        let shown = if state.is_empty() { "unknown" } else { state };
        return Err(format!("Tailscale is not connected (state: {})", shown));
    }

    let self_node = root
        .get("Self")
        .filter(|v| v.is_object())
        .ok_or_else(|| "Tailscale status has no Self device".to_string())?;
    let me = node_device(self_node)
        .ok_or_else(|| "Tailscale has no IPv4 address for this device".to_string())?;

    let mut peers = Vec::new();
    if let Some(map) = root.get("Peer").and_then(Value::as_object) {
        for node in map.values() {
            if let Some(device) = node_device(node) {
                let online = node.get("Online").and_then(Value::as_bool).unwrap_or(false);
                let os = text(node, "OS").to_string();
                peers.push(Peer { device, online, os });
            }
        }
    }

    peers.sort_by(|a, b| {
        b.online
            .cmp(&a.online)
            .then_with(|| a.device.name.to_lowercase().cmp(&b.device.name.to_lowercase()))
            .then_with(|| a.device.id.cmp(&b.device.id))
    });

    Ok(Tailnet { me, peers })
}

/// Whether `ip` is in Tailscale's address ranges: IPv4 100.64.0.0/10 or IPv6 fd7a:115c:a1e0::/48.
/// The host uses this to refuse connections from outside the tailnet. An IPv4-mapped IPv6 address
/// counts as its IPv4 address.
pub fn is_tailscale_addr(ip: &IpAddr) -> bool {
    match ip.to_canonical() {
        IpAddr::V4(v4) => {
            let o = v4.octets();
            o[0] == 100 && (64..=127).contains(&o[1])
        }
        IpAddr::V6(v6) => {
            let s = v6.segments();
            s[0] == 0xfd7a && s[1] == 0x115c && s[2] == 0xa1e0
        }
    }
}

/// Runs `<program> status --json` and returns its output.
fn status(program: &str) -> io::Result<Output> {
    Command::new(program).args(["status", "--json"]).output()
}

/// A string field of a JSON object, or "" when missing or not a string.
fn text<'a>(node: &'a Value, key: &str) -> &'a str {
    node.get(key).and_then(Value::as_str).unwrap_or("")
}

/// The first IPv4 address in a node's `TailscaleIPs`.
fn ipv4_of(node: &Value) -> Option<String> {
    node.get("TailscaleIPs")?
        .as_array()?
        .iter()
        .filter_map(Value::as_str)
        .find(|ip| ip.contains('.') && ip.parse::<Ipv4Addr>().is_ok())
        .map(str::to_string)
}

/// A tailnet node as a `Device`. `None` when the node has no IPv4 address.
fn node_device(node: &Value) -> Option<Device> {
    let addr = ipv4_of(node)?;
    let dns_name = text(node, "DNSName").trim_end_matches('.').to_string();
    let host = text(node, "HostName").to_string();

    let mut id = text(node, "ID").to_string();
    if id.is_empty() {
        id = dns_name.clone();
    }
    if id.is_empty() {
        id = host.clone();
    }

    let name = if host.is_empty() {
        dns_name.split('.').next().unwrap_or("").to_string()
    } else {
        host
    };

    Some(Device { id, name, dns_name, addr })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::IpAddr;

    const FIXTURE: &str = r#"{
        "BackendState": "Running",
        "Self": {
            "ID": "nSELF1",
            "HostName": "laptop",
            "DNSName": "laptop.tail1234.ts.net.",
            "TailscaleIPs": ["100.64.0.10", "fd7a:115c:a1e0::10"],
            "Online": true,
            "OS": "macOS"
        },
        "Peer": {
            "nodekey:abc": {
                "ID": "n2",
                "HostName": "Winbox",
                "DNSName": "winbox.tail1234.ts.net.",
                "TailscaleIPs": ["100.101.102.103", "fd7a:115c:a1e0::2"],
                "Online": false,
                "OS": "windows"
            },
            "nodekey:def": {
                "ID": "n3",
                "HostName": "pc",
                "DNSName": "pc.tail1234.ts.net.",
                "TailscaleIPs": ["100.101.102.104", "fd7a:115c:a1e0::3"],
                "Online": true,
                "OS": "linux"
            },
            "nodekey:ghi": {
                "ID": "n4",
                "HostName": "v6only",
                "DNSName": "v6only.tail1234.ts.net.",
                "TailscaleIPs": ["fd7a:115c:a1e0::4"],
                "Online": true,
                "OS": "linux"
            }
        }
    }"#;

    #[test]
    fn parses_self_and_sorted_peers() {
        let t = parse(FIXTURE).unwrap();

        assert_eq!(t.me.id, "nSELF1");
        assert_eq!(t.me.name, "laptop");
        assert_eq!(t.me.dns_name, "laptop.tail1234.ts.net");
        assert_eq!(t.me.addr, "100.64.0.10");

        // The IPv6-only peer is skipped; the online peer sorts before the offline one.
        assert_eq!(t.peers.len(), 2);
        assert_eq!(
            t.peers[0].device,
            Device {
                id: "n3".into(),
                name: "pc".into(),
                dns_name: "pc.tail1234.ts.net".into(),
                addr: "100.101.102.104".into(),
            }
        );
        assert!(t.peers[0].online);
        assert_eq!(t.peers[0].os, "linux");

        assert_eq!(t.peers[1].device.name, "Winbox");
        assert_eq!(t.peers[1].device.dns_name, "winbox.tail1234.ts.net");
        assert_eq!(t.peers[1].device.addr, "100.101.102.103");
        assert!(!t.peers[1].online);
        assert_eq!(t.peers[1].os, "windows");
    }

    #[test]
    fn stopped_backend_is_an_error() {
        let json = FIXTURE.replace("\"Running\"", "\"Stopped\"");
        let err = parse(&json).unwrap_err();
        assert!(err.contains("Stopped"), "{}", err);
    }

    #[test]
    fn missing_self_is_an_error() {
        let json = r#"{"BackendState": "Running", "Peer": null}"#;
        assert!(parse(json).is_err());
    }

    #[test]
    fn null_peer_map_means_no_peers() {
        let json = r#"{
            "BackendState": "Running",
            "Self": {"ID": "n1", "HostName": "pc", "TailscaleIPs": ["100.1.2.3"]},
            "Peer": null
        }"#;
        let t = parse(json).unwrap();
        assert_eq!(t.me.name, "pc");
        assert!(t.peers.is_empty());
    }

    #[test]
    fn tailscale_address_ranges() {
        let ip = |s: &str| s.parse::<IpAddr>().unwrap();
        assert!(is_tailscale_addr(&ip("100.64.0.1")));
        assert!(is_tailscale_addr(&ip("100.127.255.255")));
        assert!(!is_tailscale_addr(&ip("100.128.0.1")));
        assert!(!is_tailscale_addr(&ip("192.168.1.5")));
        assert!(is_tailscale_addr(&ip("fd7a:115c:a1e0::5")));
        assert!(!is_tailscale_addr(&ip("fd7a:115c:a1e1::5")));
        assert!(is_tailscale_addr(&ip("::ffff:100.64.0.1")));
        assert!(!is_tailscale_addr(&ip("::ffff:192.168.1.5")));
    }
}

//! Sharing chats between devices on a Tailscale network.
//!
//! Everything here is optional and off by default (`SyncSettings::enabled`). A single machine
//! without Tailscale never starts a listener, never runs `tailscale`, and never opens a socket.
//!
//! - `tailscale`: finds this device and its peers with `tailscale status --json`.
//! - `proto`: the JSON messages a host and a client exchange.
//! - `host`: serves this device's chats to other devices (`archonui --serve`).
//! - `client`: connects to another device's host and drives its chats.

#![allow(dead_code)]

pub mod client;
pub mod hub;
pub mod host;
pub mod proto;
pub mod tailscale;

use serde::{Deserialize, Serialize};

/// Port a host listens on, on its Tailscale address only.
pub const DEFAULT_PORT: u16 = 7420;

/// A device on the tailnet, as saved in settings.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Device {
    /// Tailscale's stable node id (`"nXXXX"`). Survives renames; used as the key everywhere.
    pub id: String,
    /// Short host name, e.g. `"pc"`.
    pub name: String,
    /// MagicDNS name, e.g. `"pc.tail1234.ts.net"` (no trailing dot). Empty when MagicDNS is off.
    #[serde(default)]
    pub dns_name: String,
    /// Tailscale IPv4 address, e.g. `"100.101.102.103"`.
    pub addr: String,
}

/// The Devices settings: which tailnet devices share this workspace and which one is the cloud.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SyncSettings {
    /// Master switch. Off: no listener, no discovery, no cloud copy.
    pub enabled: bool,
    /// Port this device's host listens on.
    pub port: u16,
    /// Devices the user picked from the detected tailnet devices to join the workspace.
    /// Only these may connect to this device's host, and only these show chats here.
    pub devices: Vec<Device>,
    /// `Device::id` of the device that keeps the always-on cloud copy, if any. Must be one of
    /// `devices` or this device itself.
    pub cloud: Option<String>,
}

impl Default for SyncSettings {
    fn default() -> Self {
        Self { enabled: false, port: DEFAULT_PORT, devices: Vec::new(), cloud: None }
    }
}

impl SyncSettings {
    /// Whether `addr` (a peer's IP) belongs to a device the user included.
    pub fn allows(&self, addr: &str) -> bool {
        self.devices.iter().any(|d| d.addr == addr)
    }

    pub fn is_cloud(&self, id: &str) -> bool {
        self.cloud.as_deref() == Some(id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn off_by_default_and_empty_settings_load() {
        let s: SyncSettings = serde_json::from_str("{}").unwrap();
        assert_eq!(s, SyncSettings::default());
        assert!(!s.enabled);
        assert_eq!(s.port, DEFAULT_PORT);
        assert!(!s.allows("100.1.2.3"));
    }

    #[test]
    fn allows_only_included_devices() {
        let s = SyncSettings {
            enabled: true,
            devices: vec![Device { id: "n1".into(), name: "pc".into(), dns_name: String::new(), addr: "100.1.2.3".into() }],
            cloud: Some("n1".into()),
            ..Default::default()
        };
        assert!(s.allows("100.1.2.3"));
        assert!(!s.allows("100.9.9.9"));
        assert!(s.is_cloud("n1"));
    }
}

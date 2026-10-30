//! Sharing chats between devices on a Tailscale network.
//!
//! Everything here is optional and off by default (`SyncSettings::enabled`). A single machine
//! without Tailscale never starts a listener, never runs `tailscale`, and never opens a socket.
//!
//! - `tailscale`: finds this device and its peers with `tailscale status --json`.
//! - `proto`: the JSON messages a host and a client exchange.
//! - `host`: serves this device's chats to other devices (`archonui --serve`).
//! - `client`: connects to another device's host and drives its chats.
//! - `lock`: keeps `--serve` and the app from writing the same chats.

#![allow(dead_code)]

pub mod assets;
pub mod client;
pub mod cloud;
pub mod hub;
pub mod host;
pub mod lock;
#[cfg(test)]
mod perf;
pub mod proto;
pub mod snapshot;
pub mod tailscale;

use serde::{Deserialize, Serialize};
use std::net::IpAddr;

/// Port a host listens on, on its Tailscale address only.
pub const DEFAULT_PORT: u16 = 7420;

/// This machine's name: `HOSTNAME` when exported, else the kernel's host name. Shells set
/// `HOSTNAME` without exporting it, so the variable alone is usually missing.
pub fn local_host_name() -> Option<String> {
    if let Some(name) = std::env::var("HOSTNAME").ok().filter(|n| !n.is_empty()) {
        return Some(name);
    }
    let mut buf = [0u8; 256];
    // SAFETY: `gethostname` writes at most `buf.len()` bytes into `buf`.
    if unsafe { libc::gethostname(buf.as_mut_ptr().cast(), buf.len()) } != 0 {
        return None;
    }
    let len = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
    let name = String::from_utf8_lossy(&buf[..len]).into_owned();
    (!name.is_empty()).then_some(name)
}

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
    /// Tailscale IPv6 address, e.g. `"fd7a:115c:a1e0::3"`. Empty when unknown, as in settings saved
    /// before it was kept.
    #[serde(default)]
    pub addr6: String,
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
    /// The user has picked or cleared the cloud themselves, so a `--serve` host is never made the cloud
    /// on its own again (see `adopt_serving_cloud`).
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub cloud_picked: bool,
    /// Copy secret-looking files (.env, keys, certificates) to the cloud too. Off: they stay on this device.
    pub copy_secrets: bool,
}

impl Default for SyncSettings {
    fn default() -> Self {
        Self { enabled: false, port: DEFAULT_PORT, devices: Vec::new(), cloud: None, cloud_picked: false, copy_secrets: false }
    }
}

impl SyncSettings {
    /// Whether `addr` (a peer's IP) belongs to a device the user included, by its IPv4 or IPv6 address.
    pub fn allows(&self, addr: &str) -> bool {
        let Ok(ip) = addr.parse::<IpAddr>().map(|ip| ip.to_canonical()) else { return false };
        let is = |a: &str| a.parse::<IpAddr>().is_ok_and(|a| a.to_canonical() == ip);
        self.devices.iter().any(|d| is(&d.addr) || is(&d.addr6))
    }

    /// Updates the included devices from what Tailscale reports now (`found`), matched by id: a
    /// renamed device, a new address, or the IPv6 address older settings lack. Returns whether any changed.
    pub fn refresh_devices<'a>(&mut self, found: impl IntoIterator<Item = &'a Device>) -> bool {
        let mut changed = false;
        for new in found {
            if let Some(d) = self.devices.iter_mut().find(|d| d.id == new.id && *d != new) {
                *d = new.clone();
                changed = true;
            }
        }
        changed
    }

    pub fn is_cloud(&self, id: &str) -> bool {
        self.cloud.as_deref() == Some(id)
    }

    /// After a connection to device `id`: when its `Welcome` says it is a headless `archonui --serve`
    /// (the always-on box) and no cloud device is chosen yet, it becomes the cloud. Only an included
    /// device can be, and once the user has picked or cleared the cloud themselves it is left to them.
    /// Returns whether it changed.
    pub fn adopt_serving_cloud(&mut self, id: &str, serve: bool) -> bool {
        if !serve || self.cloud.is_some() || self.cloud_picked || !self.devices.iter().any(|d| d.id == id) {
            return false;
        }
        self.cloud = Some(id.to_string());
        true
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
    fn a_serving_device_becomes_the_cloud_only_on_first_setup() {
        let vps = Device { id: "n-vps".into(), name: "vps".into(), dns_name: String::new(), addr: "100.80.70.23".into(), addr6: String::new() };
        let mut s = SyncSettings { enabled: true, devices: vec![vps.clone()], ..Default::default() };
        assert!(!s.adopt_serving_cloud("n-vps", false), "a desktop app answering is not the cloud");
        assert!(!s.adopt_serving_cloud("n-other", true), "only an added device");
        assert!(s.adopt_serving_cloud("n-vps", true));
        assert!(s.is_cloud("n-vps"));
        assert!(!s.adopt_serving_cloud("n-vps", true), "already the cloud");
        let mut picked = SyncSettings { cloud: Some("n-pc".into()), ..s.clone() };
        assert!(!picked.adopt_serving_cloud("n-vps", true), "the user's own pick stays");
        assert!(picked.is_cloud("n-pc"));
        let mut cleared = SyncSettings { cloud: None, cloud_picked: true, ..s };
        assert!(!cleared.adopt_serving_cloud("n-vps", true), "a cloud the user turned off stays off");
    }

    #[test]
    fn this_machine_has_a_name() {
        assert!(local_host_name().is_some_and(|n| !n.is_empty() && !n.contains('\0')));
    }

    #[test]
    fn allows_only_included_devices() {
        let s = SyncSettings {
            enabled: true,
            devices: vec![Device {
                id: "n1".into(),
                name: "pc".into(),
                dns_name: String::new(),
                addr: "100.1.2.3".into(),
                addr6: "fd7a:115c:a1e0::3".into(),
            }],
            cloud: Some("n1".into()),
            ..Default::default()
        };
        assert!(s.allows("100.1.2.3"));
        assert!(!s.allows("100.9.9.9"));
        // From its IPv6 address too, however it is written; an IPv4-mapped one counts as IPv4.
        assert!(s.allows("fd7a:115c:a1e0::3"));
        assert!(s.allows("fd7a:115c:a1e0:0:0:0:0:3"));
        assert!(s.allows("::ffff:100.1.2.3"));
        assert!(!s.allows("fd7a:115c:a1e0::4"));
        assert!(!s.allows(""));
        assert!(s.is_cloud("n1"));
    }

    #[test]
    fn old_devices_load_and_learn_their_ipv6_address() {
        let mut s: SyncSettings =
            serde_json::from_str(r#"{"enabled":true,"devices":[{"id":"n1","name":"pc","addr":"100.1.2.3"}]}"#).unwrap();
        assert_eq!(s.devices[0].addr6, "");
        assert!(!s.allows("fd7a:115c:a1e0::3"));
        let found = Device { addr6: "fd7a:115c:a1e0::3".into(), ..s.devices[0].clone() };
        let other = Device { id: "n2".into(), ..found.clone() };
        assert!(s.refresh_devices([&found, &other]));
        assert_eq!(s.devices, [found.clone()], "only included devices are updated");
        assert!(s.allows("fd7a:115c:a1e0::3"));
        assert!(!s.refresh_devices([&found]));
    }
}

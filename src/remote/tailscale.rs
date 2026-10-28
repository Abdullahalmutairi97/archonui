//! Finds this device and its tailnet peers with `tailscale status --json`.

use super::Device;
use serde_json::Value;
use std::io::{self, Read};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::process::{Command, Output, Stdio};
use std::thread;
use std::time::{Duration, Instant};

/// Tailscale's command line, looked up on PATH.
const BINARY: &str = "tailscale";
/// The macOS app keeps its command line inside the bundle. Tried when `tailscale` is not on PATH.
const MAC_BINARY: &str = "/Applications/Tailscale.app/Contents/MacOS/Tailscale";
/// How long `tailscale status` may take. A stuck daemon must not hold up the app or `--serve`.
const TIMEOUT: Duration = Duration::from_secs(5);

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
/// not installed, not running, logged out, or gives no answer within `TIMEOUT`.
pub fn discover() -> Result<Tailnet, String> {
    let output = match status(BINARY) {
        Err(e) if e.kind() == io::ErrorKind::NotFound => match status(MAC_BINARY) {
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                return Err(NOT_INSTALLED.to_string());
            }
            other => other,
        },
        other => other,
    }
    .map_err(|e| match e.kind() {
        io::ErrorKind::TimedOut => e.to_string(),
        _ => format!("Could not run Tailscale: {}", e),
    })?;

    if !output.status.success() {
        return Err(explain_failure(&String::from_utf8_lossy(&output.stderr)));
    }
    parse(&String::from_utf8_lossy(&output.stdout))
}

const NOT_INSTALLED: &str = "Tailscale is not installed. Get it from tailscale.com/download, sign in, and open this page again";

/// What a `BackendState` other than `Running` means, and what to do about it.
pub fn explain_state(state: &str) -> String {
    match state {
        "NeedsLogin" => "Tailscale is installed but signed out. Sign in with `sudo tailscale up` (or the Tailscale app), using the same account as your other devices".into(),
        "NeedsMachineAuth" => "This device is waiting for approval in the Tailscale admin console (login.tailscale.com/admin/machines)".into(),
        "Stopped" => "Tailscale is turned off on this device. Turn it on with `sudo tailscale up` (or the Tailscale app)".into(),
        "Starting" | "NoState" | "" => "Tailscale is still starting. Give it a few seconds".into(),
        other => format!("Tailscale is not connected (state: {other})"),
    }
}

/// What a failed `tailscale status` means: most often the background service isn't running.
fn explain_failure(stderr: &str) -> String {
    let stderr = stderr.trim();
    let lower = stderr.to_lowercase();
    if lower.contains("tailscaled") || lower.contains("connection refused") || lower.contains("no such file") {
        "Tailscale's background service isn't running. Start it with `sudo systemctl enable --now tailscaled`, then sign in".into()
    } else if lower.contains("logged out") || lower.contains("needslogin") {
        explain_state("NeedsLogin")
    } else if stderr.is_empty() {
        "Tailscale is not running".into()
    } else {
        stderr.to_string()
    }
}

/// Whether ArchonUI answers on a peer's port, from a quick TCP connect.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Probe {
    /// Something listens there: ArchonUI with sharing on, or `archonui --serve`.
    Answers,
    /// The device is up but nothing listens: ArchonUI isn't running there, or its sharing is off.
    Closed,
    /// No answer in time: asleep, or a firewall drops the port on the Tailscale interface.
    Silent,
}

/// How long a probe waits for a peer. Tailscale peers answer in well under this when they are up.
pub const PROBE_TIMEOUT: Duration = Duration::from_millis(1500);

/// Probes `port` on each peer that could run ArchonUI (online, not a phone), all at once. Returns
/// `(device id, result)` for each one probed.
pub fn probe(peers: &[Peer], port: u16) -> Vec<(String, Probe)> {
    let probes: Vec<_> = peers
        .iter()
        .filter(|p| p.online && !matches!(p.os.to_lowercase().as_str(), "ios" | "android"))
        .map(|p| {
            let (id, addr) = (p.device.id.clone(), p.device.addr.clone());
            thread::spawn(move || (id, probe_one(&addr, port, PROBE_TIMEOUT)))
        })
        .collect();
    probes.into_iter().filter_map(|h| h.join().ok()).collect()
}

/// One TCP connect to `addr:port`.
pub fn probe_one(addr: &str, port: u16, timeout: Duration) -> Probe {
    let Ok(ip) = addr.parse::<IpAddr>() else { return Probe::Silent };
    match std::net::TcpStream::connect_timeout(&std::net::SocketAddr::new(ip, port), timeout) {
        Ok(_) => Probe::Answers,
        Err(e) if e.kind() == io::ErrorKind::ConnectionRefused => Probe::Closed,
        Err(_) => Probe::Silent,
    }
}

/// Pure parser for the JSON, so tests need no Tailscale.
pub fn parse(json: &str) -> Result<Tailnet, String> {
    let root: Value =
        serde_json::from_str(json).map_err(|e| format!("Could not read Tailscale status: {}", e))?;

    let state = text(&root, "BackendState");
    if state != "Running" {
        return Err(explain_state(state));
    }

    let self_node = root
        .get("Self")
        .filter(|v| v.is_object())
        .ok_or_else(|| "Tailscale status has no Self device".to_string())?;
    let mut me = node_device(self_node)
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
    unique_names(std::iter::once(&mut me).chain(peers.iter_mut().map(|p| &mut p.device)));

    peers.sort_by(|a, b| {
        b.online
            .cmp(&a.online)
            .then_with(|| a.device.name.to_lowercase().cmp(&b.device.name.to_lowercase()))
            .then_with(|| a.device.id.cmp(&b.device.id))
    });

    Ok(Tailnet { me, peers })
}

/// Devices sharing a host name (two machines both called "ubuntu", say) are named by their MagicDNS
/// label instead, which Tailscale keeps unique ("ubuntu", "ubuntu-1"), so the device list, the
/// sidebar and restored folders can tell them apart.
fn unique_names<'a>(devices: impl Iterator<Item = &'a mut Device>) {
    let mut devices: Vec<&mut Device> = devices.collect();
    let names: Vec<String> = devices.iter().map(|d| d.name.to_lowercase()).collect();
    for (i, d) in devices.iter_mut().enumerate() {
        if names.iter().filter(|n| **n == names[i]).count() > 1 {
            if let Some(label) = d.dns_name.split('.').next().filter(|l| !l.is_empty()) {
                d.name = label.to_string();
            }
        }
    }
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

/// Runs `<program> status --json` and returns its output, or `TimedOut` after `TIMEOUT`.
fn status(program: &str) -> io::Result<Output> {
    let mut cmd = Command::new(program);
    cmd.args(["status", "--json"]);
    output_within(cmd, TIMEOUT)
}

/// `Command::output`, but the command is killed and `TimedOut` returned once `timeout` has passed.
/// The output is read while it runs, so a long one can't fill the pipe and stall it.
fn output_within(mut cmd: Command, timeout: Duration) -> io::Result<Output> {
    let mut child = cmd.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn()?;
    let read = |pipe: Option<Box<dyn Read + Send>>| {
        thread::spawn(move || {
            let mut buf = Vec::new();
            if let Some(mut pipe) = pipe {
                let _ = pipe.read_to_end(&mut buf);
            }
            buf
        })
    };
    let stdout = read(child.stdout.take().map(|p| Box::new(p) as Box<dyn Read + Send>));
    let stderr = read(child.stderr.take().map(|p| Box::new(p) as Box<dyn Read + Send>));
    let deadline = Instant::now() + timeout;
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            let secs = timeout.as_secs_f32();
            return Err(io::Error::new(io::ErrorKind::TimedOut, format!("Tailscale did not answer within {secs} s")));
        }
        thread::sleep(Duration::from_millis(20));
    };
    let stdout = stdout.join().unwrap_or_default();
    let stderr = stderr.join().unwrap_or_default();
    Ok(Output { status, stdout, stderr })
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

/// The first IPv6 address in a node's `TailscaleIPs`, or "" when it has none.
fn ipv6_of(node: &Value) -> String {
    let ips = node.get("TailscaleIPs").and_then(Value::as_array);
    ips.into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .find(|ip| ip.parse::<Ipv6Addr>().is_ok())
        .unwrap_or_default()
        .to_string()
}

/// A tailnet node as a `Device`. `None` when the node has no IPv4 address.
fn node_device(node: &Value) -> Option<Device> {
    let addr = ipv4_of(node)?;
    let addr6 = ipv6_of(node);
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

    Some(Device { id, name, dns_name, addr, addr6 })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::IpAddr;

    #[test]
    fn a_stuck_command_is_given_up_on() {
        let mut cmd = Command::new("sleep");
        cmd.arg("30");
        let started = Instant::now();
        let err = output_within(cmd, Duration::from_millis(200)).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::TimedOut);
        assert!(started.elapsed() < Duration::from_secs(5), "{:?}", started.elapsed());
    }

    #[test]
    fn a_long_output_is_read_whole() {
        // More than a pipe holds, so the command would stall if nothing read it while it ran.
        let mut cmd = Command::new("sh");
        cmd.args(["-c", "head -c 300000 /dev/zero; echo oops >&2; exit 3"]);
        let out = output_within(cmd, Duration::from_secs(20)).unwrap();
        assert_eq!(out.stdout.len(), 300_000);
        assert_eq!(out.stderr, b"oops\n");
        assert_eq!(out.status.code(), Some(3));
    }

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
        assert_eq!(t.me.addr6, "fd7a:115c:a1e0::10");

        // The IPv6-only peer is skipped; the online peer sorts before the offline one.
        assert_eq!(t.peers.len(), 2);
        assert_eq!(
            t.peers[0].device,
            Device {
                id: "n3".into(),
                name: "pc".into(),
                dns_name: "pc.tail1234.ts.net".into(),
                addr: "100.101.102.104".into(),
                addr6: "fd7a:115c:a1e0::3".into(),
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
    fn devices_with_the_same_host_name_get_their_dns_names() {
        let json = r#"{
            "BackendState": "Running",
            "Self": {"ID": "n1", "HostName": "ubuntu", "DNSName": "ubuntu.t.ts.net.", "TailscaleIPs": ["100.64.0.1"]},
            "Peer": {
                "a": {"ID": "n2", "HostName": "Ubuntu", "DNSName": "ubuntu-1.t.ts.net.", "TailscaleIPs": ["100.64.0.2"]},
                "b": {"ID": "n3", "HostName": "pc", "DNSName": "pc.t.ts.net.", "TailscaleIPs": ["100.64.0.3"]}
            }
        }"#;
        let t = parse(json).unwrap();
        assert_eq!(t.me.name, "ubuntu");
        let names: Vec<&str> = t.peers.iter().map(|p| p.device.name.as_str()).collect();
        assert_eq!(names, ["pc", "ubuntu-1"]);
    }

    #[test]
    fn stopped_backend_is_an_error() {
        let json = FIXTURE.replace("\"Running\"", "\"Stopped\"");
        let err = parse(&json).unwrap_err();
        assert!(err.contains("turned off") && err.contains("tailscale up"), "{}", err);
    }

    #[test]
    fn each_tailscale_problem_says_what_to_do() {
        assert!(explain_state("NeedsLogin").contains("signed out"));
        assert!(explain_state("NeedsMachineAuth").contains("admin console"));
        assert!(explain_state("Starting").contains("starting"));
        assert!(explain_state("Weird").contains("Weird"));
        let daemon = "failed to connect to local tailscaled; it doesn't appear to be running (sudo systemctl start tailscaled ?)";
        assert!(explain_failure(daemon).contains("systemctl enable --now tailscaled"));
        assert_eq!(explain_failure(""), "Tailscale is not running");
        assert_eq!(explain_failure("something else\n"), "something else");
    }

    #[test]
    fn a_probe_tells_a_listener_from_a_closed_port() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        assert_eq!(probe_one("127.0.0.1", port, Duration::from_secs(2)), Probe::Answers);
        drop(listener);
        assert_eq!(probe_one("127.0.0.1", port, Duration::from_secs(2)), Probe::Closed);
        assert_eq!(probe_one("not an ip", port, Duration::from_secs(2)), Probe::Silent);
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
        assert_eq!(t.me.addr6, "", "no IPv6 address listed");
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

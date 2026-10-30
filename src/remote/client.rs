//! Client for another device's host (`crate::remote::host`). Connects over the tailnet, sends
//! `ClientMsg`s and streams back `HostMsg`s from a thread of its own. `run` is the
//! `archonui --remote` command line on top of it.

use super::proto::{decode, encode, ClientMsg, HostMsg, VERSION};
use super::{tailscale, Device, DEFAULT_PORT};
use crate::engine::Out;
use std::{
    io::{self, Write},
    net::{TcpStream, ToSocketAddrs},
    path::PathBuf,
    sync::mpsc::{self, TryRecvError},
    thread,
    time::Duration,
};
use tungstenite::{stream::MaybeTlsStream, Message, WebSocket};

type Socket = WebSocket<MaybeTlsStream<TcpStream>>;

/// How long the host has to answer `Hello`, and the WebSocket handshake before it.
const WELCOME_TIMEOUT: Duration = Duration::from_secs(5);
/// How long a TCP connect may take. An offline tailnet device otherwise holds the caller for minutes.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
/// How long the IO thread waits for a frame before it sends what is queued.
const POLL: Duration = Duration::from_millis(50);
/// How long a write may stall before the host is taken as gone. Without it a host that stops reading
/// holds the IO thread for good, and everything sent meanwhile piles up in its queue.
const WRITE_TIMEOUT: Duration = Duration::from_secs(30);

/// What the client's reader thread reports.
pub enum ClientEvent {
    Msg(HostMsg),
    /// The connection ended, with the reason when there was one.
    Closed(Option<String>),
}

/// A connection to one host. Dropping it stops the IO thread.
pub struct Client {
    tx: mpsc::Sender<String>,
    pub events: smol::channel::Receiver<ClientEvent>,
    /// The host's device name, from its `Welcome`.
    pub host: String,
    /// Whether the host is a headless `archonui --serve`, from its `Welcome` (false from older hosts).
    pub serve: bool,
}

impl Client {
    /// Connects to `ws://{addr}:{port}`, sends `Hello` as `this_device`, waits up to 5 s for
    /// `Welcome`, then starts the IO thread.
    pub fn connect(addr: &str, port: u16, this_device: &str) -> Result<Client, String> {
        let stream = connect_tcp(addr, port)?;
        stream.set_read_timeout(Some(WELCOME_TIMEOUT)).map_err(|e| e.to_string())?;
        stream.set_write_timeout(Some(WRITE_TIMEOUT)).map_err(|e| e.to_string())?;
        let url = ws_url(addr, port);
        let (mut ws, _) =
            tungstenite::client::client_with_config(url.as_str(), MaybeTlsStream::Plain(stream), Some(super::proto::ws_config()))
                .map_err(|e| format!("cannot connect to {addr}:{port}: {e}"))?;
        let hello = encode(&ClientMsg::Hello { device: this_device.to_string(), version: VERSION });
        ws.send(Message::Text(hello.into())).map_err(|e| format!("cannot send Hello: {e}"))?;
        let (host, serve) = read_welcome(&mut ws)?;
        set_read_timeout(&ws, Some(POLL))?;

        let (tx, outgoing) = mpsc::channel();
        let (events_tx, events) = smol::channel::unbounded();
        thread::spawn(move || io_loop(ws, outgoing, events_tx));
        Ok(Client { tx, events, host, serve })
    }

    /// Queues `msg` for the host.
    pub fn send(&self, msg: &ClientMsg) -> Result<(), String> {
        self.send_frame(encode(msg))
    }

    /// Queues a message already encoded, e.g. by `proto::put_project_frame` off the UI thread.
    pub fn send_frame(&self, text: String) -> Result<(), String> {
        self.tx.send(text).map_err(|_| "connection closed".to_string())
    }
}

/// The URL of a host: `ws://addr:port`, with an IPv6 address in brackets.
fn ws_url(addr: &str, port: u16) -> String {
    if addr.contains(':') && !addr.starts_with('[') {
        format!("ws://[{addr}]:{port}")
    } else {
        format!("ws://{addr}:{port}")
    }
}

/// Opens a TCP connection to `addr:port`, giving each address it resolves to up to `CONNECT_TIMEOUT`.
fn connect_tcp(addr: &str, port: u16) -> Result<TcpStream, String> {
    let host = addr.trim_start_matches('[').trim_end_matches(']');
    let addrs = (host, port).to_socket_addrs().map_err(|e| format!("cannot find {addr}: {e}"))?;
    let mut error = format!("cannot find {addr}");
    for a in addrs {
        match TcpStream::connect_timeout(&a, CONNECT_TIMEOUT) {
            Ok(stream) => return Ok(stream),
            Err(e) => error = format!("cannot connect to {addr}:{port}: {e}"),
        }
    }
    Err(error)
}

/// Sets the read timeout of a plain (non-TLS) socket. Other streams are left alone.
fn set_read_timeout(ws: &Socket, timeout: Option<Duration>) -> Result<(), String> {
    if let MaybeTlsStream::Plain(s) = ws.get_ref() {
        s.set_read_timeout(timeout).map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Reads the host's answer to `Hello` and returns its device name and whether it is `--serve`.
fn read_welcome(ws: &mut Socket) -> Result<(String, bool), String> {
    // Frames other than text (pings, say) are skipped.
    let text = loop {
        match ws.read() {
            Ok(Message::Text(text)) => break text,
            Ok(_) => {}
            Err(tungstenite::Error::Io(e))
                if matches!(e.kind(), io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut) =>
            {
                return Err("the host did not answer within 5 s".into());
            }
            Err(e) => return Err(format!("handshake failed: {e}")),
        }
    };
    match decode::<HostMsg>(text.as_str())? {
        HostMsg::Welcome { device, version, serve } if version == VERSION => Ok((device, serve)),
        HostMsg::Welcome { version, .. } => Err(format!("host speaks protocol {version}, this client {VERSION}")),
        HostMsg::Error { message, .. } => Err(message),
        _ => Err("the host did not send Welcome".into()),
    }
}

/// Owns the socket: reads host messages into `events` and writes the strings queued on
/// `outgoing`. Ends when the connection closes or the `Client` is dropped.
fn io_loop(mut ws: Socket, outgoing: mpsc::Receiver<String>, events: smol::channel::Sender<ClientEvent>) {
    loop {
        match ws.read() {
            Ok(Message::Text(text)) => match decode::<HostMsg>(text.as_str()) {
                Ok(msg) => {
                    if events.send_blocking(ClientEvent::Msg(msg)).is_err() {
                        return;
                    }
                }
                Err(e) => eprintln!("archonui: {e}"),
            },
            Ok(Message::Close(_)) => {
                let _ = events.send_blocking(ClientEvent::Closed(None));
                return;
            }
            Ok(_) => {}
            Err(tungstenite::Error::Io(e))
                if matches!(e.kind(), io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut) => {}
            Err(e) => {
                let _ = events.send_blocking(ClientEvent::Closed(Some(e.to_string())));
                return;
            }
        }
        loop {
            match outgoing.try_recv() {
                Ok(text) => {
                    if let Err(e) = ws.send(Message::Text(text.into())) {
                        let _ = events.send_blocking(ClientEvent::Closed(Some(e.to_string())));
                        return;
                    }
                }
                Err(TryRecvError::Empty) => break,
                // The `Client` was dropped.
                Err(TryRecvError::Disconnected) => return,
            }
        }
    }
}

/// The command line after `--remote`.
enum Command {
    List,
    Show(String),
    Ask(String, String),
    New(String, String, String),
}

/// `archonui --remote <host> list | show <session> | ask <session> <prompt...> |
/// new <agent> <folder> <prompt...>`. Returns the process exit code.
pub fn run(args: &[String]) -> i32 {
    let Some((target, rest)) = args.split_first() else { return usage() };
    if matches!(target.as_str(), "--help" | "-h") {
        usage();
        return 0;
    }
    let words: Vec<&str> = rest.iter().map(String::as_str).collect();
    let command = match words.as_slice() {
        ["list"] => Command::List,
        ["show", id] => Command::Show(id.to_string()),
        ["ask", id, prompt @ ..] if !prompt.is_empty() => Command::Ask(id.to_string(), prompt.join(" ")),
        ["new", agent, folder, prompt @ ..] if !prompt.is_empty() => {
            Command::New(agent.to_string(), folder.to_string(), prompt.join(" "))
        }
        _ => return usage(),
    };
    let (addr, port) = parse_target(target);
    let addr = match resolve(&addr, port) {
        Ok(addr) => addr,
        Err(e) => return fail(e),
    };
    let me = super::local_host_name().unwrap_or_else(|| "client".to_string());
    let client = match Client::connect(&addr, port, &me) {
        Ok(client) => client,
        Err(e) if e.contains("reset") || e.contains("Connection closed") => {
            return fail(format!("{e}\n  It doesn't let this device in: on it, open Settings → Devices and turn on {me}."))
        }
        Err(e) if e.contains("refused") => {
            return fail(format!("{e}\n  Nothing answers there: open ArchonUI on it with sharing on, or run `archonui --serve`."))
        }
        Err(e) => return fail(e),
    };
    match command {
        Command::List => list(&client),
        Command::Show(id) => show(&client, &id),
        Command::Ask(id, prompt) => ask(&client, &id, &prompt),
        Command::New(agent, folder, prompt) => new_chat(&client, &agent, &folder, &prompt),
    }
}

/// `--devices`: this device and its tailnet peers, marking those where ArchonUI answers, so a new
/// setup can be checked from a terminal (or a server without a screen).
pub fn devices() -> i32 {
    let net = match super::tailscale::discover() {
        Ok(net) => net,
        Err(e) => return fail(format!("{e}.")),
    };
    // The port from Settings → Devices, read without loading (or writing) anything else.
    let settings = crate::app::persist::Paths::current().config.join("settings.json");
    let port = std::fs::read_to_string(settings)
        .ok()
        .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
        .and_then(|v| v.pointer("/sync/port")?.as_u64())
        .and_then(|p| u16::try_from(p).ok())
        .unwrap_or(DEFAULT_PORT);
    println!("this device: {} {}", net.me.name, net.me.addr);
    let probes = super::tailscale::probe(&net.peers, port);
    for p in &net.peers {
        let probe = probes.iter().find(|(id, _)| *id == p.device.id).map(|(_, probe)| *probe);
        let state = match (p.online, probe) {
            (false, _) => "offline",
            (true, Some(super::tailscale::Probe::Answers)) => "runs ArchonUI",
            (true, Some(super::tailscale::Probe::Closed)) => "online, ArchonUI not running",
            (true, Some(super::tailscale::Probe::Silent)) => "online, no answer on the port (firewall?)",
            (true, None) => "online",
        };
        println!("{:<20} {:<16} {:<8} {state}", p.device.name, p.device.addr, p.os);
    }
    0
}

/// Splits `pc`, `pc:9000` or `100.1.2.3:81` into a host and a port. Without a port the
/// default is used. A bare IPv6 address (several colons) is taken whole; `[fd7a::1]:81` has a port.
fn parse_target(s: &str) -> (String, u16) {
    if let Some((host, after)) = s.strip_prefix('[').and_then(|rest| rest.split_once(']')) {
        let port = after.strip_prefix(':').and_then(|p| p.parse().ok()).unwrap_or(DEFAULT_PORT);
        return (host.to_string(), port);
    }
    if let Some((host, port)) = s.rsplit_once(':').filter(|(host, _)| !host.contains(':')) {
        if let Ok(port) = port.parse() {
            return (host.to_string(), port);
        }
    }
    (s.to_string(), DEFAULT_PORT)
}

/// The address to connect to for `host`. A name the system resolves (an IP, or a MagicDNS name) is
/// used as given. Otherwise, as when MagicDNS is off, the tailnet device of that name is looked up
/// with `tailscale status`; only then is Tailscale run.
fn resolve(host: &str, port: u16) -> Result<String, String> {
    if (host, port).to_socket_addrs().is_ok_and(|mut a| a.next().is_some()) {
        return Ok(host.to_string());
    }
    let tailnet = tailscale::discover().map_err(|e| format!("cannot find {host}: it is not a known host name, and {e}"))?;
    find_device(&tailnet, host)
        .map(|d| d.addr.clone())
        .ok_or_else(|| format!("cannot find {host}: no host or tailnet device has that name"))
}

/// The device in `tailnet` named `name`: its host name, its MagicDNS name, or that name's first label.
fn find_device<'a>(tailnet: &'a tailscale::Tailnet, name: &str) -> Option<&'a Device> {
    let name = name.trim_end_matches('.');
    if name.is_empty() {
        return None;
    }
    std::iter::once(&tailnet.me).chain(tailnet.peers.iter().map(|p| &p.device)).find(|d| {
        d.name.eq_ignore_ascii_case(name)
            || d.dns_name.eq_ignore_ascii_case(name)
            || d.dns_name.split('.').next().is_some_and(|label| !label.is_empty() && label.eq_ignore_ascii_case(name))
    })
}

fn usage() -> i32 {
    eprintln!("usage: archonui --remote <host>[:port] list");
    eprintln!("       archonui --remote <host>[:port] show <session>");
    eprintln!("       archonui --remote <host>[:port] ask <session> <prompt...>");
    eprintln!("       archonui --remote <host>[:port] new <agent> <folder> <prompt...>");
    eprintln!("<host> is a tailnet device's name or address; the port defaults to {DEFAULT_PORT}.");
    eprintln!("<folder> is on the host; quote '~' for the host's home.");
    2
}

fn fail(message: String) -> i32 {
    eprintln!("archonui: {message}");
    1
}

fn closed(reason: Option<String>) -> String {
    match reason {
        Some(reason) => format!("connection closed: {reason}"),
        None => "connection closed".to_string(),
    }
}

/// Waits for the first host reply that `pick` accepts. A host `Error` or a closed connection
/// ends the wait with an error. Other messages are skipped.
fn await_reply<T>(client: &Client, mut pick: impl FnMut(HostMsg) -> Option<T>) -> Result<T, String> {
    loop {
        match smol::block_on(client.events.recv()) {
            Ok(ClientEvent::Msg(HostMsg::Error { message, .. })) => return Err(message),
            Ok(ClientEvent::Msg(msg)) => {
                if let Some(value) = pick(msg) {
                    return Ok(value);
                }
            }
            Ok(ClientEvent::Closed(reason)) => return Err(closed(reason)),
            Err(_) => return Err(closed(None)),
        }
    }
}

fn list(client: &Client) -> i32 {
    if let Err(e) = client.send(&ClientMsg::ListSessions) {
        return fail(e);
    }
    match await_reply(client, |m| match m {
        HostMsg::Sessions { sessions } => Some(sessions),
        _ => None,
    }) {
        Ok(sessions) => {
            for s in sessions {
                let busy = if s.busy { "busy" } else { "    " };
                println!("{:<14} {busy} {:<8} {}", s.id, s.agent, s.title);
            }
            0
        }
        Err(e) => fail(e),
    }
}

/// Subscribes to a chat and returns its `store::Session` as JSON.
fn subscribe(client: &Client, id: &str) -> Result<serde_json::Value, String> {
    client.send(&ClientMsg::Subscribe { session: id.to_string() })?;
    await_reply(client, |m| match m {
        HostMsg::Snapshot { session, .. } => Some(session),
        _ => None,
    })
}

fn show(client: &Client, id: &str) -> i32 {
    match subscribe(client, id) {
        Ok(session) => {
            for m in session["messages"].as_array().into_iter().flatten() {
                println!("{}: {}", m["role"].as_str().unwrap_or("?"), m["text"].as_str().unwrap_or_default());
                println!();
            }
            0
        }
        Err(e) => fail(e),
    }
}

fn ask(client: &Client, id: &str, prompt: &str) -> i32 {
    let sent = subscribe(client, id).and_then(|_| {
        client.send(&ClientMsg::Prompt {
            session: id.to_string(),
            text: prompt.to_string(),
            mode: None,
            model: None,
            effort: None,
            default_model: false,
            default_effort: false,
            files: Vec::new(),
        })
    });
    match sent {
        Ok(()) => stream_turn(client, id),
        Err(e) => fail(e),
    }
}

fn new_chat(client: &Client, agent: &str, folder: &str, prompt: &str) -> i32 {
    let created = client
        .send(&ClientMsg::NewSession { agent: agent.to_string(), cwd: PathBuf::from(folder), project: None })
        .and_then(|_| {
            await_reply(client, |m| match m {
                HostMsg::Created { session } => Some(session),
                _ => None,
            })
        });
    match created {
        Ok(summary) => {
            eprintln!("created {}", summary.id);
            ask(client, &summary.id, prompt)
        }
        Err(e) => fail(e),
    }
}

/// Prints a chat's events until the host says the turn finished. Returns the exit code.
fn stream_turn(client: &Client, id: &str) -> i32 {
    // `Final` repeats the whole reply; it's printed only when nothing was streamed.
    let mut streamed = false;
    loop {
        match smol::block_on(client.events.recv()) {
            Ok(ClientEvent::Msg(HostMsg::Event { session, out: Some(out) })) if session == id => match out {
                Out::Text(t) => {
                    streamed = true;
                    print!("{t}");
                    let _ = io::stdout().flush();
                }
                Out::Final(t) if !streamed => {
                    print!("{t}");
                    let _ = io::stdout().flush();
                }
                Out::ToolStart { kind, target, .. } => eprintln!("· {kind:?} {target}"),
                Out::Error(e) => eprintln!("error: {e}"),
                _ => {}
            },
            Ok(ClientEvent::Msg(HostMsg::Finished { session, error, stopped })) if session == id => {
                println!();
                return match error {
                    _ if stopped => fail("stopped".into()),
                    Some(e) => fail(e),
                    None => 0,
                };
            }
            // This command line can't answer; say so rather than sit silent until the host times it out.
            Ok(ClientEvent::Msg(HostMsg::Approval { session, tool, .. })) if session == id => {
                eprintln!("· {tool} waits for approval; answer it in the app on a device following this chat");
            }
            Ok(ClientEvent::Msg(HostMsg::Error { message, .. })) => return fail(message),
            Ok(ClientEvent::Closed(reason)) => return fail(closed(reason)),
            Ok(_) => {}
            Err(_) => return fail(closed(None)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn targets_take_an_optional_port() {
        assert_eq!(parse_target("pc"), ("pc".to_string(), 7420));
        assert_eq!(parse_target("pc:9000"), ("pc".to_string(), 9000));
        assert_eq!(parse_target("100.1.2.3"), ("100.1.2.3".to_string(), DEFAULT_PORT));
        assert_eq!(parse_target("100.1.2.3:81"), ("100.1.2.3".to_string(), 81));
        assert_eq!(parse_target("pc:x"), ("pc:x".to_string(), DEFAULT_PORT));
    }

    /// A project copy bigger than tungstenite's default 16 MiB frame gets through both ways.
    #[test]
    fn large_project_copies_get_through() {
        use super::super::{hub::Hub, SyncSettings};
        // A free port can be taken by another test before the hub binds it, so try a few.
        let (port, (hub, requests)) = (0..20)
            .find_map(|_| {
                let port = std::net::TcpListener::bind("127.0.0.1:0").ok()?.local_addr().ok()?.port();
                let bind = std::net::SocketAddr::from(([127, 0, 0, 1], port));
                Hub::start(bind, "host".into(), false, SyncSettings::default()).ok().map(|h| (port, h))
            })
            .expect("a free port");
        let client = Client::connect("127.0.0.1", port, "me").unwrap();
        let big = "A".repeat(20 << 20);

        let msg = ClientMsg::PutProject { project: "p".into(), name: "p".into(), device: "n1".into(), commit: "abcd".into(), bundle: big.clone() };
        client.send(&msg).unwrap();
        let (tx, rx) = mpsc::channel();
        thread::spawn(move || {
            let _ = tx.send(requests.recv_blocking());
        });
        let req = rx.recv_timeout(Duration::from_secs(20)).expect("the host got the copy").unwrap();
        match req.msg {
            ClientMsg::PutProject { bundle, .. } => assert_eq!(bundle.len(), big.len()),
            other => panic!("unexpected {other:?}"),
        }

        let reply = HostMsg::ProjectData { project: "p".into(), device: "n1".into(), commit: "abcd".into(), bundle: big.clone() };
        hub.reply(req.conn, &reply);
        let events = client.events.clone();
        let (tx, rx) = mpsc::channel();
        thread::spawn(move || {
            let _ = tx.send(events.recv_blocking());
        });
        match rx.recv_timeout(Duration::from_secs(20)).expect("the client got the copy").unwrap() {
            ClientEvent::Msg(HostMsg::ProjectData { bundle, .. }) => assert_eq!(bundle.len(), big.len()),
            ClientEvent::Msg(other) => panic!("unexpected {other:?}"),
            ClientEvent::Closed(e) => panic!("closed: {e:?}"),
        }
    }

    #[test]
    fn ipv6_hosts_are_bracketed_in_the_url() {
        assert_eq!(parse_target("fd7a:115c:a1e0::5"), ("fd7a:115c:a1e0::5".to_string(), DEFAULT_PORT));
        assert_eq!(parse_target("[fd7a:115c:a1e0::5]:81"), ("fd7a:115c:a1e0::5".to_string(), 81));
        assert_eq!(parse_target("[::1]"), ("::1".to_string(), DEFAULT_PORT));
        assert_eq!(ws_url("fd7a:115c:a1e0::5", 7420), "ws://[fd7a:115c:a1e0::5]:7420");
        assert_eq!(ws_url("100.1.2.3", 7420), "ws://100.1.2.3:7420");
        assert_eq!(ws_url("pc", 81), "ws://pc:81");
    }

    #[test]
    fn connects_over_ipv6() {
        let Ok(listener) = std::net::TcpListener::bind("[::1]:0") else { return };
        let port = listener.local_addr().unwrap().port();
        drop(listener);
        let Ok((_hub, _requests)) = crate::remote::hub::Hub::start(format!("[::1]:{port}").parse().unwrap(), "host".into(), false, Default::default()) else {
            return;
        };
        let client = Client::connect("::1", port, "me").unwrap();
        assert_eq!(client.host, "host");
    }

    #[test]
    fn a_silent_host_times_out() {
        // Takes the TCP connection (the backlog does) but never answers the WebSocket handshake.
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let started = std::time::Instant::now();
        assert!(Client::connect("127.0.0.1", port, "test").is_err());
        assert!(started.elapsed() < Duration::from_secs(15));
    }

    #[test]
    fn devices_are_found_by_host_or_magicdns_name() {
        let device = |name: &str, dns: &str, addr: &str| Device { id: addr.into(), name: name.into(), dns_name: dns.into(), addr: addr.into(), addr6: String::new() };
        let peer = |d: Device| tailscale::Peer { device: d, online: true, os: "linux".into() };
        let tailnet = tailscale::Tailnet {
            me: device("minipc", "minipc.tail1.ts.net", "100.0.0.1"),
            peers: vec![
                peer(device("Winbox", "winbox-1.tail1.ts.net", "100.0.0.2")),
                peer(device("ashpc", "", "100.0.0.3")),
            ],
        };
        let addr = |name| find_device(&tailnet, name).map(|d| d.addr.as_str());
        assert_eq!(addr("minipc"), Some("100.0.0.1"));
        assert_eq!(addr("winbox"), Some("100.0.0.2"), "host name, any case");
        assert_eq!(addr("winbox-1"), Some("100.0.0.2"), "MagicDNS label");
        assert_eq!(addr("winbox-1.tail1.ts.net."), Some("100.0.0.2"), "full MagicDNS name");
        assert_eq!(addr("ashpc"), Some("100.0.0.3"), "MagicDNS off");
        assert_eq!(addr("nothere"), None);
        assert_eq!(addr(""), None);
    }
}

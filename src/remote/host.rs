//! The host side of sharing chats between devices: `archonui --serve` listens on this device's
//! Tailscale address and lets other devices list chats, send prompts and follow their replies.
//! The agents keep running here. A client only sends prompts and receives the streamed events.
//! The messages are in `proto`.

use super::{
    proto::{self, ClientMsg, HostMsg, SessionSummary},
    tailscale, SyncSettings,
};
use crate::{
    app::{
        persist::Paths,
        state::{AppState, Placement},
    },
    engine::{self, DetectedAgent, Event, Mode},
    store::{self, Session},
};
use std::{
    collections::HashMap,
    io,
    net::{IpAddr, SocketAddr, TcpListener, TcpStream},
    sync::{mpsc, Arc, Mutex, MutexGuard},
    thread,
    time::Duration,
};

/// How long a new connection may take to send its WebSocket handshake.
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);
/// How often a connection checks its queue while the client is quiet.
const POLL: Duration = Duration::from_millis(50);

/// What every connection shares. Locked briefly, and never across a socket write.
struct Hub {
    state: AppState,
    /// Agents found on this device when the host started.
    agents: Vec<DetectedAgent>,
    /// The host name sent in `Welcome`.
    device: String,
    /// Connection ids subscribed to each chat.
    subs: HashMap<String, Vec<u64>>,
    /// The outgoing queue of each open connection, by connection id.
    conns: HashMap<u64, mpsc::Sender<String>>,
    next_conn: u64,
}

type Shared = Arc<Mutex<Hub>>;

fn lock(shared: &Shared) -> MutexGuard<'_, Hub> {
    shared.lock().unwrap_or_else(|e| e.into_inner())
}

impl Hub {
    /// Queues `msg` for every connection subscribed to `session`, except `except`. Connections whose
    /// queue is gone are unsubscribed.
    fn broadcast(&mut self, session: &str, msg: &HostMsg, except: Option<u64>) {
        let text = proto::encode(msg);
        let Some(ids) = self.subs.get(session) else { return };
        let mut gone = Vec::new();
        for id in ids.iter().copied().filter(|id| Some(*id) != except) {
            let sent = self.conns.get(&id).is_some_and(|tx| tx.send(text.clone()).is_ok());
            if !sent {
                gone.push(id);
            }
        }
        if let Some(ids) = self.subs.get_mut(session) {
            ids.retain(|id| !gone.contains(id));
        }
    }

    /// Forgets a connection that has closed.
    fn disconnect(&mut self, conn: u64) {
        self.conns.remove(&conn);
        for ids in self.subs.values_mut() {
            ids.retain(|id| *id != conn);
        }
        self.subs.retain(|_, ids| !ids.is_empty());
    }
}

/// Saves the chats that changed, and prints the files that could not be written.
fn flush(hub: &mut Hub) {
    for e in hub.state.flush_sessions() {
        eprintln!("archonui host: {e}");
    }
}

/// Serves this device's chats on `bind` until the process ends. Loads the settings and chats itself.
pub fn serve(bind: SocketAddr) -> io::Result<()> {
    let device = host_name(&tailscale::discover());
    listen(bind, device, AppState::load(&Paths::current()))
}

/// `archonui --serve [--bind ADDR:PORT]`. Without `--bind` the host listens on this device's
/// Tailscale address and the saved port, and only when Devices sharing is on. Returns the exit code.
pub fn run(args: &[String]) -> i32 {
    let mut bind = None;
    let mut rest = args.iter();
    while let Some(arg) = rest.next() {
        match arg.as_str() {
            "--bind" => {
                let Some(value) = rest.next() else {
                    eprintln!("usage: archonui --serve [--bind ADDR:PORT]");
                    return 2;
                };
                match value.parse::<SocketAddr>() {
                    Ok(addr) => bind = Some(addr),
                    Err(e) => {
                        eprintln!("bad --bind {value}: {e}");
                        return 2;
                    }
                }
            }
            other => {
                eprintln!("unknown option {other}; usage: archonui --serve [--bind ADDR:PORT]");
                return 2;
            }
        }
    }
    let state = AppState::load(&Paths::current());
    let tailnet = tailscale::discover();
    let addr = match bind {
        Some(addr) => addr,
        None if !state.settings.sync.enabled => {
            eprintln!("Devices sharing is off. Turn it on in settings, or pass --bind ADDR:PORT.");
            return 1;
        }
        None => match &tailnet {
            Ok(t) => match t.me.addr.parse::<IpAddr>() {
                Ok(ip) => SocketAddr::new(ip, state.settings.sync.port),
                Err(e) => {
                    eprintln!("this device's Tailscale address {} is not an IP address: {e}", t.me.addr);
                    return 1;
                }
            },
            Err(e) => {
                eprintln!("{e}");
                return 1;
            }
        },
    };
    let device = host_name(&tailnet);
    match listen(addr, device, state) {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("archonui host: {e}");
            1
        }
    }
}

/// The name a host shows its clients: this device's Tailscale host name, else `HOSTNAME`, else "host".
fn host_name(tailnet: &Result<tailscale::Tailnet, String>) -> String {
    tailnet
        .as_ref()
        .map(|t| t.me.name.clone())
        .ok()
        .filter(|n| !n.is_empty())
        .or_else(|| std::env::var("HOSTNAME").ok().filter(|n| !n.is_empty()))
        .unwrap_or_else(|| "host".into())
}

/// Whether a peer at `ip` may connect. Loopback always may, for local testing. Anyone else must be
/// a Tailscale address, and when the user picked devices, one of them.
fn peer_allowed(ip: IpAddr, sync: &SyncSettings, tailscale_peer: bool) -> bool {
    if ip.is_loopback() {
        return true;
    }
    tailscale_peer && (sync.devices.is_empty() || sync.allows(&ip.to_string()))
}

/// The list entry for chat `s`.
fn summary(s: &Session, busy: bool) -> SessionSummary {
    SessionSummary {
        id: s.id.clone(),
        title: s.title.clone(),
        agent: s.agent.clone(),
        project: s.project.clone(),
        cwd: s.cwd.clone(),
        updated: s.updated,
        busy,
        messages: s.messages.len(),
    }
}

fn listen(bind: SocketAddr, device: String, state: AppState) -> io::Result<()> {
    let listener = TcpListener::bind(bind)?;
    let harness_settings = store::Settings { harnesses: state.settings.harnesses.clone(), ..Default::default() };
    let agents = engine::detect(&harness_settings);
    let shared: Shared = Arc::new(Mutex::new(Hub {
        state,
        agents,
        device,
        subs: HashMap::new(),
        conns: HashMap::new(),
        next_conn: 1,
    }));
    eprintln!("archonui host listening on {bind}");
    for stream in listener.incoming() {
        let stream = match stream {
            Ok(stream) => stream,
            Err(e) => {
                eprintln!("archonui host: could not accept a connection: {e}");
                continue;
            }
        };
        let Ok(ip) = stream.peer_addr().map(|a| a.ip()) else { continue };
        let allowed = peer_allowed(ip, &lock(&shared).state.settings.sync, tailscale::is_tailscale_addr(&ip));
        if !allowed {
            eprintln!("archonui host: refused a connection from {ip}");
            continue;
        }
        let shared = Arc::clone(&shared);
        thread::spawn(move || connection(stream, shared));
    }
    Ok(())
}

/// Runs one client connection until it closes: the handshake, then each message in and each queued
/// message out, in order.
fn connection(stream: TcpStream, shared: Shared) {
    if stream.set_read_timeout(Some(HANDSHAKE_TIMEOUT)).is_err() {
        return;
    }
    let mut ws = match tungstenite::accept(stream) {
        Ok(ws) => ws,
        Err(e) => {
            eprintln!("archonui host: handshake failed: {e}");
            return;
        }
    };
    if ws.get_ref().set_read_timeout(Some(POLL)).is_err() {
        return;
    }
    let (tx, rx) = mpsc::channel::<String>();
    let conn = {
        let mut hub = lock(&shared);
        let id = hub.next_conn;
        hub.next_conn += 1;
        hub.conns.insert(id, tx.clone());
        id
    };
    let mut hello = false;
    let mut closing = false;
    'conn: loop {
        while let Ok(text) = rx.try_recv() {
            if ws.send(tungstenite::Message::text(text)).is_err() {
                break 'conn;
            }
        }
        if closing {
            break;
        }
        match ws.read() {
            Ok(tungstenite::Message::Text(text)) => {
                closing = handle(&shared, conn, &tx, &mut hello, text.as_str()) == Next::Close;
            }
            Ok(tungstenite::Message::Close(_)) => break,
            Ok(_) => {}
            Err(tungstenite::Error::Io(e)) if matches!(e.kind(), io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut) => {}
            Err(_) => break,
        }
    }
    let _ = ws.close(None);
    lock(&shared).disconnect(conn);
}

/// Whether a connection goes on after one message.
#[derive(Debug, PartialEq, Eq)]
enum Next {
    Continue,
    Close,
}

/// Queues `msg` for this connection's writer.
fn queue(me: &mpsc::Sender<String>, msg: &HostMsg) {
    let _ = me.send(proto::encode(msg));
}

/// Handles one client message. Replies are queued on `me`, so they go out in order with events.
fn handle(shared: &Shared, conn: u64, me: &mpsc::Sender<String>, hello: &mut bool, text: &str) -> Next {
    let msg = match proto::decode::<ClientMsg>(text) {
        Ok(msg) => msg,
        Err(message) => {
            queue(me, &HostMsg::Error { message });
            return if *hello { Next::Continue } else { Next::Close };
        }
    };
    if !*hello {
        return match msg {
            ClientMsg::Hello { version, .. } if version == proto::VERSION => {
                *hello = true;
                let device = lock(shared).device.clone();
                queue(me, &HostMsg::Welcome { device, version: proto::VERSION });
                Next::Continue
            }
            ClientMsg::Hello { version, .. } => {
                queue(
                    me,
                    &HostMsg::Error { message: format!("Protocol version {version} does not match this host's {}", proto::VERSION) },
                );
                Next::Close
            }
            _ => {
                queue(me, &HostMsg::Error { message: "Say hello first".into() });
                Next::Close
            }
        };
    }
    let mut hub = lock(shared);
    match msg {
        ClientMsg::Hello { .. } => queue(me, &HostMsg::Error { message: "Already said hello".into() }),
        ClientMsg::ListSessions => {
            let mut list: Vec<&Session> = hub.state.sessions().iter().filter(|s| s.visible()).collect();
            list.sort_by(|a, b| b.updated.cmp(&a.updated));
            let sessions = list.into_iter().map(|s| summary(s, hub.state.busy(&s.id))).collect();
            queue(me, &HostMsg::Sessions { sessions });
        }
        ClientMsg::Subscribe { session } => {
            let busy = hub.state.busy(&session);
            match hub.state.session(&session).map(serde_json::to_value) {
                Some(Ok(value)) => {
                    let ids = hub.subs.entry(session).or_default();
                    if !ids.contains(&conn) {
                        ids.push(conn);
                    }
                    queue(me, &HostMsg::Snapshot { session: value, busy });
                }
                Some(Err(e)) => queue(me, &HostMsg::Error { message: format!("Could not send the chat: {e}") }),
                None => queue(me, &HostMsg::Error { message: format!("There is no chat {session} on this device") }),
            }
        }
        ClientMsg::Unsubscribe { session } => {
            if let Some(ids) = hub.subs.get_mut(&session) {
                ids.retain(|id| *id != conn);
            }
        }
        ClientMsg::NewSession { agent, cwd, project } => {
            if !hub.agents.iter().any(|a| a.enabled && a.harness.id() == agent.as_str()) {
                let message = format!("{} is not set up on this device", engine::agent_name(&agent));
                queue(me, &HostMsg::Error { message });
            } else {
                let id = hub.state.new_session(&agent, cwd, Placement::Focused);
                if project.is_some() {
                    hub.state.move_to_project(&id, project);
                }
                flush(&mut hub);
                if let Some(s) = hub.state.session(&id) {
                    let entry = summary(s, false);
                    queue(me, &HostMsg::Created { session: entry });
                }
            }
        }
        ClientMsg::Prompt { session, text, mode, model, effort } => {
            if let Err(message) = start_prompt(&mut hub, shared, &session, &text, mode, model, effort) {
                queue(me, &HostMsg::Error { message });
            }
        }
        ClientMsg::Cancel { session } => {
            if !hub.state.stop(&session) {
                queue(me, &HostMsg::Error { message: "No reply is running in that chat".into() });
            }
        }
        ClientMsg::Draft { session, text } => {
            if hub.state.session(&session).is_none() {
                queue(me, &HostMsg::Error { message: format!("There is no chat {session} on this device") });
            } else {
                hub.state.set_draft(&session, &text);
                flush(&mut hub);
                hub.broadcast(&session, &HostMsg::Draft { session: session.clone(), text }, Some(conn));
            }
        }
    }
    Next::Continue
}

/// Starts a turn in `session` with `text`, first applying the options the client sent. Errors are
/// for the client who asked. A turn that cannot start is reported to the chat's subscribers instead.
fn start_prompt(
    hub: &mut Hub,
    shared: &Shared,
    session: &str,
    text: &str,
    mode: Option<Mode>,
    model: Option<String>,
    effort: Option<String>,
) -> Result<(), String> {
    let (cur_mode, cur_model, cur_effort) = match hub.state.session(session) {
        Some(s) => (s.mode, s.model.clone(), s.effort.clone()),
        None => return Err(format!("There is no chat {session} on this device")),
    };
    if hub.state.busy(session) {
        return Err("A reply is already running".into());
    }
    if text.trim().is_empty() {
        return Err("The prompt is empty".into());
    }
    if mode.is_some() || model.is_some() || effort.is_some() {
        hub.state.set_options(session, model.or(cur_model), effort.or(cur_effort), mode.unwrap_or(cur_mode));
    }
    let Some(rx) = hub.state.send(session, text, &hub.agents) else {
        // `send` has put the reason in the chat when it could.
        flush(hub);
        let error = Some("Could not start the agent".to_string());
        hub.broadcast(session, &HostMsg::Finished { session: session.to_string(), error }, None);
        return Ok(());
    };
    flush(hub);
    hub.broadcast(session, &HostMsg::Started { session: session.to_string(), text: text.to_string() }, None);
    let shared = Arc::clone(shared);
    let id = session.to_string();
    thread::spawn(move || pump(shared, id, rx));
    Ok(())
}

/// Folds one turn's events into its chat and forwards them to the chat's subscribers, until the
/// turn's channel closes.
fn pump(shared: Shared, session: String, rx: smol::channel::Receiver<Event>) {
    while let Ok(ev) = smol::block_on(rx.recv()) {
        let mut guard = lock(&shared);
        let hub: &mut Hub = &mut guard;
        match ev {
            Event::Out(out) => {
                let msg = HostMsg::Event { session: session.clone(), out: out.clone() };
                hub.state.apply(&session, Event::Out(out));
                hub.broadcast(&session, &msg, None);
            }
            Event::Finished(result) => {
                let error = result.as_ref().err().cloned();
                hub.state.apply(&session, Event::Finished(result));
                flush(hub);
                hub.broadcast(&session, &HostMsg::Finished { session: session.clone(), error }, None);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::remote::Device;
    use std::{net::Ipv4Addr, path::PathBuf};

    fn device(addr: &str) -> Device {
        Device { id: "n1".into(), name: "pc".into(), dns_name: String::new(), addr: addr.into() }
    }

    #[test]
    fn loopback_is_always_allowed() {
        let none = SyncSettings::default();
        let picked = SyncSettings { enabled: true, devices: vec![device("100.1.2.3")], ..Default::default() };
        let lo = IpAddr::V4(Ipv4Addr::LOCALHOST);
        assert!(peer_allowed(lo, &none, false));
        assert!(peer_allowed(lo, &picked, false));
        assert!(peer_allowed(IpAddr::V6(std::net::Ipv6Addr::LOCALHOST), &picked, false));
    }

    #[test]
    fn non_tailscale_peers_are_refused() {
        let none = SyncSettings::default();
        let ip: IpAddr = "192.168.1.5".parse().unwrap();
        assert!(!peer_allowed(ip, &none, false));
    }

    #[test]
    fn tailscale_peers_are_allowed_only_when_picked() {
        let open = SyncSettings::default();
        let picked = SyncSettings { enabled: true, devices: vec![device("100.1.2.3")], ..Default::default() };
        let listed: IpAddr = "100.1.2.3".parse().unwrap();
        let other: IpAddr = "100.9.9.9".parse().unwrap();
        assert!(peer_allowed(other, &open, true), "no devices picked: any tailnet peer may connect");
        assert!(peer_allowed(listed, &picked, true));
        assert!(!peer_allowed(other, &picked, true));
    }

    #[test]
    fn summary_copies_the_chat_list_fields() {
        let mut s = Session::new("claude", PathBuf::from("/work/repo"));
        s.title = "Fix the build".into();
        s.project = Some("p1".into());
        s.updated = 42;
        let entry = summary(&s, true);
        assert_eq!(entry.id, s.id);
        assert_eq!(entry.title, "Fix the build");
        assert_eq!(entry.agent, "claude");
        assert_eq!(entry.project.as_deref(), Some("p1"));
        assert_eq!(entry.cwd, PathBuf::from("/work/repo"));
        assert_eq!(entry.updated, 42);
        assert!(entry.busy);
        assert_eq!(entry.messages, 0);
    }
}

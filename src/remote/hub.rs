//! The networking side of a host: accepts client connections, does the Hello/Welcome handshake and
//! queues replies. It knows nothing about chats. Every later client message goes out as a `Request`
//! for whoever owns the chats (the headless `host` driver, or the desktop window).

use super::{
    proto::{self, ClientMsg, HostMsg},
    tailscale, SyncSettings,
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

/// A client message from connection `conn`, for whoever owns the chats.
#[derive(Clone, Debug)]
pub struct Request {
    pub conn: u64,
    pub msg: ClientMsg,
}

/// The state the accept loop and every connection share. Locked briefly, and never across a socket write.
struct Inner {
    /// The host name sent in `Welcome`.
    device: String,
    /// Who may connect. Checked for each new connection.
    sync: SyncSettings,
    /// Connection ids subscribed to each chat.
    subs: HashMap<String, Vec<u64>>,
    /// Each open connection, by connection id. A connection whose entry goes ends within `POLL`.
    conns: HashMap<u64, Conn>,
    next_conn: u64,
    /// Set by `stop`: the listener and every connection close.
    stopped: bool,
}

/// One open connection: its outgoing queue and the peer's address.
struct Conn {
    tx: mpsc::Sender<String>,
    ip: IpAddr,
}

/// A host's connections. Cheap to clone; all clones share the same connections.
#[derive(Clone)]
pub struct Hub {
    inner: Arc<Mutex<Inner>>,
    /// The address actually bound: differs from the one asked for when that had port 0.
    local: SocketAddr,
}

impl Hub {
    /// Binds `bind`, accepts connections on a background thread (same peer checks as before, using
    /// `sync`), does the Hello/Welcome handshake itself, and forwards every later message as a
    /// `Request` on the returned channel. Returns once bound, so a bind error comes back here.
    pub fn start(bind: SocketAddr, device: String, sync: SyncSettings) -> io::Result<(Hub, smol::channel::Receiver<Request>)> {
        let listener = TcpListener::bind(bind)?;
        let hub = Hub {
            inner: Arc::new(Mutex::new(Inner {
                device,
                sync,
                subs: HashMap::new(),
                conns: HashMap::new(),
                next_conn: 1,
                stopped: false,
            })),
            local: listener.local_addr()?,
        };
        // Polled, so `stop` can close the listener.
        listener.set_nonblocking(true)?;
        let (requests, rx) = smol::channel::unbounded();
        let accepting = hub.clone();
        thread::spawn(move || accept(listener, accepting, requests));
        Ok((hub, rx))
    }

    /// The address the host listens on, with the real port when it was started on port 0.
    pub fn local_addr(&self) -> SocketAddr {
        self.local
    }

    /// Queues `msg` for one connection. Nothing happens if it has closed.
    pub fn reply(&self, conn: u64, msg: &HostMsg) {
        let text = proto::encode(msg);
        if let Some(c) = self.lock().conns.get(&conn) {
            let _ = c.tx.send(text);
        }
    }

    /// Queues `msg` for every connection subscribed to `session`, except `except`. Connections whose
    /// queue is gone are unsubscribed.
    pub fn broadcast(&self, session: &str, msg: &HostMsg, except: Option<u64>) {
        let text = proto::encode(msg);
        let mut guard = self.lock();
        let inner = &mut *guard;
        let Some(ids) = inner.subs.get(session) else { return };
        let mut gone = Vec::new();
        for id in ids.iter().copied().filter(|id| Some(*id) != except) {
            let sent = inner.conns.get(&id).is_some_and(|c| c.tx.send(text.clone()).is_ok());
            if !sent {
                gone.push(id);
            }
        }
        if let Some(ids) = inner.subs.get_mut(session) {
            ids.retain(|id| !gone.contains(id));
        }
    }

    /// Sends `conn` the events of `session` from now on.
    pub fn subscribe(&self, conn: u64, session: &str) {
        let mut guard = self.lock();
        let ids = guard.subs.entry(session.to_string()).or_default();
        if !ids.contains(&conn) {
            ids.push(conn);
        }
    }

    /// Stops sending `conn` the events of `session`.
    pub fn unsubscribe(&self, conn: u64, session: &str) {
        if let Some(ids) = self.lock().subs.get_mut(session) {
            ids.retain(|id| *id != conn);
        }
    }

    /// Replaces the settings the peer check uses from now on. Open connections the new settings
    /// refuse (sharing turned off, the device no longer included) are closed.
    pub fn set_sync(&self, sync: SyncSettings) {
        let mut guard = self.lock();
        let inner = &mut *guard;
        inner.conns.retain(|_, c| peer_allowed(c.ip, &sync, tailscale::is_tailscale_addr(&c.ip)));
        inner.sync = sync;
    }

    /// Stops listening and closes every connection. The port is free within `POLL`.
    pub fn stop(&self) {
        let mut guard = self.lock();
        guard.stopped = true;
        guard.conns.clear();
        guard.subs.clear();
    }

    /// Whether `other` is a clone of this hub.
    pub fn same(&self, other: &Hub) -> bool {
        Arc::ptr_eq(&self.inner, &other.inner)
    }

    /// Whether connection `conn` may go on.
    fn is_open(&self, conn: u64) -> bool {
        self.lock().conns.contains_key(&conn)
    }

    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Records a new connection's queue and returns its id.
    fn add_conn(&self, tx: mpsc::Sender<String>, ip: IpAddr) -> Option<u64> {
        let mut guard = self.lock();
        if guard.stopped {
            return None;
        }
        let id = guard.next_conn;
        guard.next_conn += 1;
        guard.conns.insert(id, Conn { tx, ip });
        Some(id)
    }

    /// Forgets a connection that has closed.
    fn disconnect(&self, conn: u64) {
        let mut guard = self.lock();
        let inner = &mut *guard;
        inner.conns.remove(&conn);
        for ids in inner.subs.values_mut() {
            ids.retain(|id| *id != conn);
        }
        inner.subs.retain(|_, ids| !ids.is_empty());
    }
}

/// Whether a peer at `ip` may connect. Loopback always may, for local testing. Anyone else needs
/// sharing on, a Tailscale address, and when the user picked devices, one of them. An IPv4 peer seen
/// on an IPv6 socket (`::ffff:a.b.c.d`) is checked as the IPv4 address it is.
fn peer_allowed(ip: IpAddr, sync: &SyncSettings, tailscale_peer: bool) -> bool {
    let ip = ip.to_canonical();
    if ip.is_loopback() {
        return true;
    }
    sync.enabled && tailscale_peer && (sync.devices.is_empty() || sync.allows(&ip.to_string()))
}

/// Accepts connections until `Hub::stop`, one thread each. Dropping the listener frees the port.
fn accept(listener: TcpListener, hub: Hub, requests: smol::channel::Sender<Request>) {
    loop {
        if hub.lock().stopped {
            return;
        }
        let stream = match listener.accept() {
            Ok((stream, _)) => stream,
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                thread::sleep(POLL);
                continue;
            }
            Err(e) => {
                eprintln!("archonui host: could not accept a connection: {e}");
                thread::sleep(POLL);
                continue;
            }
        };
        // Some platforms hand out accepted sockets in the listener's non-blocking mode.
        if stream.set_nonblocking(false).is_err() {
            continue;
        }
        let Ok(ip) = stream.peer_addr().map(|a| a.ip()) else { continue };
        let allowed = peer_allowed(ip, &hub.lock().sync, tailscale::is_tailscale_addr(&ip));
        if !allowed {
            eprintln!("archonui host: refused a connection from {ip}");
            continue;
        }
        let hub = hub.clone();
        let requests = requests.clone();
        thread::spawn(move || connection(stream, ip, hub, requests));
    }
}

/// Runs one client connection until it closes: the handshake, then each message in and each queued
/// message out, in order.
fn connection(stream: TcpStream, ip: IpAddr, hub: Hub, requests: smol::channel::Sender<Request>) {
    if stream.set_read_timeout(Some(HANDSHAKE_TIMEOUT)).is_err() {
        return;
    }
    let mut ws = match tungstenite::accept_with_config(stream, Some(proto::ws_config())) {
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
    let Some(conn) = hub.add_conn(tx.clone(), ip) else { return };
    let mut hello = false;
    let mut closing = false;
    'conn: loop {
        // Closed by `stop` or by settings that no longer allow this peer.
        if !hub.is_open(conn) {
            break;
        }
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
                closing = incoming(&hub, &requests, conn, &tx, &mut hello, text.as_str()) == Next::Close;
            }
            Ok(tungstenite::Message::Close(_)) => break,
            Ok(_) => {}
            Err(tungstenite::Error::Io(e)) if matches!(e.kind(), io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut) => {}
            Err(_) => break,
        }
    }
    let _ = ws.close(None);
    hub.disconnect(conn);
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

/// Handles one text frame from a connection. Hello is answered here and never forwarded; every
/// other message after it goes to the chat owner as a `Request`.
fn incoming(
    hub: &Hub,
    requests: &smol::channel::Sender<Request>,
    conn: u64,
    me: &mpsc::Sender<String>,
    hello: &mut bool,
    text: &str,
) -> Next {
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
                let device = hub.lock().device.clone();
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
    match msg {
        ClientMsg::Hello { .. } => queue(me, &HostMsg::Error { message: "Already said hello".into() }),
        msg => {
            if requests.send_blocking(Request { conn, msg }).is_err() {
                return Next::Close;
            }
        }
    }
    Next::Continue
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::remote::Device;
    use std::net::Ipv4Addr;

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
        // Sharing on, with or without picked devices, still only the tailnet.
        let open = SyncSettings { enabled: true, ..Default::default() };
        let picked = SyncSettings { enabled: true, devices: vec![device("192.168.1.5")], ..Default::default() };
        for addr in ["192.168.1.5", "8.8.8.8", "10.0.0.2", "100.128.0.1", "2001:db8::1"] {
            let ip: IpAddr = addr.parse().unwrap();
            let tailnet = tailscale::is_tailscale_addr(&ip);
            assert!(!tailnet, "{addr} is outside Tailscale's ranges");
            assert!(!peer_allowed(ip, &open, tailnet), "{addr}");
            assert!(!peer_allowed(ip, &picked, tailnet), "{addr}: a picked device must still be on the tailnet");
        }
    }

    /// A real listener on loopback answers a client that says hello, whatever the settings.
    #[test]
    fn a_loopback_hub_reports_its_port_and_welcomes_a_client() {
        let (hub, _requests) = Hub::start("127.0.0.1:0".parse().unwrap(), "box".into(), SyncSettings::default()).unwrap();
        let addr = hub.local_addr();
        assert!(addr.ip().is_loopback());
        assert_ne!(addr.port(), 0);
        let client = crate::remote::client::Client::connect("127.0.0.1", addr.port(), "laptop").unwrap();
        assert_eq!(client.host, "box");
    }

    #[test]
    fn tailscale_peers_are_allowed_only_when_picked() {
        let open = SyncSettings { enabled: true, ..Default::default() };
        let picked = SyncSettings { enabled: true, devices: vec![device("100.1.2.3")], ..Default::default() };
        let listed: IpAddr = "100.1.2.3".parse().unwrap();
        let other: IpAddr = "100.9.9.9".parse().unwrap();
        assert!(peer_allowed(other, &open, true), "no devices picked: any tailnet peer may connect");
        assert!(peer_allowed(listed, &picked, true));
        assert!(!peer_allowed(other, &picked, true));
        let off = SyncSettings { enabled: false, ..picked };
        assert!(!peer_allowed(listed, &off, true), "sharing off: nobody from the tailnet");
    }

    /// A free loopback address for a test host.
    fn free_addr() -> SocketAddr {
        let probe = TcpListener::bind("127.0.0.1:0").unwrap();
        probe.local_addr().unwrap()
    }

    #[test]
    fn stop_frees_the_port_and_closes_connections() {
        let bind = free_addr();
        let sync = SyncSettings { enabled: true, ..Default::default() };
        let (hub, _requests) = Hub::start(bind, "host".into(), sync).unwrap();
        let client = crate::remote::client::Client::connect("127.0.0.1", bind.port(), "pc").unwrap();
        hub.stop();
        let closed = smol::block_on(async {
            loop {
                match client.events.recv().await {
                    Ok(crate::remote::client::ClientEvent::Closed(_)) | Err(_) => return true,
                    Ok(_) => {}
                }
            }
        });
        assert!(closed, "the open connection ends");
        let mut freed = false;
        for _ in 0..40 {
            if TcpListener::bind(bind).is_ok() {
                freed = true;
                break;
            }
            thread::sleep(POLL);
        }
        assert!(freed, "nothing listens after stop");
    }

    #[test]
    fn settings_that_refuse_a_peer_close_its_connection() {
        let (hub, _requests) = Hub::start(free_addr(), "host".into(), SyncSettings::default()).unwrap();
        let picked = SyncSettings { enabled: true, devices: vec![device("100.100.1.3")], ..Default::default() };
        let (tx, _rx) = mpsc::channel();
        let conn = hub.add_conn(tx, "100.100.1.3".parse().unwrap()).unwrap();
        hub.set_sync(picked.clone());
        assert!(hub.is_open(conn), "still included");
        hub.set_sync(SyncSettings { devices: Vec::new(), ..picked.clone() });
        assert!(hub.is_open(conn), "no devices picked: any tailnet peer");
        hub.set_sync(SyncSettings { devices: vec![device("100.9.9.9")], ..picked.clone() });
        assert!(!hub.is_open(conn), "no longer included");
        let (tx, _rx) = mpsc::channel();
        let conn = hub.add_conn(tx, "100.100.1.3".parse().unwrap()).unwrap();
        hub.set_sync(SyncSettings { enabled: false, ..picked });
        assert!(!hub.is_open(conn), "sharing off");
        hub.stop();
        let (tx, _rx) = mpsc::channel();
        assert!(hub.add_conn(tx, "127.0.0.1".parse().unwrap()).is_none(), "a stopped hub takes no connection");
    }

    #[test]
    fn ipv4_mapped_peers_are_checked_as_ipv4() {
        let picked = SyncSettings { enabled: true, devices: vec![device("100.101.2.3")], ..Default::default() };
        let mapped: IpAddr = "::ffff:100.101.2.3".parse().unwrap();
        assert!(tailscale::is_tailscale_addr(&mapped));
        assert!(peer_allowed(mapped, &picked, tailscale::is_tailscale_addr(&mapped)));
        let lo: IpAddr = "::ffff:127.0.0.1".parse().unwrap();
        assert!(peer_allowed(lo, &SyncSettings::default(), false));
        let other: IpAddr = "::ffff:100.101.9.9".parse().unwrap();
        assert!(!peer_allowed(other, &picked, true));
    }
}

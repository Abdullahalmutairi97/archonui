//! The host side of sharing chats between devices: `archonui --serve` listens on this device's
//! Tailscale address and lets other devices list chats, send prompts and follow their replies.
//! The agents keep running here. A client only sends prompts and receives the streamed events.
//! The messages are in `proto`, the sockets in `hub`.
//!
//! `handle` and `forward` hold the chat logic. They are plain functions, so the desktop window can
//! call them from its own thread too.

use super::{
    cloud::Store,
    hub::{Hub, Request},
    lock,
    proto::{self, ClientMsg, HostMsg, RemoteFile, SessionSummary, MAX_REMOTE_FILES_BYTES},
    tailscale,
};
use base64::{engine::general_purpose::STANDARD, Engine as _};
use crate::{
    app::{
        persist::Paths,
        state::{AppState, Placement},
    },
    engine::{self, approve, attach::Attachment, DetectedAgent, Event, Mode},
    store::{self, Role, Session},
};
use std::{
    collections::HashMap,
    io,
    net::{IpAddr, SocketAddr},
    path::{Path, PathBuf},
    thread,
    time::Duration,
};

/// What a waiting approval is told when the user denies it.
const DENIED: &str = "The user denied this action.";

/// Serves this device's chats on `bind` until the process ends. Loads the settings and chats itself.
pub fn serve(bind: SocketAddr) -> io::Result<()> {
    let _chats = lock_chats().map_err(|e| io::Error::new(io::ErrorKind::WouldBlock, e))?;
    let device = host_name(&discover());
    listen(bind, device, AppState::load(&Paths::current()))
}

/// Takes the chats folder for this host alone, or says why it can't.
fn lock_chats() -> Result<lock::Lock, String> {
    let dir = store::dir();
    match lock::try_lock(&dir, true) {
        Ok(Some(lock)) => Ok(lock),
        Ok(None) => Err(lock::busy_message(&dir, true)),
        Err(e) => Err(format!("cannot use the chats folder {}: {e}", dir.display())),
    }
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
    // Before loading: loading may already write the chats.
    let _chats = match lock_chats() {
        Ok(lock) => lock,
        Err(e) => {
            eprintln!("archonui host: {e}");
            return 1;
        }
    };
    let state = AppState::load(&Paths::current());
    let tailnet = discover();
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
    if let Err(e) = &tailnet {
        eprintln!("archonui host: {e}; going by the name {device}");
    }
    match listen(addr, device, state) {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("archonui host: {}", bind_error(addr, &e));
            1
        }
    }
}

/// Why the host could not listen on `addr`, in words that say what to do.
fn bind_error(addr: SocketAddr, e: &io::Error) -> String {
    match e.kind() {
        io::ErrorKind::AddrInUse => format!(
            "cannot listen on {addr}: the port is in use, perhaps by the archonUI app with Devices sharing on or another --serve. Pass --bind ADDR:PORT with a free port."
        ),
        io::ErrorKind::AddrNotAvailable => format!("cannot listen on {addr}: this device has no such address"),
        io::ErrorKind::PermissionDenied => format!("cannot listen on {addr}: permission denied (ports below 1024 need root)"),
        _ => format!("cannot listen on {addr}: {e}"),
    }
}

/// How long `--serve` waits for `tailscale status`.
const DISCOVER_TIMEOUT: Duration = Duration::from_secs(5);

/// `tailscale::discover`, given up after `DISCOVER_TIMEOUT`: a stuck Tailscale daemon must not keep
/// the host from starting. A late answer is dropped with its thread, which blocks every signal so a
/// stuck one never takes the Ctrl-C meant for `catch_stop_signals`.
fn discover() -> Result<tailscale::Tailnet, String> {
    let (tx, rx) = std::sync::mpsc::channel();
    thread::spawn(move || {
        // SAFETY: plain libc calls on a local signal set.
        unsafe {
            let mut all: libc::sigset_t = std::mem::zeroed();
            libc::sigfillset(&mut all);
            libc::pthread_sigmask(libc::SIG_BLOCK, &all, std::ptr::null_mut());
        }
        let _ = tx.send(tailscale::discover());
    });
    rx.recv_timeout(DISCOVER_TIMEOUT)
        .unwrap_or_else(|_| Err(format!("Tailscale did not answer within {} s", DISCOVER_TIMEOUT.as_secs())))
}

/// The name a host shows its clients: this device's Tailscale host name, else its own host name, else "host".
fn host_name(tailnet: &Result<tailscale::Tailnet, String>) -> String {
    tailnet
        .as_ref()
        .map(|t| t.me.name.clone())
        .ok()
        .filter(|n| !n.is_empty())
        .or_else(super::local_host_name)
        .unwrap_or_else(|| "host".into())
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

/// The answer when this device keeps no cloud copy.
fn no_cloud() -> HostMsg {
    HostMsg::Error { message: "This device does not keep a cloud copy".into() }
}

/// Saves the chats that changed, and prints the files that could not be written.
fn flush(state: &mut AppState) {
    for e in state.flush_sessions() {
        eprintln!("archonui host: {e}");
    }
}

/// Handles one client request. Replies and broadcasts go out through `hub`. When a prompt starts a
/// turn, returns the chat id and the turn's events, for the caller to follow. Cloud requests are
/// answered from `cloud`, or refused when this device keeps none.
pub fn handle(
    state: &mut AppState,
    agents: &[DetectedAgent],
    hub: &Hub,
    cloud: Option<&Store>,
    req: Request,
) -> Option<(String, smol::channel::Receiver<Event>)> {
    let conn = req.conn;
    let mut turn = None;
    // Chats shown here from another device are not this host's to share.
    if let Some(id) = target(&req.msg).filter(|id| state.is_remote(id).is_some()) {
        hub.reply(conn, &HostMsg::Error { message: format!("There is no chat {id} on this device") });
        return None;
    }
    match req.msg {
        ClientMsg::Hello { .. } => hub.reply(conn, &HostMsg::Error { message: "Already said hello".into() }),
        ClientMsg::ListSessions => {
            let mut list: Vec<&Session> = state.sessions().iter().filter(|s| s.visible() && s.origin.is_none()).collect();
            list.sort_by(|a, b| b.updated.cmp(&a.updated));
            let sessions = list.into_iter().map(|s| summary(s, state.busy(&s.id))).collect();
            hub.reply(conn, &HostMsg::Sessions { sessions });
        }
        ClientMsg::Subscribe { session } => {
            let busy = state.busy(&session);
            match state.session(&session).map(serde_json::to_value) {
                Some(Ok(value)) => {
                    hub.subscribe(conn, &session);
                    hub.reply(conn, &HostMsg::Snapshot { session: value, busy });
                }
                Some(Err(e)) => hub.reply(conn, &HostMsg::Error { message: format!("Could not send the chat: {e}") }),
                None => hub.reply(conn, &HostMsg::Error { message: format!("There is no chat {session} on this device") }),
            }
        }
        ClientMsg::Unsubscribe { session } => hub.unsubscribe(conn, &session),
        ClientMsg::NewSession { agent, cwd, project } => {
            // An empty agent or folder means this host's defaults: the asking device can't know them.
            let agent = if agent.is_empty() {
                let enabled = |id: &str| agents.iter().any(|a| a.enabled && a.harness.id() == id);
                state
                    .settings
                    .default_harness
                    .clone()
                    .filter(|h| enabled(h))
                    .or_else(|| agents.iter().find(|a| a.enabled).map(|a| a.harness.id().to_string()))
                    .unwrap_or_default()
            } else {
                agent
            };
            let home = std::env::var_os("HOME").map(PathBuf::from);
            let named = !cwd.as_os_str().is_empty();
            let cwd = if !named {
                state
                    .settings
                    .default_folder
                    .clone()
                    .filter(|p| p.is_dir())
                    .or_else(|| home.clone())
                    .unwrap_or_else(|| PathBuf::from("/"))
            } else {
                host_folder(cwd, home.as_deref())
            };
            if !agents.iter().any(|a| a.enabled && a.harness.id() == agent.as_str()) {
                let message = format!("{} is not set up on this device", engine::agent_name(&agent));
                hub.reply(conn, &HostMsg::Error { message });
            } else if !cwd.is_dir() {
                let message = format!("There is no folder {} on this device", cwd.display());
                hub.reply(conn, &HostMsg::Error { message });
            } else {
                // A folder the client named wins over the project open here, which would move the chat.
                let current = if named { state.workspace.current_project.take() } else { None };
                let id = state.new_session(&agent, cwd, Placement::Focused);
                if current.is_some() {
                    state.workspace.current_project = current;
                }
                if project.is_some() {
                    state.move_to_project(&id, project);
                }
                flush(state);
                if let Some(s) = state.session(&id) {
                    hub.reply(conn, &HostMsg::Created { session: summary(s, false) });
                }
            }
        }
        ClientMsg::Prompt { session, text, mode, model, effort, files } => {
            match start_prompt(state, agents, hub, &session, &text, mode, model, effort, files) {
                Ok(started) => turn = started,
                Err(message) => hub.reply(conn, &HostMsg::Error { message }),
            }
        }
        ClientMsg::Cancel { session } => {
            if !state.stop(&session) {
                hub.reply(conn, &HostMsg::Error { message: "No reply is running in that chat".into() });
            }
        }
        msg @ (ClientMsg::PutProject { .. }
        | ClientMsg::PutChats { .. }
        | ClientMsg::ListCloud
        | ClientMsg::GetProject { .. }
        | ClientMsg::GetChats { .. }) => handle_cloud(cloud, hub, Request { conn, msg }),
        ClientMsg::Draft { session, text } => {
            if state.session(&session).is_none() {
                hub.reply(conn, &HostMsg::Error { message: format!("There is no chat {session} on this device") });
            } else {
                state.set_draft(&session, &text);
                flush(state);
                hub.broadcast(&session, &HostMsg::Draft { session: session.clone(), text }, Some(conn));
            }
        }
        // An answer the owner did not match to a waiting request.
        ClientMsg::Answer { .. } => hub.reply(conn, &HostMsg::Error { message: "Nothing is waiting for that answer".into() }),
        ClientMsg::Unknown => {
            let message = "This device's archonUI does not know that request; update it to match the other device".into();
            hub.reply(conn, &HostMsg::Error { message });
        }
    }
    turn
}

/// Whether `msg` is about the cloud copy. Those need no chats and can be slow (they run git), so a
/// caller with a UI thread hands them to `handle_cloud` on another thread.
pub fn is_cloud(msg: &ClientMsg) -> bool {
    matches!(
        msg,
        ClientMsg::PutProject { .. } | ClientMsg::PutChats { .. } | ClientMsg::ListCloud | ClientMsg::GetProject { .. } | ClientMsg::GetChats { .. }
    )
}

/// Answers one cloud request from `cloud`, or refuses it when this device keeps none. Blocking.
pub fn handle_cloud(cloud: Option<&Store>, hub: &Hub, req: Request) {
    let conn = req.conn;
    match req.msg {
        ClientMsg::PutProject { project, name, device, commit, bundle } => {
            let reply = match cloud {
                None => no_cloud(),
                Some(store) => match STANDARD.decode(&bundle) {
                    Err(e) => HostMsg::Error { message: format!("The bundle is not base64: {e}") },
                    Ok(bytes) => match store.put_project(&project, &name, &device, &commit, &bytes) {
                        Ok(()) => HostMsg::Stored { project, device, commit },
                        Err(message) => HostMsg::Error { message },
                    },
                },
            };
            hub.reply(conn, &reply);
        }
        ClientMsg::PutChats { device, sessions } => match cloud {
            None => hub.reply(conn, &no_cloud()),
            Some(store) => {
                // Nothing is sent back on success; the sender only needs to know about failures.
                if let Err(message) = store.put_chats(&device, &sessions) {
                    hub.reply(conn, &HostMsg::Error { message });
                }
            }
        },
        ClientMsg::ListCloud => {
            let reply = match cloud {
                None => no_cloud(),
                Some(store) => HostMsg::Cloud { projects: store.projects(), chats: store.chat_devices() },
            };
            hub.reply(conn, &reply);
        }
        ClientMsg::GetProject { project, device } => {
            let reply = match cloud {
                None => no_cloud(),
                Some(store) => match store.get_project(&project, &device) {
                    Ok((_, bundle)) if !proto::fits(bundle.len().div_ceil(3) * 4) => {
                        HostMsg::Error { message: format!("That copy is too big to send ({} MB)", bundle.len() >> 20) }
                    }
                    Ok((commit, bundle)) => HostMsg::ProjectData { project, device, commit, bundle: STANDARD.encode(bundle) },
                    Err(message) => HostMsg::Error { message },
                },
            };
            hub.reply(conn, &reply);
        }
        ClientMsg::GetChats { device } => {
            let reply = match cloud {
                None => no_cloud(),
                Some(store) => HostMsg::Chats { sessions: store.chats(&device), device },
            };
            hub.reply(conn, &reply);
        }
        _ => {}
    }
}

/// The chat a message is about, if any.
fn target(msg: &ClientMsg) -> Option<&str> {
    match msg {
        ClientMsg::Subscribe { session }
        | ClientMsg::Unsubscribe { session }
        | ClientMsg::Prompt { session, .. }
        | ClientMsg::Cancel { session }
        | ClientMsg::Draft { session, .. }
        | ClientMsg::Answer { session, .. } => Some(session),
        ClientMsg::Hello { .. }
        | ClientMsg::ListSessions
        | ClientMsg::NewSession { .. }
        | ClientMsg::PutProject { .. }
        | ClientMsg::PutChats { .. }
        | ClientMsg::ListCloud
        | ClientMsg::GetProject { .. }
        | ClientMsg::GetChats { .. }
        | ClientMsg::Unknown => None,
    }
}

/// Starts a turn in `session` with `text`, first applying the options the client sent. Errors are
/// for the client who asked. A turn that cannot start is reported to the chat's subscribers instead.
fn start_prompt(
    state: &mut AppState,
    agents: &[DetectedAgent],
    hub: &Hub,
    session: &str,
    text: &str,
    mode: Option<Mode>,
    model: Option<String>,
    effort: Option<String>,
    files: Vec<RemoteFile>,
) -> Result<Option<(String, smol::channel::Receiver<Event>)>, String> {
    let (cur_mode, cur_model, cur_effort, agent) = match state.session(session) {
        Some(s) => (s.mode, s.model.clone(), s.effort.clone(), s.agent.clone()),
        None => return Err(format!("There is no chat {session} on this device")),
    };
    if state.busy(session) {
        return Err("A reply is already running".into());
    }
    if text.trim().is_empty() && files.is_empty() {
        return Err("The prompt is empty".into());
    }
    // Checked before the files are saved, so they don't wait for the chat's next prompt.
    if !agents.iter().any(|a| a.harness.id() == agent) {
        return Err(format!("{} is not set up on this device", engine::agent_name(&agent)));
    }
    let before = state.session(session).map_or(0, |s| s.messages.len());
    let attachments = save_files(session, files)?;
    if !attachments.is_empty() {
        state.add_pending(session, attachments);
    }
    if mode.is_some() || model.is_some() || effort.is_some() {
        state.set_options(session, model.or(cur_model), effort.or(cur_effort), mode.unwrap_or(cur_mode));
    }
    let Some(rx) = state.send(session, text, agents) else {
        flush(state);
        for msg in start_failed(state, session, before) {
            hub.broadcast(session, &msg, None);
        }
        return Ok(None);
    };
    flush(state);
    hub.broadcast(session, &HostMsg::Started { session: session.to_string(), text: text.to_string() }, None);
    Ok(Some((session.to_string(), rx)))
}

/// What a chat's subscribers are sent when a prompt in `session` could not start: `Finished` with the
/// reason `send` put in the chat (it ends `--remote ask` and sends queued prompts), then the chat as it
/// is now, which holds the prompt and the error that no `Started` announced.
pub(crate) fn start_failed(state: &AppState, session: &str, before: usize) -> Vec<HostMsg> {
    let error = Some(start_error(state, session, before));
    let mut msgs = vec![HostMsg::Finished { session: session.to_string(), error, stopped: false }];
    if let Some(Ok(value)) = state.session(session).map(serde_json::to_value) {
        msgs.push(HostMsg::Snapshot { session: value, busy: false });
    }
    msgs
}

/// Why `send` did not start a turn in `session`: the last error it added after message `before`.
fn start_error(state: &AppState, session: &str, before: usize) -> String {
    state
        .session(session)
        .and_then(|s| s.messages.get(before..))
        .and_then(|added| added.iter().rev().find(|m| m.role == Role::Error))
        .map_or_else(|| "Could not start the agent".to_string(), |m| m.text.clone())
}

/// Saves files sent with a prompt into the chat's attachments here. Refuses too many or too large.
/// All or nothing: when one file fails, the ones already saved are removed again.
fn save_files(session: &str, files: Vec<RemoteFile>) -> Result<Vec<Attachment>, String> {
    if files.len() > engine::attach::MAX_FILES {
        return Err(format!("At most {} files can go with one message", engine::attach::MAX_FILES));
    }
    // Every file is decoded and counted before any is written.
    let mut total = 0u64;
    let mut decoded = Vec::with_capacity(files.len());
    for f in files {
        let bytes = STANDARD.decode(&f.data).map_err(|e| format!("{} could not be read: {e}", f.name))?;
        total += bytes.len() as u64;
        if total > MAX_REMOTE_FILES_BYTES {
            return Err("The files are too large to send to another device".into());
        }
        decoded.push((f.name, bytes));
    }
    let mut out: Vec<Attachment> = Vec::new();
    for (name, bytes) in decoded {
        let saved = store::store_bytes(session, &name, &bytes)
            .map_err(|e| format!("Could not save {name}: {e}"))
            .and_then(|path| Attachment::inspect(&path).inspect_err(|_| drop(std::fs::remove_file(&path))));
        match saved {
            Ok(a) => out.push(a),
            Err(e) => {
                for a in &out {
                    let _ = std::fs::remove_file(&a.path);
                }
                // Only removed when empty: files of earlier messages stay.
                let _ = std::fs::remove_dir(store::attachments_dir(session));
                return Err(e);
            }
        }
    }
    Ok(out)
}

/// Folds one event of chat `session`'s turn into the chat, and forwards it to the chat's subscribers.
/// Call it for each event, in order, until the turn finishes.
pub fn forward(state: &mut AppState, hub: &Hub, session: &str, ev: Event) {
    match ev {
        Event::Out(out) => {
            let msg = HostMsg::Event { session: session.to_string(), out: Some(out.clone()) };
            state.apply(session, Event::Out(out));
            hub.broadcast(session, &msg, None);
        }
        Event::Finished(result) => {
            let error = result.as_ref().err().cloned();
            let stopped = state.stopping(session);
            state.apply(session, Event::Finished(result));
            flush(state);
            hub.broadcast(session, &HostMsg::Finished { session: session.to_string(), error, stopped }, None);
        }
    }
}

/// What the headless owner thread handles: a client request, an event of a running turn, or an approval
/// request from an agent. `Expired` is sent when the listener has timed a request out on its own.
enum Work {
    Req(Request),
    Turn(String, Event),
    Approval(approve::Request),
    Expired(u64),
    /// Ctrl-C, `kill` or a closed terminal: stop the running turns, save the chats and end.
    Stop,
}

/// Signals that end `--serve`.
const STOP_SIGNALS: [libc::c_int; 3] = [libc::SIGINT, libc::SIGTERM, libc::SIGHUP];

/// How long a stopping host waits for its agents to exit. A turn's cancel sends SIGKILL after 3 s.
const STOP_WAIT: Duration = Duration::from_secs(4);

/// Turns the stop signals into `Work::Stop`, so the host can end its agents and save its chats first.
/// Their agents run in process groups of their own, so they would not get a Ctrl-C and would outlive
/// the host. Call it before starting any thread: the signals are blocked in this thread and the ones
/// it starts, and waited for on one thread. Agents start with no signals blocked, as `Command` clears
/// the mask. A signal that was ignored when the host started (`nohup`, `&` in a script) stays ignored.
fn catch_stop_signals(work: smol::channel::Sender<Work>) {
    // SAFETY: plain libc calls on a zeroed, then initialised, local signal set.
    let set = unsafe {
        let mut set: libc::sigset_t = std::mem::zeroed();
        libc::sigemptyset(&mut set);
        for sig in STOP_SIGNALS {
            let mut old: libc::sigaction = std::mem::zeroed();
            let ignored = libc::sigaction(sig, std::ptr::null(), &mut old) == 0 && old.sa_sigaction == libc::SIG_IGN;
            if !ignored {
                libc::sigaddset(&mut set, sig);
            }
        }
        if libc::pthread_sigmask(libc::SIG_BLOCK, &set, std::ptr::null_mut()) != 0 {
            return;
        }
        set
    };
    thread::spawn(move || loop {
        let mut sig = 0;
        // SAFETY: `set` is an initialised signal set owned by this thread.
        if unsafe { libc::sigwait(&set, &mut sig) } == 0 && work.send_blocking(Work::Stop).is_err() {
            return;
        }
    });
}

/// Ends the running turns as Stop does, keeping their partial replies, saves the chats, tells the
/// subscribers, and waits up to `STOP_WAIT` for the agents to exit. A second signal ends the wait.
fn stop(state: &mut AppState, hub: &Hub, approvals: &mut Approvals, work: &smol::channel::Receiver<Work>) {
    let mut running: Vec<String> = state.sessions().iter().filter(|s| state.busy(&s.id)).map(|s| s.id.clone()).collect();
    if running.is_empty() {
        eprintln!("archonui host: stopped");
        return;
    }
    eprintln!("archonui host: stopping {} running repl{}", running.len(), if running.len() == 1 { "y" } else { "ies" });
    for id in &running {
        approvals.end(id, hub);
    }
    state.shutdown();
    for id in &running {
        hub.broadcast(id, &HostMsg::Finished { session: id.clone(), error: None, stopped: true }, None);
    }
    let deadline = std::time::Instant::now() + STOP_WAIT;
    while !running.is_empty() {
        let left = deadline.saturating_duration_since(std::time::Instant::now());
        let next = smol::block_on(smol::future::or(async { work.recv().await.ok() }, async {
            smol::Timer::after(left).await;
            None
        }));
        match next {
            Some(Work::Turn(id, Event::Finished(_))) => running.retain(|r| *r != id),
            Some(Work::Stop) | None => break,
            Some(_) => {}
        }
    }
    if !running.is_empty() {
        eprintln!("archonui host: {} agent(s) had not exited yet", running.len());
    }
    // Lets the connections send the last `Finished` before the process ends.
    thread::sleep(Duration::from_millis(200));
    eprintln!("archonui host: stopped");
}

/// The approval requests of this headless host's chats that wait for an answer, by the id clients see.
#[derive(Default)]
struct Approvals {
    next: u64,
    waiting: HashMap<u64, approve::Request>,
}

impl Approvals {
    /// Keeps `req` waiting and tells the chat's subscribers about it. Returns its id.
    fn add(&mut self, req: approve::Request, hub: &Hub) -> u64 {
        let id = self.next;
        self.next += 1;
        let msg = HostMsg::Approval { session: req.session.clone(), request: id, tool: req.tool_name.clone(), input: req.input.clone() };
        hub.broadcast(&req.session, &msg, None);
        self.waiting.insert(id, req);
        id
    }

    /// Answers request `id` with `verdict` and tells the subscribers it is done. Nothing if it is not waiting.
    fn settle(&mut self, id: u64, verdict: approve::Verdict, hub: &Hub) {
        if let Some(req) = self.waiting.remove(&id) {
            hub.broadcast(&req.session, &HostMsg::ApprovalDone { session: req.session.clone(), request: id }, None);
            req.answer(verdict);
        }
    }

    /// Applies a client's `Answer` to a request waiting in its chat, from a connection following that chat.
    /// Returns false when none matches, so `handle` refuses the message. With `always`, the tool is allowed for the rest of the session, and the
    /// other waiting requests for it are allowed too.
    fn answer(&mut self, req: &Request, allowances: &approve::Allowances, hub: &Hub) -> bool {
        let ClientMsg::Answer { session, request, allow, always } = &req.msg else { return false };
        if !hub.follows(req.conn, session) {
            return false;
        }
        let Some(tool) = self.waiting.get(request).filter(|r| &r.session == session).map(|r| r.tool_name.clone()) else {
            return false;
        };
        if *allow && *always {
            allowances.allow(session, &tool);
            let same: Vec<u64> = self
                .waiting
                .iter()
                .filter(|(_, r)| &r.session == session && r.tool_name == tool)
                .map(|(id, _)| *id)
                .collect();
            for id in same {
                self.settle(id, approve::Verdict::Allow, hub);
            }
        }
        let verdict = if *allow { approve::Verdict::Allow } else { approve::Verdict::Deny(DENIED.into()) };
        self.settle(*request, verdict, hub);
        true
    }

    /// The turn in chat `session` ended: its waiting requests are denied, as the agent that asked is gone.
    fn end(&mut self, session: &str, hub: &Hub) {
        let ids: Vec<u64> = self.waiting.iter().filter(|(_, r)| r.session == session).map(|(id, _)| *id).collect();
        for id in ids {
            self.settle(id, approve::Verdict::Deny("The chat stopped before an answer.".into()), hub);
        }
    }

    /// Sends `conn` the requests waiting in chat `session`, when it subscribes.
    fn send_waiting(&self, hub: &Hub, conn: u64, session: &str) {
        for (id, req) in self.waiting.iter().filter(|(_, r)| r.session == session) {
            let msg = HostMsg::Approval { session: session.to_string(), request: *id, tool: req.tool_name.clone(), input: req.input.clone() };
            hub.reply(conn, &msg);
        }
    }
}

/// Sends `Work::Expired(id)` once the listener's timeout has passed, for a request it denied on its own.
fn expire_later(id: u64, work: smol::channel::Sender<Work>) {
    thread::spawn(move || {
        thread::sleep(approve::TIMEOUT + Duration::from_secs(1));
        let _ = work.send_blocking(Work::Expired(id));
    });
}

/// Pushes each event of one turn into the owner's queue, until the turn's channel closes.
fn follow(id: String, turn: smol::channel::Receiver<Event>, work: smol::channel::Sender<Work>) {
    thread::spawn(move || {
        while let Ok(ev) = smol::block_on(turn.recv()) {
            if work.send_blocking(Work::Turn(id.clone(), ev)).is_err() {
                break;
            }
        }
    });
}

/// Serves `state`'s chats on `bind`. One thread owns the chats and the agents: it takes requests
/// and turn events in turn, so nothing needs a lock.
fn listen(bind: SocketAddr, device: String, mut state: AppState) -> io::Result<()> {
    // Running `--serve` is the user turning sharing on for this process, even with `--bind`.
    let sync = super::SyncSettings { enabled: true, ..state.settings.sync.clone() };
    let (work_tx, work_rx) = smol::channel::unbounded::<Work>();
    // Before any thread starts, so every thread has the signals blocked.
    catch_stop_signals(work_tx.clone());
    let (hub, requests) = Hub::start(bind, device, sync)?;
    let harness_settings = store::Settings { harnesses: state.settings.harnesses.clone(), ..Default::default() };
    let agents = engine::detect(&harness_settings);
    // The headless host keeps the cloud copy: `--serve` is meant for the always-on box.
    let cloud = Store::open_default();
    eprintln!("archonui host listening on {}", hub.local_addr());
    let from_requests = work_tx.clone();
    thread::spawn(move || {
        while let Ok(req) = smol::block_on(requests.recv()) {
            if from_requests.send_blocking(Work::Req(req)).is_err() {
                break;
            }
        }
    });
    // Tools that need approval ask this process; clients answer them through `Answer`.
    let allowances = approve::Allowances::default();
    let (approval_tx, approval_rx) = smol::channel::unbounded::<approve::Request>();
    if let Err(e) = approve::listen(approve::socket(), approval_tx, allowances.clone(), approve::TIMEOUT) {
        eprintln!("archonui host: could not listen for approvals: {e}");
    }
    let from_approvals = work_tx.clone();
    thread::spawn(move || {
        while let Ok(req) = smol::block_on(approval_rx.recv()) {
            if from_approvals.send_blocking(Work::Approval(req)).is_err() {
                break;
            }
        }
    });
    let mut approvals = Approvals::default();
    while let Ok(work) = smol::block_on(work_rx.recv()) {
        match work {
            Work::Req(req) => {
                if approvals.answer(&req, &allowances, &hub) {
                    continue;
                }
                let conn = req.conn;
                let subscribed = match &req.msg {
                    ClientMsg::Subscribe { session } => Some(session.clone()),
                    _ => None,
                };
                if let Some((id, turn)) = handle(&mut state, &agents, &hub, Some(&cloud), req) {
                    follow(id, turn, work_tx.clone());
                }
                if let Some(session) = subscribed.filter(|s| state.session(s).is_some()) {
                    approvals.send_waiting(&hub, conn, &session);
                }
            }
            Work::Turn(id, ev) => {
                let finished = matches!(ev, Event::Finished(_));
                forward(&mut state, &hub, &id, ev);
                if finished {
                    approvals.end(&id, &hub);
                }
            }
            Work::Approval(req) => {
                if state.session(&req.session).is_none() {
                    req.answer(approve::Verdict::Deny("Unknown session.".into()));
                } else {
                    let id = approvals.add(req, &hub);
                    expire_later(id, work_tx.clone());
                }
            }
            Work::Expired(id) => approvals.settle(id, approve::Verdict::Deny("Timed out".into()), &hub),
            Work::Stop => {
                stop(&mut state, &hub, &mut approvals, &work_rx);
                break;
            }
        }
    }
    Ok(())
}

/// A folder asked for by another device, with a leading `~` meaning this device's home: the asking
/// device's shell can't know it.
fn host_folder(cwd: PathBuf, home: Option<&Path>) -> PathBuf {
    match (cwd.strip_prefix("~"), home) {
        // `join("")` would add a trailing slash, which then shows in the chat's folder.
        (Ok(rest), Some(home)) if rest.as_os_str().is_empty() => home.to_path_buf(),
        (Ok(rest), Some(home)) => home.join(rest),
        _ => cwd,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_leading_tilde_is_this_devices_home() {
        let home = Path::new("/home/me");
        assert_eq!(host_folder(PathBuf::from("~"), Some(home)), PathBuf::from("/home/me"));
        // `PathBuf` equality ignores a trailing slash; the folder sent to clients does not.
        assert_eq!(host_folder(PathBuf::from("~"), Some(home)).to_str(), Some("/home/me"));
        assert_eq!(host_folder(PathBuf::from("~/code/x"), Some(home)), PathBuf::from("/home/me/code/x"));
        assert_eq!(host_folder(PathBuf::from("/srv/x"), Some(home)), PathBuf::from("/srv/x"));
        assert_eq!(host_folder(PathBuf::from("~other/x"), Some(home)), PathBuf::from("~other/x"));
        assert_eq!(host_folder(PathBuf::from("~"), None), PathBuf::from("~"));
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

    fn state() -> AppState {
        use crate::app::{settings::AppSettings, workspace::Workspace};
        AppState::new(Vec::new(), Vec::new(), Workspace::default(), AppSettings::default())
    }

    fn pi_at(path: &str) -> DetectedAgent {
        let pi = engine::harness("pi").unwrap();
        DetectedAgent { harness: pi, path: path.into(), version: String::new(), enabled: true, auth: pi.auth_status() }
    }

    #[test]
    fn a_turn_that_cannot_start_reports_the_reason() {
        let mut app = state();
        let id = app.new_session("pi", std::env::temp_dir(), Placement::Focused);
        let agents = [pi_at("/nonexistent/archonui-test-pi")];
        assert!(app.send(&id, "hello", &agents).is_none());
        let error = start_error(&app, &id, 0);
        assert!(error.starts_with("Could not start Pi:"), "{error}");
    }

    /// Roles and texts of chat `id`, to compare two devices' copies.
    fn lines(app: &AppState, id: &str) -> Vec<(Role, String)> {
        app.session(id).unwrap().messages.iter().map(|m| (m.role, m.text.clone())).collect()
    }

    #[test]
    fn a_turn_that_cannot_start_leaves_followers_with_the_hosts_chat() {
        let mut host = state();
        let id = host.new_session("pi", std::env::temp_dir(), Placement::Focused);
        // A device following the chat, with its copy from before the prompt.
        let mut follower = state();
        let copy = serde_json::from_value(serde_json::to_value(host.session(&id).unwrap()).unwrap()).unwrap();
        follower.put_remote(copy, "host", false);

        let agents = [pi_at("/nonexistent/archonui-test-pi")];
        assert!(host.send(&id, "hello", &agents).is_none());
        let msgs = start_failed(&host, &id, 0);
        assert!(matches!(&msgs[0], HostMsg::Finished { error: Some(e), stopped: false, .. } if e.starts_with("Could not start Pi:")));
        // As the window applies them (`ui::remote`).
        for msg in msgs {
            match msg {
                HostMsg::Finished { session, error, stopped } => drop(follower.remote_finished(&session, error, stopped)),
                HostMsg::Snapshot { session, busy } => follower.put_remote(serde_json::from_value(session).unwrap(), "host", busy),
                other => panic!("unexpected {other:?}"),
            }
        }
        assert_eq!(lines(&follower, &id), lines(&host, &id));
        assert_eq!(lines(&host, &id)[0], (Role::User, "hello".to_string()));
        assert!(!follower.busy(&id));
    }

    #[test]
    fn a_request_from_a_newer_client_gets_an_error() {
        use crate::remote::client::{Client, ClientEvent};
        let port = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
        let (hub, requests) = Hub::start(SocketAddr::from(([127, 0, 0, 1], port)), "host".into(), Default::default()).unwrap();
        let client = Client::connect("127.0.0.1", port, "newer").unwrap();
        // The frame a newer client would send, as this version reads it.
        let req: ClientMsg = crate::remote::proto::decode(r#"{"type":"something_new","session":"s1"}"#).unwrap();
        client.send(&req).unwrap();
        let req = smol::block_on(requests.recv()).unwrap();
        assert_eq!(req.msg, ClientMsg::Unknown);
        handle(&mut state(), &[], &hub, None, req);
        match smol::block_on(client.events.recv()).unwrap() {
            ClientEvent::Msg(HostMsg::Error { message }) => assert!(message.contains("does not know"), "{message}"),
            ClientEvent::Msg(other) => panic!("unexpected {other:?}"),
            ClientEvent::Closed(reason) => panic!("closed: {reason:?}"),
        }
    }

    fn file(name: &str, data: &str) -> RemoteFile {
        RemoteFile { name: name.into(), data: data.into() }
    }

    fn saved_files(session: &str) -> Vec<PathBuf> {
        let dir = std::fs::read_dir(store::attachments_dir(session));
        dir.map_or_else(|_| Vec::new(), |d| d.filter_map(Result::ok).map(|e| e.path()).collect())
    }

    #[test]
    fn files_that_fail_leave_nothing_saved() {
        let ok = STANDARD.encode("hello");
        let long = format!("{}.txt", "x".repeat(300));

        // Bad base64 after a good file.
        let s = store::new_id();
        let err = save_files(&s, vec![file("a.txt", &ok), file("b.txt", "not base64!")]).unwrap_err();
        assert!(err.starts_with("b.txt could not be read"), "{err}");
        assert!(saved_files(&s).is_empty());

        // A name the file system refuses, after a good file was already written.
        let s = store::new_id();
        let err = save_files(&s, vec![file("a.txt", &ok), file(&long, &ok)]).unwrap_err();
        assert!(err.starts_with("Could not save"), "{err}");
        assert!(saved_files(&s).is_empty(), "{:?}", saved_files(&s));
        assert!(!store::attachments_dir(&s).exists());

        // Files of an earlier message stay.
        let s = store::new_id();
        let kept = save_files(&s, vec![file("old.txt", &ok)]).unwrap();
        assert!(save_files(&s, vec![file("a.txt", &ok), file(&long, &ok)]).is_err());
        assert_eq!(saved_files(&s), vec![kept[0].path.clone()]);
        std::fs::remove_dir_all(store::attachments_dir(&s)).ok();

        // All good: all saved.
        let s = store::new_id();
        assert_eq!(save_files(&s, vec![file("a.txt", &ok), file("b.txt", &ok)]).unwrap().len(), 2);
        assert_eq!(saved_files(&s).len(), 2);
        std::fs::remove_dir_all(store::attachments_dir(&s)).ok();
    }

    #[test]
    fn a_new_chat_runs_in_the_folder_the_client_named() {
        let project_dir = std::env::temp_dir().join(format!("archonui-host-project-{}", store::new_id()));
        std::fs::create_dir_all(&project_dir).unwrap();
        let mut app = state();
        let p = app.add_project("open here", project_dir.clone());
        app.workspace.current_project = Some(p.clone());
        let (hub, _requests) = Hub::start("127.0.0.1:0".parse().unwrap(), "host".into(), Default::default()).unwrap();
        let agents = [pi_at("/nonexistent/archonui-test-pi")];
        let mut ask = |cwd: PathBuf| -> (PathBuf, Option<String>) {
            let before: Vec<String> = app.sessions().iter().map(|s| s.id.clone()).collect();
            let msg = ClientMsg::NewSession { agent: "pi".into(), cwd, project: None };
            handle(&mut app, &agents, &hub, None, Request { conn: 1, msg });
            let s = app.sessions().iter().find(|s| !before.contains(&s.id)).unwrap();
            (s.cwd.clone(), s.project.clone())
        };

        let named = ask(std::env::temp_dir());
        let default = ask(PathBuf::new());
        assert_eq!(named, (std::env::temp_dir(), None));
        assert_eq!(app.workspace.current_project.as_deref(), Some(p.as_str()), "the host's project stays open");

        // No folder: the host's defaults, which include its open project.
        assert_eq!(default, (project_dir.clone(), Some(p.clone())));
        std::fs::remove_dir_all(&project_dir).ok();
    }

    #[test]
    fn cloud_requests_are_told_apart_from_chat_requests() {
        assert!(is_cloud(&ClientMsg::ListCloud));
        assert!(is_cloud(&ClientMsg::GetChats { device: "n1".into() }));
        assert!(is_cloud(&ClientMsg::PutChats { device: "n1".into(), sessions: Vec::new() }));
        assert!(!is_cloud(&ClientMsg::ListSessions));
        assert!(!is_cloud(&ClientMsg::Cancel { session: "s1".into() }));
    }

    /// Waits up to 30 s for `f`; `None` when it timed out.
    fn within<T>(f: impl std::future::Future<Output = Option<T>>) -> Option<T> {
        smol::block_on(smol::future::or(f, async {
            smol::Timer::after(Duration::from_secs(30)).await;
            None
        }))
    }

    fn next_msg(c: &super::super::client::Client) -> HostMsg {
        use super::super::client::ClientEvent;
        within(async {
            match c.events.recv().await {
                Ok(ClientEvent::Msg(m)) => Some(m),
                _ => None,
            }
        })
        .expect("a message from the host")
    }

    fn next_request(requests: &smol::channel::Receiver<Request>) -> Request {
        within(async { requests.recv().await.ok() }).expect("a request from a client")
    }

    /// Two real clients on loopback follow one chat. An approval reaches both; the first answer settles
    /// it, both are told it is done, and the second answer is refused.
    #[test]
    fn an_approval_reaches_every_follower_and_the_first_answer_wins() {
        use super::super::client::Client;
        let (hub, requests) = Hub::start("127.0.0.1:0".parse().unwrap(), "host".into(), Default::default()).unwrap();
        let port = hub.local_addr().port();
        let a = Client::connect("127.0.0.1", port, "a").unwrap();
        let b = Client::connect("127.0.0.1", port, "b").unwrap();
        let mut conns = Vec::new();
        for c in [&a, &b] {
            c.send(&ClientMsg::Subscribe { session: "s1".into() }).unwrap();
            let req = next_request(&requests);
            assert!(matches!(&req.msg, ClientMsg::Subscribe { session } if session == "s1"));
            hub.subscribe(req.conn, "s1");
            conns.push(req.conn);
        }
        assert_ne!(conns[0], conns[1]);

        let allowances = approve::Allowances::default();
        let mut approvals = Approvals::default();
        let (req, verdict) = approve::forwarded("s1".into(), "Bash".into(), serde_json::json!({"command": "ls"}));
        let id = approvals.add(req, &hub);
        for c in [&a, &b] {
            match next_msg(c) {
                HostMsg::Approval { session, request, tool, input } => {
                    assert_eq!((session.as_str(), request, tool.as_str()), ("s1", id, "Bash"));
                    assert_eq!(input["command"], "ls");
                }
                other => panic!("expected an approval, got {other:?}"),
            }
        }

        // A answers first.
        a.send(&ClientMsg::Answer { session: "s1".into(), request: id, allow: true, always: false }).unwrap();
        let first = next_request(&requests);
        assert_eq!(first.conn, conns[0]);
        assert!(approvals.answer(&first, &allowances, &hub));
        assert_eq!(verdict.recv_timeout(Duration::from_secs(30)).unwrap(), approve::Verdict::Allow);
        for c in [&a, &b] {
            assert_eq!(next_msg(c), HostMsg::ApprovalDone { session: "s1".into(), request: id });
        }

        // B's late answer matches nothing: the owner hands it to `handle`, which refuses it.
        b.send(&ClientMsg::Answer { session: "s1".into(), request: id, allow: false, always: false }).unwrap();
        let second = next_request(&requests);
        assert_eq!(second.conn, conns[1]);
        assert!(!approvals.answer(&second, &allowances, &hub));
        assert!(handle(&mut state(), &[], &hub, None, second).is_none());
        match next_msg(&b) {
            HostMsg::Error { message } => assert!(message.contains("Nothing is waiting"), "{message}"),
            other => panic!("expected an error, got {other:?}"),
        }
        assert!(verdict.try_recv().is_err(), "only one verdict is ever sent");
        assert!(!allowances.allowed("s1", "Bash"));

        // A connection that doesn't follow the chat can't answer for it, even with the right ids.
        let (req, verdict) = approve::forwarded("s1".into(), "Bash".into(), serde_json::json!({}));
        let id = approvals.add(req, &hub);
        hub.unsubscribe(conns[1], "s1");
        let stranger = Request { conn: conns[1], msg: ClientMsg::Answer { session: "s1".into(), request: id, allow: true, always: true } };
        assert!(!approvals.answer(&stranger, &allowances, &hub));
        assert!(verdict.try_recv().is_err());
        assert!(!allowances.allowed("s1", "Bash"));
    }

    /// "Always" settles every waiting request for the same tool in the chat and remembers it; a turn
    /// that ends denies what still waits; a late follower is sent what waits.
    #[test]
    fn always_allow_settles_the_same_tool_and_a_finished_turn_denies_the_rest() {
        use super::super::client::Client;
        let (hub, requests) = Hub::start("127.0.0.1:0".parse().unwrap(), "host".into(), Default::default()).unwrap();
        let c = Client::connect("127.0.0.1", hub.local_addr().port(), "a").unwrap();
        c.send(&ClientMsg::Subscribe { session: "s1".into() }).unwrap();
        let conn = next_request(&requests).conn;
        hub.subscribe(conn, "s1");

        let allowances = approve::Allowances::default();
        let mut approvals = Approvals::default();
        let ask = |tool: &str| approve::forwarded("s1".into(), tool.into(), serde_json::json!({}));
        let (r1, v1) = ask("Bash");
        let (r2, v2) = ask("Bash");
        let (r3, v3) = ask("Write");
        let id1 = approvals.add(r1, &hub);
        approvals.add(r2, &hub);
        approvals.add(r3, &hub);

        // A follower that arrives now is sent all three.
        let late = Client::connect("127.0.0.1", hub.local_addr().port(), "b").unwrap();
        late.send(&ClientMsg::ListSessions).unwrap();
        let late_conn = next_request(&requests).conn;
        approvals.send_waiting(&hub, late_conn, "s1");
        for _ in 0..3 {
            assert!(matches!(next_msg(&late), HostMsg::Approval { .. }));
        }

        let answer = Request { conn, msg: ClientMsg::Answer { session: "s1".into(), request: id1, allow: true, always: true } };
        assert!(approvals.answer(&answer, &allowances, &hub));
        assert_eq!(v1.recv_timeout(Duration::from_secs(30)).unwrap(), approve::Verdict::Allow);
        assert_eq!(v2.recv_timeout(Duration::from_secs(30)).unwrap(), approve::Verdict::Allow);
        assert!(allowances.allowed("s1", "Bash"));
        assert!(v3.try_recv().is_err(), "another tool still waits");

        approvals.end("s1", &hub);
        assert!(matches!(v3.recv_timeout(Duration::from_secs(30)).unwrap(), approve::Verdict::Deny(_)));
        assert!(approvals.waiting.is_empty());
    }
}

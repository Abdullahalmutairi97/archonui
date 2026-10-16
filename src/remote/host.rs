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
    proto::{ClientMsg, HostMsg, RemoteFile, SessionSummary, MAX_REMOTE_FILES_BYTES},
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
    }
    turn
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
        | ClientMsg::GetChats { .. } => None,
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
        // `send` has put the reason in the chat; clients get the same words.
        let error = Some(start_error(state, session, before));
        flush(state);
        hub.broadcast(session, &HostMsg::Finished { session: session.to_string(), error, stopped: false }, None);
        return Ok(None);
    };
    flush(state);
    hub.broadcast(session, &HostMsg::Started { session: session.to_string(), text: text.to_string() }, None);
    Ok(Some((session.to_string(), rx)))
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
fn save_files(session: &str, files: Vec<RemoteFile>) -> Result<Vec<Attachment>, String> {
    if files.len() > engine::attach::MAX_FILES {
        return Err(format!("At most {} files can go with one message", engine::attach::MAX_FILES));
    }
    let mut total = 0u64;
    let mut out = Vec::new();
    for f in files {
        let bytes = STANDARD.decode(&f.data).map_err(|e| format!("{} could not be read: {e}", f.name))?;
        total += bytes.len() as u64;
        if total > MAX_REMOTE_FILES_BYTES {
            return Err("The files are too large to send to another device".into());
        }
        let path = store::store_bytes(session, &f.name, &bytes).map_err(|e| format!("Could not save {}: {e}", f.name))?;
        out.push(Attachment::inspect(&path)?);
    }
    Ok(out)
}

/// Folds one event of chat `session`'s turn into the chat, and forwards it to the chat's subscribers.
/// Call it for each event, in order, until the turn finishes.
pub fn forward(state: &mut AppState, hub: &Hub, session: &str, ev: Event) {
    match ev {
        Event::Out(out) => {
            let msg = HostMsg::Event { session: session.to_string(), out: out.clone() };
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

    /// Applies a client's `Answer` to a request waiting in its chat. Returns false when none matches, so
    /// `handle` refuses the message. With `always`, the tool is allowed for the rest of the session, and the
    /// other waiting requests for it are allowed too.
    fn answer(&mut self, req: &Request, allowances: &approve::Allowances, hub: &Hub) -> bool {
        let ClientMsg::Answer { session, request, allow, always } = &req.msg else { return false };
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
    let (hub, requests) = Hub::start(bind, device, sync)?;
    let harness_settings = store::Settings { harnesses: state.settings.harnesses.clone(), ..Default::default() };
    let agents = engine::detect(&harness_settings);
    // The headless host keeps the cloud copy: `--serve` is meant for the always-on box.
    let cloud = Store::open_default();
    eprintln!("archonui host listening on {bind}");
    let (work_tx, work_rx) = smol::channel::unbounded::<Work>();
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
        }
    }
    Ok(())
}

/// A folder asked for by another device, with a leading `~` meaning this device's home: the asking
/// device's shell can't know it.
fn host_folder(cwd: PathBuf, home: Option<&Path>) -> PathBuf {
    match (cwd.strip_prefix("~"), home) {
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
}

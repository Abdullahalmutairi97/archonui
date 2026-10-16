//! The host side of sharing chats between devices: `archonui --serve` listens on this device's
//! Tailscale address and lets other devices list chats, send prompts and follow their replies.
//! The agents keep running here. A client only sends prompts and receives the streamed events.
//! The messages are in `proto`, the sockets in `hub`.
//!
//! `handle` and `forward` hold the chat logic. They are plain functions, so the desktop window can
//! call them from its own thread too.

use super::{
    hub::{Hub, Request},
    proto::{ClientMsg, HostMsg, SessionSummary},
    tailscale,
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
    io,
    net::{IpAddr, SocketAddr},
    thread,
};

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

/// Saves the chats that changed, and prints the files that could not be written.
fn flush(state: &mut AppState) {
    for e in state.flush_sessions() {
        eprintln!("archonui host: {e}");
    }
}

/// Handles one client request. Replies and broadcasts go out through `hub`. When a prompt starts a
/// turn, returns the chat id and the turn's events, for the caller to follow.
pub fn handle(state: &mut AppState, agents: &[DetectedAgent], hub: &Hub, req: Request) -> Option<(String, smol::channel::Receiver<Event>)> {
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
            if !agents.iter().any(|a| a.enabled && a.harness.id() == agent.as_str()) {
                let message = format!("{} is not set up on this device", engine::agent_name(&agent));
                hub.reply(conn, &HostMsg::Error { message });
            } else {
                let id = state.new_session(&agent, cwd, Placement::Focused);
                if project.is_some() {
                    state.move_to_project(&id, project);
                }
                flush(state);
                if let Some(s) = state.session(&id) {
                    hub.reply(conn, &HostMsg::Created { session: summary(s, false) });
                }
            }
        }
        ClientMsg::Prompt { session, text, mode, model, effort } => {
            match start_prompt(state, agents, hub, &session, &text, mode, model, effort) {
                Ok(started) => turn = started,
                Err(message) => hub.reply(conn, &HostMsg::Error { message }),
            }
        }
        ClientMsg::Cancel { session } => {
            if !state.stop(&session) {
                hub.reply(conn, &HostMsg::Error { message: "No reply is running in that chat".into() });
            }
        }
        ClientMsg::PutProject { .. }
        | ClientMsg::PutChats { .. }
        | ClientMsg::ListCloud
        | ClientMsg::GetProject { .. }
        | ClientMsg::GetChats { .. } => {
            hub.reply(conn, &HostMsg::Error { message: "This device does not keep a cloud copy".into() })
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
        | ClientMsg::Draft { session, .. } => Some(session),
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
) -> Result<Option<(String, smol::channel::Receiver<Event>)>, String> {
    let (cur_mode, cur_model, cur_effort) = match state.session(session) {
        Some(s) => (s.mode, s.model.clone(), s.effort.clone()),
        None => return Err(format!("There is no chat {session} on this device")),
    };
    if state.busy(session) {
        return Err("A reply is already running".into());
    }
    if text.trim().is_empty() {
        return Err("The prompt is empty".into());
    }
    if mode.is_some() || model.is_some() || effort.is_some() {
        state.set_options(session, model.or(cur_model), effort.or(cur_effort), mode.unwrap_or(cur_mode));
    }
    let Some(rx) = state.send(session, text, agents) else {
        // `send` has put the reason in the chat when it could.
        flush(state);
        let error = Some("Could not start the agent".to_string());
        hub.broadcast(session, &HostMsg::Finished { session: session.to_string(), error }, None);
        return Ok(None);
    };
    flush(state);
    hub.broadcast(session, &HostMsg::Started { session: session.to_string(), text: text.to_string() }, None);
    Ok(Some((session.to_string(), rx)))
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
            state.apply(session, Event::Finished(result));
            flush(state);
            hub.broadcast(session, &HostMsg::Finished { session: session.to_string(), error }, None);
        }
    }
}

/// What the headless owner thread handles: a client request, or an event of a running turn.
enum Work {
    Req(Request),
    Turn(String, Event),
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
    while let Ok(work) = smol::block_on(work_rx.recv()) {
        match work {
            Work::Req(req) => {
                if let Some((id, turn)) = handle(&mut state, &agents, &hub, req) {
                    follow(id, turn, work_tx.clone());
                }
            }
            Work::Turn(id, ev) => forward(&mut state, &hub, &id, ev),
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

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

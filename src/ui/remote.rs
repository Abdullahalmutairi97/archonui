//! The window's side of sharing chats between devices (`crate::remote`): while sharing is on, this
//! window hosts its own chats for the included devices, and shows theirs as remote chats.
//! Off (the default), none of this runs.

use super::{approval::DENIED, shell::Shell};
use crate::{
    engine::{
        approve::{self, Verdict},
        Event,
    },
    remote::{
        client::{Client, ClientEvent},
        cloud::Store,
        host,
        hub::{Hub, Request},
        proto::{ClientMsg, CloudProject, HostMsg, RemoteFile, MAX_REMOTE_FILES_BYTES},
        snapshot, tailscale, Device,
    },
    store::Session,
};
use base64::{engine::general_purpose::STANDARD, Engine as _};
use gpui::Context;
use std::{
    collections::{HashMap, HashSet},
    net::{IpAddr, SocketAddr},
    sync::mpsc,
    time::Duration,
};

/// How often the chat lists of connected devices are asked for again, to pick up new chats.
const LIST_EVERY: Duration = Duration::from_secs(20);
/// How long to wait before reconnecting to a device that went away.
const RETRY_AFTER: Duration = Duration::from_secs(15);
/// How often projects and chats are copied to the cloud, besides after each reply.
const BACKUP_EVERY: Duration = Duration::from_secs(5 * 60);

/// Sharing state of the window. Not saved.
#[derive(Default)]
pub struct RemoteUi {
    /// This window's host while sharing is on. Turning sharing off stops it.
    hub: Option<Hub>,
    /// The host is being started.
    starting: bool,
    /// This device, from Tailscale.
    me: Option<Device>,
    /// Connected devices, by `Device::id`.
    clients: HashMap<String, Client>,
    /// Devices being connected to.
    connecting: HashSet<String>,
    /// Chats subscribed to over each device's current connection, by `Device::id`.
    subscribed: HashMap<String, HashSet<String>>,
    /// The periodic list refresh and backup run.
    ticking: bool,
    /// Bumped each time sharing stops, so the timers started before end.
    epoch: u64,
    /// The cloud copy, when this device is the one marked as cloud.
    cloud: Option<Store>,
    /// Cloud requests from other devices, answered in order off the UI thread. Started with the first one.
    cloud_jobs: Option<smol::channel::Sender<(Store, Hub, Request)>>,
    /// Project id -> the latest snapshot commit the cloud confirmed, so later copies send only changes.
    pushed: HashMap<String, String>,
    /// A copy to the cloud is being prepared.
    backing_up: bool,
    /// What the cloud holds, for Settings → Devices; `None` until asked.
    pub(super) cloud_projects: Option<Vec<CloudProject>>,
    /// A chat asked for with "New on <device>": opened in the focused tile when it arrives.
    open_new: Option<(String, Option<String>)>,
    /// Approvals of other devices' chats waiting for an answer here, by the local request id.
    pub(super) forwarded: HashMap<u64, Forwarded>,
}

/// An approval asked on another device, waiting for the answer given here.
pub(super) struct Forwarded {
    pub(super) device: String,
    pub(super) session: String,
    /// The host's id for the request.
    pub(super) request: u64,
    /// The user chose "Always allow" for it here.
    pub(super) always: bool,
}

impl Shell {
    /// Brings sharing in line with the settings: starts or calms the host, connects to newly
    /// included devices and drops the rest. Cheap when nothing changed; call it after settings change.
    pub(super) fn refresh_remote(&mut self, cx: &mut Context<Self>) {
        let sync = self.state.settings.sync.clone();
        if let Some(hub) = &self.remote.hub {
            hub.set_sync(sync.clone());
        }
        let me = match next_step(&sync, self.remote.me.as_ref()) {
            Step::Off => {
                self.stop_sharing();
                return;
            }
            Step::Run(me) => me,
            Step::FindThisDevice => {
                self.find_this_device(cx);
                return;
            }
        };
        // This device keeps the cloud copy when the user marked it as the cloud.
        if sync.is_cloud(&me.id) {
            if self.remote.cloud.is_none() {
                self.remote.cloud = Some(Store::open_default());
            }
        } else {
            self.remote.cloud = None;
            self.remote.cloud_jobs = None;
        }
        if self.remote.hub.is_none() {
            self.start_hub(&me, cx);
        }
        // Devices no longer included are dropped, with their chats (live or from the cloud).
        let shown = self.state.sessions().iter().filter_map(|s| s.origin.clone());
        for id in dropped_devices(&sync, self.remote.clients.keys().cloned().chain(shown)) {
            self.remote.clients.remove(&id);
            self.remote.subscribed.remove(&id);
            self.state.remove_remote(&id);
            self.drop_device_approvals(&id);
        }
        for device in sync.devices.iter().filter(|d| d.id != me.id) {
            if !self.remote.clients.contains_key(&device.id) && !self.remote.connecting.contains(&device.id) {
                self.connect_device(device.clone(), me.name.clone(), sync.port, cx);
            }
        }
        if !self.remote.ticking {
            self.remote.ticking = true;
            let epoch = self.remote.epoch;
            cx.spawn(async move |this, cx| loop {
                cx.background_executor().timer(LIST_EVERY).await;
                let alive = this.update(cx, |this, _| {
                    for client in this.remote.clients.values() {
                        let _ = client.send(&ClientMsg::ListSessions);
                    }
                    this.remote.epoch == epoch
                });
                if !matches!(alive, Ok(true)) {
                    break;
                }
            })
            .detach();
            cx.spawn(async move |this, cx| loop {
                cx.background_executor().timer(BACKUP_EVERY).await;
                let alive = this.update(cx, |this, cx| {
                    let alive = this.remote.epoch == epoch;
                    if alive {
                        this.backup(cx);
                    }
                    alive
                });
                if !matches!(alive, Ok(true)) {
                    break;
                }
            })
            .detach();
        }
    }

    /// Asks Tailscale for this device off the UI thread: it gives the address to listen on and the
    /// name devices see.
    fn find_this_device(&mut self, cx: &mut Context<Self>) {
        if self.remote.starting {
            return;
        }
        self.remote.starting = true;
        cx.spawn(async move |this, cx| {
            let found = cx.background_executor().spawn(async { tailscale::discover() }).await;
            let _ = this.update(cx, |this, cx| {
                this.remote.starting = false;
                match found {
                    Ok(net) => {
                        this.remote.me = Some(net.me);
                        this.refresh_remote(cx);
                    }
                    Err(e) => this.toast(format!("Sharing: {e}"), cx),
                }
            });
        })
        .detach();
    }

    /// Turns sharing off at once: stops the host, drops every connection, every chat shown from
    /// another device and every approval forwarded from one, and ends the timers. Cheap when already off.
    fn stop_sharing(&mut self) {
        if let Some(hub) = self.remote.hub.take() {
            hub.stop();
        }
        let shown: HashSet<String> = self.state.sessions().iter().filter_map(|s| s.origin.clone()).collect();
        for id in shown {
            self.state.remove_remote(&id);
        }
        self.drop_forwarded(|_| true);
        let r = &mut self.remote;
        r.clients.clear();
        r.subscribed.clear();
        r.cloud = None;
        r.cloud_jobs = None;
        r.pushed.clear();
        r.cloud_projects = None;
        r.open_new = None;
        if r.ticking {
            r.ticking = false;
            r.epoch += 1;
        }
    }

    /// Starts this window's host on the Tailscale address and handles its requests on the UI thread.
    fn start_hub(&mut self, me: &Device, cx: &mut Context<Self>) {
        let Ok(ip) = me.addr.parse::<IpAddr>() else { return };
        let bind = SocketAddr::new(ip, self.state.settings.sync.port);
        match Hub::start(bind, me.name.clone(), self.state.settings.sync.clone()) {
            Ok((hub, requests)) => {
                self.remote.hub = Some(hub.clone());
                cx.spawn(async move |this, cx| {
                    while let Ok(req) = requests.recv().await {
                        // Requests still queued when sharing stopped belong to no host.
                        let current = this.update(cx, |this, cx| {
                            let current = this.remote.hub.as_ref().is_some_and(|h| h.same(&hub));
                            if current {
                                this.host_request(req, cx);
                            }
                            current
                        });
                        if !matches!(current, Ok(true)) {
                            break;
                        }
                    }
                })
                .detach();
            }
            // Most often `archonui --serve` already runs on this device.
            Err(e) => self.toast(format!("Sharing: could not listen on {bind}: {e}"), cx),
        }
    }

    /// One request from another device for this window's chats. A prompt runs here like one typed
    /// in the window, so the window and every device follow the same turn.
    fn host_request(&mut self, req: Request, cx: &mut Context<Self>) {
        let Some(hub) = self.remote.hub.clone() else { return };
        let conn = req.conn;
        if let ClientMsg::Answer { session, request, allow, always } = &req.msg {
            // Only this device's own requests: a forwarded one is answered by the device it came from.
            let own = !self.remote.forwarded.contains_key(request) && self.state.is_remote(session).is_none();
            let waiting = own && self.approvals.queue.iter().any(|p| p.id == *request && &p.request.session == session);
            if waiting {
                let verdict = if *allow { Verdict::Allow } else { Verdict::Deny(DENIED.into()) };
                self.answer_approval(*request, verdict, *allow && *always, cx);
                cx.notify();
                return;
            }
        }
        if host::is_cloud(&req.msg) {
            if let Some(store) = self.remote.cloud.clone() {
                self.cloud_job(store, hub, req, cx);
                return;
            }
        }
        let subscribed = match &req.msg {
            ClientMsg::Subscribe { session } => Some(session.clone()),
            _ => None,
        };
        if let Some((id, events)) = host::handle(&mut self.state, &self.agents, &hub, self.remote.cloud.as_ref(), req) {
            self.follow_turn(&id, events, cx);
        }
        if let Some(session) = subscribed {
            self.send_waiting_approvals(&hub, conn, &session);
        }
        cx.notify();
    }

    /// Answers a cloud request off the UI thread: storing a project runs git, which can take a while.
    fn cloud_job(&mut self, store: Store, hub: Hub, req: Request, cx: &mut Context<Self>) {
        let jobs = self.remote.cloud_jobs.get_or_insert_with(|| {
            let (tx, rx) = smol::channel::unbounded::<(Store, Hub, Request)>();
            cx.background_executor()
                .spawn(async move {
                    while let Ok((store, hub, req)) = rx.recv().await {
                        host::handle_cloud(Some(&store), &hub, req);
                    }
                })
                .detach();
            tx
        });
        let _ = jobs.try_send((store, hub, req));
    }

    /// Tells connection `conn` which approvals of this device's chat `session` are waiting, when it subscribes.
    fn send_waiting_approvals(&self, hub: &Hub, conn: u64, session: &str) {
        if self.state.is_remote(session).is_some() || self.state.session(session).is_none() {
            return;
        }
        for p in self.approvals.queue.iter().filter(|p| p.request.session == session) {
            if self.remote.forwarded.contains_key(&p.id) {
                continue;
            }
            let msg = HostMsg::Approval {
                session: session.to_string(),
                request: p.id,
                tool: p.request.tool_name.clone(),
                input: p.request.input.clone(),
            };
            hub.reply(conn, &msg);
        }
    }

    /// Tells the devices following this device's chat `session` that tool use `id` waits for an answer.
    pub(super) fn share_approval(&self, session: &str, id: u64, tool: &str, input: &serde_json::Value) {
        let Some(hub) = &self.remote.hub else { return };
        if self.state.is_remote(session).is_some() {
            return;
        }
        let msg = HostMsg::Approval { session: session.to_string(), request: id, tool: tool.to_string(), input: input.clone() };
        hub.broadcast(session, &msg, None);
    }

    /// Tells the devices following this device's chat `session` that request `id` is no longer waiting.
    /// Nothing for a request that came from another device, or for a chat shown from another device.
    pub(super) fn share_approval_done(&self, session: &str, id: u64) {
        let Some(hub) = &self.remote.hub else { return };
        if self.state.is_remote(session).is_some() || self.remote.forwarded.contains_key(&id) {
            return;
        }
        hub.broadcast(session, &HostMsg::ApprovalDone { session: session.to_string(), request: id }, None);
    }

    /// Drops the forwarded approvals of `device` that are waiting. Dropping one unanswered sends nothing.
    fn drop_device_approvals(&mut self, device: &str) {
        self.drop_forwarded(|f| f.device == device);
    }

    /// Drops the forwarded approvals `gone` matches, unanswered.
    fn drop_forwarded(&mut self, gone: impl Fn(&Forwarded) -> bool) {
        let ids: Vec<u64> = self.remote.forwarded.iter().filter(|(_, f)| gone(*f)).map(|(id, _)| *id).collect();
        self.approvals.queue.retain(|p| !ids.contains(&p.id));
        for id in ids {
            self.remote.forwarded.remove(&id);
        }
    }

    /// Waits off the UI thread for the answer to forwarded approval `id`, then sends it to the device it came
    /// from. Nothing is sent when the request was dropped unanswered.
    fn await_forwarded(id: u64, answered: mpsc::Receiver<Verdict>, cx: &mut Context<Self>) {
        // A plain thread, not the shared background pool: the wait can last up to `approve::TIMEOUT`.
        let (done, verdict) = smol::channel::bounded(1);
        std::thread::spawn(move || {
            let _ = done.send_blocking(answered.recv_timeout(approve::TIMEOUT).ok());
        });
        cx.spawn(async move |this, cx| {
            let verdict = verdict.recv().await.ok().flatten();
            let _ = this.update(cx, |this, _| {
                let Some(forwarded) = this.remote.forwarded.remove(&id) else { return };
                let Some(verdict) = verdict else { return };
                if let Some(client) = this.remote.clients.get(&forwarded.device) {
                    let msg = ClientMsg::Answer {
                        session: forwarded.session,
                        request: forwarded.request,
                        allow: verdict == Verdict::Allow,
                        always: forwarded.always,
                    };
                    let _ = client.send(&msg);
                }
            });
        })
        .detach();
    }

    /// Tells the devices following chat `id` about one of its turn events. Only for this device's
    /// own chats, before the event is applied here.
    pub(super) fn share_event(&self, id: &str, ev: &Event) {
        let Some(hub) = &self.remote.hub else { return };
        if self.state.is_remote(id).is_some() {
            return;
        }
        let msg = match ev {
            Event::Out(out) => HostMsg::Event { session: id.to_string(), out: out.clone() },
            Event::Finished(result) => HostMsg::Finished {
                session: id.to_string(),
                error: result.as_ref().err().cloned(),
                stopped: self.state.stopping(id),
            },
        };
        hub.broadcast(id, &msg, None);
    }

    /// Tells the devices following chat `id` that a prompt typed here started a turn.
    pub(super) fn share_started(&self, id: &str, text: &str) {
        if let Some(hub) = &self.remote.hub {
            hub.broadcast(id, &HostMsg::Started { session: id.to_string(), text: text.to_string() }, None);
        }
    }

    /// Connects to `device` off the UI thread, then follows its messages.
    fn connect_device(&mut self, device: Device, me: String, port: u16, cx: &mut Context<Self>) {
        self.remote.connecting.insert(device.id.clone());
        cx.spawn(async move |this, cx| {
            let addr = device.addr.clone();
            let connected = cx.background_executor().spawn(async move { Client::connect(&addr, port, &me) }).await;
            let events = this
                .update(cx, |this, cx| {
                    this.remote.connecting.remove(&device.id);
                    // Sharing may have been turned off, or the device dropped, while connecting.
                    let wanted = this.state.settings.sync.enabled && this.state.settings.sync.devices.iter().any(|d| d.id == device.id);
                    match connected {
                        Ok(client) if wanted => {
                            let _ = client.send(&ClientMsg::ListSessions);
                            let is_cloud = this.state.settings.sync.is_cloud(&device.id);
                            if is_cloud {
                                let _ = client.send(&ClientMsg::ListCloud);
                            }
                            let events = client.events.clone();
                            this.remote.clients.insert(device.id.clone(), client);
                            this.remote.subscribed.remove(&device.id);
                            if is_cloud {
                                this.ask_cloud_for_away_devices();
                                this.backup(cx);
                            }
                            cx.notify();
                            Some(events)
                        }
                        Ok(_) => None,
                        Err(_) => {
                            // Offline devices are normal; show their chats from the cloud and try again later.
                            this.ask_cloud_for_chats(&device.id);
                            this.retry_later(cx);
                            None
                        }
                    }
                })
                .ok()
                .flatten();
            let Some(events) = events else { return };
            while let Ok(ev) = events.recv().await {
                // Messages still queued from a connection that was dropped (sharing off, the device
                // removed, or replaced by a newer connection) are left unread.
                let current = this.update(cx, |this, cx| {
                    let current = this.remote.clients.get(&device.id).is_some_and(|c| c.events.same_channel(&events));
                    if current {
                        this.device_event(&device.id, ev, cx);
                    }
                    current
                });
                if !matches!(current, Ok(true)) {
                    break;
                }
            }
        })
        .detach();
    }

    fn retry_later(&mut self, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(RETRY_AFTER).await;
            let _ = this.update(cx, |this, cx| this.refresh_remote(cx));
        })
        .detach();
    }

    /// One message from a connected device about its chats.
    fn device_event(&mut self, device: &str, ev: ClientEvent, cx: &mut Context<Self>) {
        let msg = match ev {
            ClientEvent::Msg(msg) => msg,
            ClientEvent::Closed(_) => {
                self.remote.clients.remove(device);
                self.remote.subscribed.remove(device);
                self.state.remove_remote(device);
                self.drop_device_approvals(device);
                self.ask_cloud_for_chats(device);
                self.retry_later(cx);
                cx.notify();
                return;
            }
        };
        match msg {
            HostMsg::Sessions { sessions } => {
                let subscribed = self.remote.subscribed.entry(device.to_string()).or_default();
                let wanted = to_subscribe(&self.state, device, sessions.iter().map(|s| s.id.as_str()), subscribed);
                if let Some(client) = self.remote.clients.get(device) {
                    for id in wanted {
                        if client.send(&ClientMsg::Subscribe { session: id.clone() }).is_ok() {
                            subscribed.insert(id);
                        }
                    }
                }
            }
            HostMsg::Snapshot { session, busy } => match serde_json::from_value::<Session>(session) {
                Ok(s) if self.state.session(&s.id).is_none_or(|old| old.origin.as_deref() == Some(device)) => {
                    let id = s.id.clone();
                    self.state.put_remote(s, device, busy);
                    // The chat started with "New on <device>" opens here as soon as it arrives.
                    if self.remote.open_new.as_ref().is_some_and(|(d, opened)| d == device && opened.as_deref() == Some(id.as_str())) {
                        self.remote.open_new = None;
                        self.state.workspace.layout.open_in_focused(&id);
                        self.save(cx);
                    }
                }
                // Same id as one of this device's chats: never replace a local chat.
                Ok(_) => {}
                Err(e) => eprintln!("archonui: a chat from {device} could not be read: {e}"),
            },
            HostMsg::Created { session } => {
                if let Some((asked, _)) = self.remote.open_new.as_ref().filter(|(d, opened)| d == device && opened.is_none()) {
                    let asked = asked.clone();
                    self.remote.open_new = Some((asked, Some(session.id.clone())));
                }
                if let Some(client) = self.remote.clients.get(device) {
                    if client.send(&ClientMsg::Subscribe { session: session.id.clone() }).is_ok() {
                        self.remote.subscribed.entry(device.to_string()).or_default().insert(session.id);
                    }
                }
            }
            // A turn's messages only reach chats shown from the device that sent them: never a chat of
            // this device (one adopted here keeps the id) or one from another device.
            HostMsg::Started { session, .. }
            | HostMsg::Event { session, .. }
            | HostMsg::Finished { session, .. }
            | HostMsg::Draft { session, .. }
                if self.state.is_remote(&session) != Some(device) => {}
            HostMsg::Started { session, text } => self.state.remote_started(&session, &text),
            HostMsg::Event { session, out } => {
                self.state.remote_event(&session, out);
            }
            HostMsg::Finished { session, error, stopped } => {
                let applied = self.state.remote_finished(&session, error, stopped);
                let stopped = applied.ended.as_ref().is_some_and(|e| matches!(e.outcome, crate::app::notify::Outcome::Stopped));
                self.send_queued(&session, stopped, cx);
            }
            HostMsg::Draft { session, text } => self.state.set_draft(&session, &text),
            HostMsg::Approval { session, request, tool, input } => {
                let known = self.remote.forwarded.values().any(|f| f.device == device && f.session == session && f.request == request);
                // A subscriber gets the waiting approvals again on each subscribe: show each one once.
                if !known {
                    let (req, answered) = approve::forwarded(session.clone(), tool, input);
                    let id = self.queue_approval(req, cx);
                    self.remote.forwarded.insert(id, Forwarded { device: device.to_string(), session, request, always: false });
                    Self::await_forwarded(id, answered, cx);
                }
            }
            HostMsg::ApprovalDone { session, request } => {
                self.drop_forwarded(|f| f.device == device && f.session == session && f.request == request);
            }
            // A copy the cloud couldn't apply (it lacks an earlier snapshot): send everything next time.
            HostMsg::Error { message } if message.starts_with("git ") => {
                eprintln!("archonui: cloud copy from {device}: {message}");
                self.remote.pushed.clear();
            }
            HostMsg::Error { message } => self.toast(message, cx),
            HostMsg::Welcome { .. } => {}
            HostMsg::Stored { project, commit, .. } => {
                self.remote.pushed.insert(project, commit);
            }
            HostMsg::Cloud { projects, .. } => self.remote.cloud_projects = Some(projects),
            HostMsg::ProjectData { project, device: from, commit, bundle } => self.restore_project(&project, &from, &commit, &bundle, cx),
            HostMsg::Chats { device: owner, sessions } => {
                // Only while that device is away; when it is online its own copies are live.
                if !self.remote.clients.contains_key(&owner) && self.state.settings.sync.devices.iter().any(|d| d.id == owner) {
                    for value in sessions {
                        match serde_json::from_value::<Session>(value) {
                            Ok(s) if self.state.session(&s.id).is_none_or(|old| old.origin.as_deref() == Some(owner.as_str())) => {
                                self.state.put_remote(s, &owner, false)
                            }
                            Ok(_) => {}
                            Err(e) => eprintln!("archonui: a stored chat of {owner} could not be read: {e}"),
                        }
                    }
                }
            }
        }
        cx.notify();
    }

    /// Sends a prompt typed here to the device chat `id` lives on. The turn shows up here when the
    /// device reports it started. Returns whether it was sent.
    pub(super) fn send_remote(&mut self, id: &str, text: &str, cx: &mut Context<Self>) -> bool {
        let Some(device) = self.state.is_remote(id).map(str::to_string) else { return false };
        if !self.remote.clients.contains_key(&device) {
            // The device is away (this copy came from the cloud): continue the chat here instead.
            // Its agent may not be installed here: the default one takes over, with the chat so far.
            let agent = self.state.session(id).map(|s| s.agent.clone()).unwrap_or_default();
            let installed = self.agents.iter().any(|a| a.harness.id() == agent);
            let takeover = match super::shell::pick_harness(self.state.settings.default_harness.as_deref(), &self.agents) {
                _ if installed => None,
                Some(other) => Some(other),
                None => {
                    self.toast("No agent is enabled", cx);
                    return false;
                }
            };
            let cwd = self.default_folder();
            if self.state.adopt_remote(id, cwd) {
                if let Some(other) = takeover {
                    self.switch_agent(id, other, cx);
                }
                let name = self.chat_device_name(&device);
                self.toast(format!("{name} is away, so this chat continues here"), cx);
                self.flush_sessions(cx);
                return self.send_now(id, text, cx);
            }
            return false;
        }
        let files = match self.pending_as_remote_files(id) {
            Ok(files) => files,
            Err(e) => {
                self.toast(e, cx);
                return false;
            }
        };
        let Some(s) = self.state.session(id) else { return false };
        let msg = ClientMsg::Prompt {
            session: id.to_string(),
            text: text.to_string(),
            mode: Some(s.mode),
            model: s.model.clone(),
            effort: s.effort.clone(),
            files,
        };
        match self.remote.clients.get(&device).map(|c| c.send(&msg)) {
            Some(Ok(())) => {
                // Sent with the prompt; the host keeps its own copies.
                while !self.state.pending(id).is_empty() {
                    self.state.remove_pending(id, 0);
                }
                true
            }
            _ => {
                self.toast("That device is not connected", cx);
                false
            }
        }
    }

    /// The files waiting to go with the next message in chat `id`, read and encoded for its device.
    fn pending_as_remote_files(&self, id: &str) -> Result<Vec<RemoteFile>, String> {
        let pending = self.state.pending(id);
        if pending.len() > crate::engine::attach::MAX_FILES {
            return Err(format!("At most {} files can go with one message", crate::engine::attach::MAX_FILES));
        }
        if pending.iter().map(|a| a.size).sum::<u64>() > MAX_REMOTE_FILES_BYTES {
            return Err("The files are too large to send to another device".into());
        }
        pending
            .iter()
            .map(|a| {
                let bytes = std::fs::read(&a.path).map_err(|e| format!("Could not read {}: {e}", a.name))?;
                Ok(RemoteFile { name: a.name.clone(), data: STANDARD.encode(bytes) })
            })
            .collect()
    }

    /// Connected devices as (id, name), for the sidebar's "New on <device>" buttons.
    pub(super) fn connected_devices(&self) -> Vec<(String, String)> {
        let mut out: Vec<(String, String)> = self.remote.clients.keys().map(|id| (id.clone(), self.chat_device_name(id))).collect();
        out.sort_by(|a, b| a.1.cmp(&b.1));
        out
    }

    /// Starts a chat on `device` with its default agent and folder; it opens here when it arrives.
    pub(super) fn new_remote_chat(&mut self, device: &str, cx: &mut Context<Self>) {
        let Some(client) = self.remote.clients.get(device) else {
            self.toast("That device is not connected", cx);
            return;
        };
        let msg = ClientMsg::NewSession { agent: String::new(), cwd: std::path::PathBuf::new(), project: None };
        if client.send(&msg).is_ok() {
            self.remote.open_new = Some((device.to_string(), None));
        }
    }

    /// Stops the reply in chat `id`: here for this device's chats, on its device for a remote one.
    pub(super) fn stop_chat(&mut self, id: &str) -> bool {
        match self.state.is_remote(id).map(str::to_string) {
            Some(device) => self
                .remote
                .clients
                .get(&device)
                .is_some_and(|c| c.send(&ClientMsg::Cancel { session: id.to_string() }).is_ok()),
            None => self.state.stop(id),
        }
    }

    /// The name of the device a remote chat lives on, for the sidebar.
    pub(super) fn chat_device(&self, id: &str) -> Option<String> {
        let device = self.state.is_remote(id)?;
        Some(self.chat_device_name(device))
    }

    fn chat_device_name(&self, device: &str) -> String {
        let name = self.state.settings.sync.devices.iter().find(|d| d.id == device).map(|d| d.name.clone());
        name.unwrap_or_else(|| device.to_string())
    }

    /// The connection to the cloud device, when one is marked and connected (and it isn't this device).
    fn cloud_client(&self) -> Option<&Client> {
        let cloud = self.state.settings.sync.cloud.as_deref()?;
        self.remote.clients.get(cloud)
    }

    /// Asks the cloud for the stored chats of `device`, to show while it is away.
    fn ask_cloud_for_chats(&self, device: &str) {
        if self.state.settings.sync.is_cloud(device) {
            return;
        }
        if let Some(cloud) = self.cloud_client() {
            let _ = cloud.send(&ClientMsg::GetChats { device: device.to_string() });
        }
    }

    /// Asks the cloud for the chats of every included device that isn't connected.
    fn ask_cloud_for_away_devices(&self) {
        let me = self.remote.me.as_ref().map(|d| d.id.as_str());
        for d in &self.state.settings.sync.devices {
            if Some(d.id.as_str()) != me && !self.remote.clients.contains_key(&d.id) {
                self.ask_cloud_for_chats(&d.id);
            }
        }
    }

    /// Asks the cloud what it holds, for Settings → Devices.
    pub(super) fn refresh_cloud_list(&mut self) {
        if let Some(store) = &self.remote.cloud {
            self.remote.cloud_projects = Some(store.projects());
        } else if let Some(cloud) = self.cloud_client() {
            let _ = cloud.send(&ClientMsg::ListCloud);
        }
    }

    /// Copies this device's projects (working files, committed or not) and chats to the cloud.
    /// Only what changed since the cloud's last confirmation is sent. Runs off the UI thread.
    pub(super) fn backup(&mut self, cx: &mut Context<Self>) {
        if self.remote.backing_up || !self.state.settings.sync.enabled {
            return;
        }
        let Some(me) = self.remote.me.clone() else { return };
        if self.state.settings.sync.is_cloud(&me.id) || self.cloud_client().is_none() {
            return;
        }
        // Chats first: small, and what matters most when this device is away.
        let sessions: Vec<serde_json::Value> = self
            .state
            .sessions()
            .iter()
            .filter(|s| s.origin.is_none() && s.visible())
            .filter_map(|s| serde_json::to_value(s).ok())
            .collect();
        if let Some(cloud) = self.cloud_client() {
            let _ = cloud.send(&ClientMsg::PutChats { device: me.id.clone(), sessions });
        }
        let projects: Vec<(String, String, std::path::PathBuf, Option<String>)> = self
            .state
            .projects()
            .iter()
            .filter(|p| p.path.is_dir())
            .map(|p| (p.id.clone(), p.name.clone(), p.path.clone(), self.remote.pushed.get(&p.id).cloned()))
            .collect();
        if projects.is_empty() {
            return;
        }
        self.remote.backing_up = true;
        let device = me.id;
        let secrets = self.state.settings.sync.copy_secrets;
        cx.spawn(async move |this, cx| {
            let ready = cx
                .background_executor()
                .spawn(async move {
                    let mut out = Vec::new();
                    for (id, name, path, pushed) in projects {
                        let made = snapshot::take(&path, &device, secrets).map(|_| snapshot::current(&path, &device));
                        let commit = match made {
                            Ok(Some(commit)) => commit,
                            Ok(None) => continue,
                            Err(e) => {
                                eprintln!("archonui: could not snapshot {name}: {e}");
                                continue;
                            }
                        };
                        if pushed.as_deref() == Some(commit.as_str()) {
                            continue;
                        }
                        match snapshot::bundle(&path, &device, pushed.as_deref()) {
                            Ok(bytes) => out.push(ClientMsg::PutProject {
                                project: id,
                                name,
                                device: device.clone(),
                                commit,
                                bundle: STANDARD.encode(bytes),
                            }),
                            Err(e) => eprintln!("archonui: could not pack {name}: {e}"),
                        }
                    }
                    out
                })
                .await;
            let _ = this.update(cx, |this, _| {
                this.remote.backing_up = false;
                if let Some(cloud) = this.cloud_client() {
                    for msg in &ready {
                        let _ = cloud.send(msg);
                    }
                }
            });
        })
        .detach();
    }

    /// Asks the cloud for a project's latest copy from `device`; it is restored when it arrives.
    pub(super) fn open_from_cloud(&mut self, project: &str, device: &str, cx: &mut Context<Self>) {
        if let Some(store) = self.remote.cloud.clone() {
            // This device is the cloud: read the copy straight from the store, off the UI thread.
            let (project, device) = (project.to_string(), device.to_string());
            cx.spawn(async move |this, cx| {
                let (p, d) = (project.clone(), device.clone());
                let got = cx.background_executor().spawn(async move { store.get_project(&p, &d) }).await;
                let _ = this.update(cx, |this, cx| match got {
                    Ok((commit, bytes)) => {
                        let bundle = STANDARD.encode(bytes);
                        this.restore_project(&project, &device, &commit, &bundle, cx);
                    }
                    Err(e) => this.toast(e, cx),
                });
            })
            .detach();
        } else if let Some(cloud) = self.cloud_client() {
            let _ = cloud.send(&ClientMsg::GetProject { project: project.to_string(), device: device.to_string() });
        } else {
            self.toast("The cloud device is not connected", cx);
        }
    }

    /// Writes a project copy from the cloud into a new folder next to the default folder, and adds it
    /// as a project. Never writes into an existing folder.
    fn restore_project(&mut self, project: &str, device: &str, commit: &str, bundle: &str, cx: &mut Context<Self>) {
        let name = self
            .remote
            .cloud_projects
            .as_ref()
            .and_then(|list| list.iter().find(|p| p.project == project && p.device == device))
            .map(|p| p.name.clone())
            .unwrap_or_else(|| project.to_string());
        let from = self.chat_device_name(device);
        let base = self.default_folder();
        let safe = |s: &str| s.chars().map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '-' }).collect::<String>();
        let stem = format!("{}-from-{}", safe(&name), safe(&from));
        let mut dest = base.join(&stem);
        let mut n = 2;
        while dest.exists() {
            dest = base.join(format!("{stem}-{n}"));
            n += 1;
        }
        let Ok(bytes) = STANDARD.decode(bundle) else {
            self.toast("The cloud sent a broken copy", cx);
            return;
        };
        let commit = commit.to_string();
        cx.spawn(async move |this, cx| {
            let target = dest.clone();
            let done = cx.background_executor().spawn(async move { snapshot::restore(&bytes, &commit, &target) }).await;
            let _ = this.update(cx, |this, cx| match done {
                Ok(()) => {
                    this.state.add_project(&format!("{name} (from {from})"), dest.clone());
                    this.save(cx);
                    this.toast(format!("Opened {name} from the cloud in {}", dest.display()), cx);
                }
                Err(e) => this.toast(format!("Could not open {name}: {e}"), cx),
            });
        })
        .detach();
    }
}

/// What `refresh_remote` does next, from the settings alone.
#[derive(Debug, PartialEq)]
enum Step {
    /// Sharing is off: nothing listens, connects, runs `tailscale` or copies.
    Off,
    /// Sharing is on but this device isn't known yet: ask Tailscale.
    FindThisDevice,
    /// Host, connect and copy as this device.
    Run(Device),
}

fn next_step(sync: &crate::remote::SyncSettings, me: Option<&Device>) -> Step {
    match me {
        _ if !sync.enabled => Step::Off,
        None => Step::FindThisDevice,
        Some(me) => Step::Run(me.clone()),
    }
}

/// The chats listed by `device` to subscribe to: new ones, and ones shown from the cloud while it was
/// away, but none already subscribed and never one that is this device's own (adopted here) or another device's.
fn to_subscribe<'a>(
    state: &crate::app::state::AppState,
    device: &str,
    listed: impl Iterator<Item = &'a str>,
    subscribed: &HashSet<String>,
) -> Vec<String> {
    listed
        .filter(|id| !subscribed.contains(*id))
        .filter(|id| state.session(id).is_none_or(|s| s.origin.as_deref() == Some(device)))
        .map(str::to_string)
        .collect()
}

/// The devices whose connection or chats must go: every one of `known` the settings no longer include.
fn dropped_devices(sync: &crate::remote::SyncSettings, known: impl Iterator<Item = String>) -> Vec<String> {
    let mut out: Vec<String> = known.filter(|id| !sync.devices.iter().any(|d| &d.id == id)).collect();
    out.sort();
    out.dedup();
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::remote::SyncSettings;

    fn device(id: &str) -> Device {
        Device { id: id.into(), name: id.into(), dns_name: String::new(), addr: "100.1.2.3".into() }
    }

    /// The hard rule: a fresh install, or settings from before sharing existed, never reach Tailscale,
    /// a listener, a connection or a timer.
    #[test]
    fn sharing_is_off_with_fresh_or_old_settings() {
        let dir = std::env::temp_dir().join(format!("archonui-sharing-off-{}", crate::store::new_id()));
        std::fs::create_dir_all(&dir).unwrap();
        let fresh = crate::app::persist::load(&dir, &dir.join("no-legacy-settings.json"));
        assert_eq!(next_step(&fresh.settings.sync, None), Step::Off);
        std::fs::write(dir.join("settings.json"), "{}").unwrap();
        let old = crate::app::persist::load(&dir, &dir.join("no-legacy-settings.json"));
        assert_eq!(next_step(&old.settings.sync, None), Step::Off);
        // Off wins even when this device was found while sharing was on.
        let me = device("me");
        assert_eq!(next_step(&SyncSettings::default(), Some(&me)), Step::Off);
        assert_eq!(next_step(&SyncSettings { enabled: true, ..Default::default() }, None), Step::FindThisDevice);
        let ui = RemoteUi::default();
        assert!(ui.hub.is_none() && ui.clients.is_empty() && ui.cloud.is_none() && ui.cloud_jobs.is_none() && !ui.ticking);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn listed_chats_are_subscribed_once_and_never_over_a_local_one() {
        use crate::app::{settings::AppSettings, state::AppState, workspace::Workspace};
        let local = Session::new("claude", "/home/me".into());
        let adopted = Session::new("claude", "/home/me".into());
        let (local_id, adopted_id) = (local.id.clone(), adopted.id.clone());
        let mut state = AppState::new(vec![local, adopted], Vec::new(), Workspace::default(), AppSettings::default());
        let from_cloud = Session::new("claude", "/home/pc".into());
        let from_cloud_id = from_cloud.id.clone();
        state.put_remote(from_cloud, "pc", false);
        let other = Session::new("claude", "/home/vps".into());
        let other_id = other.id.clone();
        state.put_remote(other, "vps", false);
        let live = Session::new("claude", "/home/pc".into());
        let live_id = live.id.clone();
        state.put_remote(live, "pc", false);
        let subscribed: HashSet<String> = [live_id.clone()].into();
        let listed = ["new-chat", local_id.as_str(), adopted_id.as_str(), from_cloud_id.as_str(), other_id.as_str(), live_id.as_str()];
        let wanted = to_subscribe(&state, "pc", listed.into_iter(), &subscribed);
        assert_eq!(wanted, vec!["new-chat".to_string(), from_cloud_id], "new chats and cloud copies of pc's chats only");
    }

    #[test]
    fn devices_no_longer_included_are_dropped_with_their_cloud_chats() {
        let sync = SyncSettings { enabled: true, devices: vec![device("pc")], ..Default::default() };
        // "laptop" is connected, "vps" only shows chats from the cloud: both were unticked.
        let known = ["pc", "laptop", "vps", "vps"].into_iter().map(String::from);
        assert_eq!(dropped_devices(&sync, known), vec!["laptop".to_string(), "vps".to_string()]);
    }
}

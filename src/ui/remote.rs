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
        proto::{self, About, ClientMsg, CloudProject, HostMsg, RemoteFile, MAX_REMOTE_FILES_BYTES},
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
    time::{Duration, Instant},
};

/// How often the chat lists of connected devices are asked for again, to pick up new chats.
const LIST_EVERY: Duration = Duration::from_secs(20);
/// How long to wait before reconnecting to a device that went away.
const RETRY_AFTER: Duration = Duration::from_secs(15);
/// How often projects and chats are copied to the cloud, besides after each reply.
const BACKUP_EVERY: Duration = Duration::from_secs(5 * 60);
/// How long after a device's connection drops its chats wait for it rather than continue here.
const RECONNECT_GRACE: Duration = Duration::from_secs(60);

/// Sharing state of the window. Not saved.
#[derive(Default)]
pub struct RemoteUi {
    /// This window's host while sharing is on. Turning sharing off stops it.
    hub: Option<Hub>,
    /// The host is being started.
    starting: bool,
    /// This device, from Tailscale.
    me: Option<Device>,
    /// When finding this device last failed.
    failed_at: Option<Instant>,
    /// When starting the host last failed.
    listen_failed_at: Option<Instant>,
    /// Connected devices, by `Device::id`.
    clients: HashMap<String, Client>,
    /// Devices being connected to.
    connecting: HashSet<String>,
    /// Chats subscribed to over each device's current connection, by `Device::id`.
    subscribed: HashMap<String, HashSet<String>>,
    /// The periodic list refresh and backup run.
    ticking: bool,
    /// A reconnect is scheduled. One at a time: it reconnects every missing device.
    retrying: bool,
    /// Bumped each time sharing stops, so the timers started before end.
    epoch: u64,
    /// The cloud copy, when this device is the one marked as cloud.
    cloud: Option<Store>,
    /// Cloud requests from other devices, answered in order off the UI thread. Started with the first one.
    cloud_jobs: Option<smol::channel::Sender<(Store, Hub, Request)>>,
    /// Project id -> the latest snapshot commit the cloud confirmed, so later copies send only changes.
    pushed: HashMap<String, String>,
    /// Projects sent to the cloud that it has not stored yet.
    sending: HashSet<String>,
    /// Cloud copy errors already shown, so one that repeats every round shows once.
    copy_errors: HashSet<String>,
    /// Projects too big for one message to the cloud. Skipped until the cloud changes, rather than
    /// packed again, minutes of work for a big history, every round.
    too_big: HashSet<String>,
    /// A copy to the cloud is being prepared.
    backing_up: bool,
    /// When the current cloud last held everything this device has (seconds since the epoch).
    last_backup: Option<u64>,
    /// When the five-minute backup timer fires next.
    next_backup: Option<Instant>,
    /// A reply ended while a copy was being prepared: copy again once it is sent.
    backup_again: bool,
    /// What the cloud holds, for Settings → Devices; `None` until asked.
    pub(super) cloud_projects: Option<Vec<CloudProject>>,
    /// The cloud was asked what it holds over its current connection; the page doesn't ask again.
    pub(super) cloud_asked: bool,
    /// The cloud `cloud_projects` and `pushed` are about, so a newly marked cloud starts afresh.
    cloud_for: Option<String>,
    /// A chat asked for with "New on <device>": opened in the focused tile when it arrives.
    open_new: Option<(String, Option<String>)>,
    /// Approvals of other devices' chats waiting for an answer here, by the local request id.
    pub(super) forwarded: HashMap<u64, Forwarded>,
    /// `chats_fingerprint` of the chats last sent to the current cloud; `None` sends them next round.
    chats_fingerprint: Option<u64>,
    /// When each device's connection last dropped, for `RECONNECT_GRACE`.
    dropped: HashMap<String, Instant>,
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

impl RemoteUi {
    /// Forgets what the cloud confirmed and every copy on its way, for a new or lost cloud.
    fn forget_copies(&mut self) {
        self.pushed.clear();
        self.sending.clear();
        self.too_big.clear();
        self.chats_fingerprint = None;
        self.last_backup = None;
    }

    /// Project `project` (`name`) needs `bytes` to copy, more than one message holds: it is no longer
    /// tried. Returns what to show, once.
    fn copy_too_big(&mut self, project: String, name: &str, bytes: u64) -> Option<String> {
        eprintln!("archonui: cloud copy: {name} needs {} MB, more than the {} MB one copy can carry", bytes >> 20, proto::MAX_BUNDLE_BYTES >> 20);
        self.too_big.insert(project);
        let text = format!(
            "Cloud copy: {name} is too big to copy ({} MB; at most {} MB fits), so it is left out",
            bytes >> 20,
            proto::MAX_BUNDLE_BYTES >> 20
        );
        self.copy_errors.insert(text.clone()).then_some(text)
    }

    /// The cloud stored `project` at `commit`: later copies send only what is newer.
    fn copy_stored(&mut self, project: String, commit: String) {
        self.sending.remove(&project);
        self.pushed.insert(project, commit);
        if self.sending.is_empty() {
            self.last_backup = Some(crate::store::now());
        }
    }

    /// The cloud could not store a copy. `about` names the request: a project goes back to a full
    /// copy next round, which the cloud can always apply, and a chats copy resets nothing. A host
    /// from before `about` names nothing, so every unconfirmed project goes back to a full copy.
    /// Returns what to show: a git error is only logged (the full copy fixes it), and any other
    /// error shows once, not on every round it repeats.
    fn copy_failed(&mut self, message: String, about: Option<&About>) -> Option<String> {
        eprintln!("archonui: cloud copy: {message}");
        match about {
            None => {
                for project in &self.sending {
                    self.pushed.remove(project);
                }
                self.chats_fingerprint = None;
            }
            Some(About { project: Some(project), .. }) => {
                self.sending.remove(project);
                self.pushed.remove(project);
            }
            // The chats copy failed: send them again next round.
            Some(_) => self.chats_fingerprint = None,
        }
        if message.starts_with("git ") || !self.copy_errors.insert(message.clone()) {
            return None;
        }
        Some(format!("Cloud copy: {message}"))
    }
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
        // Another cloud holds none of what the last one confirmed or listed.
        let new_cloud = self.remote.cloud_for != sync.cloud;
        if new_cloud {
            self.remote.cloud_for = sync.cloud.clone();
            self.remote.forget_copies();
            self.remote.copy_errors.clear();
            self.remote.cloud_projects = None;
            self.remote.cloud_asked = false;
        }
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
            if self.remote.listen_failed_at.is_some_and(|t| t.elapsed() < RETRY_AFTER) {
                // Kept pending while listening fails, even when every device is connected.
                self.retry_later(cx);
            } else {
                self.start_hub(&me, cx);
            }
        }
        // Devices no longer included are dropped, with their chats (live or from the cloud).
        let shown = self.state.sessions().iter().filter_map(|s| s.origin.clone());
        for id in dropped_devices(&sync, self.remote.clients.keys().cloned().chain(shown)) {
            self.remote.clients.remove(&id);
            self.remote.subscribed.remove(&id);
            self.remote.dropped.remove(&id);
            self.state.remove_remote(&id);
            self.drop_device_approvals(&id);
        }
        for device in sync.devices.iter().filter(|d| d.id != me.id) {
            if !self.remote.clients.contains_key(&device.id) && !self.remote.connecting.contains(&device.id) {
                self.connect_device(device.clone(), me.name.clone(), sync.port, cx);
            }
        }
        // A device already connected when it was marked as the cloud gets what a new connection
        // to the cloud would: the away devices' chats asked for, and a first copy.
        if new_cloud && self.cloud_client().is_some() {
            self.ask_cloud_for_away_devices();
            self.backup(cx);
        }
        if !self.remote.ticking {
            self.remote.ticking = true;
            let epoch = self.remote.epoch;
            cx.spawn(async move |this, cx| loop {
                cx.background_executor().timer(LIST_EVERY).await;
                let alive = this.update(cx, |this, cx| {
                    for client in this.remote.clients.values() {
                        let _ = client.send(&ClientMsg::ListSessions);
                    }
                    // Empty chats other devices asked for here and then left.
                    if let Some(hub) = this.remote.hub.clone() {
                        if !host::expire_requested(&mut this.state, &hub).is_empty() {
                            this.save(cx);
                        }
                    }
                    this.remote.epoch == epoch
                });
                if !matches!(alive, Ok(true)) {
                    break;
                }
            })
            .detach();
            cx.spawn(async move |this, cx| loop {
                let _ = this.update(cx, |this, _| this.remote.next_backup = Some(Instant::now() + BACKUP_EVERY));
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
    /// name devices see. After a failure it waits `RETRY_AFTER` before asking again, since
    /// `refresh_remote` runs on every save.
    fn find_this_device(&mut self, cx: &mut Context<Self>) {
        if self.remote.starting || self.remote.failed_at.is_some_and(|t| t.elapsed() < RETRY_AFTER) {
            return;
        }
        self.remote.starting = true;
        cx.spawn(async move |this, cx| {
            let found = cx.background_executor().spawn(async { tailscale::discover() }).await;
            let _ = this.update(cx, |this, cx| {
                this.remote.starting = false;
                match found {
                    Ok(net) => {
                        this.remote.failed_at = None;
                        this.remote.me = Some(net.me);
                        this.refresh_remote(cx);
                    }
                    Err(e) => {
                        this.remote.failed_at = Some(Instant::now());
                        this.toast(format!("Sharing: {e}"), cx);
                    }
                }
            });
        })
        .detach();
    }

    /// Turns sharing off at once: stops the host, drops every connection, every chat shown from
    /// another device, every approval forwarded from one and what Tailscale said about this device,
    /// and ends the timers. Cheap when already off.
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
        r.forget_copies();
        r.next_backup = None;
        r.dropped.clear();
        r.copy_errors.clear();
        r.backup_again = false;
        r.cloud_projects = None;
        r.cloud_asked = false;
        r.cloud_for = None;
        r.open_new = None;
        // Asked again when sharing comes back on: Tailscale may have a new address or name for it by then.
        r.me = None;
        // Turned back on, the host is tried at once.
        r.listen_failed_at = None;
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
                self.remote.listen_failed_at = None;
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
            // Most often `archonui --serve` already runs on this device, or sharing was just turned off
            // and on and the old listener has not let go of the port yet. Tried again after
            // `RETRY_AFTER`; the toast shows once per run of failures.
            Err(e) => {
                let first = self.remote.listen_failed_at.is_none();
                self.remote.listen_failed_at = Some(Instant::now());
                if first {
                    self.toast(format!("Sharing: could not listen on {bind}: {e}"), cx);
                }
                self.retry_later(cx);
            }
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
            // And only from a connection following that chat.
            let waiting = own
                && hub.follows(conn, session)
                && self.approvals.queue.iter().any(|p| p.id == *request && &p.request.session == session);
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
        let deleting = match &req.msg {
            ClientMsg::Delete { session } => Some(session.clone()),
            _ => None,
        };
        if let Some((id, events)) = host::handle(&mut self.state, &self.agents, &hub, self.remote.cloud.as_ref(), req) {
            self.follow_turn(&id, events, cx);
        }
        // A chat another device deleted takes its shell and tile here with it.
        if let Some(id) = deleting.filter(|id| self.state.session(id).is_none()) {
            self.terminal_forget(&id);
            self.save(cx);
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
            Event::Out(out) => HostMsg::Event { session: id.to_string(), out: Some(out.clone()) },
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

    /// Tells the devices following chat `id` that a prompt typed here could not start, so their copy
    /// gets the prompt and the error too. `before` is the chat's message count before the prompt.
    pub(super) fn share_start_failed(&self, id: &str, before: usize) {
        if let Some(hub) = &self.remote.hub {
            for msg in host::start_failed(&self.state, id, before) {
                hub.broadcast(id, &msg, None);
            }
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
                                this.remote.cloud_asked = client.send(&ClientMsg::ListCloud).is_ok();
                            }
                            let events = client.events.clone();
                            this.remote.clients.insert(device.id.clone(), client);
                            this.remote.dropped.remove(&device.id);
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

    /// Reconnects missing devices after `RETRY_AFTER`. Each failed connect calls this, so only one
    /// retry is kept pending; otherwise every round would multiply the timers and connects.
    fn retry_later(&mut self, cx: &mut Context<Self>) {
        if self.remote.retrying {
            return;
        }
        self.remote.retrying = true;
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(RETRY_AFTER).await;
            let _ = this.update(cx, |this, cx| {
                this.remote.retrying = false;
                this.refresh_remote(cx);
            });
        })
        .detach();
    }

    /// One message from a connected device about its chats.
    fn device_event(&mut self, device: &str, ev: ClientEvent, cx: &mut Context<Self>) {
        let msg = match ev {
            ClientEvent::Msg(msg) => msg,
            ClientEvent::Closed(_) => {
                if self.state.settings.sync.is_cloud(device) {
                    self.remote.cloud_asked = false;
                    // Copies sent over the lost connection get no answer; `pushed` still holds what it stored.
                    self.remote.sending.clear();
                    self.remote.chats_fingerprint = None;
                }
                self.remote.clients.remove(device);
                self.remote.subscribed.remove(device);
                self.state.remote_away(device);
                self.remote.dropped.insert(device.to_string(), Instant::now());
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
            // A turn's messages and approvals only reach chats shown from the device that sent them: never
            // a chat of this device (one adopted here keeps the id) or one from another device.
            HostMsg::Started { session, .. }
            | HostMsg::Event { session, .. }
            | HostMsg::Finished { session, .. }
            | HostMsg::Draft { session, .. }
            | HostMsg::Approval { session, .. }
            | HostMsg::Renamed { session, .. }
            | HostMsg::Deleted { session }
                if self.state.is_remote(&session) != Some(device) => {}
            HostMsg::Started { session, text } => self.state.remote_started(&session, &text),
            HostMsg::Event { session, out } => {
                // An event this version does not know is skipped.
                if let Some(out) = out {
                    self.state.remote_event(&session, out);
                }
            }
            HostMsg::Finished { session, error, stopped } => {
                let applied = self.state.remote_finished(&session, error, stopped);
                let stopped = applied.ended.as_ref().is_some_and(|e| matches!(e.outcome, crate::app::notify::Outcome::Stopped));
                self.send_queued(&session, stopped, cx);
            }
            HostMsg::Draft { session, text } => self.state.set_draft(&session, &text),
            HostMsg::Renamed { session, title } => {
                self.state.rename_session(&session, &title);
            }
            HostMsg::Deleted { session } => {
                self.drop_forwarded(|f| f.device == device && f.session == session);
                if let Some(subscribed) = self.remote.subscribed.get_mut(device) {
                    subscribed.remove(&session);
                }
                self.state.delete_session(&session);
                self.terminal_forget(&session);
                self.save(cx);
            }
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
            // A host older than the request (rename or delete, say). Not a failed copy: the cloud knows those.
            HostMsg::Error { message, .. } if proto::is_unknown_request(&message) => {
                let name = self.chat_device_name(device);
                self.toast(format!("{name} runs an older archonUI that can't do that; update it there"), cx);
            }
            // Answers about the cloud copy count only from the cloud: a late one from a cloud since
            // replaced would mark copies the new one lacks as stored, or list what it doesn't hold.
            msg if cloud_reply(&msg) && !self.state.settings.sync.is_cloud(device) => {}
            HostMsg::Error { message, about: Some(about) } if is_copy(&about) => {
                if !self.state.settings.sync.is_cloud(device) {
                    eprintln!("archonui: cloud copy to {device}, no longer the cloud: {message}");
                } else if let Some(text) = self.remote.copy_failed(message, Some(&about)) {
                    self.toast(text, cx);
                }
            }
            // A host from before `about` names no request: an error while copies are on their way is
            // taken as a failed copy, and those projects are sent whole next time.
            HostMsg::Error { message, about: None } if self.state.settings.sync.is_cloud(device) && !self.remote.sending.is_empty() => {
                if let Some(text) = self.remote.copy_failed(message, None) {
                    self.toast(text, cx);
                }
            }
            HostMsg::Error { message, about: None } if message.starts_with("git ") && self.state.settings.sync.is_cloud(device) => {
                eprintln!("archonui: cloud copy from {device}: {message}");
                self.remote.pushed.clear();
            }
            HostMsg::Error { message, .. } => self.toast(message, cx),
            HostMsg::Welcome { .. } | HostMsg::Unknown => {}
            HostMsg::Stored { project, commit, .. } => self.remote.copy_stored(project, commit),
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
            if reconnecting(self.remote.dropped.get(&device), Instant::now()) {
                let name = self.chat_device_name(&device);
                self.toast(format!("{name} is reconnecting; try again in a moment"), cx);
                return false;
            }
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
            let name = self.chat_device_name(&device);
            let Some(new_id) = self.state.adopt_remote(id, cwd, &name) else { return false };
            if let Some(other) = takeover {
                self.switch_agent(&new_id, other, cx);
            }
            self.toast(format!("{name} is away, so this chat continues here"), cx);
            self.flush_sessions(cx);
            return self.send_now(&new_id, text, cx);
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
            // "None" picked here must reach the host too, not keep its last choice.
            default_model: s.model.is_none(),
            default_effort: s.effort.is_none(),
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

    /// Asks the device chat `id` lives on to rename it; the name changes here when it answers.
    /// Returns false, doing nothing, for this device's own chats.
    pub(super) fn rename_remote(&mut self, id: &str, title: &str, cx: &mut Context<Self>) -> bool {
        let msg = ClientMsg::Rename { session: id.to_string(), title: title.to_string() };
        self.ask_chat_device(id, msg, "rename", cx)
    }

    /// Asks the device chat `id` lives on to delete it; it goes here when it answers.
    /// Returns false, doing nothing, for this device's own chats.
    pub(super) fn delete_remote(&mut self, id: &str, cx: &mut Context<Self>) -> bool {
        self.ask_chat_device(id, ClientMsg::Delete { session: id.to_string() }, "delete", cx)
    }

    /// Sends `msg` to the device remote chat `id` lives on, or says why it can't. False for a chat of this device.
    fn ask_chat_device(&mut self, id: &str, msg: ClientMsg, what: &str, cx: &mut Context<Self>) -> bool {
        let Some(device) = self.state.is_remote(id).map(str::to_string) else { return false };
        let sent = self.remote.clients.get(&device).is_some_and(|c| c.send(&msg).is_ok());
        if !sent {
            // A copy from the cloud: changing it here would be undone, so it waits for the device.
            let name = self.chat_device_name(&device);
            self.toast(format!("{name} is not connected; {what} this chat there or once it's back"), cx);
        }
        true
    }

    /// Tells the devices following this device's chat `id` that it was renamed here.
    pub(super) fn share_renamed(&self, id: &str, title: &str) {
        let Some(hub) = &self.remote.hub else { return };
        if self.state.is_remote(id).is_none() {
            hub.broadcast(id, &HostMsg::Renamed { session: id.to_string(), title: title.to_string() }, None);
        }
    }

    /// Tells the devices following chat `id`, just deleted here, that it is gone.
    pub(super) fn share_deleted(&self, id: &str) {
        if let Some(hub) = &self.remote.hub {
            hub.broadcast(id, &HostMsg::Deleted { session: id.to_string() }, None);
        }
    }

    /// `share_deleted` for each of `ids`. Nothing while sharing is off.
    pub(super) fn share_deleted_all(&self, ids: &[String]) {
        for id in ids {
            self.share_deleted(id);
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

    /// A device's name, by its id: this device's own too, as in the cloud copy list.
    /// The cloud copy's progress for the Devices page: a round running, the last full backup, the next timed one.
    pub(super) fn backup_status(&self) -> (bool, Option<u64>, Option<Instant>) {
        (self.remote.backing_up, self.remote.last_backup, self.remote.next_backup)
    }

    /// Whether this window has a live connection to device `id`.
    pub(super) fn device_connected(&self, id: &str) -> bool {
        self.remote.clients.contains_key(id)
    }

    pub(super) fn chat_device_name(&self, device: &str) -> String {
        device_name(&self.state.settings.sync, self.remote.me.as_ref(), device)
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
        let r = &self.remote;
        for id in away_devices(&self.state.settings.sync, me, |id| r.clients.contains_key(id) || r.connecting.contains(id)) {
            self.ask_cloud_for_chats(&id);
        }
    }

    /// Asks the cloud what it holds, for Settings → Devices.
    pub(super) fn refresh_cloud_list(&mut self) {
        if let Some(store) = &self.remote.cloud {
            self.remote.cloud_projects = Some(store.projects());
        } else if let Some(cloud) = self.cloud_client() {
            self.remote.cloud_asked = cloud.send(&ClientMsg::ListCloud).is_ok();
        }
    }

    /// Asks the cloud what it holds unless it was already asked over this connection, for the
    /// Devices page, which calls it on each render.
    pub(super) fn ask_cloud_list_once(&mut self) {
        if self.remote.cloud_projects.is_none() && !self.remote.cloud_asked {
            self.refresh_cloud_list();
        }
    }

    /// Copies this device's projects (working files, committed or not) and chats to the cloud.
    /// Only what changed since the cloud's last confirmation is sent. Runs off the UI thread.
    pub(super) fn backup(&mut self, cx: &mut Context<Self>) {
        if !self.state.settings.sync.enabled {
            return;
        }
        if self.remote.backing_up {
            self.remote.backup_again = true;
            return;
        }
        let Some(me) = self.remote.me.clone() else { return };
        if self.state.settings.sync.is_cloud(&me.id) || self.cloud_client().is_none() {
            return;
        }
        // Chats first: small, and what matters most when this device is away. Sent only when they
        // differ from what this cloud last got.
        let sessions: Vec<serde_json::Value> = self
            .state
            .sessions()
            .iter()
            .filter(|s| s.origin.is_none() && s.visible())
            .filter_map(|s| serde_json::to_value(s).ok())
            .collect();
        let fingerprint = chats_fingerprint(&sessions);
        if self.remote.chats_fingerprint != Some(fingerprint) {
            if let Some(cloud) = self.cloud_client() {
                if cloud.send(&ClientMsg::PutChats { device: me.id.clone(), sessions }).is_ok() {
                    self.remote.chats_fingerprint = Some(fingerprint);
                }
            }
        }
        let projects: Vec<(String, String, std::path::PathBuf, Option<String>)> = self
            .state
            .projects()
            .iter()
            .filter(|p| p.path.is_dir() && !self.remote.too_big.contains(&p.id))
            .map(|p| (p.id.clone(), p.name.clone(), p.path.clone(), self.remote.pushed.get(&p.id).cloned()))
            .collect();
        if projects.is_empty() {
            return;
        }
        self.remote.backing_up = true;
        let device = me.id;
        let secrets = self.state.settings.sync.copy_secrets;
        // The copies are made against what this cloud confirmed; another one would refuse them.
        let cloud = self.state.settings.sync.cloud.clone();
        // One project at a time, each sent as soon as it is ready, so only one copy is in memory
        // here at once; the frame is built off the UI thread.
        cx.spawn(async move |this, cx| {
            for (id, name, path, pushed) in projects {
                let device = device.clone();
                let made = cx.background_executor().spawn(async move { project_copy(&path, id, name, &device, pushed, secrets) }).await;
                let go_on = this.update(cx, |this, cx| {
                    if this.state.settings.sync.cloud != cloud {
                        // Made for a cloud since replaced: copy again, whole, to the new one.
                        this.remote.backup_again = true;
                        return false;
                    }
                    match made {
                        ProjectCopy::Frame { project, frame } => {
                            if this.cloud_client().is_some_and(|c| c.send_frame(frame).is_ok()) {
                                this.remote.sending.insert(project);
                            }
                        }
                        ProjectCopy::TooBig { project, name, bytes } => {
                            if let Some(text) = this.remote.copy_too_big(project, &name, bytes) {
                                this.toast(text, cx);
                            }
                        }
                        ProjectCopy::Nothing => {}
                    }
                    true
                });
                if !matches!(go_on, Ok(true)) {
                    break;
                }
            }
            let _ = this.update(cx, |this, cx| {
                this.remote.backing_up = false;
                // Nothing left waiting for the cloud to confirm: it holds everything as of now.
                if this.remote.sending.is_empty() && this.state.settings.sync.cloud == cloud {
                    this.remote.last_backup = Some(crate::store::now());
                }
                cx.notify();
                if std::mem::take(&mut this.remote.backup_again) {
                    this.backup(cx);
                }
            });
        })
        .detach();
    }

    /// The name the cloud lists for `project` from `device`, else its id.
    fn cloud_project_name(&self, project: &str, device: &str) -> String {
        self.remote
            .cloud_projects
            .iter()
            .flatten()
            .find(|p| p.project == project && p.device == device)
            .map_or_else(|| project.to_string(), |p| p.name.clone())
    }

    /// Asks the cloud for a project's latest copy from `device`; it is restored when it arrives.
    pub(super) fn open_from_cloud(&mut self, project: &str, device: &str, cx: &mut Context<Self>) {
        if let Some(store) = self.remote.cloud.clone() {
            self.toast(format!("Opening a copy of {}…", self.cloud_project_name(project, device)), cx);
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
            self.toast(format!("Opening a copy of {}…", self.cloud_project_name(project, device)), cx);
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
        let Ok(bytes) = STANDARD.decode(bundle) else {
            self.toast("The cloud sent a broken copy", cx);
            return;
        };
        let commit = commit.to_string();
        cx.spawn(async move |this, cx| {
            // The folder is picked as it is made, so a second "Open copy" still running gets another one.
            let done = cx.background_executor().spawn(async move { snapshot::restore_new(&bytes, &commit, &base, &stem) }).await;
            let _ = this.update(cx, |this, cx| match done {
                Ok(dest) => {
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

/// One project's copy for the cloud, as `project_copy` made it.
enum ProjectCopy {
    /// The `PutProject` frame, ready to send.
    Frame { project: String, frame: String },
    /// Over the most one message carries.
    TooBig { project: String, name: String, bytes: u64 },
    /// Unchanged since the cloud's copy, or it failed (logged).
    Nothing,
}

/// Snapshots project `id` at `path` and packs what the cloud, which holds `pushed`, lacks. Blocking.
fn project_copy(path: &std::path::Path, id: String, name: String, device: &str, pushed: Option<String>, secrets: bool) -> ProjectCopy {
    let made = snapshot::take(path, &id, device, secrets).map(|_| snapshot::current(path, &id, device));
    let commit = match made {
        Ok(Some(commit)) => commit,
        Ok(None) => return ProjectCopy::Nothing,
        Err(e) => {
            eprintln!("archonui: could not snapshot {name}: {e}");
            return ProjectCopy::Nothing;
        }
    };
    if pushed.as_deref() == Some(commit.as_str()) {
        return ProjectCopy::Nothing;
    }
    match snapshot::bundle(path, &id, device, pushed.as_deref(), proto::MAX_BUNDLE_BYTES) {
        // Sent anyway, it would only drop the connection to the cloud.
        Ok(snapshot::Bundle::TooBig(bytes)) => ProjectCopy::TooBig { project: id, name, bytes },
        Ok(snapshot::Bundle::Ready(bytes)) => {
            let frame = proto::put_project_frame(&id, &name, device, &commit, &bytes);
            ProjectCopy::Frame { project: id, frame }
        }
        Err(e) => {
            eprintln!("archonui: could not pack {name}: {e}");
            ProjectCopy::Nothing
        }
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

/// A hash of the chats as they are sent, to tell whether the cloud already has them.
fn chats_fingerprint(sessions: &[serde_json::Value]) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    for s in sessions {
        s.to_string().hash(&mut hasher);
    }
    hasher.finish()
}

/// Whether a connection that dropped at `dropped` may still come back, so its chats wait for it.
fn reconnecting(dropped: Option<&Instant>, now: Instant) -> bool {
    dropped.is_some_and(|then| now.duration_since(*then) < RECONNECT_GRACE)
}

/// The included devices, other than `me`, to show from the cloud. One connected or still being
/// connected to (`reached`) is left out: at start-up the cloud often answers first, and its copies
/// would be taken for an away device's, so typing in one would continue it here while it is online.
/// A connect that fails asks the cloud then.
fn away_devices(sync: &crate::remote::SyncSettings, me: Option<&str>, reached: impl Fn(&str) -> bool) -> Vec<String> {
    sync.devices.iter().filter(|d| Some(d.id.as_str()) != me && !reached(&d.id)).map(|d| d.id.clone()).collect()
}

/// The name of device `id`: an included device's, or this device's (`me`), whose projects the cloud
/// holds too. The id itself when neither.
fn device_name(sync: &crate::remote::SyncSettings, me: Option<&Device>, id: &str) -> String {
    let found = sync.devices.iter().chain(me).find(|d| d.id == id);
    found.map_or_else(|| id.to_string(), |d| d.name.clone())
}

/// Whether `msg` answers a cloud request, so only the cloud's own count.
fn cloud_reply(msg: &HostMsg) -> bool {
    matches!(msg, HostMsg::Stored { .. } | HostMsg::Cloud { .. } | HostMsg::ProjectData { .. } | HostMsg::Chats { .. })
}

/// Whether an error answers a copy to the cloud: a `PutProject` or `PutChats`.
fn is_copy(about: &About) -> bool {
    matches!(about.request.as_str(), "put_project" | "put_chats")
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
        Device { id: id.into(), name: id.into(), dns_name: String::new(), addr: "100.1.2.3".into(), addr6: String::new() }
    }

    #[test]
    fn only_cloud_answers_are_cloud_replies() {
        let stored = HostMsg::Stored { project: "p".into(), device: "d".into(), commit: "c".into() };
        assert!(cloud_reply(&stored));
        assert!(cloud_reply(&HostMsg::Chats { device: "d".into(), sessions: Vec::new() }));
        assert!(!cloud_reply(&HostMsg::Sessions { sessions: Vec::new() }));
        assert!(!cloud_reply(&HostMsg::Error { message: "x".into(), about: None }));
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
    fn devices_being_connected_to_are_not_shown_from_the_cloud() {
        let sync = SyncSettings { enabled: true, devices: vec![device("me"), device("pc"), device("laptop"), device("vps")], ..Default::default() };
        // The vps is connected, the pc is still being connected to.
        let away = away_devices(&sync, Some("me"), |id| id == "vps" || id == "pc");
        assert_eq!(away, ["laptop"]);
    }

    #[test]
    fn this_devices_own_cloud_copies_are_named() {
        let sync = SyncSettings { enabled: true, devices: vec![device("pc")], ..Default::default() };
        let me = Device { name: "laptop".into(), ..device("n-me") };
        assert_eq!(device_name(&sync, Some(&me), "n-me"), "laptop");
        assert_eq!(device_name(&sync, Some(&me), "pc"), "pc");
        assert_eq!(device_name(&sync, None, "n-me"), "n-me");
    }

    #[test]
    fn devices_no_longer_included_are_dropped_with_their_cloud_chats() {
        let sync = SyncSettings { enabled: true, devices: vec![device("pc")], ..Default::default() };
        // "laptop" is connected, "vps" only shows chats from the cloud: both were unticked.
        let known = ["pc", "laptop", "vps", "vps"].into_iter().map(String::from);
        assert_eq!(dropped_devices(&sync, known), vec!["laptop".to_string(), "vps".to_string()]);
    }

    #[test]
    fn a_failed_cloud_copy_is_sent_whole_next_time_and_shown_once() {
        let mut r = RemoteUi::default();
        r.pushed.insert("pa".into(), "c1".into());
        r.pushed.insert("pb".into(), "c2".into());
        r.pushed.insert("pc".into(), "c3".into());
        r.sending.extend(["pa".to_string(), "pb".to_string()]);
        r.copy_stored("pb".into(), "c4".into());

        // Not a git error: before, it was shown every round and the stale base kept.
        let err = "refs/archon/wip/n1 is at abc, not def".to_string();
        assert!(r.copy_failed(err.clone(), None).is_some());
        assert_eq!(r.pushed.get("pa"), None, "the next copy of pa is a full one");
        assert_eq!(r.pushed.get("pb").map(String::as_str), Some("c4"), "stored before the error");
        assert_eq!(r.pushed.get("pc").map(String::as_str), Some("c3"), "not on its way");
        r.pushed.insert("pa".into(), "c5".into());
        assert_eq!(r.copy_failed(err, None), None, "shown once");
        assert_eq!(r.pushed.get("pa"), None, "still unconfirmed, so reset again");
        assert_eq!(r.copy_failed("git fetch: missing prerequisite".into(), None), None, "git errors are only logged");
        assert!(r.copy_failed("disk full".into(), None).is_some(), "a new error shows");

        r.copy_stored("pa".into(), "c6".into());
        assert!(r.sending.is_empty());
        r.forget_copies();
        assert!(r.pushed.is_empty());
    }

    #[test]
    fn a_project_too_big_to_copy_is_shown_once_and_left_out_until_the_cloud_changes() {
        let mut r = RemoteUi::default();
        let text = r.copy_too_big("pa".into(), "big", 1000 << 20).expect("shown");
        assert!(text.contains("big is too big") && text.contains("1000 MB") && text.contains("383 MB"), "{text}");
        assert!(r.too_big.contains("pa"));
        assert_eq!(r.copy_too_big("pa".into(), "big", 1000 << 20), None, "shown once");
        r.forget_copies();
        assert!(r.too_big.is_empty(), "a new cloud tries again");
    }

    #[test]
    fn a_project_copy_is_a_ready_frame_and_nothing_once_the_cloud_has_it() {
        let dir = std::env::temp_dir().join(format!("archonui-project-copy-{}", crate::store::new_id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("a.txt"), "hello\n").unwrap();
        let ProjectCopy::Frame { project, frame } = project_copy(&dir, "p1".into(), "app".into(), "n1", None, false) else {
            panic!("expected a frame");
        };
        assert_eq!(project, "p1");
        let Ok(ClientMsg::PutProject { project, name, device, commit, bundle }) = proto::decode::<ClientMsg>(&frame) else {
            panic!("not a put_project: {frame}");
        };
        assert_eq!((project.as_str(), name.as_str(), device.as_str()), ("p1", "app", "n1"));
        assert!(!bundle.is_empty());
        assert!(matches!(project_copy(&dir, "p1".into(), "app".into(), "n1", Some(commit), false), ProjectCopy::Nothing));
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn a_named_failed_copy_resets_only_its_project() {
        let mut r = RemoteUi::default();
        for (p, c) in [("pa", "c1"), ("pb", "c2")] {
            r.pushed.insert(p.into(), c.into());
        }
        r.sending.extend(["pa".to_string(), "pb".to_string()]);
        let pa = About { request: "put_project".into(), project: Some("pa".into()) };
        assert!(r.copy_failed("refs/archon/wip/n1 is at abc, not def".into(), Some(&pa)).is_some());
        assert_eq!(r.pushed.get("pa"), None, "the next copy of pa is a full one");
        assert_eq!(r.pushed.get("pb").map(String::as_str), Some("c2"), "pb is still on its way and keeps its base");
        assert!(!r.sending.contains("pa") && r.sending.contains("pb"));
        // A failed chats copy resets no project.
        let chats = About { request: "put_chats".into(), project: None };
        assert!(r.copy_failed("disk full".into(), Some(&chats)).is_some());
        assert_eq!(r.pushed.get("pb").map(String::as_str), Some("c2"));
    }

    #[test]
    fn chats_are_sent_again_only_when_something_in_them_changed() {
        let mut chat = Session::new("claude", "/w".into());
        chat.updated = 1000;
        let print = |s: &Session| chats_fingerprint(&[serde_json::to_value(s).unwrap()]);
        let before = print(&chat);
        assert_eq!(before, print(&chat), "the same chats");
        chat.title = "Renamed".into();
        let renamed = print(&chat);
        assert_ne!(before, renamed, "a rename alone, with the same time and messages");
        chat.updated = 1001;
        assert_ne!(renamed, print(&chat));
        assert_ne!(before, chats_fingerprint(&[]));
    }

    #[test]
    fn a_failed_chats_copy_is_sent_again() {
        let mut r = RemoteUi { chats_fingerprint: Some(7), ..Default::default() };
        r.copy_failed("disk full".into(), Some(&About { request: "put_chats".into(), project: None }));
        assert_eq!(r.chats_fingerprint, None);
        let mut r = RemoteUi { chats_fingerprint: Some(7), ..Default::default() };
        r.copy_failed("disk full".into(), Some(&About { request: "put_project".into(), project: Some("p".into()) }));
        assert_eq!(r.chats_fingerprint, Some(7), "a project's failure leaves the chats alone");
    }

    /// An error from the cloud about anything but a copy, e.g. a chat prompt on a VPS that also runs
    /// chats, is never taken as a failed copy.
    #[test]
    fn only_copy_errors_are_copy_failures() {
        assert!(is_copy(&About { request: "put_project".into(), project: Some("p".into()) }));
        assert!(is_copy(&About { request: "put_chats".into(), project: None }));
        for request in ["prompt", "subscribe", "get_project", "list_cloud", "new_session", ""] {
            assert!(!is_copy(&About { request: request.into(), project: None }), "{request}");
        }
    }

    #[test]
    fn reconnecting_checks_the_grace_period() {
        let now = Instant::now();
        assert!(!reconnecting(None, now), "never dropped means not reconnecting");
        assert!(reconnecting(Some(&now.checked_sub(Duration::from_secs(10)).unwrap()), now), "10s ago is within 60s grace");
        assert!(!reconnecting(Some(&now.checked_sub(Duration::from_secs(61)).unwrap()), now), "61s ago is past the grace period");
    }
}

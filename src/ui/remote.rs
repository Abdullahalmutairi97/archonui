//! The window's side of sharing chats between devices (`crate::remote`): while sharing is on, this
//! window hosts its own chats for the included devices, and shows theirs as remote chats.
//! Off (the default), none of this runs.

use super::shell::Shell;
use crate::{
    engine::Event,
    remote::{
        client::{Client, ClientEvent},
        host,
        hub::{Hub, Request},
        proto::{ClientMsg, HostMsg},
        tailscale, Device,
    },
    store::Session,
};
use gpui::Context;
use std::{
    collections::{HashMap, HashSet},
    net::{IpAddr, SocketAddr},
    time::Duration,
};

/// How often the chat lists of connected devices are asked for again, to pick up new chats.
const LIST_EVERY: Duration = Duration::from_secs(20);
/// How long to wait before reconnecting to a device that went away.
const RETRY_AFTER: Duration = Duration::from_secs(15);

/// Sharing state of the window. Not saved.
#[derive(Default)]
pub struct RemoteUi {
    /// This window's host, once started. It keeps listening while the app runs; turning sharing off
    /// makes it refuse new devices.
    hub: Option<Hub>,
    /// The host is being started.
    starting: bool,
    /// This device, from Tailscale.
    me: Option<Device>,
    /// Connected devices, by `Device::id`.
    clients: HashMap<String, Client>,
    /// Devices being connected to.
    connecting: HashSet<String>,
    /// The periodic list refresh runs.
    ticking: bool,
}

impl Shell {
    /// Brings sharing in line with the settings: starts or calms the host, connects to newly
    /// included devices and drops the rest. Cheap when nothing changed; call it after settings change.
    pub(super) fn refresh_remote(&mut self, cx: &mut Context<Self>) {
        let sync = self.state.settings.sync.clone();
        if let Some(hub) = &self.remote.hub {
            hub.set_sync(sync.clone());
        }
        if !sync.enabled {
            let devices: Vec<String> = self.remote.clients.drain().map(|(id, _)| id).collect();
            for id in devices {
                self.state.remove_remote(&id);
            }
            return;
        }
        let Some(me) = self.remote.me.clone() else {
            // Tailscale first: it gives the address to listen on and the name devices see.
            if !self.remote.starting {
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
            return;
        };
        if self.remote.hub.is_none() {
            self.start_hub(&me, cx);
        }
        // Devices no longer included are dropped, with their chats.
        let included: HashSet<&str> = sync.devices.iter().map(|d| d.id.as_str()).collect();
        let dropped: Vec<String> = self.remote.clients.keys().filter(|id| !included.contains(id.as_str())).cloned().collect();
        for id in dropped {
            self.remote.clients.remove(&id);
            self.state.remove_remote(&id);
        }
        for device in sync.devices.iter().filter(|d| d.id != me.id) {
            if !self.remote.clients.contains_key(&device.id) && !self.remote.connecting.contains(&device.id) {
                self.connect_device(device.clone(), me.name.clone(), sync.port, cx);
            }
        }
        if !self.remote.ticking {
            self.remote.ticking = true;
            cx.spawn(async move |this, cx| loop {
                cx.background_executor().timer(LIST_EVERY).await;
                let alive = this.update(cx, |this, _| {
                    for client in this.remote.clients.values() {
                        let _ = client.send(&ClientMsg::ListSessions);
                    }
                });
                if alive.is_err() {
                    break;
                }
            })
            .detach();
        }
    }

    /// Starts this window's host on the Tailscale address and handles its requests on the UI thread.
    fn start_hub(&mut self, me: &Device, cx: &mut Context<Self>) {
        let Ok(ip) = me.addr.parse::<IpAddr>() else { return };
        let bind = SocketAddr::new(ip, self.state.settings.sync.port);
        match Hub::start(bind, me.name.clone(), self.state.settings.sync.clone()) {
            Ok((hub, requests)) => {
                self.remote.hub = Some(hub);
                cx.spawn(async move |this, cx| {
                    while let Ok(req) = requests.recv().await {
                        if this.update(cx, |this, cx| this.host_request(req, cx)).is_err() {
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
        if let Some((id, events)) = host::handle(&mut self.state, &self.agents, &hub, req) {
            self.follow_turn(&id, events, cx);
        }
        cx.notify();
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
            Event::Finished(result) => HostMsg::Finished { session: id.to_string(), error: result.as_ref().err().cloned() },
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
                            let events = client.events.clone();
                            this.remote.clients.insert(device.id.clone(), client);
                            cx.notify();
                            Some(events)
                        }
                        Ok(_) => None,
                        Err(_) => {
                            // Offline devices are normal; try again later, quietly.
                            this.retry_later(cx);
                            None
                        }
                    }
                })
                .ok()
                .flatten();
            let Some(events) = events else { return };
            while let Ok(ev) = events.recv().await {
                if this.update(cx, |this, cx| this.device_event(&device.id, ev, cx)).is_err() {
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
                self.state.remove_remote(device);
                self.retry_later(cx);
                cx.notify();
                return;
            }
        };
        match msg {
            HostMsg::Sessions { sessions } => {
                if let Some(client) = self.remote.clients.get(device) {
                    for s in sessions.iter().filter(|s| self.state.is_remote(&s.id).is_none()) {
                        let _ = client.send(&ClientMsg::Subscribe { session: s.id.clone() });
                    }
                }
            }
            HostMsg::Snapshot { session, busy } => match serde_json::from_value::<Session>(session) {
                Ok(s) if self.state.session(&s.id).is_none_or(|old| old.origin.as_deref() == Some(device)) => {
                    self.state.put_remote(s, device, busy)
                }
                // Same id as one of this device's chats: never replace a local chat.
                Ok(_) => {}
                Err(e) => eprintln!("archonui: a chat from {device} could not be read: {e}"),
            },
            HostMsg::Created { session } => {
                if let Some(client) = self.remote.clients.get(device) {
                    let _ = client.send(&ClientMsg::Subscribe { session: session.id });
                }
            }
            HostMsg::Started { session, text } => self.state.remote_started(&session, &text),
            HostMsg::Event { session, out } => {
                self.state.remote_event(&session, out);
            }
            HostMsg::Finished { session, error } => {
                let applied = self.state.remote_finished(&session, error);
                let stopped = applied.ended.as_ref().is_some_and(|e| matches!(e.outcome, crate::app::notify::Outcome::Stopped));
                self.send_queued(&session, stopped, cx);
            }
            HostMsg::Draft { session, text } => self.state.set_draft(&session, &text),
            HostMsg::Error { message } => self.toast(message, cx),
            HostMsg::Welcome { .. } => {}
            HostMsg::Stored { .. } | HostMsg::Cloud { .. } | HostMsg::ProjectData { .. } | HostMsg::Chats { .. } => {}
        }
        cx.notify();
    }

    /// Sends a prompt typed here to the device chat `id` lives on. The turn shows up here when the
    /// device reports it started. Returns whether it was sent.
    pub(super) fn send_remote(&mut self, id: &str, text: &str, cx: &mut Context<Self>) -> bool {
        let Some(device) = self.state.is_remote(id).map(str::to_string) else { return false };
        if !self.state.pending(id).is_empty() {
            self.toast("Files can't be sent to a chat on another device yet", cx);
            return false;
        }
        let Some(s) = self.state.session(id) else { return false };
        let msg = ClientMsg::Prompt {
            session: id.to_string(),
            text: text.to_string(),
            mode: Some(s.mode),
            model: s.model.clone(),
            effort: s.effort.clone(),
        };
        match self.remote.clients.get(&device).map(|c| c.send(&msg)) {
            Some(Ok(())) => true,
            _ => {
                self.toast("That device is not connected", cx);
                false
            }
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
        let name = self.state.settings.sync.devices.iter().find(|d| d.id == device).map(|d| d.name.clone());
        Some(name.unwrap_or_else(|| device.to_string()))
    }
}

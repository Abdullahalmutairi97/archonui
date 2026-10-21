//! The Devices page: share chats with other devices on the tailnet, and pick the cloud copy. Off by default.

use super::controls::{act, edit, toggle_row};
use crate::remote::{Device, SyncSettings};
use crate::ui::{
    composer::icons::stroke_icon,
    scale::px,
    shell::Shell,
    theme::{Rgba, Theme},
    widgets::{
        button::{Button, ButtonStyle},
        section::{card as section_card, section_head, SettingRow},
        toggle::Toggle,
    },
};
use gpui::{div, prelude::*, AnyElement, Context, Hsla, SharedString, WeakEntity};

/// Two screens: devices sharing with each other.
const SHARE: &str = "M1.5 3h8v5.5h-8zM6.5 8.5v1.5M6.5 7.5h8V13h-8z";
const DESKTOP: &str = "M2 3h12v8H2zM6 14h4M8 11v3";
const PHONE: &str = "M5 1.5h6v13H5zM7.5 12.5h1";
const CLOUD: &str = "M4.5 12.5a3 3 0 0 1-.4-6 4 4 0 0 1 7.8.6 2.7 2.7 0 0 1-.4 5.4z";
const FOLDER: &str = "M2 4h4l1.5 1.5H14v7.5H2z";
/// Status dots: the diff view's green, readable on every dark theme, and an amber for a device that left.
const LIVE: Rgba = crate::ui::theme::GREEN;
const AWAY: Rgba = crate::ui::theme::AMBER;

/// How a device is doing, as its row shows it.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Status {
    ThisDevice,
    Connected,
    Online,
    Offline,
    Gone,
}

impl Status {
    fn label(self) -> &'static str {
        match self {
            Status::ThisDevice => "This device",
            Status::Connected => "Connected",
            Status::Online => "Online",
            Status::Offline => "Offline",
            Status::Gone => "Not on your tailnet",
        }
    }

    fn colour(self, theme: &Theme) -> Rgba {
        match self {
            Status::ThisDevice | Status::Connected | Status::Online => LIVE,
            Status::Offline => theme.faint,
            Status::Gone => AWAY,
        }
    }
}

/// One row of the device list.
struct Row {
    device: Device,
    os: String,
    status: Status,
    shared: bool,
}

impl Shell {
    pub(super) fn devices_page(&mut self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let theme = self.theme.clone();
        let weak = cx.entity().downgrade();
        let enabled = self.state.settings.sync.enabled;
        // The first render with sharing on starts the search. Later renders reuse its result.
        if enabled && self.settings.tailnet.is_none() {
            self.detect_tailnet(cx);
        }
        let sync = self.state.settings.sync.clone();
        let me = match &self.settings.tailnet {
            Some(Ok(net)) => Some(net.me.clone()),
            _ => None,
        };

        let subtitle = match (&me, enabled) {
            (_, false) => "Uses your Tailscale network. Nothing leaves this device while it's off.".to_string(),
            (Some(me), true) => format!("On · this device is {}", me.name),
            (None, true) => "On".to_string(),
        };
        let toggle = Toggle::new("sync-enabled", enabled, &theme).on_change(edit(&weak, |this: &mut Shell, on: bool, cx| {
            this.state.settings.sync.enabled = on;
            this.save(cx);
        }));
        let mut blocks = vec![section_head("", None, Vec::new(), &theme), card(SHARE, enabled, "Share chats between devices", &subtitle, Some(enabled), toggle.into_any_element(), &theme)];
        if !enabled {
            return blocks;
        }

        blocks.push(self.device_list(&sync, &weak, cx));
        if sync.cloud.is_some() {
            blocks.push(self.cloud_section(&weak, cx));
        }
        blocks
    }

    /// Every device on the tailnet: this one, the shared ones, the online ones, and the offline ones on request.
    fn device_list(&mut self, sync: &SyncSettings, weak: &WeakEntity<Shell>, _cx: &mut Context<Self>) -> AnyElement {
        let theme = self.theme.clone();
        let refresh = div()
            .id("devices-refresh")
            .px(px(11.))
            .py(px(5.))
            .rounded(px(7.))
            .text_size(px(12.))
            .text_color(theme.acc2.to_hsla())
            .cursor_pointer()
            .hover(|s| s.bg(theme.line.to_hsla()))
            .on_click({
                let weak = weak.clone();
                move |_, _, cx| {
                    weak.update(cx, |this, cx| {
                        this.detect_tailnet(cx);
                        cx.notify();
                    })
                    .ok();
                }
            })
            .child(if self.settings.tailnet_loading { "Looking…" } else { "Refresh" });
        let header = section_head("Your devices", None, vec![refresh.into_any_element()], &theme);
        let list = section_card(Vec::new(), &theme);
        let net = match self.settings.tailnet.clone() {
            None => return div().child(header).child(list.child(note("Looking for Tailscale devices…", &theme))).into_any_element(),
            Some(Err(message)) => {
                let text = format!("{message}. Install Tailscale and sign in, then press Refresh.");
                return div().child(header).child(list.child(note(&text, &theme))).into_any_element();
            }
            Some(Ok(net)) => net,
        };

        let rows = device_rows(sync, &net, |id| self.device_connected(id));
        let hidden = rows.iter().filter(|r| hidden_unless_asked(r)).count();
        let show_offline = self.settings.show_offline;
        let mut list = list;
        for row in rows.into_iter().filter(|r| show_offline || !hidden_unless_asked(r)) {
            list = list.child(self.device_row(row, sync, weak));
        }
        let mut out = div().child(header).child(list);
        if hidden > 0 {
            let label = if show_offline {
                "Hide offline devices".to_string()
            } else if hidden == 1 {
                "Show 1 offline device".to_string()
            } else {
                format!("Show {hidden} offline devices")
            };
            out = out.child(
                div()
                    .id("devices-offline")
                    .py(px(12.))
                    .px(px(4.))
                    .text_size(px(12.))
                    .text_color(theme.muted.to_hsla())
                    .cursor_pointer()
                    .hover(|s| s.text_color(theme.fg.to_hsla()))
                    .on_click({
                        let weak = weak.clone();
                        move |_, _, cx| {
                            weak.update(cx, |this, cx| {
                                this.settings.show_offline = !this.settings.show_offline;
                                cx.notify();
                            })
                            .ok();
                        }
                    })
                    .child(label),
            );
        }
        out.into_any_element()
    }

    /// One device: icon, name, status, and its share switch and cloud pill.
    fn device_row(&self, row: Row, sync: &SyncSettings, weak: &WeakEntity<Shell>) -> AnyElement {
        let theme = &self.theme;
        let id = row.device.id.clone();
        let is_cloud = sync.is_cloud(&id);
        let this_device = row.status == Status::ThisDevice;
        let glyph = if is_cloud {
            CLOUD
        } else if matches!(row.os.to_lowercase().as_str(), "ios" | "android") {
            PHONE
        } else {
            DESKTOP
        };
        let lit = row.shared || this_device;
        let status = row.status;
        let detail = match os_name(&row.os) {
            Some(os) => format!("{} · {os}", status.label()),
            None => status.label().to_string(),
        };

        let mut controls = div().flex_none().flex().items_center().gap(px(10.));
        // The cloud is one of the shared devices or this one.
        if lit {
            let device = row.device.clone();
            let pill = div()
                .id(SharedString::from(format!("device-cloud-{id}")))
                .flex()
                .items_center()
                .gap(px(5.))
                .px(px(9.))
                .py(px(4.))
                .rounded(px(999.))
                .text_size(px(12.))
                .cursor_pointer()
                .on_click({
                    let weak = weak.clone();
                    move |_, _, cx| {
                        weak.update(cx, |this, cx| {
                            let sync = &mut this.state.settings.sync;
                            sync.cloud = if sync.is_cloud(&device.id) { None } else { Some(device.id.clone()) };
                            this.save(cx);
                        })
                        .ok();
                    }
                });
            controls = controls.child(if is_cloud {
                pill.px(px(10.))
                    .gap(px(6.))
                    .bg(theme.acc.to_hsla())
                    .text_color(theme.on.to_hsla())
                    .font_weight(gpui::FontWeight::MEDIUM)
                    .child(stroke_icon(CLOUD, 13., 1.4, theme.on.to_hsla()))
                    .child("Cloud")
            } else {
                pill.text_color(theme.acc2.to_hsla()).hover(|s| s.opacity(0.8)).child("Use as cloud")
            });
        }
        if this_device {
            // Where the other rows have their switch, so the cloud pills line up.
            controls = controls.child(div().w(px(34.)));
        } else {
            let device = row.device.clone();
            controls = controls.child(
                Toggle::new(SharedString::from(format!("device-include-{id}")), row.shared, theme).on_change(edit(
                    weak,
                    move |this: &mut Shell, on: bool, cx| {
                        set_included(&mut this.state.settings.sync, &device, on);
                        this.save(cx);
                    },
                )),
            );
        }

        let icon_colour: Hsla = if lit { theme.fg.to_hsla() } else { theme.muted.to_hsla() };
        div()
            .flex()
            .items_center()
            .gap(px(14.))
            .px(px(18.))
            .py(px(12.))
            .bg(theme.tile.to_hsla())
            .child(
                div()
                    .flex_none()
                    .size(px(34.))
                    .rounded(px(9.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .bg(theme.line.to_hsla())
                    .child(stroke_icon(glyph, 17., 1.4, icon_colour)),
            )
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .flex()
                    .flex_col()
                    .gap(px(2.))
                    .child(div().text_color(if lit { theme.fg.to_hsla() } else { theme.muted.to_hsla() }).child(row.device.name.clone()))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(6.))
                            .text_size(px(12.))
                            .text_color(theme.muted.to_hsla())
                            .child(div().flex_none().w(px(7.)).h(px(7.)).rounded_full().bg(status.colour(theme).to_hsla()))
                            .child(detail),
                    ),
            )
            .child(controls)
            .into_any_element()
    }

    /// What the cloud holds: its status, each project copy with a button that opens it here, and the secrets switch.
    fn cloud_section(&mut self, weak: &WeakEntity<Shell>, _cx: &mut Context<Self>) -> AnyElement {
        let theme = self.theme.clone();
        self.ask_cloud_list_once();
        let sync = self.state.settings.sync.clone();
        let Some(cloud) = sync.cloud.clone() else { return div().into_any_element() };
        let me = match &self.settings.tailnet {
            Some(Ok(net)) => Some(net.me.id.clone()),
            _ => None,
        };
        let is_me = me.as_deref() == Some(cloud.as_str());
        // A cloud that is neither this device nor shared with is never connected to, so nothing is copied.
        let left_out = me.is_some() && !is_me && !sync.devices.iter().any(|d| d.id == cloud);
        let name = self.chat_device_name(&cloud);
        let (subtitle, ok) = if is_me {
            ("This device keeps the copies".to_string(), true)
        } else if left_out {
            ("Not shared with — turn its switch on above to copy to it".to_string(), false)
        } else {
            let (running, last, next) = self.backup_status();
            let next = next.map(|at| at.saturating_duration_since(std::time::Instant::now()));
            let connected = self.device_connected(&cloud);
            (backup_line(connected, running, last, next, crate::store::now()), connected)
        };
        let back_up = Button::new("cloud-backup", "Back up now", &theme).style(ButtonStyle::Filled).on_click(act(weak, |this, cx| {
            this.refresh_cloud_list();
            this.backup(cx);
            cx.notify();
        }));
        let status = card(CLOUD, true, &name, &subtitle, Some(ok), back_up.into_any_element(), &theme);

        let mut list = section_card(Vec::new(), &theme);
        let projects = self.remote.cloud_projects.clone().unwrap_or_default();
        if projects.is_empty() {
            list = list.child(note(
                if self.remote.cloud_projects.is_none() { "Asking the cloud what it holds…" } else { "No copies yet. Projects are copied after the next reply." },
                &theme,
            ));
        }
        for p in projects {
            let from = self.chat_device_name(&p.device);
            let up_to_date = me.as_deref() == Some(p.device.as_str()) && self.copy_confirmed(&p.project, &p.commit);
            let line = copy_line(&from, p.updated, up_to_date, crate::store::now());
            list = list.child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(14.))
                    .px(px(18.))
                    .py(px(12.))
                    .bg(theme.tile.to_hsla())
                    .child(stroke_icon(FOLDER, 16., 1.4, theme.muted.to_hsla()))
                    .child(div().flex_1().min_w(px(0.)).overflow_hidden().text_ellipsis().whitespace_nowrap().child(p.name.clone()))
                    .child(div().flex_none().text_size(px(12.)).text_color(theme.muted.to_hsla()).child(line)),
            );
        }

        let secrets: SettingRow = toggle_row(
            "sync-copy-secrets",
            "Copy secret files",
            ".env files, keys and certificates",
            sync.copy_secrets,
            weak,
            &theme,
            |this, on, cx| {
                this.state.settings.sync.copy_secrets = on;
                this.save(cx);
            },
            None,
        );
        div()
            .flex()
            .flex_col()
            .child(section_head("Cloud copy", None, Vec::new(), &theme))
            .child(status)
            .child(div().pt(px(12.)).child(list.child(secrets)))
            .into_any_element()
    }

    /// Finds this device and its tailnet peers off the UI thread. `tailscale status` blocks.
    /// A search already running is not started twice.
    fn detect_tailnet(&mut self, cx: &mut Context<Self>) {
        if self.settings.tailnet_loading {
            return;
        }
        self.settings.tailnet_loading = true;
        cx.spawn(async move |this, cx| {
            let found = cx
                .background_executor()
                .spawn(async move { crate::remote::tailscale::discover() })
                .await;
            let _ = this.update(cx, |this, cx| {
                // Included devices keep the addresses Tailscale reports now, IPv6 included.
                let refreshed = found.as_ref().is_ok_and(|net| {
                    let seen = std::iter::once(&net.me).chain(net.peers.iter().map(|p| &p.device));
                    this.state.settings.sync.refresh_devices(seen)
                });
                this.settings.tailnet = Some(found);
                this.settings.tailnet_loading = false;
                if refreshed {
                    this.save(cx);
                }
                cx.notify();
            });
        })
        .detach();
    }
}

/// A card with an icon tile, a title, a status line (with a dot when `ok` is set) and a control on the right.
fn card(glyph: &'static str, lit: bool, title: &str, subtitle: &str, ok: Option<bool>, control: AnyElement, theme: &Theme) -> AnyElement {
    let mut line = div().flex().items_center().gap(px(6.)).text_size(px(12.)).text_color(theme.muted.to_hsla());
    if let Some(ok) = ok {
        let dot = if ok { LIVE } else { theme.fg.alpha(0.4) };
        line = line.child(div().flex_none().w(px(7.)).h(px(7.)).rounded_full().bg(dot.to_hsla()));
    }
    let (radius, border) = crate::ui::widgets::section::card_style();
    let icon = if glyph == SHARE { if lit { theme.acc2 } else { theme.muted } } else { theme.fg };
    div()
        .flex()
        .items_center()
        .gap(px(14.))
        .px(px(18.))
        .py(px(16.))
        .rounded(px(radius))
        .bg(theme.tile.to_hsla())
        .when(border, |d| d.border_1().border_color(theme.line.to_hsla()))
        .child(
            div()
                .flex_none()
                .size(px(40.))
                .rounded(px(10.))
                .flex()
                .items_center()
                .justify_center()
                .bg(theme.line.to_hsla())
                .child(stroke_icon(glyph, 19., 1.4, icon.to_hsla())),
        )
        .child(
            div()
                .flex_1()
                .min_w(px(0.))
                .flex()
                .flex_col()
                .gap(px(3.))
                .child(div().child(title.to_string()))
                .child(line.child(subtitle.to_string())),
        )
        .child(div().flex_none().child(control))
        .into_any_element()
}

/// The cloud card's status line: when the last full backup was and when the next timed one is.
/// Times are whole minutes; `now` and `last` are seconds since the epoch.
fn backup_line(connected: bool, running: bool, last: Option<u64>, next: Option<std::time::Duration>, now: u64) -> String {
    let ago = |at: u64| ago(at, now);
    let next = next.map(|d| match d.as_secs().div_ceil(60) {
        0 | 1 => "in under a minute".to_string(),
        m => format!("in {m} min"),
    });
    match (connected, running, last, next) {
        (false, _, Some(at), _) => format!("Not connected · last backup {}", ago(at)),
        (false, _, None, _) => "Not connected · copies start when it's back".to_string(),
        (true, true, _, _) => "Backing up…".to_string(),
        (true, false, Some(at), Some(next)) => format!("Backed up {} · next {next}", ago(at)),
        (true, false, Some(at), None) => format!("Backed up {}", ago(at)),
        (true, false, None, Some(next)) => format!("Connected · first backup {next}"),
        (true, false, None, None) => "Connected · first backup soon".to_string(),
    }
}

/// How long before `now` `at` was, in words: "just now", "3 min ago", "2 h ago", "4 d ago".
fn ago(at: u64, now: u64) -> String {
    match now.saturating_sub(at) / 60 {
        0 => "just now".to_string(),
        m if m < 60 => format!("{m} min ago"),
        m if m < 60 * 24 => format!("{} h ago", m / 60),
        m => format!("{} d ago", m / (60 * 24)),
    }
}

/// A copy's line in the cloud list. The time is when the cloud last got a change of it: an unchanged
/// project isn't sent again. This device's own copy says up to date once the cloud has confirmed it.
fn copy_line(from: &str, updated: u64, up_to_date: bool, now: u64) -> String {
    if up_to_date {
        format!("From {from} · up to date")
    } else {
        format!("From {from} · changed {}", ago(updated, now))
    }
}

/// A muted line inside a list box.
fn note(text: &str, theme: &Theme) -> AnyElement {
    div().px(px(16.)).py(px(14.)).text_size(px(12.)).text_color(theme.muted.to_hsla()).child(text.to_string()).into_any_element()
}

/// The device list in its order: this device, the shared ones, the other online ones, then the rest;
/// each group by name. Shared devices Tailscale no longer lists stay, so they can be turned off.
fn device_rows(sync: &SyncSettings, net: &crate::remote::tailscale::Tailnet, connected: impl Fn(&str) -> bool) -> Vec<Row> {
    let shared = |id: &str| sync.devices.iter().any(|d| d.id == id);
    let mut rows = vec![Row { device: net.me.clone(), os: String::new(), status: Status::ThisDevice, shared: true }];
    for peer in &net.peers {
        let id = peer.device.id.as_str();
        let status = if connected(id) {
            Status::Connected
        } else if peer.online {
            Status::Online
        } else {
            Status::Offline
        };
        rows.push(Row { device: peer.device.clone(), os: peer.os.clone(), status, shared: shared(id) });
    }
    let peer_ids: Vec<String> = net.peers.iter().map(|p| p.device.id.clone()).collect();
    for device in gone_devices(sync, &net.me.id, &peer_ids) {
        rows.push(Row { device, os: String::new(), status: Status::Gone, shared: true });
    }
    let rank = |r: &Row| match (r.status, r.shared) {
        (Status::ThisDevice, _) => 0,
        (_, true) => 1,
        (Status::Connected | Status::Online, false) => 2,
        _ => 3,
    };
    rows.sort_by(|a, b| rank(a).cmp(&rank(b)).then_with(|| a.device.name.to_lowercase().cmp(&b.device.name.to_lowercase())));
    rows
}

/// An offline device that isn't shared with: listed only when asked, to keep the list short.
fn hidden_unless_asked(row: &Row) -> bool {
    !row.shared && row.status == Status::Offline
}

/// Tailscale's OS name as people write it; `None` when there is nothing to show.
fn os_name(os: &str) -> Option<String> {
    let name = match os.to_lowercase().as_str() {
        "" => return None,
        "linux" => "Linux".to_string(),
        "windows" => "Windows".to_string(),
        "macos" => "macOS".to_string(),
        "ios" => "iOS".to_string(),
        "android" => "Android".to_string(),
        _ => os.to_string(),
    };
    Some(name)
}

/// Ticks or unticks `device`. The cloud must be an included device, so unticking it clears the cloud.
fn set_included(sync: &mut SyncSettings, device: &Device, on: bool) {
    sync.devices.retain(|d| d.id != device.id);
    if on {
        sync.devices.push(device.clone());
    } else if sync.is_cloud(&device.id) {
        sync.cloud = None;
    }
}

/// The included devices that are neither this device (`me`) nor among the peers Tailscale lists now.
fn gone_devices(sync: &SyncSettings, me: &str, peers: &[String]) -> Vec<Device> {
    sync.devices.iter().filter(|d| d.id != me && !peers.contains(&d.id)).cloned().collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::remote::tailscale::{Peer, Tailnet};

    fn device(id: &str) -> Device {
        Device { id: id.into(), name: id.into(), dns_name: String::new(), addr: "100.64.0.9".into(), addr6: String::new() }
    }

    fn peer(id: &str, online: bool) -> Peer {
        Peer { device: device(id), online, os: "linux".into() }
    }

    #[test]
    fn a_device_that_left_the_tailnet_can_still_be_unticked() {
        let mut sync = SyncSettings { enabled: true, devices: vec![device("pc"), device("vps")], cloud: Some("vps".into()), ..Default::default() };
        // Only the pc is still on the tailnet.
        let gone = gone_devices(&sync, "me", &["pc".to_string()]);
        assert_eq!(gone, [device("vps")]);
        set_included(&mut sync, &gone[0], false);
        assert_eq!(sync.devices, [device("pc")]);
        assert_eq!(sync.cloud, None, "the cloud went with it");
        assert!(gone_devices(&sync, "me", &["pc".to_string()]).is_empty());
    }

    #[test]
    fn devices_list_this_one_then_shared_then_online_then_the_rest() {
        let sync = SyncSettings { enabled: true, devices: vec![device("zed"), device("left")], ..Default::default() };
        let net = Tailnet {
            me: device("me"),
            peers: vec![peer("phone", false), peer("box", true), peer("zed", false), peer("alpha", true)],
        };
        let rows = device_rows(&sync, &net, |id| id == "box");
        let order: Vec<(&str, Status)> = rows.iter().map(|r| (r.device.id.as_str(), r.status)).collect();
        assert_eq!(
            order,
            [
                ("me", Status::ThisDevice),
                ("left", Status::Gone),
                ("zed", Status::Offline),
                ("alpha", Status::Online),
                ("box", Status::Connected),
                ("phone", Status::Offline),
            ]
        );
        let hidden: Vec<&str> = rows.iter().filter(|r| hidden_unless_asked(r)).map(|r| r.device.id.as_str()).collect();
        assert_eq!(hidden, ["phone"], "a shared device stays listed even while offline");
    }

    #[test]
    fn the_cloud_card_says_when_it_last_backed_up_and_when_next() {
        use std::time::Duration;
        let now = 1_000_000;
        let min = |m: u64| Duration::from_secs(m * 60);
        assert_eq!(backup_line(true, false, Some(now - 120), Some(min(3)), now), "Backed up 2 min ago · next in 3 min");
        assert_eq!(backup_line(true, false, Some(now - 10), Some(Duration::from_secs(40)), now), "Backed up just now · next in under a minute");
        assert_eq!(backup_line(true, true, Some(now - 120), Some(min(3)), now), "Backing up…");
        assert_eq!(backup_line(true, false, None, Some(min(5)), now), "Connected · first backup in 5 min");
        assert_eq!(backup_line(false, false, Some(now - 2 * 3600), None, now), "Not connected · last backup 2 h ago");
        assert_eq!(backup_line(false, false, None, None, now), "Not connected · copies start when it's back");
    }

    #[test]
    fn a_copy_says_when_it_last_changed_or_that_it_is_up_to_date() {
        let now = 1_000_000;
        assert_eq!(copy_line("AshPC", now - 180, false, now), "From AshPC · changed 3 min ago");
        assert_eq!(copy_line("minipc", now - 180, true, now), "From minipc · up to date");
        assert_eq!(copy_line("vps", now - 3 * 86400, false, now), "From vps · changed 3 d ago");
    }

    #[test]
    fn the_glyphs_parse() {
        for d in [SHARE, DESKTOP, PHONE, CLOUD, FOLDER] {
            assert!(crate::ui::composer::icons::parse_path(d).is_ok(), "{d}");
        }
    }
}

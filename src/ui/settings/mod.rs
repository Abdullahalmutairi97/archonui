//! The settings modal: 1320×880, nav + 9 pages. Spec: docs/design/settings.md.

mod appearance;
mod colour;
mod controls;
mod devices;
mod extensions;
mod general;
mod harnesses;
mod keyboard;
mod models;
mod skills;
mod tools;

pub(crate) use controls::key_label;
pub(crate) use models::long_context_model;

use super::{
    shell::Shell,
    widgets::{
        slider::{Slider, SliderRange},
        text_input::{TextInput, TextInputConfig, TextInputEvent},
    },
};
use crate::{
    app::settings::{Action, AppIcon},
    engine::{self, mcp::McpStatus, plugins::Extension, skills::Skill},
};
use gpui::{
    div, hsla, img, point, prelude::*, AnyElement, App, BoxShadow, Context, Entity, Image, KeyDownEvent, MouseButton,
    ObjectFit, Subscription, Window,
};
use crate::ui::scale::px;
use std::{collections::HashMap, sync::Arc};

/// Harness ids in the order the pages list them. Each one is looked up with `engine::harness`.
const HARNESS_IDS: [&str; 5] = ["claude", "codex", "gemini", "opencode", "pi"];

/// The settings dialog's ideal size, in design px. It shrinks to fit the window.
const DIALOG_SIZE: (f32, f32) = (1320., 880.);
/// The dialog's smallest size, in design px. The window wins over this when it is smaller.
const DIALOG_MIN: (f32, f32) = (560., 420.);
/// Space kept between the dialog and the window edges, in design px.
const DIALOG_MARGIN: f32 = 24.;
/// The sidebar's width range, in design px. It is a share of the dialog width, so it narrows with the window.
const NAV_WIDTH: (f32, f32) = (168., 236.);

/// The page body's side padding, in design px: roomy beside the sidebar, tighter in a compact dialog.
const BODY_PAD: f32 = 36.;
const COMPACT_PAD: f32 = 20.;
/// Below this dialog width, in design px, the sidebar becomes a strip of pages across the top, so the
/// page gets the whole width.
const COMPACT_BELOW: f32 = 720.;

/// How many columns of at least `min` wide, `gap` apart, fit in `width`: from 1 up to `max`.
pub(super) fn columns(width: f32, min: f32, gap: f32, max: u16) -> u16 {
    let fit = ((width + gap) / (min + gap)).floor();
    if fit.is_finite() { (fit as u16).clamp(1, max.max(1)) } else { 1 }
}

/// `ideal` clamped to `avail` and floored at `min`, but never wider than `avail`.
fn fit(ideal: f32, min: f32, avail: f32) -> f32 {
    ideal.min(avail).max(min.min(avail))
}

/// Keys that only change modifiers. A rebinding waits for a key that isn't one of these.
const MODIFIER_KEYS: [&str; 5] = ["shift", "control", "alt", "platform", "function"];

/// The binding to store for a captured keystroke, or why it can't be one. A bare printable key would fire
/// while typing, and `super`/`fn` aren't understood by the key matcher, so those never get stored.
fn capture_spec(ks: &gpui::Keystroke) -> Result<String, &'static str> {
    let m = ks.modifiers;
    if m.platform || m.function {
        return Err("Shortcuts can't use the Super or Fn key");
    }
    if ks.key == "-" {
        return Err("That key can't be bound. Try another");
    }
    let function_key = ks.key.strip_prefix('f').is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()));
    if !(m.control || m.alt || function_key) {
        return Err("Include Ctrl or Alt in the shortcut");
    }
    Ok(ks.unparse())
}

/// The settings pages, in nav order.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Page {
    #[default]
    General,
    Appearance,
    Keyboard,
    Harnesses,
    Models,
    Tools,
    Skills,
    Extensions,
    Devices,
}

/// One row of the nav: a section heading or a page.
enum NavEntry {
    Head(&'static str),
    Page(Page),
}

/// The pages in the nav. Models are part of Agents, and Skills and Extensions part of Tools; their
/// own `Page` values still open those pages, scrolled from the top.
const NAV: [NavEntry; 6] = [
    NavEntry::Page(Page::General),
    NavEntry::Page(Page::Appearance),
    NavEntry::Page(Page::Harnesses),
    NavEntry::Page(Page::Tools),
    NavEntry::Page(Page::Keyboard),
    NavEntry::Page(Page::Devices),
];

impl Page {
    /// The page whose label is `name`, in any case.
    pub fn from_label(name: &str) -> Option<Self> {
        [
            Page::General,
            Page::Appearance,
            Page::Keyboard,
            Page::Harnesses,
            Page::Models,
            Page::Tools,
            Page::Skills,
            Page::Extensions,
            Page::Devices,
        ]
        .into_iter()
            .find(|p| p.label().eq_ignore_ascii_case(name))
    }

    fn label(self) -> &'static str {
        match self {
            Page::General => "General",
            Page::Appearance => "Appearance",
            Page::Keyboard => "Keyboard",
            Page::Harnesses => "Agents",
            Page::Models => "Models",
            Page::Tools => "Tools",
            Page::Skills => "Skills",
            Page::Extensions => "Extensions",
            Page::Devices => "Devices",
        }
    }

    fn desc(self) -> &'static str {
        match self {
            Page::General => "Startup, agents and notifications",
            Page::Appearance => "Themes, wallpaper and layout",
            Page::Keyboard => "Shortcuts",
            Page::Harnesses => "Agent apps and their models",
            Page::Models => "Agent apps and their models",
            Page::Tools => "Tools, skills and plugins",
            Page::Skills => "Tools, skills and plugins",
            Page::Extensions => "Tools, skills and plugins",
            Page::Devices => "Share chats over Tailscale",
        }
    }
}

/// The layout and effect sliders. Each one writes one setting in `Shell::set_slide`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) enum Slide {
    Gaps,
    Radius,
    Blur,
    Dim,
    Opacity,
    Text,
    Scale,
    MaxSteps,
    Terminal,
    FilterPixel,
}

/// Which font the Fonts block's chips set on Appearance.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum FontSlot {
    #[default]
    Titles,
    Text,
}

impl Slide {
    /// Sliders whose value re-lays-out the whole window apply only when the drag ends, not on every tick.
    pub(super) fn commits_on_release(self) -> bool {
        matches!(self, Slide::Scale)
    }
}

/// A text field and the subscription that keeps the shell in step with it.
type Field = (Entity<TextInput>, Subscription);

/// Settings modal state, not persisted.
#[derive(Default)]
pub struct SettingsUi {
    pub(super) open: bool,
    pub(super) page: Page,
    /// The page body's scroll, reset when the page changes.
    pub(super) body_scroll: gpui::ScrollHandle,
    /// The app name field on Appearance, made on first render.
    pub(super) name: Option<Field>,
    /// The extension search field, made on first render.
    pub(super) search: Option<Field>,
    pub(super) sliders: HashMap<Slide, Entity<Slider>>,
    /// Logos and wallpapers, decoded once, by a key such as `logo:<name>`, `wall:<name>` or `file:<path>`.
    pub(super) images: HashMap<String, Arc<Image>>,
    /// The action waiting for a key on the Keyboard page.
    pub(super) capture: Option<Action>,
    /// The last rebound action and the binding it had before, so Swap can hand that binding to the
    /// action that now shares its keys.
    pub(super) displaced: Option<(Action, String)>,
    /// The open colour picker, if any.
    pub(super) picker: Option<colour::Picker>,
    /// Whether the Appearance page's custom colour swatches are shown.
    pub(super) colours_open: bool,
    /// The font slot whose list is open on Appearance, if any.
    pub(super) font_open: Option<FontSlot>,
    /// Discovered skills; `None` until the first search finishes.
    pub(super) skills: Option<Vec<Skill>>,
    /// The latest skills search. An older one finishing late (say, one started before a sync) is dropped
    /// rather than drawn over the newer result.
    pub(super) skills_search: u64,
    /// Installed and available extensions; `None` until the first search finishes.
    pub(super) plugins: Option<(Vec<Extension>, Vec<Extension>)>,
    /// MCP server checks by server id. `None` while a check runs.
    pub(super) mcp: HashMap<String, Option<McpStatus>>,
    /// The harness whose test connection is running.
    pub(super) testing: Option<&'static str>,
    /// Tailnet devices from Tailscale on the Devices page; `None` until the first search finishes.
    pub(super) tailnet: Option<Result<crate::remote::tailscale::Tailnet, String>>,
    /// Whether a Tailscale search is running.
    pub(super) tailnet_loading: bool,
    /// Whether the Devices page lists offline devices that aren't shared with.
    pub(super) show_offline: bool,
    /// The page body's width inside its padding, in design px, so pages can drop columns as it narrows.
    pub(super) body_w: f32,
}

impl Shell {
    pub(super) fn settings_open(&mut self, cx: &mut Context<Self>) {
        self.settings.open = true;
        self.settings.page = Page::General;
        self.settings.capture = None;
        self.settings.picker = None;
        // The Devices page asks the cloud what it holds again, once.
        self.remote.cloud_asked = false;
        self.load_catalogues(cx);
        cx.notify();
    }

    pub(super) fn settings_close(&mut self, cx: &mut Context<Self>) {
        self.settings.open = false;
        self.settings.capture = None;
        self.settings.picker = None;
        cx.notify();
    }

    /// Searches for skills and extensions off the UI thread. Both are blocking reads of the disk and the CLIs.
    /// They run apart, so skills (a quick disk scan) don't wait on the extension catalogues the CLIs list.
    fn load_catalogues(&mut self, cx: &mut Context<Self>) {
        self.reload_skills(cx);
        cx.spawn(async move |this, cx| {
            let (installed, available) = cx
                .background_executor()
                .spawn(async move { (engine::plugins::installed(), engine::plugins::available()) })
                .await;
            let _ = this.update(cx, |this, cx| {
                this.settings.plugins = Some((installed, available));
                cx.notify();
            });
        })
        .detach();
    }

    /// Checks the enabled MCP servers off the UI thread, or only `only`. Each check is one server's round trip.
    pub(super) fn check_mcp(&mut self, only: Option<String>, cx: &mut Context<Self>) {
        let servers: Vec<_> = self
            .state
            .settings
            .mcp_servers
            .iter()
            .filter(|s| s.enabled && only.as_ref().map_or(true, |id| &s.id == id))
            .cloned()
            .collect();
        for server in servers {
            let id = server.id.clone();
            // A check already running for this server is not started twice (nav clicks, rapid toggling).
            if matches!(self.settings.mcp.get(&id), Some(None)) {
                continue;
            }
            self.settings.mcp.insert(id.clone(), None);
            cx.spawn(async move |this, cx| {
                let status = cx.background_executor().spawn(async move { engine::mcp::check(&server) }).await;
                let _ = this.update(cx, |this, cx| {
                    this.settings.mcp.insert(id, Some(status));
                    cx.notify();
                });
            })
            .detach();
        }
        cx.notify();
    }

    /// Writes one layout or effect slider to its setting. Gaps change tile sizes, so they go through `widths_changed`.
    pub(super) fn set_slide(&mut self, which: Slide, value: f32, cx: &mut Context<Self>) {
        let s = &mut self.state.settings;
        match which {
            Slide::Gaps => {
                s.gaps = value;
                self.widths_changed(true, cx);
                return;
            }
            Slide::Radius => s.radius = value,
            Slide::Blur => s.panel_blur = value,
            Slide::Dim => s.wallpaper_dim = value,
            Slide::Opacity => s.window_opacity = value,
            Slide::Text => s.text_size = value,
            Slide::Scale => {
                s.ui_scale = value / 100.;
                crate::ui::scale::set_zoom(s.ui_scale);
                self.widths_changed(true, cx);
            }
            Slide::MaxSteps => s.max_steps = value.round() as u32,
            Slide::Terminal => s.terminal_font_size = value,
            Slide::FilterPixel => s.filter_pixel = value,
        }
        self.save(cx);
    }

    /// The slider for `which`, reading `value` from settings and writing back through `set_slide`.
    pub(super) fn slide(
        &mut self,
        which: Slide,
        value: f32,
        range: SliderRange,
        unit: &'static str,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = self.theme.clone();
        let weak = cx.entity().downgrade();
        controls::slider_control(which, value, range, unit, &weak, &theme, &mut self.settings.sliders, cx)
    }

    /// Creates the text fields on first use. Both need a window, so they can't be made in `settings_open`.
    fn ensure_fields(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.settings.name.is_none() {
            let input = cx.new(|cx| TextInput::new(name_config(), window, cx));
            input.update(cx, |input, cx| input.set_text(&self.state.settings.app_name, cx));
            let subscription = cx.subscribe(&input, |this, input, event: &TextInputEvent, cx| {
                if let TextInputEvent::Changed = event {
                    this.state.settings.app_name = input.read(cx).text();
                    this.refresh_theme(cx);
                }
            });
            self.settings.name = Some((input, subscription));
        }
        if self.settings.search.is_none() {
            let input = cx.new(|cx| TextInput::new(search_config(), window, cx));
            let subscription = cx.subscribe(&input, |_, _, event: &TextInputEvent, cx| {
                if let TextInputEvent::Changed = event {
                    cx.notify();
                }
            });
            self.settings.search = Some((input, subscription));
        }
    }

    /// The modal, when open (drawn by `render_overlays`).
    pub(super) fn render_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        if !self.settings.open {
            return None;
        }
        self.ensure_fields(window, cx);
        let theme = self.theme.clone();
        // The window is in real px, so the design-unit sizes are converted before they are compared.
        let viewport = window.viewport_size();
        let margin = crate::ui::scale::real(DIALOG_MARGIN);
        let avail_w = (f32::from(viewport.width) - 2. * margin).max(0.);
        let avail_h = (f32::from(viewport.height) - 2. * margin).max(0.);
        let dialog_w = fit(crate::ui::scale::real(DIALOG_SIZE.0), crate::ui::scale::real(DIALOG_MIN.0), avail_w);
        let dialog_h = fit(crate::ui::scale::real(DIALOG_SIZE.1), crate::ui::scale::real(DIALOG_MIN.1), avail_h);
        let dialog_design_w = dialog_w / crate::ui::scale::zoom();
        let compact = dialog_design_w < COMPACT_BELOW;
        let nav_width = if compact { 0. } else { (dialog_design_w * 0.25).clamp(NAV_WIDTH.0, NAV_WIDTH.1).min(dialog_design_w * 0.5) };
        let pad = if compact { COMPACT_PAD } else { BODY_PAD };
        self.settings.body_w = (dialog_design_w - nav_width - 2. * pad).max(0.);
        let blocks = match self.settings.page {
            Page::General => self.general_page(cx),
            Page::Appearance => self.appearance_page(cx),
            Page::Keyboard => self.keyboard_page(cx),
            Page::Harnesses | Page::Models => {
                let mut blocks = self.harnesses_page(cx);
                blocks.extend(self.models_page(cx));
                blocks
            }
            Page::Tools | Page::Skills | Page::Extensions => {
                let mut blocks = self.tools_page(cx);
                blocks.push(controls::heading("SKILLS", None, &theme));
                blocks.extend(self.skills_page(cx));
                blocks.push(controls::heading("EXTENSIONS", None, &theme));
                blocks.extend(self.extensions_page(cx));
                blocks
            }
            Page::Devices => self.devices_page(cx),
        };
        let nav = if compact { self.settings_strip(cx) } else { self.settings_nav(nav_width, cx) };
        // One scroll state per page, so a page opens at its top.
        let body = div()
            .id(self.settings.page.label())
            .flex_1()
            .min_h(px(0.))
            .overflow_y_scroll()
            .track_scroll(&self.settings.body_scroll)
            .flex()
            .flex_col()
            .gap(px(28.))
            .px(px(pad))
            .pt(px(8.))
            .pb(px(48.))
            .children(blocks);
        let content = div()
            .flex_1()
            .min_w(px(0.))
            .flex()
            .flex_col()
            .child(self.settings_header(pad, cx))
            .child(body);
        let dialog = div()
            .flex()
            .flex_none()
            .w(gpui::px(dialog_w))
            .h(gpui::px(dialog_h))
            .rounded(px(14.))
            .bg(theme.pop.to_hsla())
            .border_1()
            .border_color(theme.line2.to_hsla())
            .shadow(vec![BoxShadow {
                color: hsla(0., 0., 0., 0.65),
                offset: point(px(0.), px(40.)),
                blur_radius: px(120.),
                spread_radius: px(0.),
            }])
            .overflow_hidden()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .when(compact, |d| d.flex_col())
            .child(nav)
            .child(content);
        Some(
            div()
                .absolute()
                .top_0()
                .left_0()
                .size_full()
                .occlude()
                .flex()
                .items_center()
                .justify_center()
                .bg(hsla(0., 0., 0., 0.45))
                .on_mouse_down(MouseButton::Left, cx.listener(|this, _, _, cx| this.settings_close(cx)))
                .child(dialog)
                .into_any_element(),
        )
    }

    /// The nav column: logo and title, then the pages with their counts, and the version footer.
    fn settings_nav(&mut self, width: f32, cx: &mut Context<Self>) -> AnyElement {
        let theme = self.theme.clone();
        let logo = self.icon_logo();
        let active = self.settings.page;
        let counts = self.nav_counts();
        let mut nav = div()
            .id("settings-nav-column")
            .w(px(width))
            .flex_none()
            .min_h(px(0.))
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .gap(px(2.))
            .pt(px(22.))
            .pb(px(18.))
            .px(px(12.))
            .border_r_1()
            .border_color(theme.line.to_hsla())
            .bg(theme.line.to_hsla())
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .px(px(10.))
                    .pb(px(10.))
                    .when_some(logo, |row, logo| {
                        row.child(img(logo).w(px(22.)).h(px(22.)).flex_none().object_fit(ObjectFit::Contain))
                    })
                    .child(
                        div()
                            .font_family(theme.display_font.clone())
                            .text_size(px(26.))
                            .line_height(px(26.))
                            .child("Settings"),
                    ),
            );
        for entry in NAV.iter() {
            nav = match entry {
                NavEntry::Head(title) => nav.child(
                    div()
                        .pt(px(16.))
                        .pb(px(6.))
                        .px(px(12.))
                        .text_size(px(10.5))
                        .text_color(theme.muted.to_hsla())
                        .child(*title),
                ),
                NavEntry::Page(page) => {
                    let page = *page;
                    let is_active = page == active;
                    let count = counts.get(&page).copied();
                    nav.child(
                        div()
                            .id(("settings-nav", page as usize))
                            .flex()
                            .items_center()
                            .justify_between()
                            .px(px(12.))
                            .py(px(9.))
                            .rounded(px(7.))
                            .text_size(px(13.5))
                            .cursor_pointer()
                            .when(is_active, |d| d.bg(theme.line2.to_hsla()).text_color(theme.fg.to_hsla()))
                            .when(!is_active, |d| {
                                d.text_color(theme.muted.to_hsla()).hover(|s| s.text_color(theme.fg.to_hsla()))
                            })
                            .on_click(cx.listener(move |this, _, _, cx| this.settings_go(page, cx)))
                            .child(page.label())
                            .when_some(count, |row, n| {
                                row.child(div().text_size(px(11.)).text_color(theme.muted.to_hsla()).child(n.to_string()))
                            }),
                    )
                }
            };
        }
        nav.child(div().flex_1()).child(
            div()
                .px(px(12.))
                .text_size(px(11.))
                .text_color(theme.muted.to_hsla())
                .child(format!("{} {}", self.state.settings.app_name, env!("CARGO_PKG_VERSION"))),
        )
        .into_any_element()
    }

    /// Opens `page` at its top.
    fn settings_go(&mut self, page: Page, cx: &mut Context<Self>) {
        self.settings.page = page;
        self.settings.body_scroll.set_offset(gpui::point(px(0.), px(0.)));
        if page == Page::Tools {
            self.check_mcp(None, cx);
        }
        cx.notify();
    }

    /// The compact dialog's nav: every page in one row across the top, scrolling sideways when they
    /// don't fit.
    fn settings_strip(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let theme = self.theme.clone();
        let active = self.settings.page;
        let counts = self.nav_counts();
        let mut strip = div()
            .id("settings-nav-strip")
            .flex_none()
            .flex()
            .items_center()
            .gap(px(4.))
            .px(px(12.))
            .py(px(10.))
            .overflow_x_scroll()
            .border_b_1()
            .border_color(theme.line.to_hsla())
            .bg(theme.line.to_hsla());
        for entry in NAV.iter() {
            let NavEntry::Page(page) = entry else { continue };
            let page = *page;
            let is_active = page == active;
            strip = strip.child(
                div()
                    .id(("settings-strip", page as usize))
                    .flex_none()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .px(px(11.))
                    .py(px(7.))
                    .rounded(px(7.))
                    .whitespace_nowrap()
                    .text_size(px(13.))
                    .cursor_pointer()
                    .when(is_active, |d| d.bg(theme.line2.to_hsla()).text_color(theme.fg.to_hsla()))
                    .when(!is_active, |d| d.text_color(theme.muted.to_hsla()).hover(|s| s.text_color(theme.fg.to_hsla())))
                    .on_click(cx.listener(move |this, _, _, cx| this.settings_go(page, cx)))
                    .child(page.label())
                    .when_some(counts.get(&page).copied(), |row, n| {
                        row.child(div().text_size(px(11.)).text_color(theme.muted.to_hsla()).child(n.to_string()))
                    }),
            );
        }
        strip.into_any_element()
    }

    /// The page title and subtitle, with the close button on the right. `pad` matches the body's sides.
    fn settings_header(&self, pad: f32, cx: &mut Context<Self>) -> AnyElement {
        let theme = &self.theme;
        let page = self.settings.page;
        div()
            .flex()
            .items_start()
            .gap(px(16.))
            .pt(px(26.))
            .pr(px(22.))
            .pl(px(pad))
            .pb(px(20.))
            .border_b_1()
            .border_color(theme.line.to_hsla())
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .flex()
                    .flex_col()
                    .gap(px(8.))
                    .child(
                        div()
                            .font_family(theme.display_font.clone())
                            .text_size(px(36.))
                            .line_height(px(36.))
                            .child(page.label()),
                    )
                    .child(div().text_size(px(13.)).text_color(theme.muted.to_hsla()).child(page.desc())),
            )
            .child(
                div()
                    .id("settings-close")
                    .flex_none()
                    .w(px(32.))
                    .h(px(32.))
                    .rounded(px(7.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_color(theme.muted.to_hsla())
                    .cursor_pointer()
                    .hover(|s| s.bg(theme.line2.to_hsla()).text_color(theme.fg.to_hsla()))
                    .on_click(cx.listener(|this, _, _, cx| this.settings_close(cx)))
                    .child(div().text_size(px(17.)).child("✕")),
            )
            .into_any_element()
    }

    /// The count beside each page: harnesses ready, models on, MCP servers on, skills on, extensions installed.
    fn nav_counts(&self) -> HashMap<Page, usize> {
        let s = &self.state.settings;
        let mut counts = HashMap::new();
        counts.insert(Page::Harnesses, self.agents.iter().filter(|a| a.enabled).count());
        let models: usize = self
            .agents
            .iter()
            .filter(|a| a.enabled)
            .map(|a| {
                let id = a.harness.id();
                self.caps.get(id).and_then(Option::as_ref).map_or(0, |caps| {
                    caps.models.iter().filter(|m| s.model_enabled(id, &m.id)).count()
                })
            })
            .sum();
        counts.insert(Page::Models, models);
        counts.insert(Page::Tools, s.mcp_servers.iter().filter(|m| m.enabled).count());
        if s.sync.enabled {
            counts.insert(Page::Devices, s.sync.devices.len());
        }
        // Skills and extensions show no count until their discovery has finished, rather than a misleading 0.
        if let Some(skills) = &self.settings.skills {
            counts.insert(Page::Skills, skills.iter().filter(|k| s.skills_enabled.get(&k.id).copied().unwrap_or(true)).count());
        }
        if let Some((installed, _)) = &self.settings.plugins {
            counts.insert(Page::Extensions, installed.len());
        }
        counts
    }

    /// The app's logo: the chosen icon, or the theme's logo when the icon follows the theme.
    pub(super) fn icon_logo(&mut self) -> Option<Arc<Image>> {
        match self.state.settings.icon.clone() {
            AppIcon::Custom(path) if path.is_file() => {
                let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                self.cached_image(format!("file:{}", path.display()), || {
                    let bytes = std::fs::read(&path).ok()?;
                    Some(Arc::new(Image::from_bytes(appearance::image_format(&name), bytes)))
                })
            }
            AppIcon::Preset(id) => self.logo_image(&crate::ui::theme::preset(&id).logo.clone()),
            AppIcon::Theme | AppIcon::Custom(_) => {
                let name = self.theme.logo.clone();
                self.logo_image(&name)
            }
        }
    }

    /// The embedded logo `name`, decoded once.
    pub(super) fn logo_image(&mut self, name: &str) -> Option<Arc<Image>> {
        let bytes = glass_logo(name)?;
        self.cached_image(format!("logo:{name}"), || {
            Some(Arc::new(Image::from_bytes(appearance::image_format(name), bytes.to_vec())))
        })
    }

    /// Keys while the modal is open. Escape closes it or cancels a rebinding; keys stay inside the modal.
    /// Closed, Ctrl+, opens it. Returns true when consumed.
    pub(super) fn settings_key(&mut self, ev: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let m = ev.keystroke.modifiers;
        let key = ev.keystroke.key.as_str();
        if !self.settings.open {
            if key == "," && m.control && !m.alt && !m.shift && !m.platform {
                self.settings_open(cx);
                return true;
            }
            return false;
        }
        if let Some(action) = self.settings.capture {
            if key == "escape" {
                self.settings.capture = None;
            } else if key == "backspace" {
                self.state.settings.keys.insert(action, String::new());
                self.settings.capture = None;
                self.save(cx);
            } else if !MODIFIER_KEYS.contains(&key) {
                match capture_spec(&ev.keystroke) {
                    Ok(spec) => {
                        let old = self.state.settings.keys.insert(action, spec).unwrap_or_default();
                        self.settings.displaced = Some((action, old));
                        self.settings.capture = None;
                        self.save(cx);
                    }
                    // Stay in capture so the user can press another key.
                    Err(why) => self.toast(why, cx),
                }
            }
            cx.notify();
            return true;
        }
        if key == "escape" {
            if self.settings.picker.is_some() {
                self.settings.picker = None;
                cx.notify();
            } else {
                self.settings_close(cx);
            }
            return true;
        }
        // A focused field takes its own keys; everything else stays inside the modal.
        !self.settings_field_focused(window, cx)
    }

    fn settings_field_focused(&self, window: &Window, cx: &App) -> bool {
        [self.settings.name.as_ref(), self.settings.search.as_ref()]
            .into_iter()
            .flatten()
            .any(|(input, _)| input.read(cx).focus_handle().is_focused(window))
    }
}

/// The theme logo `name`, or `None` for an unknown name or a missing file.
fn glass_logo(name: &str) -> Option<Arc<[u8]>> {
    super::glass::logo_bytes(name)
}

/// The update channel's label in the nav footer.
fn channel_name(channel: crate::app::settings::UpdateChannel) -> &'static str {
    use crate::app::settings::UpdateChannel;
    match channel {
        UpdateChannel::Stable => "Stable",
        UpdateChannel::Beta => "Beta",
        UpdateChannel::Nightly => "Nightly",
    }
}

fn name_config() -> TextInputConfig {
    TextInputConfig {
        placeholder: "archonui".into(),
        max_len: Some(18),
        font_size: px(15.),
        line_height: px(20.),
        ..Default::default()
    }
}

fn search_config() -> TextInputConfig {
    TextInputConfig {
        placeholder: "Search plugins and extensions".into(),
        font_size: px(13.),
        line_height: px(18.),
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn columns_drop_as_the_body_narrows() {
        assert_eq!(columns(1000., 320., 14., 2), 2);
        assert_eq!(columns(654., 320., 14., 2), 2, "exactly two fit");
        assert_eq!(columns(653., 320., 14., 2), 1);
        assert_eq!(columns(100., 320., 14., 2), 1, "never fewer than one");
        assert_eq!(columns(f32::NAN, 320., 14., 2), 1);
    }

    fn spec(s: &str) -> Result<String, &'static str> {
        capture_spec(&gpui::Keystroke::parse(s).unwrap())
    }

    #[test]
    fn captured_shortcuts_need_a_modifier_that_cannot_be_typed() {
        assert_eq!(spec("ctrl-shift-w").as_deref(), Ok("ctrl-shift-w"));
        assert_eq!(spec("alt-q").as_deref(), Ok("alt-q"));
        assert_eq!(spec("f5").as_deref(), Ok("f5"));
        assert!(spec("a").is_err(), "a bare letter would fire while typing");
        assert!(spec("shift-a").is_err(), "so would a shifted one");
        assert!(spec("enter").is_err());
        assert!(spec("ctrl--").is_err(), "a minus can't survive the spec's dash separator");
        assert!(spec("cmd-k").is_err() && spec("fn-k").is_err());
    }

    #[test]
    fn dialog_fits_inside_the_window() {
        assert_eq!(fit(1320., 560., 2000.), 1320.);
        assert_eq!(fit(1320., 560., 900.), 900.);
        assert_eq!(fit(1320., 560., 300.), 300., "the window wins over the minimum");
    }
}

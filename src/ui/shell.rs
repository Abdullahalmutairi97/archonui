//! The root view of the new UI: wallpaper, left sidebar, tiled sessions and the right tools panel
//! (docs/design/layout.md §1). Each region lives in its own file as `impl Shell` methods.

use crate::{
    app::{
        notify::{self, Ended},
        persist::{Paths, Saver},
        settings::{theme_key, AppIcon},
        state::{AppState, Applied},
    },
    engine::{self, title::{Exchange, TitleModel}, Capabilities, DetectedAgent},
    store,
    ui::{
        anim::{self, Animator},
        approval::{self, ApprovalUi},
        composer::ComposerUi,
        settings::SettingsUi,
        share::ShareUi,
        stream::StreamUi,
        switcher::SwitcherUi,
        tabs::TabsUi,
        glass::{Backdrop, WallpaperCache, WallpaperSource},
        rpanel::PanelUi,
        sidebar::SidebarUi,
        theme::{self, Effects, Theme},
        tiles::TilesUi,
        tiling::Rect,
        widgets::toast::Toasts,
    },
};
use gpui::{
    div, prelude::*, size, AnyWindowHandle, App, Context, Entity, FocusHandle, Focusable, KeyDownEvent,
    MouseButton, MouseDownEvent, Window,
};
use std::{collections::HashMap, path::{Path, PathBuf}, time::Instant};

/// Width of a collapsed sidebar or tools panel, px.
pub const RAIL: f32 = 40.;

/// Where each region sits this frame, in window px. Sidebar and panel widths animate.
#[derive(Clone, Copy, Debug)]
pub struct Regions {
    pub window: Rect,
    pub left: Rect,
    pub right: Rect,
    /// The tiling area: inset by the gap on all four sides.
    pub centre: Rect,
    pub gap: f32,
}

pub struct Shell {
    pub(super) state: AppState,
    pub(super) paths: Paths,
    pub(super) saver: Saver,
    /// Resolved from the settings; rebuilt by `refresh_theme` after appearance changes.
    pub(super) theme: Theme,
    pub(super) agents: Vec<DetectedAgent>,
    /// Files given on the command line, attached to a new session once the agents are known.
    startup_files: Vec<std::path::PathBuf>,
    pub(super) scanning: bool,
    /// Commands and models per harness id; `None` while discovery runs.
    pub(super) caps: HashMap<&'static str, Option<Capabilities>>,
    /// Shared so a `Backdrop` can borrow it while regions render with `&mut self`.
    pub(super) wallpapers: std::rc::Rc<WallpaperCache>,
    pub(super) toasts: Entity<Toasts>,
    pub(super) focus: FocusHandle,
    /// The main window, to ask whether it is active from outside a render.
    pub(super) handle: AnyWindowHandle,
    pub(super) sidebar: SidebarUi,
    pub(super) tiles: TilesUi,
    pub(super) panel: PanelUi,
    pub(super) composers: ComposerUi,
    pub(super) stream: StreamUi,
    pub(super) approvals: ApprovalUi,
    pub(super) tabs: TabsUi,
    pub(super) settings: SettingsUi,
    pub(super) share: ShareUi,
    pub(super) switcher: SwitcherUi,
    /// Animated widths of the left sidebar and the tools panel.
    pub(super) left_w: Animator<f32>,
    pub(super) right_w: Animator<f32>,
}

impl Shell {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let paths = Paths::current();
        let state = AppState::load(&paths);
        anim::set_reduced_motion(state.settings.reduce_motion);
        crate::ui::scale::set_zoom(state.settings.ui_scale);
        let theme = resolve_theme(&state);
        let (wallpapers, ready) = WallpaperCache::new();
        cx.spawn(async move |this, cx| {
            while ready.recv().await.is_ok() {
                if this.update(cx, |_, cx| cx.notify()).is_err() {
                    break;
                }
            }
        })
        .detach();
        let toasts = cx.new(|_| Toasts::new(&theme));
        let focus = cx.focus_handle();
        window.focus(&focus);
        let handle = window.window_handle();
        let saver = Saver::new();
        let save_errors = saver.errors();
        cx.spawn(async move |this, cx| {
            while let Ok(err) = save_errors.recv().await {
                if this.update(cx, |this, cx| this.toast(err, cx)).is_err() {
                    break;
                }
            }
        })
        .detach();
        let mut this = Self {
            state,
            paths,
            saver,
            theme,
            agents: Vec::new(),
            startup_files: Vec::new(),
            scanning: false,
            caps: HashMap::new(),
            wallpapers: std::rc::Rc::new(wallpapers),
            toasts,
            focus,
            handle,
            sidebar: SidebarUi::default(),
            tiles: TilesUi::default(),
            panel: PanelUi::default(),
            composers: ComposerUi::default(),
            stream: StreamUi::default(),
            approvals: approval::start(cx),
            tabs: TabsUi::default(),
            settings: SettingsUi::default(),
            share: ShareUi::default(),
            switcher: SwitcherUi::default(),
            left_w: Animator::new(0., anim::SIDEBAR_COLLAPSE),
            right_w: Animator::new(0., anim::PANEL_COLLAPSE),
        };
        let (l, r) = this.target_widths();
        this.left_w = Animator::new(l, anim::SIDEBAR_COLLAPSE);
        this.right_w = Animator::new(r, anim::PANEL_COLLAPSE);
        this.rescan(cx);
        this.dev_open(cx);
        this
    }

    /// A developer switch for screenshots: `ARCHONUI_OPEN=settings:appearance` opens that settings page,
    /// and `ARCHONUI_SCROLL=600` scrolls it down by that many pixels. Unset, nothing happens.
    fn dev_open(&mut self, cx: &mut Context<Self>) {
        let Ok(target) = std::env::var("ARCHONUI_OPEN") else { return };
        if let Some(page) = target.strip_prefix("settings:") {
            self.settings_open(cx);
            if let Some(page) = super::settings::Page::from_label(page) {
                self.settings.page = page;
            }
            if let Some(y) = std::env::var("ARCHONUI_SCROLL").ok().and_then(|v| v.parse::<f32>().ok()) {
                self.settings.body_scroll.set_offset(gpui::point(gpui::px(0.), gpui::px(-y)));
            }
        }
    }

    /// Detects installed harnesses, then discovers each one's commands and models.
    pub(super) fn rescan(&mut self, cx: &mut Context<Self>) {
        if self.scanning {
            return;
        }
        self.scanning = true;
        let legacy = store::Settings { harnesses: self.state.settings.harnesses.clone(), ..Default::default() };
        let cwd = self.default_folder();
        cx.spawn(async move |this, cx| {
            let agents = cx.background_executor().spawn(async move { engine::detect(&legacy) }).await;
            let _ = this.update(cx, |this, cx| {
                this.scanning = false;
                this.caps = agents.iter().map(|a| (a.harness.id(), None)).collect();
                for agent in agents.clone() {
                    let cwd = cwd.clone();
                    cx.spawn(async move |this, cx| {
                        let id = agent.harness.id();
                        let caps = cx.background_executor().spawn(async move { engine::discover(&agent, &cwd) }).await;
                        let _ = this.update(cx, |this, cx| {
                            this.caps.insert(id, Some(caps));
                            cx.notify();
                        });
                    })
                    .detach();
                }
                this.agents = agents;
                this.open_startup_files(cx);
                cx.notify();
            });
        })
        .detach();
    }

    /// Folder for new sessions: the setting if it still exists, else where ArchonUI was started.
    pub(super) fn default_folder(&self) -> std::path::PathBuf {
        self.state
            .settings
            .default_folder
            .clone()
            .filter(|p| p.is_dir())
            .or_else(|| std::env::var_os("ARCHONUI_CWD").map(Into::into))
            .or_else(|| std::env::current_dir().ok())
            .unwrap_or_else(|| "/".into())
    }

    /// Re-resolves the theme after an appearance setting changed, and saves the settings.
    pub(super) fn refresh_theme(&mut self, cx: &mut Context<Self>) {
        self.theme = resolve_theme(&self.state);
        self.save(cx);
    }

    /// Saves settings, workspace and projects (coalesced) and redraws.
    pub(super) fn save(&mut self, cx: &mut Context<Self>) {
        self.state.save_config(&self.saver, &self.paths);
        cx.notify();
    }

    /// Flushes dirty sessions and toasts any file write errors.
    pub(super) fn flush_sessions(&mut self, cx: &mut Context<Self>) {
        for err in self.state.flush_sessions() {
            self.toast(err, cx);
        }
    }

    pub(super) fn toast(&self, msg: impl Into<String>, cx: &mut Context<Self>) {
        let msg = msg.into();
        self.toasts.update(cx, |t, cx| t.show(msg, cx));
    }

    pub(super) fn effects(&self) -> Effects {
        Effects {
            blur: self.state.settings.panel_blur,
            dim: self.state.settings.wallpaper_dim,
            opacity: self.state.settings.window_opacity / 100.,
        }
    }

    /// Acts on a turn that ended: the desktop alert and sound, and a fast title for a chat's first exchange.
    pub(super) fn turn_ended(&mut self, id: &str, applied: &Applied, cx: &mut Context<Self>) {
        if let Some(ended) = &applied.ended {
            self.alert_turn_ended(ended, cx);
        }
        if let Some(exchange) = applied.exchange.clone() {
            self.suggest_title(id, exchange, cx);
        }
    }

    /// The desktop notification and sound for a turn that ended, as the settings and the window's focus say.
    fn alert_turn_ended(&self, ended: &Ended, cx: &mut Context<Self>) {
        let window_active = cx.update_window(self.handle, |_, window, _| window.is_window_active()).unwrap_or(false);
        let settings = &self.state.settings;
        let alert = notify::alert(settings.notify, settings.sound, window_active, &ended.outcome);
        if alert.desktop {
            let summary = if ended.title.is_empty() { settings.app_name.clone() } else { ended.title.clone() };
            let body = notify::body(&ended.outcome, &ended.reply);
            let icon = self.identity_icon();
            notify::spawn("notify-send", &notify::notify_args(&settings.app_name, icon.as_deref(), &summary, &body));
        }
        if alert.sound {
            if let Some((program, args)) = notify::sound_command(notify::on_path, |path| path.is_file()) {
                notify::spawn(&program, &args);
            }
        }
    }

    /// The app identity's icon as a file: the custom icon file, else the logo cached in the data dir.
    fn identity_icon(&self) -> Option<PathBuf> {
        let key = match &self.state.settings.icon {
            AppIcon::Custom(path) if path.is_file() => return Some(path.clone()),
            AppIcon::Preset(id) => theme::preset(id).logo.to_string(),
            AppIcon::Theme | AppIcon::Custom(_) => self.theme.logo.clone(),
        };
        let bytes = crate::ui::glass::logo_bytes(&key)?;
        // A logo file is cached under its file name; the embedded logo's key is already one.
        let name = Path::new(&key).file_name().map_or_else(|| key.clone(), |n| n.to_string_lossy().into_owned());
        notify::cache_file(&self.paths.data.join("cache"), &name, &bytes).ok()
    }

    /// Names a chat's first exchange in the background, on its harness's cheap model. The title is
    /// kept only if the chat is still auto-titled when it arrives.
    fn suggest_title(&self, id: &str, exchange: Exchange, cx: &mut Context<Self>) {
        let Some(session) = self.state.session(id) else { return };
        let Some(agent) = self.agents.iter().find(|a| a.harness.id() == session.agent).cloned() else { return };
        let models: &[engine::ModelInfo] =
            self.caps.get(session.agent.as_str()).and_then(Option::as_ref).map_or(&[], |c| c.models.as_slice());
        let model = match engine::title::title_model(&session.agent, models) {
            TitleModel::Cheap(model) => Some(model),
            TitleModel::Default => None,
            TitleModel::Skip => return,
        };
        let cwd = session.cwd.clone();
        let id = id.to_string();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { engine::title::suggest(&agent, model.as_deref(), &cwd, &exchange) })
                .await;
            let _ = this.update(cx, |this, cx| {
                match result {
                    Ok(title) => {
                        if this.state.apply_title(&id, &title) {
                            this.flush_sessions(cx);
                        }
                    }
                    Err(e) => eprintln!("could not title session {id}: {e}"),
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// The wallpaper to draw: the user's pick for this theme, else the theme's own, through the
    /// background filter if one is on.
    pub(super) fn wallpaper_source(&self) -> WallpaperSource {
        let settings = &self.state.settings;
        let base = match settings.wallpapers.get(theme_key(&settings.theme)) {
            Some(path) if path.is_file() => WallpaperSource::File(path.clone()),
            _ => WallpaperSource::theme(&self.theme),
        };
        let t = &self.theme;
        let hex = |c: theme::Rgba| (u32::from(c.r) << 16) | (u32::from(c.g) << 8) | u32::from(c.b);
        let palette = match settings.filter {
            crate::app::settings::BackgroundFilter::Off => return base,
            crate::app::settings::BackgroundFilter::Duotone => vec![hex(t.bg), hex(t.fg)],
            crate::app::settings::BackgroundFilter::Tritone => vec![hex(t.bg), hex(theme::mix(t.bg, t.acc, 0.6)), hex(t.fg)],
        };
        let pixel = settings.filter_pixel.round() as u32;
        WallpaperSource::Dithered(Box::new(base), crate::ui::glass::Dither { palette, pixel })
    }

    /// Sidebar and panel widths the workspace asks for.
    fn target_widths(&self) -> (f32, f32) {
        let w = &self.state.workspace;
        (if w.left_open { w.left_width } else { RAIL }, if w.right_open { w.right_width } else { RAIL })
    }

    /// Call after changing `left_open`, `right_open` or `right_width`: animates to the new widths
    /// (a drag passes `instant`) and saves.
    pub(super) fn widths_changed(&mut self, instant: bool, cx: &mut Context<Self>) {
        let (l, r) = self.target_widths();
        let now = Instant::now();
        if instant || anim::reduced_motion() {
            self.left_w = Animator::new(l, anim::SIDEBAR_COLLAPSE);
            self.right_w = Animator::new(r, anim::PANEL_COLLAPSE);
        } else {
            self.left_w.set_target(l, now);
            self.right_w.set_target(r, now);
        }
        self.save(cx);
    }

    /// Height of the sidebar header and the tools panel tab bar: their bottom borders line up with the
    /// tile headers' (52 px, below a tile's 2 px focus-ring inset and the gap above the tiles).
    pub(super) fn header_height(&self) -> f32 {
        self.state.settings.gap() + 54.
    }

    /// Region rectangles for this frame (layout.md §1.3).
    pub(super) fn regions(&self, window: &Window) -> Regions {
        // Regions are in design units: the window size divided by the interface scale.
        let size = window.viewport_size();
        let (ww, wh) = (crate::ui::scale::design(size.width), crate::ui::scale::design(size.height));
        let now = Instant::now();
        let (lw, rw) = (self.left_w.value(now), self.right_w.value(now));
        let g = self.state.settings.gap();
        Regions {
            window: Rect { x: 0., y: 0., w: ww, h: wh },
            left: Rect { x: 0., y: 0., w: lw, h: wh },
            right: Rect { x: ww - rw, y: 0., w: rw, h: wh },
            centre: Rect { x: lw + g, y: g, w: (ww - lw - rw - 2. * g).max(0.), h: (wh - 2. * g).max(0.) },
            gap: g,
        }
    }

    fn on_key(&mut self, ev: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        // Regions get the first look at keys (focused inputs, the terminal, the editor…).
        if self.share_key(ev, window, cx) || self.switcher_key(&ev.keystroke, window, cx) || self.settings_key(ev, window, cx) || self.tab_key(ev, window, cx) || self.approval_key(ev, window, cx) || self.composer_key(ev, window, cx) || self.tiles_key(ev, window, cx) || self.panel_key(ev, window, cx) || self.sidebar_key(ev, window, cx) {
            return;
        }
        self.global_key(ev, window, cx);
    }
}

/// The theme for the current settings: preset, the user's colour overrides and font choice.
pub(super) fn resolve_theme(state: &AppState) -> Theme {
    let s = &state.settings;
    let key = theme_key(&s.theme);
    let custom: HashMap<String, String> =
        s.custom_colours.get(key).map(|m| m.iter().map(|(k, v)| (k.clone(), v.clone())).collect()).unwrap_or_default();
    theme::resolve(theme::preset(&s.theme), &custom, s.fonts.get(key).cloned().unwrap_or_default())
}

/// The harness new sessions start with: the default from settings when it is detected and enabled,
/// else the first enabled detected harness. A disabled harness is never picked; `None` when none is enabled.
pub(in crate::ui) fn pick_harness(default: Option<&str>, agents: &[DetectedAgent]) -> Option<&'static str> {
    pick_harness_id(default, agents.iter().map(|a| (a.harness.id(), a.enabled)))
}

/// `pick_harness` over `(harness id, enabled)` pairs in detection order.
fn pick_harness_id(default: Option<&str>, agents: impl Iterator<Item = (&'static str, bool)> + Clone) -> Option<&'static str> {
    let mut enabled = agents.filter(|&(_, on)| on).map(|(id, _)| id);
    default.and_then(|d| enabled.clone().find(|id| *id == d)).or_else(|| enabled.next())
}

/// Whether a link from chat output may be opened: only `http`, `https` and `mailto`. Agent output can
/// carry `file://`, `javascript:` or other schemes, which are refused.
pub(in crate::ui) fn link_allowed(url: &str) -> bool {
    let url = url.trim();
    let Some((scheme, rest)) = url.split_once(':') else { return false };
    match scheme.to_ascii_lowercase().as_str() {
        "http" | "https" => rest.starts_with("//") && rest.len() > 2,
        "mailto" => !rest.is_empty(),
        _ => false,
    }
}

impl Focusable for Shell {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for Shell {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // A chat left with an unsent draft joins the sidebar; save it so it survives a crash. An empty
        // chat whose tile is gone is deleted.
        let listed = self.state.list_left_drafts(false);
        if self.state.drop_abandoned() || listed {
            self.flush_sessions(cx);
        }
        // rem-based sizes (`p_2`, `text_sm`…) follow the interface scale like `px` does.
        window.set_rem_size(gpui::px(16. * crate::ui::scale::zoom()));
        // The glass canvas is in design units, like the regions cut from it.
        let viewport = window.viewport_size();
        let canvas = size(
            gpui::px(crate::ui::scale::design(viewport.width)),
            gpui::px(crate::ui::scale::design(viewport.height)),
        );
        let now = Instant::now();
        anim::tick(window, self.left_w.is_running(now) || self.right_w.is_running(now));
        self.tabs_before_render(window, cx);
        let theme = self.theme.clone();
        let source = self.wallpaper_source();
        let look = self.state.settings.background.surfaces(&theme, self.effects()).wallpaper;
        let cache = self.wallpapers.clone();
        let backdrop = Backdrop { cache: &cache, source: &source, look, canvas };
        let wallpaper = crate::ui::glass::wallpaper_layer(&backdrop);
        let sidebar = self.render_sidebar(&backdrop, window, cx);
        let tiles = self.render_tiles(&backdrop, window, cx);
        let panel = self.render_panel(&backdrop, window, cx);
        let overlays = self.render_overlays(window, cx);
        // A press outside the tools panel gives the keyboard back to the chat (tabs_blur).
        let panel_left = self.regions(window).right.x;
        div()
            .id("shell")
            .key_context("Shell")
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::on_key))
            .on_mouse_down(MouseButton::Left, cx.listener(move |this, ev: &MouseDownEvent, _, cx| {
                if crate::ui::scale::design(ev.position.x) < panel_left {
                    this.tabs_blur(cx);
                }
            }))
            .size_full()
            .relative()
            .overflow_hidden()
            // Below full window opacity the desktop shows through the base colour too.
            .bg(theme.bg.alpha(self.effects().clamped_opacity()).to_hsla())
            .text_color(theme.fg.to_hsla())
            .font_family(theme.ui_font.clone())
            .child(wallpaper)
            .child(sidebar)
            .child(tiles)
            .child(panel)
            .children(overlays)
            .child(self.toasts.clone())
            .child(self.render_window_edges(window))
    }
}

impl Drop for Shell {
    fn drop(&mut self) {
        self.state.shutdown();
    }
}

/// Opens the main window: frameless, transparent where the platform allows.
impl Shell {
    /// Starts a session with the default agent, in the focused tile, carrying the command-line files.
    fn open_startup_files(&mut self, cx: &mut Context<Self>) {
        if self.startup_files.is_empty() {
            return;
        }
        let files = std::mem::take(&mut self.startup_files);
        let Some(agent) = pick_harness(self.state.settings.default_harness.as_deref(), &self.agents) else {
            self.toast("No agent is enabled", cx);
            return;
        };
        let id = self.state.new_session(agent, self.default_folder(), crate::app::state::Placement::Focused);
        let (model, effort) = self.agent_defaults(agent);
        self.state.set_options(&id, model, effort, self.state.settings.default_mode);
        self.add_files(&id, &files, cx);
        self.save(cx);
    }
}

/// Opens the main window; `files` are attached to a new session (`archonui <files>...`).
pub fn open_window(cx: &mut App, files: Vec<std::path::PathBuf>) {
    crate::ui::fonts::register(cx);
    crate::ui::widgets::text_input::init(cx);
    // Window sizes are screen px, not design px.
    let bounds = gpui::Bounds::centered(None, size(gpui::px(1600.), gpui::px(1000.)), cx);
    cx.open_window(
        gpui::WindowOptions {
            window_bounds: Some(gpui::WindowBounds::Windowed(bounds)),
            titlebar: None,
            window_decorations: Some(gpui::WindowDecorations::Client),
            window_background: gpui::WindowBackgroundAppearance::Transparent,
            window_min_size: Some(size(gpui::px(900.), gpui::px(560.))),
            app_id: Some("archonui".into()),
            ..Default::default()
        },
        move |window, cx| {
            cx.new(|cx| {
                let mut shell = Shell::new(window, cx);
                shell.startup_files = files;
                shell
            })
        },
    )
    .expect("open window");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pick_harness_falls_back_from_a_disabled_or_missing_default() {
        let agents = [("claude", false), ("codex", true), ("gemini", true)];
        let pick = |d: Option<&str>| pick_harness_id(d, agents.iter().copied());
        assert_eq!(pick(Some("gemini")), Some("gemini"));
        assert_eq!(pick(Some("claude")), Some("codex"), "a disabled default is skipped");
        assert_eq!(pick(Some("opencode")), Some("codex"), "an uninstalled default is skipped");
        assert_eq!(pick(None), Some("codex"));
        assert_eq!(pick_harness_id(Some("claude"), [("claude", false)].into_iter()), None);
        assert_eq!(pick_harness_id(None, std::iter::empty()), None);
    }

    #[test]
    fn only_web_and_mail_links_open() {
        for ok in ["https://example.com", "http://localhost:3000/x", "HTTPS://Example.com", "mailto:a@b.c", " https://x.y "] {
            assert!(link_allowed(ok), "{ok}");
        }
        for bad in ["file:///etc/passwd", "javascript:alert(1)", "JavaScript:alert(1)", "data:text/html,x", "about:blank", "/etc/passwd", "relative/path", "", "https:", "http:x", "vbscript:x", "ftp://x"] {
            assert!(!link_allowed(bad), "{bad}");
        }
    }
}

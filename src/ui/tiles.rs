//! The centre: tiled session views with resize handles. Spec: docs/design/layout.md §3, chat.md.

use super::{
    anim::{self, Animator, Lerp, INSTANT, TILE_REFLOW},
    glass::{glass_surface, logo_bytes, Backdrop, Rect as GlassRect},
    shell::Shell,
    sigil::{sigil_element, status_dot, SessionState, StatusDotColours},
    theme::{Theme, TileFill, TileState, PRESETS},
    tiling::{Arrow, Dir, Handle, Rect},
    widgets::popover::{tooltip, TooltipColors},
};
use crate::{
    app::{settings::Action, state::Placement},
    engine::{self, DetectedAgent, Mode},
    store::{self, Session},
};
use gpui::{
    canvas, div, hsla, img, linear_color_stop, linear_gradient, point, prelude::*, AnyElement, Background,
    BoxShadow, ClickEvent, ClipboardItem, Context, CursorStyle, Div, DispatchPhase, ElementId, ExternalPaths,
    FontWeight, Hsla,
    Image, ImageFormat, KeyDownEvent, Keystroke, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, Stateful,
    Window,
};
use crate::ui::scale::px;
use std::{collections::HashMap, sync::Arc, time::Instant};

/// Session info popover width in px.
const INFO_W: f32 = 300.;
/// Activity log lines the session info popover shows, most recent last.
const ACTIVITY_LINES: usize = 20;
/// Focus ring width in px: the gradient ring of a focused tile.
const RING_FOCUSED: f32 = 2.;
/// Border width in px of an unfocused tile.
const RING_IDLE: f32 = 1.;
/// Tile header height in px, border included, below the focus-ring inset: 12 px padding + the 22 px
/// title line + 2 px gap + the 11.5 px meta line + 12 px padding (layout.md §3.1). The session info
/// popover opens just below it.
const HEADER_H: f32 = 62.;
/// Tile view state that isn't persisted: rect animations, a divider drag in progress, the session info
/// popover, and the centre area from the last frame.
#[derive(Default)]
pub struct TilesUi {
    /// Each tile's rect, easing to its layout slot.
    rects: HashMap<String, Animator<Rect>>,
    /// The divider being dragged, from mouse down until mouse up.
    drag: Option<Handle>,
    /// The session whose info popover is open.
    info: Option<String>,
    /// Centre area and gap from the last frame, for actions that run outside a render.
    area: Option<(Rect, f32)>,
}

impl Lerp for Rect {
    fn lerp(from: &Self, to: &Self, t: f32) -> Self {
        Rect {
            x: f32::lerp(&from.x, &to.x, t),
            y: f32::lerp(&from.y, &to.y, t),
            w: f32::lerp(&from.w, &to.w, t),
            h: f32::lerp(&from.h, &to.h, t),
        }
    }
}

/// What a tile's header and body need from its session, copied out so rendering can borrow `self` mutably.
struct TileView {
    id: String,
    title: String,
    icon: u16,
    meta: String,
    status: SessionState,
    empty: bool,
}

impl Shell {
    /// The tiles, each in its layout slot with its resize handles; the empty state when there are none.
    pub(super) fn render_tiles(&mut self, backdrop: &Backdrop, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let regions = self.regions(window);
        let (area, gap) = (regions.centre, regions.gap);
        self.tiles.area = Some((area, gap));
        // Sessions without messages aren't saved, so a restart can leave layout tiles with no session. Drop them
        // so they don't reserve space.
        let missing: Vec<String> =
            self.state.workspace.layout.tiles.iter().filter(|id| self.state.session(id).is_none()).cloned().collect();
        for id in &missing {
            self.state.workspace.layout.close(id);
        }
        // A saved layout may hold duplicate tiles, more than six, or fractions out of range.
        self.state.workspace.layout.normalize();
        let (slots, handles) = self.state.workspace.layout.compute(area, gap);
        let now = Instant::now();
        // A divider that no longer exists (its tile closed, or the layout mode changed) ends the drag.
        if self.tiles.drag.is_some_and(|d| !handles.iter().any(|h| h.key == d.key && h.dir == d.dir)) {
            self.tiles.drag = None;
        }
        let drag = self.tiles.drag;
        self.tiles.rects.retain(|id, _| slots.iter().any(|(slot, _)| slot == id));

        let focus = self.state.workspace.layout.focus.clone();
        let theme = self.theme.clone();
        let effects = self.effects();
        let rad = self.state.settings.radius;
        let mut running = false;
        let mut layers: Vec<AnyElement> = Vec::new();
        if slots.is_empty() {
            layers.push(self.render_empty(area, cx));
        }

        for (id, slot) in &slots {
            let Some(view) = self.tile_view(id) else { continue };
            // A drag follows the pointer exactly, as the prototype disables tile transitions while dragging.
            if drag.is_some() {
                self.tiles.rects.insert(id.clone(), Animator::new(*slot, INSTANT));
            } else {
                self.tiles.rects.entry(id.clone()).or_insert_with(|| Animator::new(*slot, TILE_REFLOW)).set_target(*slot, now);
            }
            let animator = &self.tiles.rects[id];
            running |= animator.is_running(now);
            let rect = animator.value(now);

            let focused = focus.as_deref() == Some(id.as_str());
            let ring_w = if focused { RING_FOCUSED } else { RING_IDLE };
            let ring: Background = if focused && self.state.settings.session_ring {
                linear_gradient(
                    135.,
                    linear_color_stop(theme.border[0].to_hsla(), 0.),
                    linear_color_stop(theme.border[2].to_hsla(), 1.),
                )
            } else {
                theme.line2.to_hsla().into()
            };
            let body = Rect {
                x: rect.x + ring_w,
                y: rect.y + ring_w,
                w: (rect.w - 2. * ring_w).max(0.),
                h: (rect.h - 2. * ring_w).max(0.),
            };
            let inner = (rad - ring_w).max(0.);

            let surface = self.state.settings.background.tile(
                &theme,
                effects,
                TileState { focused, empty: view.empty, tint_all: self.state.settings.tint_all },
            );
            // Glass takes one solid fill, so the Canvas gradient is drawn as a plain two-stop gradient.
            let tint = match surface.fill {
                TileFill::Solid(c) => match surface.blur {
                    Some(blur) => glass_surface(
                        backdrop,
                        GlassRect { x: body.x, y: body.y, w: body.w, h: body.h },
                        inner,
                        c.to_hsla(),
                        hsla(0., 0., 0., 0.),
                        blur,
                    )
                    .into_any_element(),
                    None => at(body).rounded(px(inner)).bg(c.to_hsla()).into_any_element(),
                },
                TileFill::Gradient(stops) => at(body)
                    .rounded(px(inner))
                    .bg(linear_gradient(
                        180.,
                        linear_color_stop(stops[0].0.to_hsla(), stops[0].1),
                        linear_color_stop(stops[2].0.to_hsla(), stops[2].1),
                    ))
                    .into_any_element(),
            };

            let focus_id = id.clone();
            let drop_id = id.clone();
            let header = self.render_header(&view, focused, cx);
            let stream = self.render_stream(id, focused, window, cx);
            let approval = self.render_approval(id, cx);
            let composer = self.render_composer(id, focused, window, cx);
            layers.push(at(rect).rounded(px(rad)).bg(ring).into_any_element());
            layers.push(tint);
            layers.push(
                at(body)
                    .flex()
                    .flex_col()
                    .overflow_hidden()
                    .rounded(px(inner))
                    .text_color(theme.fg.to_hsla())
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, _: &MouseDownEvent, _, cx| this.focus_tile(&focus_id, cx)),
                    )
                    // A file dropped anywhere in the tile attaches to this session, not only over the composer.
                    .on_drop(cx.listener(move |this, paths: &ExternalPaths, _, cx| this.add_files(&drop_id, paths.paths(), cx)))
                    // The header yields height before the composer does in very short tiles.
                    .child(div().flex_shrink().min_h_0().overflow_hidden().child(header))
                    .child(div().flex_1().min_h_0().child(stream))
                    .child(approval)
                    .child(composer)
                    .into_any_element(),
            );
        }

        let dragging = drag.map(|h| h.key);
        let (grip_lit, grip_idle) = (theme.acc2.to_hsla(), theme.line2.to_hsla());
        for (ix, handle) in handles.iter().enumerate() {
            let h = *handle;
            let hit = at(h.rect);
            let hit = match h.dir {
                Dir::V => hit.cursor_col_resize(),
                Dir::H => hit.cursor_row_resize(),
            };
            layers.push(
                hit.on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, ev: &MouseDownEvent, _, cx| this.press_handle(h, ev.click_count, cx)),
                )
                .into_any_element(),
            );
            // The grip's background fades to `acc2` over 0.15 s while its divider is dragged (§6 #5).
            layers.push(anim::animated(
                ("tile-grip", ix),
                anim::GRIP,
                if dragging == Some(h.key) { 1.0 } else { 0.0 },
                move |t| at(h.grip).rounded(px(2.)).bg(Hsla::lerp(&grip_idle, &grip_lit, t)).into_any_element(),
            ));
        }

        // Drag moves and releases are window-wide, so they're registered on every frame, as the slider does.
        let shell = cx.entity();
        let drag_dir = drag.map(|h| h.dir);
        layers.push(
            canvas(
                |_, _, _| (),
                move |_, _, window, _| {
                    if let Some(dir) = drag_dir {
                        window.set_window_cursor_style(match dir {
                            Dir::V => CursorStyle::ResizeColumn,
                            Dir::H => CursorStyle::ResizeRow,
                        });
                    }
                    let moving = shell.clone();
                    window.on_mouse_event(move |ev: &MouseMoveEvent, phase, _, cx| {
                        if phase != DispatchPhase::Bubble || moving.read(cx).tiles.drag.is_none() {
                            return;
                        }
                        moving.update(cx, |this, cx| {
                            // The button was released outside the window, so no mouse-up arrived: end the drag.
                            if ev.pressed_button != Some(MouseButton::Left) {
                                this.tiles.drag = None;
                                this.tiles.rects.clear();
                                this.save(cx);
                                return;
                            }
                            let (Some(handle), Some((area, gap))) = (this.tiles.drag, this.tiles.area) else {
                                return;
                            };
                            let pointer = (crate::ui::scale::design(ev.position.x), crate::ui::scale::design(ev.position.y));
                            this.state.workspace.layout.drag(&handle, pointer, area, gap);
                            cx.notify();
                        });
                    });
                    let releasing = shell.clone();
                    window.on_mouse_event(move |_: &MouseUpEvent, phase, _, cx| {
                        if phase != DispatchPhase::Bubble || releasing.read(cx).tiles.drag.is_none() {
                            return;
                        }
                        releasing.update(cx, |this, cx| {
                            this.tiles.drag = None;
                            // Tiles were dragged with transitions off; drop their animators so reflows animate again.
                            this.tiles.rects.clear();
                            this.save(cx);
                        });
                    });
                },
            )
            .absolute()
            .top_0()
            .left_0()
            .size_full()
            .into_any_element(),
        );

        anim::tick(window, running);
        div().absolute().top_0().left_0().size_full().children(layers).into_any_element()
    }

    /// Keys for the tiles' popovers. Escape closes the session info popover. Returns true when consumed.
    pub(super) fn tiles_key(&mut self, ev: &KeyDownEvent, _window: &mut Window, cx: &mut Context<Self>) -> bool {
        if self.tiles.info.is_some() && ev.keystroke.key == "escape" {
            self.tiles.info = None;
            cx.notify();
            return true;
        }
        false
    }

    /// Global shortcuts (settings.keys): new split, close tile, focus moves, next theme, toggles.
    pub(super) fn global_key(&mut self, ev: &KeyDownEvent, _window: &mut Window, cx: &mut Context<Self>) {
        let keys = &self.state.settings.keys;
        let pressed = |action: Action| keys.get(&action).is_some_and(|k| keystroke_matches(k, &ev.keystroke));
        let action = [
            Action::SplitNewSession,
            Action::CloseTile,
            Action::FocusNext,
            Action::FocusPrev,
            Action::NextTheme,
            Action::ToggleSidebar,
            Action::TogglePanel,
            Action::NewSession,
            Action::SessionSwitcher,
        ]
        .into_iter()
        .find(|a| pressed(*a));

        let m = ev.keystroke.modifiers;
        if m.control && m.shift && !m.alt && !m.platform {
            match ev.keystroke.key.as_str() {
                "up" => return self.move_focus(Arrow::Up, cx),
                "down" => return self.move_focus(Arrow::Down, cx),
                _ => {}
            }
        }

        match action {
            Some(Action::SplitNewSession) => self.new_session_split(cx),
            Some(Action::CloseTile) => {
                if let Some(id) = self.state.workspace.layout.focus.clone() {
                    self.state.workspace.layout.close(&id);
                    self.save(cx);
                }
            }
            Some(Action::FocusNext) => self.move_focus(Arrow::Right, cx),
            Some(Action::FocusPrev) => self.move_focus(Arrow::Left, cx),
            Some(Action::NextTheme) => {
                let settings = &mut self.state.settings;
                let i = PRESETS.iter().position(|p| p.id == settings.theme).unwrap_or(0);
                settings.theme = PRESETS[(i + 1) % PRESETS.len()].id;
                self.refresh_theme(cx);
            }
            Some(Action::ToggleSidebar) => {
                self.state.workspace.left_open = !self.state.workspace.left_open;
                self.widths_changed(false, cx);
            }
            Some(Action::TogglePanel) => {
                self.state.workspace.right_open = !self.state.workspace.right_open;
                self.widths_changed(false, cx);
            }
            Some(Action::NewSession) => self.start_session(Placement::Focused, cx),
            Some(Action::SessionSwitcher) => self.open_switcher(cx),
            _ => {}
        }
    }

    /// Modals and the lightbox, drawn above everything.
    pub(super) fn render_overlays(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Vec<AnyElement> {
        self.render_lightbox(window, cx).into_iter().chain(self.render_info(cx)).chain(self.render_settings(window, cx)).chain(self.render_share(cx)).chain(self.render_switcher(window, cx)).collect()
    }

    /// Starts a session in a new tile after the focused one (`Alt+Enter`, the `+` button).
    pub(super) fn new_session_split(&mut self, cx: &mut Context<Self>) {
        let Some((area, gap)) = self.tiles.area else { return };
        self.start_session(Placement::Split { area, gap }, cx);
    }

    /// Creates a session with the focused session's agent, or the default agent, and shows it in `placement`.
    fn start_session(&mut self, placement: Placement, cx: &mut Context<Self>) {
        let focused = self.state.workspace.layout.focus.as_deref().and_then(|id| self.state.session(id));
        let agent = focused
            .filter(|s| self.installed(&s.agent).is_some())
            .map(|s| s.agent.clone())
            .or_else(|| self.preferred_agent());
        let Some(agent) = agent else {
            self.toast("No agent is installed", cx);
            return;
        };
        let (model, effort) = match focused.filter(|s| s.agent == agent) {
            Some(s) => (s.model.clone(), s.effort.clone()),
            None => self.agent_defaults(&agent),
        };
        let mode = self.state.settings.default_mode;
        let cwd = self.default_folder();
        let id = self.state.new_session(&agent, cwd, placement);
        self.state.set_options(&id, model, effort, mode);
        self.save(cx);
    }

    /// Focuses tile `id`, saving only when the focus moved.
    fn focus_tile(&mut self, id: &str, cx: &mut Context<Self>) {
        if self.state.workspace.layout.focus.as_deref() != Some(id) && self.state.workspace.layout.focus(id) {
            self.save(cx);
        }
    }

    /// Alt+arrow focus move, wrapping through the tile list.
    fn move_focus(&mut self, arrow: Arrow, cx: &mut Context<Self>) {
        self.state.workspace.layout.focus_arrow(arrow);
        self.save(cx);
    }

    /// A mouse press on a divider: a double-click resets it, a single press starts a drag.
    fn press_handle(&mut self, handle: Handle, clicks: usize, cx: &mut Context<Self>) {
        if clicks == 2 {
            self.tiles.drag = None;
            self.state.workspace.layout.reset(&handle);
            self.save(cx);
        } else {
            self.tiles.drag = Some(handle);
            cx.notify();
        }
    }

    /// Opens the session switcher (Ctrl+K).
    pub(super) fn open_switcher(&mut self, cx: &mut Context<Self>) {
        self.switcher_open(cx);
    }

    fn tile_view(&self, id: &str) -> Option<TileView> {
        let s = self.state.session(id)?;
        let status = if self.state.busy(id) {
            SessionState::Running
        } else if s.messages.is_empty() {
            SessionState::Idle
        } else {
            SessionState::Done
        };
        Some(TileView {
            id: id.to_string(),
            title: if s.title.is_empty() { "New session".into() } else { s.title.clone() },
            icon: s.icon,
            meta: self.session_meta(s),
            status,
            empty: s.messages.is_empty(),
        })
    }

    /// The header's meta line: project, model, effort, mode and running state. Spec: chat.md §0.5.
    fn session_meta(&self, s: &Session) -> String {
        let project = s
            .project
            .as_deref()
            .and_then(|p| self.state.project(p))
            .map(|p| p.name.clone())
            .or_else(|| s.cwd.file_name().map(|name| name.to_string_lossy().into_owned()))
            .unwrap_or_default();
        // Same short labels as the composer's model trigger (chat.md §5.6).
        let mut parts = vec![project, self.trigger_label(s)];
        if let Some(effort) = self.effort_state(s).and_then(|e| e.name) {
            parts.push(format!("{} effort", effort.to_lowercase()));
        }
        if s.mode != Mode::Auto {
            parts.push(format!("{} mode", s.mode.name().to_lowercase()));
        }
        if self.state.busy(&s.id) {
            parts.push("running".into());
        }
        parts.join(" · ")
    }

    fn model_text(&self, s: &Session) -> String {
        let caps = self.caps.get(s.agent.as_str()).and_then(Option::as_ref);
        match s.model.as_deref() {
            Some(id) => caps
                .and_then(|c| c.models.iter().find(|m| m.id == id))
                .map_or_else(|| id.to_string(), |m| m.label.clone()),
            None => caps
                .and_then(|c| c.default_model.as_deref())
                .map_or_else(|| "Default model".into(), |d| format!("Default ({d})")),
        }
    }

    /// The effort level a session passes, or the agent's default. `None` when the model has no effort levels.
    fn effort_text(&self, s: &Session) -> Option<String> {
        let caps = self.caps.get(s.agent.as_str()).and_then(Option::as_ref);
        let levels = caps.map(|c| c.efforts(s.model.as_deref())).unwrap_or_default();
        match s.effort.as_deref() {
            Some(level) => Some(level.to_string()),
            None if levels.is_empty() => None,
            None => Some(
                caps.and_then(|c| c.default_effort.as_deref())
                    .map_or_else(|| "default".into(), |d| format!("default ({d})")),
            ),
        }
    }

    fn installed(&self, agent: &str) -> Option<&DetectedAgent> {
        self.agents.iter().find(|a| a.harness.id() == agent)
    }

    /// The agent new sessions use when none is picked: the default if installed and enabled, else the first enabled.
    fn preferred_agent(&self) -> Option<String> {
        super::shell::pick_harness(self.state.settings.default_harness.as_deref(), &self.agents).map(str::to_string)
    }

    /// The model and effort a new session with `agent` starts with.
    pub(super) fn agent_defaults(&self, agent: &str) -> (Option<String>, Option<String>) {
        let (model, effort) = self
            .state
            .settings
            .harness_defaults
            .get(agent)
            .map_or((None, None), |d| (d.model.clone(), d.effort.clone()));
        let model = model.or(self.state.settings.harness(agent).default_model).map(|model| {
            let listed = self.caps.get(agent).and_then(Option::as_ref);
            let lists = |id: &str| listed.is_some_and(|caps| caps.models.iter().any(|m| m.id == id));
            super::settings::long_context_model(&model, self.state.settings.long_context, lists)
        });
        (model, effort)
    }

    /// The tile header: sigil with status dot, title and meta line, then the split and close buttons. Spec: layout.md §3.1.
    fn render_header(&self, view: &TileView, focused: bool, cx: &mut Context<Self>) -> AnyElement {
        let theme = &self.theme;
        let dot = StatusDotColours {
            running: theme.acc2.to_hsla(),
            done: theme.alt.to_hsla(),
            idle: theme.faint.to_hsla(),
            surface: theme.tile.to_hsla(),
        };
        let icon = if focused { theme.acc2 } else { theme.fg };
        let slot = div()
            .relative()
            .size(px(20.))
            .flex_shrink_0()
            .flex()
            .items_center()
            .justify_center()
            .child(sigil_element(view.icon, px(16.), icon.to_hsla()))
            .child(div().absolute().right(px(-2.)).bottom(px(-2.)).child(status_dot(view.status, &dot)));
        let meta_id = view.id.clone();
        let title = div()
            .flex()
            .flex_col()
            .gap(px(2.))
            .min_w_0()
            .flex_1()
            .child(
                div()
                    .font_family(theme.display_font.clone())
                    .text_size(px(20.))
                    .line_height(px(22.))
                    .text_color(theme.fg.to_hsla())
                    .overflow_hidden()
                    .text_ellipsis()
                    .line_clamp(1)
                    .child(view.title.clone()),
            )
            .child(
                div()
                    .id(ElementId::Name(format!("tile-meta-{}", view.id).into()))
                    .cursor_pointer()
                    .text_size(px(11.5))
                    .text_color(theme.muted.to_hsla())
                    .truncate()
                    .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                        this.tiles.info = Some(meta_id.clone());
                        cx.notify();
                    }))
                    .child(view.meta.clone()),
            );
        let split_id = view.id.clone();
        let split = header_button(format!("tile-split-{}", view.id), "+", "Split new session", theme).on_click(cx.listener(
            move |this, _: &ClickEvent, _, cx| {
                this.state.workspace.layout.focus(&split_id);
                this.new_session_split(cx);
            },
        ));
        let close_id = view.id.clone();
        let close = header_button(format!("tile-close-{}", view.id), "×", "Close tile", theme).on_click(cx.listener(
            move |this, _: &ClickEvent, _, cx| {
                this.state.workspace.layout.close(&close_id);
                this.save(cx);
            },
        ));
        // 52 px below the 2 px focus-ring inset; an unfocused tile's 1 px border is topped up so every
        // header's bottom border sits on the same line as the sidebar header and the panel tab bar.
        let ring_w = if focused { RING_FOCUSED } else { RING_IDLE };
        div()
            .h(px(HEADER_H + RING_FOCUSED - ring_w))
            .pt(px(RING_FOCUSED - ring_w))
            .flex_none()
            .flex()
            .items_center()
            .gap(px(10.))
            .pl(px(16.))
            .pr(px(12.))
            .border_b_1()
            .border_color(theme.line.to_hsla())
            .child(slot)
            .child(title)
            .child(split)
            .child(close)
            .into_any_element()
    }

    /// The session info popover under the open tile's header: harness, model, effort, mode, folder,
    /// agent session id (click to copy) and start time.
    fn render_info(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let id = self.tiles.info.as_deref()?;
        let (area, gap) = self.tiles.area?;
        let (slots, _) = self.state.workspace.layout.compute(area, gap);
        let (_, slot) = slots.iter().find(|(tile, _)| tile.as_str() == id)?;
        let s = self.state.session(id)?;
        let theme = &self.theme;

        let harness = match self.installed(&s.agent) {
            Some(a) => format!("{} {}", a.harness.name(), a.version),
            None => format!("{} (not installed)", engine::agent_name(&s.agent)),
        };
        let mut rows = vec![("Harness", harness), ("Model", self.model_text(s))];
        if let Some(effort) = self.effort_text(s) {
            rows.push(("Effort", effort));
        }
        rows.push(("Mode", s.mode.name().to_string()));
        rows.push(("Folder", s.cwd.display().to_string()));
        rows.push((
            "Created",
            match store::ago(s.created).as_str() {
                "now" => "just now".to_string(),
                ago => format!("{ago} ago"),
            },
        ));

        let line2 = theme.line2.to_hsla();
        // Keep the popover inside the tile; long file and activity lists scroll inside it.
        let top = slot.y + RING_FOCUSED + HEADER_H + 6.;
        let max_h = (area.y + area.h - top - 8.).max(160.);
        let mut panel = div()
            .id("tile-info")
            .absolute()
            .left(px(slot.x + 16.))
            .top(px(top))
            .w(px(INFO_W))
            .p(px(6.))
            .rounded(px(10.))
            .bg(theme.pop.to_hsla())
            .border_1()
            .border_color(line2)
            .text_color(theme.fg.to_hsla())
            .max_h(px(max_h))
            .overflow_y_scroll()
            .shadow(vec![BoxShadow {
                color: hsla(0., 0., 0., 0.45),
                offset: point(px(0.), px(14.)),
                blur_radius: px(40.),
                spread_radius: px(0.),
            }])
            .on_mouse_down_out(cx.listener(|this, _: &MouseDownEvent, _, cx| {
                this.tiles.info = None;
                cx.notify();
            }));
        for (key, value) in rows {
            panel = panel.child(
                div()
                    .px(px(10.))
                    .py(px(4.))
                    .child(div().text_size(px(11.5)).text_color(theme.muted.to_hsla()).child(key))
                    .child(div().text_size(px(12.)).truncate().child(value)),
            );
        }

        let copy = s.agent_session.clone();
        let label = if copy.is_some() { "Agent session · click to copy" } else { "Agent session" };
        let value = copy.clone().unwrap_or_else(|| "not started yet".into());
        let session_row = div()
            .id("tile-info-session")
            .px(px(10.))
            .py(px(4.))
            .rounded(px(6.))
            .child(div().text_size(px(11.5)).text_color(theme.muted.to_hsla()).child(label))
            .child(div().text_size(px(12.)).truncate().child(value));
        let session_row = match copy {
            Some(id) => session_row
                .cursor_pointer()
                .hover(move |d| d.bg(line2))
                .on_click(cx.listener(move |_, _: &ClickEvent, _, cx| {
                    cx.write_to_clipboard(ClipboardItem::new_string(id.clone()));
                })),
            None => session_row,
        };

        // Files the session carries: the message attachments, then the files the agent read or wrote.
        let mut files: Vec<(String, String)> = s
            .messages
            .iter()
            .flat_map(|m| &m.attachments)
            .map(|a| (a.name.clone(), engine::attach::human_size(a.size)))
            .collect();
        let mut seen: Vec<&str> = Vec::new();
        for tool in s.messages.iter().flat_map(|m| &m.tools) {
            let label = match tool.kind {
                store::ToolKind::Read => "read",
                store::ToolKind::Edit => "edit",
                store::ToolKind::Write => "write",
                _ => continue,
            };
            if seen.contains(&tool.target.as_str()) {
                continue;
            }
            seen.push(&tool.target);
            files.push((tool.target.clone(), label.into()));
        }

        let mut panel = panel.child(session_row);
        panel = panel.child(info_heading(if files.is_empty() { "Files".into() } else { format!("Files · {}", files.len()) }, theme));
        if files.is_empty() {
            panel = panel.child(info_note("Nothing attached yet.", theme));
        }
        for (name, meta) in &files {
            panel = panel.child(info_file_row(name, meta, theme));
        }

        let activity = self.state.activity(id);
        let activity = &activity[activity.len().saturating_sub(ACTIVITY_LINES)..];
        let working = self.state.busy(id);
        panel = panel.child(info_heading(if working { "Activity · working".into() } else { "Activity".into() }, theme));
        if activity.is_empty() {
            panel = panel.child(info_note("No reply yet in this run of ArchonUI.", theme));
        }
        for entry in activity {
            panel = panel.child(
                div().px(px(10.)).py(px(1.)).text_size(px(12.)).text_color(theme.fg.to_hsla()).child(format!("• {entry}")),
            );
        }
        Some(panel.into_any_element())
    }

    /// The empty state: logo, brand and a "New session" button, centred in `area`. Spec: layout.md §3.
    fn render_empty(&self, area: Rect, cx: &mut Context<Self>) -> AnyElement {
        let theme = &self.theme;
        let logo = logo_bytes(theme.logo)
            .map(|bytes| img(Arc::new(Image::from_bytes(ImageFormat::Png, bytes.to_vec()))).size(px(72.)).opacity(0.8));
        let button = div()
            .id("tile-empty-new")
            .px(px(22.))
            .py(px(12.))
            .rounded(px(8.))
            .bg(theme.acc.to_hsla())
            .text_color(theme.on.to_hsla())
            .font_weight(FontWeight::BOLD)
            .cursor_pointer()
            .child("New session")
            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.new_session_split(cx)));
        div()
            .absolute()
            .left(px(area.x))
            .top(px(area.y))
            .w(px(area.w))
            .h(px(area.h))
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap(px(18.))
            .children(logo)
            .child(
                div()
                    .font_family(theme.display_font.clone())
                    .text_size(px(56.))
                    .text_color(theme.fg.to_hsla())
                    .child(self.state.settings.app_name.clone()),
            )
            .child(button)
            .into_any_element()
    }
}

/// A rectangle placed absolutely at its design-px position.
fn at(r: Rect) -> Div {
    div().absolute().left(px(r.x)).top(px(r.y)).w(px(r.w)).h(px(r.h))
}

/// A muted section heading inside the session info popover, over a `line` divider.
fn info_heading(text: String, theme: &Theme) -> Div {
    div()
        .mt(px(6.))
        .px(px(10.))
        .pt(px(6.))
        .pb(px(4.))
        .border_t_1()
        .border_color(theme.line.to_hsla())
        .text_size(px(11.5))
        .text_color(theme.muted.to_hsla())
        .child(text)
}

/// A muted single line inside the session info popover.
fn info_note(text: &'static str, theme: &Theme) -> Div {
    div().px(px(10.)).py(px(4.)).text_size(px(11.5)).text_color(theme.muted.to_hsla()).child(text)
}

/// A read-only file row inside the session info popover: name on the left, size or tool on the right.
fn info_file_row(name: &str, meta: &str, theme: &Theme) -> Div {
    div()
        .flex()
        .items_center()
        .gap(px(6.))
        .px(px(10.))
        .py(px(4.))
        .child(div().flex_1().min_w_0().truncate().text_size(px(12.)).child(name.to_string()))
        .child(div().flex_none().text_size(px(11.5)).text_color(theme.muted.to_hsla()).child(meta.to_string()))
}

/// A 28×28 header button with a `line2` hover fill and a tooltip.
fn header_button(id: String, glyph: &'static str, hint: &'static str, theme: &Theme) -> Stateful<Div> {
    let line2 = theme.line2.to_hsla();
    div()
        .id(ElementId::Name(id.into()))
        .flex()
        .items_center()
        .justify_center()
        .size(px(28.))
        .rounded(px(6.))
        .text_size(px(17.))
        .text_color(theme.muted.to_hsla())
        .cursor_pointer()
        .hover(move |d| d.bg(line2))
        .tooltip(tooltip(hint, TooltipColors::of(theme)))
        .child(glyph)
}

/// Whether `ev` is the settings binding `spec`, such as `ctrl-shift-enter` or `ctrl-b`.
///
/// Modifier prefixes are peeled off the front, so a `-` key (`ctrl--`) still parses. `super`, `win` and
/// `cmd` all name the platform key (GPUI writes `super-` on Linux), and `fn` the function key.
pub(in crate::ui) fn keystroke_matches(spec: &str, ev: &Keystroke) -> bool {
    let spec = spec.to_ascii_lowercase();
    let (mut ctrl, mut alt, mut shift, mut platform, mut function) = (false, false, false, false, false);
    let mut rest = spec.as_str();
    loop {
        let Some((prefix, tail)) = rest.split_once('-') else { break };
        if tail.is_empty() {
            break;
        }
        let flag = match prefix {
            "ctrl" | "control" => &mut ctrl,
            "alt" | "option" => &mut alt,
            "shift" => &mut shift,
            "cmd" | "super" | "win" | "platform" => &mut platform,
            "fn" | "function" => &mut function,
            _ => break,
        };
        *flag = true;
        rest = tail;
    }
    let m = ev.modifiers;
    !rest.is_empty()
        && m.control == ctrl
        && m.alt == alt
        && m.shift == shift
        && m.platform == platform
        && m.function == function
        && ev.key == rest
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ks(s: &str) -> Keystroke {
        Keystroke::parse(s).unwrap()
    }

    #[test]
    fn keystroke_matches_modifiers_exactly() {
        assert!(keystroke_matches("ctrl-shift-w", &ks("ctrl-shift-w")));
        assert!(keystroke_matches("Ctrl-Shift-W", &ks("ctrl-shift-w")));
        assert!(!keystroke_matches("ctrl-shift-w", &ks("ctrl-w")));
        assert!(!keystroke_matches("ctrl-w", &ks("ctrl-shift-w")));
        assert!(keystroke_matches("ctrl-shift-enter", &ks("ctrl-shift-enter")));
        assert!(keystroke_matches("f5", &ks("f5")));
        assert!(!keystroke_matches("", &ks("f5")));
    }

    #[test]
    fn keystroke_matches_super_and_fn() {
        for spec in ["super-k", "win-k", "cmd-k"] {
            assert!(keystroke_matches(spec, &ks("cmd-k")), "{spec}");
            assert!(!keystroke_matches(spec, &ks("k")), "{spec} needs the platform key");
        }
        assert!(keystroke_matches("fn-f1", &ks("fn-f1")));
        assert!(!keystroke_matches("fn-f1", &ks("f1")));
        assert!(!keystroke_matches("f1", &ks("fn-f1")));
    }

    #[test]
    fn keystroke_matches_a_minus_key() {
        assert!(keystroke_matches("ctrl--", &ks("ctrl--")));
        assert!(!keystroke_matches("ctrl--", &ks("ctrl-=")));
        assert!(keystroke_matches("-", &ks("-")));
    }
}

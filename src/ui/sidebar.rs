//! Left sidebar: expanded (300 px) and collapsed rail (52 px). Spec: docs/design/layout.md §2,
//! icons.md §2, tools-and-behaviour.md §B4–B8 and §C items 1–16.
//!
//! The Sessions view lives in `sidebar/sessions.rs`, the Projects view in `sidebar/projects.rs`,
//! the right-click menu in `sidebar/menu.rs` and the drawn glyphs in `sidebar/icons.rs`.

mod icons;
mod menu;
mod projects;
pub(in crate::ui) mod sessions;

use super::{
    anim::{self, Animator, Lerp},
    glass::{self, Backdrop},
    shell::{Shell, RAIL},
    widgets::{
        popover::{tooltip, TooltipColors},
        text_input::{TextInput, TextInputConfig, TextInputEvent},
    },
};
use crate::{
    app::{
        workspace::LeftTab,
    },
    store,
};
use gpui::{
    div, img, prelude::*, transparent_black, AnyElement, ClickEvent, Context, ElementId, Entity, Hsla,
    ImageSource, KeyDownEvent, ObjectFit, Pixels, Point, Stateful, Subscription, Task, Window,
};
use crate::ui::scale::px;
use icons::Glyph;
use menu::MenuView;
use projects::ProjectInfo;
use sessions::Entry;
use std::{collections::HashMap, path::PathBuf, time::{Duration, Instant}};

/// Below this width the sidebar draws the rail, so a collapsing sidebar switches mid-animation.
const RAIL_BELOW: f32 = 120.;
/// Brand row height, px: 10 px top padding + the 34 px collapse button, no bottom padding or border
/// (layout.md §2.1).
const BRAND_H: f32 = 44.;
/// How often the relative times ("4m", "1h") are redrawn.
const CLOCK: Duration = Duration::from_secs(30);

/// Sidebar view state that isn't persisted.
#[derive(Default)]
pub struct SidebarUi {
    /// The search box and its observer, made on first render.
    search: Option<(Entity<TextInput>, Subscription)>,
    rename: Option<Rename>,
    menu: Option<Menu>,
    /// Git branch and language per project folder, probed off the UI thread once.
    probes: HashMap<PathBuf, Probe>,
    /// Redraws every `CLOCK` so relative times stay current.
    clock: Option<Task<()>>,
    /// Focus the shell on the next frame, after an input inside the sidebar went away.
    refocus: bool,
    /// The surface's fill, easing to the background style's `side` colour (layout.md §6 #2).
    fill: Option<Animator<Hsla>>,
    /// The right edge is being dragged to resize the sidebar.
    resizing: bool,
}

/// What a row, card or menu acts on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Target {
    Session(String),
    Project(String),
}

/// An inline name field in place of a session title or project name.
struct Rename {
    target: Target,
    input: Entity<TextInput>,
    _subs: Vec<Subscription>,
}

/// The right-click menu, anchored at window px `at`.
struct Menu {
    target: Target,
    at: Point<Pixels>,
    view: MenuView,
}

/// A project folder's probe: running in the background, or finished.
enum Probe {
    Running,
    Done(ProjectInfo),
}

impl Shell {
    pub(super) fn render_sidebar(&mut self, backdrop: &Backdrop, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        if std::mem::take(&mut self.sidebar.refocus) {
            window.focus(&self.focus);
        }
        if self.sidebar.clock.is_none() {
            self.sidebar.clock = Some(clock(cx));
        }
        if self.sidebar.search.is_none() {
            let input = cx.new(|cx| TextInput::new(search_config(), window, cx));
            let observe = cx.observe(&input, |_, _, cx| cx.notify());
            self.sidebar.search = Some((input, observe));
        }
        let theme = self.theme.clone();
        let regions = self.regions(window);
        let (w, h) = (regions.left.w, regions.left.h);
        let surfaces = self.state.settings.background.surfaces(&theme, self.effects());
        let content = if w < RAIL_BELOW { self.sidebar_rail(cx) } else { self.sidebar_expanded(cx) };
        let menu = self.menu(cx);
        let target = surfaces.side.to_hsla();
        let now = Instant::now();
        let animator = self.sidebar.fill.get_or_insert_with(|| Animator::new(target, anim::SIDEBAR_COLLAPSE));
        animator.set_target(target, now);
        let (fill, fill_running) = (animator.value(now), animator.is_running(now));
        // The glass fills the box; the content is the open width or RAIL wide and clipped to the box, so
        // the width animation reveals it. The menu sits outside the clip so it can leave the sidebar.
        let glass = glass::glass_surface(
            backdrop,
            glass::Rect { x: 0., y: 0., w, h },
            0.,
            fill,
            transparent_black(),
            surfaces.side_blur.unwrap_or(0.),
        );
        let sidebar = div()
            .absolute()
            .left_0()
            .top_0()
            .w(px(w))
            .h(px(h))
            .child(glass)
            .child(div().absolute().top_0().right_0().w(px(1.)).h_full().bg(theme.line2.to_hsla()))
            .child(div().absolute().top_0().left_0().size_full().overflow_hidden().child(content))
            .when(self.state.workspace.left_open && w >= RAIL_BELOW, |d| {
                d.child(self.sidebar_resize_edge(w, h, cx)).child(self.sidebar_resize_listener(cx))
            })
            .children(menu)
            .into_any_element();
        anim::tick(window, fill_running);
        sidebar
    }

    fn sidebar_expanded(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let body = match self.state.workspace.left_tab {
            LeftTab::Sessions => self.session_list(cx),
            LeftTab::Projects => self.projects_view(cx),
        };
        let brand = self.brand_row(cx);
        let tabs = self.left_tabs(cx);
        let footer = self.footer(cx);
        div()
            .w(px(self.state.workspace.left_width))
            .h_full()
            .flex()
            .flex_col()
            .child(brand)
            .child(tabs)
            .child(
                div()
                    .id("sidebar-scroll")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .px(px(14.))
                    .py(px(16.))
                    .flex()
                    .flex_col()
                    .gap(px(10.))
                    .child(body),
            )
            .child(footer)
            .into_any_element()
    }

    /// Collapse button, logo and app name (layout.md §2.1 rows 1–1c).
    fn brand_row(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let logo = self.identity_logo();
        let name = self.state.settings.app_name.clone();
        let theme = &self.theme;
        let collapse = icon_button("sidebar-collapse", 34., icons::glyph(Glyph::ChevronLeft, 20., 1.6, theme.muted.to_hsla()), theme)
            .tooltip(tooltip("Collapse sidebar", TooltipColors::of(theme)))
            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.set_left_open(false, cx)));
        div()
            .h(px(BRAND_H))
            .flex_none()
            .flex()
            .items_center()
            .gap(px(10.))
            .pl(px(9.))
            .pt(px(10.))
            .pr(px(14.))
            .child(collapse)
            .when_some(logo, |row, logo| {
                row.child(img(logo).w(px(28.)).h(px(28.)).flex_none().object_fit(ObjectFit::Contain))
            })
            .child(
                div()
                    .min_w_0()
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_ellipsis()
                    .font_family(theme.display_font.clone())
                    .text_size(px(24.))
                    .line_height(px(24.))
                    .child(name),
            )
            .child(super::rpanel::window_drag_area())
            .into_any_element()
    }

    /// The app identity: the chosen icon (a custom file, a preset icon, or the theme's logo).
    fn identity_logo(&mut self) -> Option<ImageSource> {
        self.theme.logo_at(28.).map(Into::into)
    }

    /// Sessions | Projects segmented control (layout.md §2.1 row 2).
    fn left_tabs(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = &self.theme;
        let active = self.state.workspace.left_tab;
        div()
            .flex()
            .gap(px(4.))
            .mx(px(14.))
            .mt(px(14.))
            .p(px(4.))
            .rounded(px(9.))
            .bg(theme.line.to_hsla())
            .child(tab_button("left-tab-sessions", "Sessions", active == LeftTab::Sessions, LeftTab::Sessions, theme, cx))
            .child(tab_button("left-tab-projects", "Projects", active == LeftTab::Projects, LeftTab::Projects, theme, cx))
    }

    pub(super) fn select_left_tab(&mut self, tab: LeftTab, cx: &mut Context<Self>) {
        self.finish_rename(true, cx);
        self.sidebar.menu = None;
        self.state.workspace.left_tab = tab;
        self.save(cx);
    }

    /// Settings and Share (layout.md §2.3).
    fn footer(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = &self.theme;
        let muted = theme.muted.to_hsla();
        div()
            .flex()
            .gap(px(6.))
            .px(px(14.))
            .py(px(12.))
            .border_t_1()
            .border_color(theme.line.to_hsla())
            .child(
                div()
                    .id("sidebar-settings")
                    .flex_1()
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .px(px(12.))
                    .py(px(9.))
                    .rounded(px(8.))
                    .text_size(px(13.))
                    .text_color(theme.fg.to_hsla())
                    .cursor_pointer()
                    .hover(|s| s.bg(theme.line2.to_hsla()))
                    .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.open_settings(cx)))
                    .child(icons::glyph(Glyph::Gear, 18., 1.5, muted))
                    .child("Settings")
                    .when(self.settings.update.dot(), |d| {
                        d.child(super::settings::update_dot(theme)).tooltip(tooltip("Update available", TooltipColors::of(theme)))
                    }),
            )
            .child(
                div()
                    .id("sidebar-share")
                    .w(px(38.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(px(8.))
                    .cursor_pointer()
                    .hover(|s| s.bg(theme.line2.to_hsla()))
                    .tooltip(tooltip("Share", TooltipColors::of(theme)))
                    .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.open_share(cx)))
                    .child(icons::glyph(Glyph::Share, 18., 1.5, muted)),
            )
    }

    /// The 52 px rail: expand, new session, a sigil per session on its band, then Share and
    /// Settings (layout.md §2.4).
    fn sidebar_rail(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let theme = self.theme.clone();
        let colours = TooltipColors::of(&theme);
        let (fg, muted) = (theme.fg.to_hsla(), theme.muted.to_hsla());
        let order: Vec<Entry> = self.session_entries("", store::now()).into_iter().flat_map(|(_, rows)| rows).collect();
        let sessions: Vec<AnyElement> = order
            .iter()
            .enumerate()
            .map(|(i, entry)| {
                let colour = if entry.focused {
                    theme.acc2.to_hsla()
                } else if entry.open {
                    fg
                } else {
                    muted
                };
                let id = entry.id.clone();
                div()
                    .id(ElementId::Name(format!("rail-{}", entry.id).into()))
                    .relative()
                    .w(px(30.))
                    .h(px(30.))
                    .flex_none()
                    .rounded(px(8.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .bg(theme.band(i, entry.focused).to_hsla())
                    .tooltip(tooltip(entry.title.clone(), colours))
                    .on_click(cx.listener(move |this, ev: &ClickEvent, window, cx| {
                        this.open_session_click(&id, ev.modifiers().alt, window, cx)
                    }))
                    .child(crate::ui::sigil::sigil_element(entry.icon, px(16.), colour))
                    .child(
                        div()
                            .absolute()
                            .right(px(1.))
                            .bottom(px(1.))
                            .child(sessions::dot(&theme, entry.state, theme.panel.to_hsla())),
                    )
                    .into_any_element()
            })
            .collect();
        // The expand button sits in a header block as tall as the expanded header, so it doesn't move.
        let header = div()
            .w_full()
            .h(px(self.header_height()))
            .flex_none()
            .flex()
            .items_center()
            .justify_center()
            .border_b_1()
            .border_color(theme.line.to_hsla())
            .child(
                icon_button("rail-expand", 30., icons::glyph(Glyph::ChevronRight, 16., 1.6, muted), &theme)
                    .tooltip(tooltip("Expand sidebar", colours))
                    .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.set_left_open(true, cx))),
            );
        div()
            .w(px(RAIL))
            .h_full()
            .flex()
            .flex_col()
            .items_center()
            .gap(px(4.))
            .pb(px(10.))
            .child(header)
            .child(
                icon_button("rail-new-session", 30., icons::glyph(Glyph::Plus, 16., 1.4, fg), &theme)
                    .mt(px(6.))
                    .tooltip(tooltip("New session", colours))
                    .on_click(cx.listener(|this, _: &ClickEvent, window, cx| this.new_session(window, cx))),
            )
            .child(div().w(px(16.)).h(px(1.)).my(px(6.)).flex_none().bg(theme.line2.to_hsla()))
            .child(
                div()
                    .id("rail-sessions")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap(px(6.))
                    .py(px(2.))
                    .children(sessions),
            )
            .child(
                icon_button("rail-share", 30., icons::glyph(Glyph::Share, 16., 1.5, fg), &theme)
                    .tooltip(tooltip("Share", colours))
                    .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.open_share(cx))),
            )
            .child(
                icon_button("rail-settings", 30., icons::glyph(Glyph::Gear, 16., 1.5, fg), &theme)
                    .tooltip(tooltip("Settings", colours))
                    .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.open_settings(cx))),
            )
            .into_any_element()
    }

    /// The 10 px strip centred on the sidebar's right edge, with the same 3×44 grip as the tools panel's.
    /// Drag resizes (220–520 px); double-click resets to 300.
    fn sidebar_resize_edge(&self, w: f32, h: f32, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = &self.theme;
        let (lit, idle) = (theme.acc2.to_hsla(), theme.line2.to_hsla());
        let resizing = self.sidebar.resizing;
        div()
            .id("sidebar-resize-edge")
            .absolute()
            .left(px(w - 5.))
            .top_0()
            .w(px(10.))
            .h(px(h))
            .flex()
            .items_center()
            .justify_center()
            .cursor_col_resize()
            .tooltip(tooltip("Drag to resize · double-click to reset", TooltipColors::of(theme)))
            .on_mouse_down(
                gpui::MouseButton::Left,
                cx.listener(|this, event: &gpui::MouseDownEvent, _, cx| {
                    if event.click_count == 2 {
                        this.set_left_width(crate::app::workspace::LEFT_WIDTH_DEFAULT, false, cx);
                    } else {
                        this.sidebar.resizing = true;
                        cx.notify();
                    }
                }),
            )
            .child(anim::animated(
                ("sidebar-grip", 0usize),
                anim::GRIP,
                if resizing { 1.0 } else { 0.0 },
                move |t| div().w(px(3.)).h(px(44.)).rounded(px(2.)).bg(Hsla::lerp(&idle, &lit, t)).into_any_element(),
            ))
    }

    /// Window-level listeners for a sidebar edge drag, so it keeps tracking outside the 10 px strip.
    fn sidebar_resize_listener(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let shell = cx.entity();
        gpui::canvas(
            |_, _, _| (),
            move |_, _, window, _| {
                let drag = shell.clone();
                window.on_mouse_event(move |event: &gpui::MouseMoveEvent, phase, _, cx| {
                    if phase != gpui::DispatchPhase::Bubble || !drag.read(cx).sidebar.resizing {
                        return;
                    }
                    // The sidebar's left edge is the window's left edge.
                    let width = crate::ui::scale::design(event.position.x);
                    drag.update(cx, |this, cx| this.set_left_width(width, true, cx));
                });
                let release = shell.clone();
                window.on_mouse_event(move |_: &gpui::MouseUpEvent, phase, _, cx| {
                    if phase != gpui::DispatchPhase::Bubble || !release.read(cx).sidebar.resizing {
                        return;
                    }
                    release.update(cx, |this, cx| {
                        this.sidebar.resizing = false;
                        this.save(cx);
                    });
                });
            },
        )
        .absolute()
        .w(px(0.))
        .h(px(0.))
    }

    /// Sets the expanded width, clamped to its range: instantly while dragging, animated otherwise.
    fn set_left_width(&mut self, width: f32, instant: bool, cx: &mut Context<Self>) {
        let (min, max) = crate::app::workspace::LEFT_WIDTH;
        self.state.workspace.left_width = width.clamp(min, max);
        self.widths_changed(instant, cx);
    }

    /// Widens or collapses the sidebar, animated (`left_open` is saved with the workspace).
    fn set_left_open(&mut self, open: bool, cx: &mut Context<Self>) {
        self.state.workspace.left_open = open;
        self.sidebar.menu = None;
        self.widths_changed(false, cx);
    }

    /// Starts an inline rename. The field takes focus; Enter or blur saves, Escape cancels.
    pub(super) fn start_rename(&mut self, target: Target, window: &mut Window, cx: &mut Context<Self>) {
        let text = match &target {
            Target::Session(id) => self.state.session(id).map(|s| s.title.clone()).unwrap_or_default(),
            Target::Project(id) => self.state.project(id).map(|p| p.name.clone()).unwrap_or_default(),
        };
        self.finish_rename(true, cx);
        let input = cx.new(|cx| TextInput::new(rename_config(), window, cx));
        input.update(cx, |input, cx| input.set_text(&text, cx));
        input.read(cx).focus(window);
        let submit = cx.subscribe(&input, |this, _, event: &TextInputEvent, cx| match event {
            TextInputEvent::Changed => {}
            TextInputEvent::Submit | TextInputEvent::Blur => this.finish_rename(true, cx),
        });
        let observe = cx.observe(&input, |_, _, cx| cx.notify());
        self.sidebar.menu = None;
        self.sidebar.rename = Some(Rename { target, input, _subs: vec![submit, observe] });
        cx.notify();
    }

    /// Ends the inline rename, saving the text when `commit` is set and it isn't blank.
    pub(super) fn finish_rename(&mut self, commit: bool, cx: &mut Context<Self>) {
        let Some(rename) = self.sidebar.rename.take() else { return };
        let text = rename.input.read(cx).text().trim().to_string();
        if commit && !text.is_empty() {
            match rename.target {
                Target::Session(id) => {
                    if self.rename_remote(&id, &text, cx) {
                        // Renamed when its device answers.
                    } else if self.state.rename_session(&id, &text) {
                        self.share_renamed(&id, &text);
                        self.persist_sidebar(cx);
                    }
                }
                Target::Project(id) => {
                    if let Some(path) = self.state.project(&id).map(|p| p.path.clone()) {
                        self.state.update_project(&id, &text, path);
                        self.save(cx);
                    }
                }
            }
        }
        cx.notify();
    }

    /// Writes the sessions as well as the config. For changes to a chat's own fields.
    pub(super) fn persist_sidebar(&mut self, cx: &mut Context<Self>) {
        self.flush_sessions(cx);
        self.save(cx);
    }

    /// Keys for the sidebar's focused inputs: Escape clears the search, or cancels a rename.
    /// Returns true when consumed.
    pub(super) fn sidebar_key(&mut self, ev: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if ev.keystroke.key != "escape" {
            return false;
        }
        let renaming = self.sidebar.rename.as_ref().is_some_and(|r| r.input.read(cx).focus_handle().is_focused(window));
        if renaming {
            self.finish_rename(false, cx);
            self.sidebar.refocus = true;
            return true;
        }
        if let Some((input, _)) = &self.sidebar.search {
            let input = input.clone();
            if input.read(cx).focus_handle().is_focused(window) && !input.read(cx).text().is_empty() {
                input.update(cx, |input, cx| input.set_text("", cx));
                return true;
            }
        }
        false
    }
}

/// A `side` × `side` ghost icon button: 34 in the expanded header, 30 on the rail.
fn icon_button(id: &'static str, side: f32, glyph: impl IntoElement, theme: &crate::ui::theme::Theme) -> Stateful<gpui::Div> {
    let line2 = theme.line2.to_hsla();
    div()
        .id(id)
        .w(px(side))
        .h(px(side))
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(7.))
        .cursor_pointer()
        .hover(move |s| s.bg(line2))
        .child(glyph)
}

/// One Sessions | Projects option. The selected one has the `line2` fill and `fg` text.
fn tab_button(
    id: &'static str,
    label: &'static str,
    active: bool,
    tab: LeftTab,
    theme: &crate::ui::theme::Theme,
    cx: &mut Context<Shell>,
) -> impl IntoElement {
    div()
        .id(id)
        .flex_1()
        .flex()
        .justify_center()
        .py(px(8.))
        .rounded(px(6.))
        .text_size(px(12.))
        .cursor_pointer()
        .when(active, |d| d.bg(theme.line2.to_hsla()).text_color(theme.fg.to_hsla()))
        .when(!active, |d| d.text_color(theme.muted.to_hsla()))
        .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| this.select_left_tab(tab, cx)))
        .child(label)
}

fn search_config() -> TextInputConfig {
    TextInputConfig {
        placeholder: "Search sessions".into(),
        font_size: px(13.),
        line_height: px(18.),
        ..Default::default()
    }
}

fn rename_config() -> TextInputConfig {
    TextInputConfig {
        placeholder: "Name".into(),
        font_size: px(13.5),
        line_height: px(20.),
        max_len: Some(120),
        ..Default::default()
    }
}

/// Redraws the sidebar every `CLOCK`, so relative times ("4m", "1h") move on without input.
fn clock(cx: &mut Context<Shell>) -> Task<()> {
    cx.spawn(async move |this, cx| loop {
        cx.background_executor().timer(CLOCK).await;
        if this.update(cx, |_, cx| cx.notify()).is_err() {
            return;
        }
    })
}

impl Shell {
    /// Settings opens from the footer and the rail.
    pub(super) fn open_settings(&mut self, cx: &mut Context<Self>) {
        self.settings_open(cx);
    }

    /// Share opens from the footer and the rail.
    pub(super) fn open_share(&mut self, cx: &mut Context<Self>) {
        self.share_open(cx);
    }
}

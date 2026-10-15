//! Right tools panel frame: 480 px (360–820) or 52 px rail, tabs, window controls.
//! Spec: docs/design/layout.md §4, tools-and-behaviour.md §A0.

mod icons;

use super::{
    anim::{self, Animator, Lerp},
    glass::{glass_surface, Backdrop, Rect as GlassRect},
    shell::Shell,
    theme::{Rgba, Theme},
    tiling::Rect,
    widgets::popover::{tooltip, TooltipColors},
};
use crate::app::workspace::{RightTab, RIGHT_WIDTH, RIGHT_WIDTH_DEFAULT};
use gpui::{
    canvas, div, prelude::*, transparent_black, AnyElement, ClickEvent, Context, CursorStyle, Div, DispatchPhase,
    Hsla, KeyDownEvent, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, ResizeEdge, Stateful, Window,
};
use crate::ui::scale::px;
use icons::Glyph;
use std::time::Instant;

/// Panel view state that isn't persisted.
#[derive(Default)]
pub struct PanelUi {
    /// Set from the edge strip's mouse-down until mouse-up.
    resizing: bool,
    /// The surface's fill, easing to the background style's `side` colour (layout.md §6 #3).
    fill: Option<Animator<Hsla>>,
}

const TABS: [RightTab; 5] = [RightTab::Browser, RightTab::Ide, RightTab::Files, RightTab::Git, RightTab::Terminal];

/// Width of the invisible frame that starts a window resize.
const WINDOW_EDGE: f32 = 6.;
/// Tab-bar height, px: 10 px padding + the 34 px collapse button (layout.md §4).
const TABBAR_H: f32 = 54.;

fn tab_label(tab: RightTab) -> &'static str {
    match tab {
        RightTab::Browser => "Browser",
        RightTab::Ide => "IDE",
        RightTab::Files => "Files",
        RightTab::Git => "Git",
        RightTab::Terminal => "Terminal",
    }
}

fn tab_glyph(tab: RightTab) -> Glyph {
    match tab {
        RightTab::Browser => Glyph::Browser,
        RightTab::Ide => Glyph::Ide,
        RightTab::Files => Glyph::Files,
        RightTab::Git => Glyph::Git,
        RightTab::Terminal => Glyph::Terminal,
    }
}

/// An empty flexible area that drags the window; double-click toggles maximise.
/// Put it in a header or the rail where there are no controls.
pub(super) fn window_drag_area() -> Div {
    div().flex_1().min_w_0().min_h_0().on_mouse_down(MouseButton::Left, |event: &MouseDownEvent, window, _| {
        if event.click_count == 2 {
            window.zoom_window();
        } else {
            window.start_window_move();
        }
    })
}

/// A square icon button with the design's muted colour and `hover` (background, text) on hover.
fn chrome_button(
    theme: &Theme,
    id: &'static str,
    glyph: Glyph,
    side: f32,
    hover: (Rgba, Rgba),
    title: Option<&'static str>,
) -> Stateful<Div> {
    let (icon, stroke) = match glyph {
        Glyph::ChevronLeft | Glyph::ChevronRight => (20., 1.6),
        _ => (14., 1.5),
    };
    let (hover_bg, hover_fg) = hover;
    div()
        .id(id)
        .size(px(side))
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(6.))
        .cursor_pointer()
        .text_color(theme.muted.to_hsla())
        .hover(move |style| style.bg(hover_bg.to_hsla()).text_color(hover_fg.to_hsla()))
        .when_some(title, |button, title| button.tooltip(tooltip(title, TooltipColors::of(theme))))
        .child(icons::glyph(glyph, icon, stroke))
}

/// Minimize, Maximize and Close, in that order. Close hovers in the accent colour.
fn window_controls(theme: &Theme, side: f32) -> [Stateful<Div>; 3] {
    [
        chrome_button(theme, "rp-minimize", Glyph::Minimize, side, (theme.line2, theme.fg), Some("Minimize"))
            .on_click(|_: &ClickEvent, window, _| window.minimize_window()),
        chrome_button(theme, "rp-maximize", Glyph::Maximize, side, (theme.line2, theme.fg), Some("Maximize"))
            .on_click(|_: &ClickEvent, window, _| window.zoom_window()),
        chrome_button(theme, "rp-close", Glyph::Close, side, (theme.acc, theme.on), Some("Close"))
            .on_click(|_: &ClickEvent, window, _| window.remove_window()),
    ]
}

impl Shell {
    pub(super) fn render_panel(&mut self, backdrop: &Backdrop, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let rect = self.regions(window).right;
        let surfaces = self.state.settings.background.surfaces(&self.theme, self.effects());
        let open = self.state.workspace.right_open;
        let target = surfaces.side.to_hsla();
        let now = Instant::now();
        let animator = self.panel.fill.get_or_insert_with(|| Animator::new(target, anim::PANEL_COLLAPSE));
        animator.set_target(target, now);
        let (fill, fill_running) = (animator.value(now), animator.is_running(now));
        let glass = glass_surface(
            backdrop,
            GlassRect { x: rect.x, y: rect.y, w: rect.w, h: rect.h },
            0.,
            fill,
            transparent_black(),
            surfaces.side_blur.unwrap_or(0.),
        );
        let content = if open { self.render_expanded(window, cx) } else { self.render_rail(cx) };
        let frame = div()
            .absolute()
            .left(px(rect.x))
            .top(px(rect.y))
            .w(px(rect.w))
            .h(px(rect.h))
            .flex()
            .flex_col()
            .overflow_hidden()
            .when(self.state.settings.show_borders(), |d| d.border_l_1().border_color(self.theme.line2.to_hsla()))
            .child(content);
        let mut panel = div().child(glass).child(frame);
        if open {
            panel = panel.child(self.render_resize_edge(rect, cx)).child(self.resize_listener(cx));
        }
        anim::tick(window, fill_running);
        panel.into_any_element()
    }

    /// Keys for the panel's focused content. Returns true when consumed.
    pub(super) fn panel_key(&mut self, _ev: &KeyDownEvent, _window: &mut Window, _cx: &mut Context<Self>) -> bool {
        false
    }

    /// Invisible frame around the window that starts a native resize. Add it last in the shell's render.
    pub(super) fn render_window_edges(&self, window: &Window) -> AnyElement {
        let size = window.viewport_size();
        let (w, h) = (crate::ui::scale::design(size.width), crate::ui::scale::design(size.height));
        let e = WINDOW_EDGE;
        let edges = [
            (ResizeEdge::TopLeft, 0., 0., e, e, CursorStyle::ResizeUpLeftDownRight),
            (ResizeEdge::Top, e, 0., w - 2. * e, e, CursorStyle::ResizeUpDown),
            (ResizeEdge::TopRight, w - e, 0., e, e, CursorStyle::ResizeUpRightDownLeft),
            (ResizeEdge::Right, w - e, e, e, h - 2. * e, CursorStyle::ResizeLeftRight),
            (ResizeEdge::BottomRight, w - e, h - e, e, e, CursorStyle::ResizeUpLeftDownRight),
            (ResizeEdge::Bottom, e, h - e, w - 2. * e, e, CursorStyle::ResizeUpDown),
            (ResizeEdge::BottomLeft, 0., h - e, e, e, CursorStyle::ResizeUpRightDownLeft),
            (ResizeEdge::Left, 0., e, e, h - 2. * e, CursorStyle::ResizeLeftRight),
        ];
        div()
            .children(edges.map(|(edge, x, y, width, height, cursor)| {
                div()
                    .absolute()
                    .left(px(x))
                    .top(px(y))
                    .w(px(width))
                    .h(px(height))
                    .cursor(cursor)
                    .on_mouse_down(MouseButton::Left, move |_: &MouseDownEvent, window, _| {
                        window.start_window_resize(edge)
                    })
            }))
            .into_any_element()
    }

    /// The expanded panel: the tab header, then the active tab's body.
    fn render_expanded(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Div {
        let content = self.render_tab_content(self.state.workspace.right_tab, window, cx);
        let theme = &self.theme;
        let header = div()
            .h(px(TABBAR_H))
            .flex_none()
            .flex()
            .items_center()
            .gap(px(4.))
            .px(px(10.))
            .border_b_1()
            .border_color(theme.line.to_hsla())
            .children(TABS.iter().enumerate().map(|(i, &tab)| self.tab_button(i, tab, cx)))
            .child(window_drag_area())
            .child(
                chrome_button(theme, "rp-collapse", Glyph::ChevronRight, 34., (theme.line2, theme.fg), Some("Collapse"))
                    .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.set_right_open(false, cx))),
            )
            .child(div().w(px(1.)).h(px(16.)).mx(px(4.)).bg(theme.line2.to_hsla()))
            .children(window_controls(theme, 28.));
        let body = div().flex().flex_col().flex_1().min_h_0().child(content);
        div().flex().flex_col().size_full().child(header).child(body)
    }

    /// The 40 px rail: expand (in a header block as tall as the tab bar), the five tab icons, then the
    /// window controls at the bottom.
    fn render_rail(&self, cx: &Context<Self>) -> Div {
        let theme = &self.theme;
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
                chrome_button(theme, "rp-expand", Glyph::ChevronLeft, 30., (theme.line2, theme.fg), None)
                    .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.set_right_open(true, cx))),
            );
        div()
            .flex()
            .flex_col()
            .items_center()
            .gap(px(6.))
            .pb(px(10.))
            .size_full()
            .child(header)
            .children(TABS.iter().enumerate().map(|(i, &tab)| self.rail_tab_button(i, tab, cx)))
            .child(window_drag_area().w_full())
            .children(window_controls(theme, 26.))
    }

    /// A header tab: text label, `line2` background when active.
    fn tab_button(&self, index: usize, tab: RightTab, cx: &Context<Self>) -> Stateful<Div> {
        let theme = &self.theme;
        let active = self.state.workspace.right_tab == tab;
        div()
            .id(("rp-tab", index))
            .flex()
            .items_center()
            .px(px(8.))
            .py(px(7.))
            .rounded(px(6.))
            .text_size(px(12.5))
            .cursor_pointer()
            .when(active, |button| button.bg(theme.line2.to_hsla()).text_color(theme.fg.to_hsla()))
            .when(!active, |button| button.text_color(theme.muted.to_hsla()))
            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| this.select_tab(tab, cx)))
            .child(tab_label(tab))
    }

    /// A rail tab icon: 30 px with a 16 px glyph, `line2` background when active, with the tab name as its tooltip.
    fn rail_tab_button(&self, index: usize, tab: RightTab, cx: &Context<Self>) -> Stateful<Div> {
        let theme = &self.theme;
        let active = self.state.workspace.right_tab == tab;
        div()
            .id(("rp-rail-tab", index))
            .size(px(30.))
            .flex_none()
            .flex()
            .items_center()
            .justify_center()
            .rounded(px(7.))
            .cursor_pointer()
            .when(active, |button| button.bg(theme.line2.to_hsla()).text_color(theme.fg.to_hsla()))
            .when(!active, |button| button.text_color(theme.muted.to_hsla()))
            .hover(|style| style.text_color(theme.fg.to_hsla()))
            .tooltip(tooltip(tab_label(tab), TooltipColors::of(theme)))
            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| this.select_tab(tab, cx)))
            .child(icons::glyph(tab_glyph(tab), 16., 1.5))
    }

    /// The 6 px-wide strip on the panel's left edge, with a 3×44 grip. Drag resizes; double-click resets.
    fn render_resize_edge(&self, rect: Rect, cx: &Context<Self>) -> impl IntoElement {
        let theme = &self.theme;
        let (lit, idle) = (theme.acc2.to_hsla(), theme.line2.to_hsla());
        let resizing = self.panel.resizing;
        div()
            .id("rp-resize-edge")
            .absolute()
            .left(px(rect.x - 5.))
            .top(px(rect.y))
            .w(px(10.))
            .h(px(rect.h))
            .flex()
            .items_center()
            .justify_center()
            .cursor_col_resize()
            .tooltip(tooltip("Drag to resize · double-click to reset", TooltipColors::of(theme)))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, event: &MouseDownEvent, _, cx| {
                    if event.click_count == 2 {
                        this.set_panel_width(RIGHT_WIDTH_DEFAULT, false, cx);
                    } else {
                        this.panel.resizing = true;
                        cx.notify();
                    }
                }),
            )
            // The grip's background fades to `acc2` over 0.15 s while the edge is dragged (§6 #5).
            .child(anim::animated(
                ("rp-grip", 0usize),
                anim::GRIP,
                if resizing { 1.0 } else { 0.0 },
                move |t| div().w(px(3.)).h(px(44.)).rounded(px(2.)).bg(Hsla::lerp(&idle, &lit, t)).into_any_element(),
            ))
    }

    /// Window-level mouse listeners for an active edge drag. Registered on every paint, as the slider does,
    /// so they still fire when the pointer leaves the 10 px strip.
    fn resize_listener(&self, cx: &Context<Self>) -> impl IntoElement {
        let shell = cx.entity();
        canvas(
            |_, _, _| (),
            move |_, _, window, _| {
                let drag = shell.clone();
                window.on_mouse_event(move |event: &MouseMoveEvent, phase, window, cx| {
                    if phase != DispatchPhase::Bubble || !drag.read(cx).panel.resizing {
                        return;
                    }
                    // The panel's right edge is the window's right edge.
                    let width = crate::ui::scale::design(window.viewport_size().width - event.position.x);
                    drag.update(cx, |this, cx| this.set_panel_width(width, true, cx));
                });
                let release = shell.clone();
                window.on_mouse_event(move |_: &MouseUpEvent, phase, _, cx| {
                    if phase != DispatchPhase::Bubble || !release.read(cx).panel.resizing {
                        return;
                    }
                    release.update(cx, |this, cx| {
                        this.panel.resizing = false;
                        this.save(cx);
                    });
                });
            },
        )
        .absolute()
        .w(px(0.))
        .h(px(0.))
    }

    /// Opens the panel on `tab`, as clicking a tab does in the design. Selecting Git reads the
    /// repository again, since `git` changes outside the app are not watched.
    fn select_tab(&mut self, tab: RightTab, cx: &mut Context<Self>) {
        self.state.workspace.right_tab = tab;
        self.set_right_open(true, cx);
        if tab == RightTab::Git {
            self.refresh_git(cx);
        }
    }

    fn set_right_open(&mut self, open: bool, cx: &mut Context<Self>) {
        self.state.workspace.right_open = open;
        self.widths_changed(false, cx);
    }

    fn set_panel_width(&mut self, width: f32, instant: bool, cx: &mut Context<Self>) {
        self.state.workspace.right_width = width.clamp(RIGHT_WIDTH.0, RIGHT_WIDTH.1);
        self.widths_changed(instant, cx);
    }

    /// Placeholder body until each tab's content is ported: shows the tab name.
    pub(super) fn render_tab_content(&mut self, tab: RightTab, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        self.render_tab(tab, window, cx)
    }
}

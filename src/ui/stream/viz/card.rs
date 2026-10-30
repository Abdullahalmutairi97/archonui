//! One chart or diagram in chat, as a card: its kind and title, a switch between the views it allows,
//! and View source, Copy (PNG or SVG), Expand and Ask the agent. Charts are painted from `scene`;
//! diagrams come from `mermaid` as SVG and show as an image drawn at twice their size.

use super::{
    mermaid,
    scene::{self, Anchor, Ink, Shape},
    spec::{cell_text, fmt, mermaid_kind, Spec, Table, View},
};
use crate::ui::{
    composer::icons::stroke_icon,
    scale::{px, zoom},
    theme::{mix, Rgba, Theme, GREEN, RED},
    widgets::text_input::{text_input, TextInput, TextInputConfig, TextInputEvent},
};
use gpui::{
    canvas, deferred, div, img, prelude::*, AnyElement, App, Bounds, ClipboardItem, Context, Entity, Hsla, Image, ImageFormat,
    MouseMoveEvent, ObjectFit, PathBuilder, Pixels, Point, SharedString, Subscription, TextRun, Window,
};
use std::{cell::Cell, rc::Rc, sync::Arc};

/// The card's icons, on the 16-unit grid.
const SRC: &str = "M5.5 4.5L2 8l3.5 3.5M10.5 4.5L14 8l-3.5 3.5";
const COPY: &str = "M5.5 5.5h7v8h-7zM3.5 10.5v-8h7";
const EXPAND: &str = "M9.5 2.5h4v4M6.5 13.5h-4v-4M13.5 2.5L9 7M2.5 13.5L7 9";
const ASK: &str = "M8 2c.5 3.2 2.8 5.5 6 6-3.2.5-5.5 2.8-6 6-.5-3.2-2.8-5.5-6-6 3.2-.5 5.5-2.8 6-6z";
const CLOSE: &str = "M4.5 4.5l7 7M11.5 4.5l-7 7";

/// What a card asks of the stream around it.
pub struct VizActions {
    /// Sends a message in the card's chat.
    pub send: Box<dyn Fn(String, &mut App)>,
    pub toast: Box<dyn Fn(String, &mut App)>,
    /// Opens a card over the whole window, or closes the one open.
    pub expand: Box<dyn Fn(Option<Entity<VizCard>>, &mut App)>,
}

/// What is under the pointer.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Hover {
    None,
    Index(usize),
    Point(usize, usize),
}

/// A diagram's drawing.
#[derive(Clone)]
enum Drawing {
    Waiting,
    Ready { key: u64, svg: String, image: Arc<Image>, size: (f32, f32) },
    Failed { key: u64, error: String },
}

pub struct VizCard {
    mermaid: bool,
    title: String,
    body: String,
    spec: Result<Spec, String>,
    view: View,
    show_source: bool,
    menu: bool,
    ask: Option<(Entity<TextInput>, Subscription)>,
    hover: Hover,
    sort: Option<(usize, bool)>,
    theme: Theme,
    ink: Ink,
    big: bool,
    streaming: bool,
    /// The plot's bounds when last painted, for the pointer and for copying at the size on screen.
    plot: Rc<Cell<Bounds<Pixels>>>,
    drawing: Drawing,
    /// A word in the header after a copy: "PNG copied".
    note: Option<SharedString>,
    actions: Rc<VizActions>,
}

impl VizCard {
    pub fn new(mermaid: bool, title: String, body: String, theme: &Theme, actions: Rc<VizActions>, big: bool, cx: &mut Context<Self>) -> Self {
        let spec = if mermaid { Err(String::new()) } else { Spec::parse(&body) };
        let view = spec.as_ref().ok().and_then(|s| s.views().first().copied()).unwrap_or(View::Table);
        let mut card = Self {
            mermaid,
            title,
            body,
            spec,
            view,
            show_source: false,
            menu: false,
            ask: None,
            hover: Hover::None,
            sort: None,
            theme: theme.clone(),
            ink: Ink::of(theme),
            big,
            streaming: false,
            plot: Rc::new(Cell::new(Bounds::default())),
            drawing: Drawing::Waiting,
            note: None,
            actions,
        };
        card.draw_diagram(cx);
        card.screenshot_switches(cx);
        card
    }

    /// Developer switches for screenshots, like `ARCHONUI_OPEN`: `ARCHONUI_VIZ_VIEW=bar` opens charts
    /// that allow it in that view, `ARCHONUI_VIZ_HOVER=2` hovers position or slice 2, and
    /// `ARCHONUI_VIZ_EXPAND=<part of a title>` opens that card over the window. Unset, nothing happens.
    fn screenshot_switches(&mut self, cx: &mut Context<Self>) {
        if self.big {
            return;
        }
        if let (Ok(name), Ok(spec)) = (std::env::var("ARCHONUI_VIZ_VIEW"), &self.spec) {
            if let Some(v) = spec.views().into_iter().find(|v| v.name() == name) {
                self.view = v;
            }
        }
        if let Some(i) = std::env::var("ARCHONUI_VIZ_HOVER").ok().and_then(|i| i.parse().ok()) {
            self.hover = if self.view == View::Scatter { Hover::Point(0, i) } else { Hover::Index(i) };
        }
        if let Ok(part) = std::env::var("ARCHONUI_VIZ_EXPAND") {
            if !part.is_empty() && self.shown_title().to_lowercase().contains(&part.to_lowercase()) {
                let card = cx.entity().downgrade();
                cx.defer(move |cx| {
                    let _ = card.update(cx, |this, cx| this.expand(cx));
                });
            }
        }
    }

    /// Keeps the card in step with its message and the theme. A growing message re-reads the block;
    /// a new theme redraws a diagram.
    pub fn update(&mut self, mermaid: bool, title: &str, body: &str, streaming: bool, theme: &Theme, cx: &mut Context<Self>) {
        let mut changed = false;
        if self.body != body || self.mermaid != mermaid || self.title != title {
            self.mermaid = mermaid;
            self.title = title.to_string();
            self.body = body.to_string();
            self.spec = if mermaid { Err(String::new()) } else { Spec::parse(body) };
            if let Ok(spec) = &self.spec {
                if !spec.views().contains(&self.view) {
                    self.view = spec.views()[0];
                }
            }
            changed = true;
        }
        if &self.theme != theme {
            self.theme = theme.clone();
            self.ink = Ink::of(theme);
            changed = true;
        }
        if self.streaming != streaming {
            self.streaming = streaming;
            changed = true;
        }
        if changed {
            self.draw_diagram(cx);
            cx.notify();
        }
    }

    /// Asks for the diagram's drawing, unless the one shown is already for this source and theme.
    /// A message still streaming waits, so a half-written diagram is not drawn.
    fn draw_diagram(&mut self, cx: &mut Context<Self>) {
        if !self.mermaid || self.streaming || self.body.trim().is_empty() {
            return;
        }
        let key = mermaid::key(&self.body, &self.ink);
        match &self.drawing {
            Drawing::Ready { key: k, .. } | Drawing::Failed { key: k, .. } if *k == key => return,
            _ => {}
        }
        let (src, ink) = (self.body.clone(), self.ink.clone());
        cx.spawn(async move |this, cx| {
            let drawn = cx
                .background_executor()
                .spawn(async move {
                    let svg = mermaid::render(&src, &ink)?;
                    let (svg, size) = fit_svg(&svg, ink.bg);
                    let png = scene::svg_to_png(&svg)?;
                    Ok::<_, String>((svg, size, png))
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                // A newer source or theme asked again while this one drew.
                if mermaid::key(&this.body, &this.ink) != key {
                    return;
                }
                this.drawing = match drawn {
                    Ok((svg, size, png)) => Drawing::Ready { key, svg, image: Arc::new(Image::from_bytes(ImageFormat::Png, png)), size },
                    Err(error) => Drawing::Failed { key, error },
                };
                cx.notify();
            });
        })
        .detach();
    }

    fn kind(&self) -> String {
        if self.mermaid {
            return mermaid_kind(&self.body).to_string();
        }
        if self.spec.is_ok() {
            self.view.kind().to_string()
        } else {
            "Chart".to_string()
        }
    }

    fn shown_title(&self) -> String {
        match &self.spec {
            Ok(spec) if !self.mermaid => spec.title().to_string(),
            _ => self.title.clone(),
        }
    }

    /// The plot's height for the current view, in design px.
    fn plot_height(&self) -> f32 {
        match (self.view, self.big) {
            (View::Pie | View::Donut, false) => 240.,
            (View::Pie | View::Donut, true) => 520.,
            (View::Scatter, false) => 280.,
            (_, false) => 260.,
            (_, true) => 560.,
        }
    }

    /// The shapes of the current chart view in a `w` × `h` box.
    fn shapes(&self, w: f32, h: f32, hover: Hover) -> Vec<Shape> {
        self.clone_for_paint().shapes(w, h, hover)
    }

    /// What the pointer at `at` (design px inside the plot) is over.
    fn hit(&self, w: f32, h: f32, at: (f32, f32)) -> Hover {
        let Ok(spec) = &self.spec else { return Hover::None };
        let found = match (self.view, spec) {
            (View::Bar | View::Line | View::Area, _) => spec.as_xy().and_then(|xy| scene::xy_hit(xy.x.len(), w, at.0)).map(Hover::Index),
            (View::Pie | View::Donut, Spec::Pie(p)) => scene::pie_hit(p, self.view == View::Donut, w, h, at.0, at.1).map(Hover::Index),
            (View::Scatter, Spec::Scatter(s)) => scene::scatter_hit(s, w, h, at.0, at.1).map(|(j, i)| Hover::Point(j, i)),
            _ => None,
        };
        found.unwrap_or(Hover::None)
    }

    /// The card as an SVG document, at the size it is shown.
    fn svg(&self) -> Option<String> {
        if self.mermaid {
            return match &self.drawing {
                Drawing::Ready { svg, .. } => Some(svg.clone()),
                _ => None,
            };
        }
        let spec = self.spec.as_ref().ok()?;
        let shown = self.plot.get().size.width;
        let w = if shown > gpui::px(0.) { f32::from(shown) / zoom() } else if self.big { 1240. } else { 720. };
        let (shapes, h) = match self.view {
            View::Table => scene::table(&spec.as_table(), w, &self.ink),
            View::Stats => match spec {
                Spec::Stats(s) => scene::stats(s, w, &self.ink),
                _ => return None,
            },
            _ => {
                let h = self.plot_height();
                (self.shapes(w, h, Hover::None), h)
            }
        };
        Some(scene::to_svg(&shapes, w, h, &self.ink))
    }

    fn flash(&mut self, note: &str, cx: &mut Context<Self>) {
        self.note = Some(note.to_string().into());
        cx.notify();
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(std::time::Duration::from_millis(1600)).await;
            let _ = this.update(cx, |this, cx| {
                this.note = None;
                cx.notify();
            });
        })
        .detach();
    }

    fn copy_svg(&mut self, cx: &mut Context<Self>) {
        self.menu = false;
        match self.svg() {
            Some(svg) => {
                cx.write_to_clipboard(ClipboardItem::new_string(svg));
                self.flash("SVG copied", cx);
            }
            None => self.flash("Nothing to copy", cx),
        }
    }

    /// Draws the card at twice its size and puts the PNG on the clipboard. GPUI only writes text there
    /// on Linux, so `wl-copy` or `xclip` carries the image.
    fn copy_png(&mut self, cx: &mut Context<Self>) {
        self.menu = false;
        let Some(svg) = self.svg() else { return self.flash("Nothing to copy", cx) };
        cx.notify();
        cx.spawn(async move |this, cx| {
            let copied = cx.background_executor().spawn(async move { scene::svg_to_png(&svg).and_then(|png| clipboard_png(&png)) }).await;
            let _ = this.update(cx, |this, cx| match copied {
                Ok(()) => this.flash("PNG copied", cx),
                Err(e) => {
                    this.flash("Copy failed", cx);
                    (this.actions.toast)(format!("Could not copy the PNG: {e}"), cx);
                }
            });
        })
        .detach();
    }

    fn toggle_ask(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.ask.take().is_some() {
            cx.notify();
            return;
        }
        let config = TextInputConfig {
            placeholder: format!("Ask the agent to change this {}…", self.kind().to_lowercase()).into(),
            font_size: px(13.),
            line_height: px(18.),
            ..Default::default()
        };
        let input = cx.new(|cx| TextInput::new(config, window, cx));
        input.read(cx).focus(window);
        let subscription = cx.subscribe(&input, |this, _, event: &TextInputEvent, cx| {
            if let TextInputEvent::Submit = event {
                this.send_ask(cx);
            }
        });
        self.ask = Some((input, subscription));
        cx.notify();
    }

    fn send_ask(&mut self, cx: &mut Context<Self>) {
        let Some((input, _)) = &self.ask else { return };
        let text = input.read(cx).text().trim().to_string();
        if text.is_empty() {
            return;
        }
        let title = Some(self.shown_title()).filter(|t| !t.is_empty()).unwrap_or_else(|| self.kind());
        self.ask = None;
        if self.big {
            (self.actions.expand)(None, cx);
        }
        (self.actions.send)(format!("Update the \"{title}\" visual: {text}"), cx);
        cx.notify();
    }

    fn expand(&mut self, cx: &mut Context<Self>) {
        // The card under the overlay keeps no hover or menu, which would draw over it.
        self.hover = Hover::None;
        self.menu = false;
        let (mermaid, title, body, theme, actions, view) = (self.mermaid, self.title.clone(), self.body.clone(), self.theme.clone(), self.actions.clone(), self.view);
        let big = cx.new(|cx| {
            let mut card = VizCard::new(mermaid, title, body, &theme, actions, true, cx);
            card.view = view;
            card
        });
        (self.actions.expand)(Some(big), cx);
    }
}

impl Render for VizCard {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = self.theme.clone();
        let muted = t.muted.to_hsla();
        let kind = self.kind();
        let views = self.spec.as_ref().map(Spec::views).unwrap_or_default();
        let button = |id: &'static str, glyph: &'static str, active: bool| {
            let fg = if active { t.fg } else { t.muted };
            div()
                .id(id)
                .size(px(26.))
                .flex_none()
                .rounded(px(6.))
                .flex()
                .items_center()
                .justify_center()
                .cursor_pointer()
                .when(active, |d| d.bg(t.line2.to_hsla()))
                .hover(|s| s.bg(t.line.to_hsla()))
                .child(stroke_icon(glyph, 14., 1.4, fg.to_hsla()))
        };
        let mut seg = div().flex().flex_none().gap(px(2.)).p(px(2.)).rounded(px(7.)).bg(t.line.to_hsla());
        for (i, v) in views.iter().copied().enumerate() {
            let on = v == self.view;
            seg = seg.child(
                div()
                    .id(("viz-view", i))
                    .px(px(9.))
                    .py(px(3.))
                    .rounded(px(5.))
                    .text_size(px(11.5))
                    .cursor_pointer()
                    .when(on, |d| d.bg(t.line2.to_hsla()).text_color(t.fg.to_hsla()))
                    .when(!on, |d| d.text_color(muted))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.view = v;
                        this.hover = Hover::None;
                        cx.notify();
                    }))
                    .child(capitalised(v.name())),
            );
        }
        let menu = self.menu.then(|| {
            let item = |id: &'static str, label: &'static str| {
                div()
                    .id(id)
                    .px(px(10.))
                    .py(px(7.))
                    .rounded(px(5.))
                    .text_size(px(12.5))
                    .text_color(t.fg.to_hsla())
                    .cursor_pointer()
                    .hover(|s| s.bg(t.line2.to_hsla()))
                    .child(label)
            };
            deferred(
                div()
                    .absolute()
                    .top(px(30.))
                    .right(px(28.))
                    .min_w(px(130.))
                    .flex()
                    .flex_col()
                    .p(px(4.))
                    .rounded(px(8.))
                    .bg(mix(t.bg, t.fg, 0.1).to_hsla())
                    .border_1()
                    .border_color(t.line2.to_hsla())
                    .shadow(vec![gpui::BoxShadow { color: gpui::hsla(0., 0., 0., 0.5), offset: gpui::point(px(0.), px(10.)), blur_radius: px(30.), spread_radius: px(0.) }])
                    .child(item("viz-copy-png", "Copy as PNG").on_click(cx.listener(|this, _, _, cx| this.copy_png(cx))))
                    .child(item("viz-copy-svg", "Copy as SVG").on_click(cx.listener(|this, _, _, cx| this.copy_svg(cx)))),
            )
            .with_priority(2)
        });
        let header = div()
            .flex()
            .items_center()
            .gap(px(10.))
            .when(self.big, |d| d.pt(px(14.)).pb(px(14.)).pl(px(22.)).pr(px(16.)))
            .when(!self.big, |d| d.pt(px(7.)).pb(px(7.)).pl(px(14.)).pr(px(8.)))
            .border_b_1()
            .border_color(t.line.to_hsla())
            .child(div().flex_none().text_size(px(11.)).text_color(t.acc2.to_hsla()).whitespace_nowrap().child(spaced_caps(&kind)))
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .overflow_hidden()
                    .text_ellipsis()
                    .whitespace_nowrap()
                    .text_size(px(if self.big { 15. } else { 13. }))
                    .text_color(t.fg.to_hsla())
                    .child(self.shown_title()),
            )
            .when_some(self.note.clone(), |d, note| d.child(div().flex_none().text_size(px(11.5)).text_color(t.acc2.to_hsla()).child(note)))
            .when(views.len() > 1, |d| d.child(seg))
            .child(
                div()
                    .relative()
                    .flex()
                    .flex_none()
                    .gap(px(2.))
                    .child(button("viz-src", SRC, self.show_source).on_click(cx.listener(|this, _, _, cx| {
                        this.show_source = !this.show_source;
                        cx.notify();
                    })))
                    .child(button("viz-copy", COPY, self.menu).on_click(cx.listener(|this, _, _, cx| {
                        this.menu = !this.menu;
                        cx.notify();
                    })))
                    .when(!self.big, |d| d.child(button("viz-expand", EXPAND, false).on_click(cx.listener(|this, _, _, cx| this.expand(cx)))))
                    .child(button("viz-ask", ASK, self.ask.is_some()).on_click(cx.listener(|this, _, window, cx| this.toggle_ask(window, cx))))
                    .when(self.big, |d| {
                        d.child(button("viz-close", CLOSE, false).on_click(cx.listener(|this, _, _, cx| (this.actions.expand)(None, cx))))
                    })
                    .children(menu),
            );

        let body = self.body_view(cx);
        let source = self.show_source.then(|| {
            let fence = if self.mermaid { "```mermaid" } else { "```viz" };
            let text = if self.mermaid {
                self.body.clone()
            } else {
                serde_json::from_str::<serde_json::Value>(&self.body).ok().and_then(|v| serde_json::to_string_pretty(&v).ok()).unwrap_or_else(|| self.body.clone())
            };
            div()
                .id("viz-source")
                .max_h(px(240.))
                .overflow_y_scroll()
                .px(px(14.))
                .py(px(10.))
                .border_t_1()
                .border_color(t.line.to_hsla())
                .font_family(t.code_font.clone())
                .text_size(px(12.))
                .line_height(px(19.2))
                .text_color(muted)
                .children(format!("{fence}\n{text}\n```").lines().map(|l| div().whitespace_nowrap().child(if l.is_empty() { " ".to_string() } else { l.to_string() })).collect::<Vec<_>>())
        });
        let ask = self.ask.as_ref().map(|(input, _)| {
            div()
                .flex()
                .items_center()
                .gap(px(8.))
                .pl(px(14.))
                .pr(px(8.))
                .py(px(8.))
                .border_t_1()
                .border_color(t.line.to_hsla())
                .on_key_down(cx.listener(|this, e: &gpui::KeyDownEvent, _, cx| {
                    if e.keystroke.key == "escape" {
                        this.ask = None;
                        cx.stop_propagation();
                        cx.notify();
                    }
                }))
                .child(stroke_icon(ASK, 13., 1.4, t.acc2.to_hsla()))
                .child(div().flex_1().min_w(px(0.)).child(text_input(input, &t)))
                .child(
                    div()
                        .id("viz-send")
                        .px(px(11.))
                        .py(px(5.))
                        .rounded(px(6.))
                        .text_size(px(12.))
                        .bg(t.acc.to_hsla())
                        .text_color(t.on.to_hsla())
                        .cursor_pointer()
                        .on_click(cx.listener(|this, _, _, cx| this.send_ask(cx)))
                        .child("Send"),
                )
        });
        let bg = if self.big { mix(t.bg, t.fg, 0.04) } else { t.bg.alpha(0.72) };
        div()
            .id("viz-card")
            .flex()
            .flex_col()
            .w_full()
            .rounded(px(if self.big { 12. } else { 8. }))
            .border_1()
            .border_color(t.line2.to_hsla())
            .bg(bg.to_hsla())
            .font_family(t.chat_font.clone())
            .text_color(t.fg.to_hsla())
            .when(self.big, |d| {
                d.max_h_full().shadow(vec![gpui::BoxShadow {
                    color: gpui::hsla(0., 0., 0., 0.6),
                    offset: gpui::point(px(0.), px(40.)),
                    blur_radius: px(120.),
                    spread_radius: px(0.),
                }])
            })
            .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(header)
            // Over the window the card is at most 90% of its height, and the chart scrolls inside it.
            .child(div().id("viz-body").when(self.big, |d| d.flex_1().min_h(px(0.)).overflow_y_scroll()).child(body))
            .children(source)
            .children(ask)
    }
}

impl VizCard {
    /// The chart, table, stats or diagram under the header.
    fn body_view(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let t = self.theme.clone();
        let pad = |d: gpui::Div| if self.big { d.px(px(24.)).py(px(20.)) } else { d.pt(px(12.)).px(px(10.)).pb(px(6.)) };
        if self.mermaid {
            return pad(div()).child(self.diagram_view()).into_any_element();
        }
        let spec = match &self.spec {
            Ok(spec) => spec.clone(),
            Err(_) if self.streaming => return placeholder("Drawing…", &t),
            Err(e) => return draw_error("chart", e, &t),
        };
        match self.view {
            View::Table => self.table_view(&spec.as_table(), cx),
            View::Stats => match &spec {
                Spec::Stats(s) => div().p(px(if self.big { 20. } else { 12. })).child(stats_view(s, &t)).into_any_element(),
                _ => div().into_any_element(),
            },
            _ => pad(div()).child(self.plot_view(&spec, cx)).into_any_element(),
        }
    }

    /// A chart: its legend, the painted plot and the tooltip for what the pointer is over.
    fn plot_view(&mut self, spec: &Spec, cx: &mut Context<Self>) -> AnyElement {
        let t = self.theme.clone();
        let h = self.plot_height();
        let hover = self.hover;
        // One legend entry per series, shown only when there is more than one.
        let names: Vec<String> = match (self.view, spec) {
            (View::Scatter, Spec::Scatter(s)) => s.series.iter().map(|x| x.name.clone()).collect(),
            (View::Bar | View::Line | View::Area, _) => spec.as_xy().map(|xy| xy.series.into_iter().map(|x| x.name).collect()).unwrap_or_default(),
            _ => Vec::new(),
        };
        let legend: Vec<(String, Rgba)> = if names.len() > 1 { names.into_iter().enumerate().map(|(j, name)| (name, self.ink.series(j))).collect() } else { Vec::new() };
        // GPUI can't turn text, so a scatter plot's y label sits over the plot instead of beside it.
        let y_label = match (self.view, spec) {
            (View::Scatter, Spec::Scatter(s)) if !s.y_label.is_empty() => Some(s.y_label.clone()),
            _ => None,
        };
        let card = cx.entity().clone();
        let plot = self.plot.clone();
        let copy = self.clone_for_paint();
        let painter = canvas(
            move |bounds, _, _| plot.set(bounds),
            move |bounds, _, window, cx| {
                let z = zoom();
                let w = f32::from(bounds.size.width) / z;
                paint(&copy.shapes(w, h, hover), bounds.origin, z, &copy.ink, window, cx);
            },
        )
        .w_full()
        .h(px(h));
        let tip = self.tooltip(spec);
        let plot_cell = self.plot.clone();
        div()
            .flex()
            .flex_col()
            .when(!legend.is_empty() || y_label.is_some(), |d| {
                d.child(
                    div()
                        .flex()
                        .flex_wrap()
                        .gap_y(px(6.))
                        .px(px(4.))
                        .pb(px(8.))
                        .text_size(px(12.))
                        .text_color(t.muted.to_hsla())
                        .children(legend.into_iter().map(|(name, c)| {
                            div().flex().flex_none().items_center().mr(px(16.)).child(div().flex_none().size(px(8.)).rounded(px(2.)).mr(px(7.)).bg(c.to_hsla())).child(div().flex_none().whitespace_nowrap().child(name))
                        }))
                        .when_some(y_label, |d, l| d.child(div().ml_auto().child(format!("↑ {l}")))),
                )
            })
            .child(
                div()
                    .id("viz-plot")
                    .relative()
                    .w_full()
                    .on_mouse_move(move |e: &MouseMoveEvent, _, cx| {
                        let b = plot_cell.get();
                        let z = zoom();
                        let at = ((f32::from(e.position.x - b.origin.x)) / z, (f32::from(e.position.y - b.origin.y)) / z);
                        let w = f32::from(b.size.width) / z;
                        card.update(cx, |this, cx| {
                            let hit = if b.contains(&e.position) { this.hit(w, h, at) } else { Hover::None };
                            if hit != this.hover {
                                this.hover = hit;
                                cx.notify();
                            }
                        });
                    })
                    .on_hover(cx.listener(|this, over: &bool, _, cx| {
                        if !over && this.hover != Hover::None {
                            this.hover = Hover::None;
                            cx.notify();
                        }
                    }))
                    .child(painter)
                    .children(tip),
            )
            .into_any_element()
    }

    /// What a paint closure needs of the card: a copy without its entity handles.
    fn clone_for_paint(&self) -> PaintCopy {
        PaintCopy { spec: self.spec.clone(), view: self.view, ink: self.ink.clone() }
    }

    /// The tooltip for an XY position or a scatter point, placed like the prototype's.
    fn tooltip(&self, spec: &Spec) -> Option<AnyElement> {
        let t = &self.theme;
        let w = f32::from(self.plot.get().size.width) / zoom();
        if w <= 0. {
            return None;
        }
        let row = |name: String, value: String, c: Option<Rgba>| {
            div()
                .flex()
                .items_center()
                .justify_between()
                .gap(px(14.))
                .child(div().flex().items_center().when_some(c, |d, c| d.child(div().size(px(8.)).rounded(px(2.)).mr(px(7.)).bg(c.to_hsla()))).child(name))
                .child(div().font_weight(gpui::FontWeight::SEMIBOLD).child(value))
        };
        let (x, top, lines): (f32, f32, Vec<AnyElement>) = match (self.hover, self.view, spec) {
            (Hover::Index(i), View::Bar | View::Line | View::Area, _) => {
                let xy = spec.as_xy()?;
                let mut lines = vec![div().text_color(t.muted.to_hsla()).mb(px(3.)).child(xy.x.get(i)?.clone()).into_any_element()];
                for (j, s) in xy.series.iter().enumerate() {
                    lines.push(row(s.name.clone(), format!("{}{}", fmt(s.data.get(i).copied().unwrap_or(0.)), xy.unit), Some(self.ink.series(j))).into_any_element());
                }
                (scene::xy_centre(xy.x.len(), w, i), if xy.series.len() > 1 { 30. } else { 4. }, lines)
            }
            (Hover::Point(j, i), View::Scatter, Spec::Scatter(s)) => {
                let p = s.series.get(j)?.points.get(i)?;
                let (px_, _) = scene::scatter_at(s, w, self.plot_height(), (p.0, p.1));
                let label = |l: &str, d: &str| if l.is_empty() { d.to_string() } else { l.to_string() };
                let mut lines = Vec::new();
                if !p.2.is_empty() {
                    lines.push(div().mb(px(3.)).child(p.2.clone()).into_any_element());
                }
                lines.push(div().flex().gap(px(4.)).text_color(t.muted.to_hsla()).child(label(&s.x_label, "x")).child(div().text_color(t.fg.to_hsla()).font_weight(gpui::FontWeight::SEMIBOLD).child(fmt(p.0))).into_any_element());
                lines.push(div().flex().gap(px(4.)).text_color(t.muted.to_hsla()).child(label(&s.y_label, "y")).child(div().text_color(t.fg.to_hsla()).font_weight(gpui::FontWeight::SEMIBOLD).child(fmt(p.1))).into_any_element());
                (px_, 30., lines)
            }
            _ => return None,
        };
        let share = x / w;
        let tip = div()
            .absolute()
            .top(px(top))
            .px(px(11.))
            .py(px(8.))
            .rounded(px(8.))
            .bg(mix(t.bg, t.fg, 0.1).to_hsla())
            .border_1()
            .border_color(t.line2.to_hsla())
            .shadow(vec![gpui::BoxShadow { color: gpui::hsla(0., 0., 0., 0.45), offset: gpui::point(px(0.), px(8.)), blur_radius: px(24.), spread_radius: px(0.) }])
            .text_size(px(12.))
            .line_height(px(18.))
            .whitespace_nowrap()
            .children(lines);
        // Right of the point near the left edge, left of it near the right, centred between.
        let tip = if share > 0.7 {
            tip.right(px(w - x + 10.))
        } else if share < 0.3 {
            tip.left(px(x + 10.))
        } else {
            tip.left(px(x - 70.))
        };
        Some(deferred(tip).with_priority(1).into_any_element())
    }

    /// A sortable table: click a heading to sort by it, again to reverse.
    fn table_view(&mut self, table: &Table, cx: &mut Context<Self>) -> AnyElement {
        let t = self.theme.clone();
        let cols = table.columns.len().max(1);
        let numeric: Vec<bool> = (0..cols).map(|c| table.numeric(c)).collect();
        let cell = |d: gpui::Div, c: usize| {
            d.flex_1().min_w(px(0.)).px(px(12.)).py(px(8.)).overflow_hidden().text_ellipsis().whitespace_nowrap().when(numeric[c], |d| d.text_right())
        };
        let mut head = div().flex().bg(mix(t.bg, t.fg, 0.06).to_hsla()).border_b_1().border_color(t.line2.to_hsla());
        for (c, name) in table.columns.iter().enumerate() {
            let sorted = self.sort.filter(|(sc, _)| *sc == c);
            let label = match sorted {
                Some((_, true)) => format!("{name} ↑"),
                Some((_, false)) => format!("{name} ↓"),
                None => name.clone(),
            };
            head = head.child(
                cell(div(), c)
                    .id(("viz-sort", c))
                    .text_size(px(12.))
                    .font_weight(gpui::FontWeight::MEDIUM)
                    .text_color(if sorted.is_some() { t.fg.to_hsla() } else { t.muted.to_hsla() })
                    .cursor_pointer()
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.sort = match this.sort {
                            Some((sc, up)) if sc == c => Some((c, !up)),
                            _ => Some((c, true)),
                        };
                        cx.notify();
                    }))
                    .child(label),
            );
        }
        let rows = table.order(self.sort).into_iter().map(|r| {
            let mut line = div().flex().border_b_1().border_color(t.line.to_hsla());
            for c in 0..cols {
                let text = cell_text(table.rows[r].get(c));
                line = line.child(
                    cell(div(), c)
                        .when(numeric[c], |d| d.font_family(t.code_font.clone()).text_size(px(12.5)))
                        .when(!numeric[c], |d| d.text_size(px(13.)))
                        .child(text),
                );
            }
            line
        });
        div()
            .flex()
            .flex_col()
            .child(head)
            .child(div().id("viz-table").max_h(px(if self.big { 640. } else { 420. })).overflow_y_scroll().children(rows))
            .into_any_element()
    }

    fn diagram_view(&self) -> AnyElement {
        let t = &self.theme;
        match &self.drawing {
            Drawing::Waiting => placeholder("Drawing…", t),
            Drawing::Failed { error, .. } => draw_error("diagram", error, t),
            Drawing::Ready { image, size, .. } => {
                let max_h = if self.big { 1400. } else { 520. };
                let scale = (max_h / size.1).min(1.);
                let (w, h) = (size.0 * scale, size.1 * scale);
                // Wider than the card, it shrinks to fit rather than being cut off.
                div()
                    .id("viz-diagram")
                    .flex()
                    .justify_center()
                    .child(img(image.clone()).w_full().max_w(px(w)).h(px(h)).object_fit(ObjectFit::Contain))
                    .into_any_element()
            }
        }
    }
}

/// The spec and view a paint closure draws, without the card's entity handles.
struct PaintCopy {
    spec: Result<Spec, String>,
    view: View,
    ink: Ink,
}

impl PaintCopy {
    fn shapes(&self, w: f32, h: f32, hover: Hover) -> Vec<Shape> {
        let Ok(spec) = &self.spec else { return Vec::new() };
        let index = match hover {
            Hover::Index(i) => Some(i),
            _ => None,
        };
        let ink = &self.ink;
        match (self.view, spec) {
            (View::Bar | View::Line | View::Area, _) => spec.as_xy().map_or_else(Vec::new, |xy| scene::xy(&xy, self.view, w, h, ink, index)),
            (View::Pie | View::Donut, Spec::Pie(p)) => scene::pie(p, self.view == View::Donut, w, h, ink, index),
            (View::Scatter, Spec::Scatter(s)) => scene::scatter(s, w, h, ink, match hover {
                Hover::Point(j, i) => Some((j, i)),
                _ => None,
            }),
            _ => Vec::new(),
        }
    }
}

/// Paints `shapes`, laid out in design px, at `origin` and `z` window px per design px.
fn paint(shapes: &[Shape], origin: Point<Pixels>, z: f32, ink: &Ink, window: &mut Window, cx: &mut App) {
    let at = |p: (f32, f32)| gpui::point(origin.x + gpui::px(p.0 * z), origin.y + gpui::px(p.1 * z));
    for shape in shapes {
        match shape {
            Shape::Line { a, b, color, width, dashed } => {
                let mut path = PathBuilder::stroke(gpui::px(width * z));
                if *dashed {
                    path = path.dash_array(&[gpui::px(2. * z), gpui::px(4. * z)]);
                }
                path.move_to(at(*a));
                path.line_to(at(*b));
                if let Ok(path) = path.build() {
                    window.paint_path(path, color.to_hsla());
                }
            }
            Shape::Rect { x, y, w, h, radius, color, alpha } => {
                let bounds = Bounds::new(at((*x, *y)), gpui::size(gpui::px(w * z), gpui::px(h * z)));
                window.paint_quad(gpui::fill(bounds, color.alpha(color.a * alpha).to_hsla()).corner_radii(gpui::px(radius * z)));
            }
            Shape::Poly { points, closed, fill, stroke, alpha } => {
                let pts: Vec<Point<Pixels>> = points.iter().map(|p| at(*p)).collect();
                if pts.len() < 2 {
                    continue;
                }
                if let Some(f) = fill {
                    let mut path = PathBuilder::fill();
                    path.add_polygon(&pts, true);
                    if let Ok(path) = path.build() {
                        window.paint_path(path, f.alpha(f.a * alpha).to_hsla());
                    }
                }
                if let Some((c, w)) = stroke {
                    let mut path = PathBuilder::stroke(gpui::px(w * z));
                    path.add_polygon(&pts, *closed);
                    if let Ok(path) = path.build() {
                        window.paint_path(path, c.alpha(c.a * alpha).to_hsla());
                    }
                }
            }
            Shape::Fade { points, color, top, bottom, .. } => {
                let pts: Vec<Point<Pixels>> = points.iter().map(|p| at(*p)).collect();
                let mut path = PathBuilder::fill();
                path.add_polygon(&pts, true);
                if let Ok(path) = path.build() {
                    let fill = gpui::linear_gradient(
                        180.,
                        gpui::linear_color_stop(color.alpha(*top).to_hsla(), 0.),
                        gpui::linear_color_stop(color.alpha(*bottom).to_hsla(), 1.),
                    );
                    window.paint_path(path, fill);
                }
            }
            Shape::Circle { c, r, fill, stroke } => {
                let centre = at(*c);
                let rr = gpui::px(r * z);
                let bounds = Bounds::new(gpui::point(centre.x - rr, centre.y - rr), gpui::size(rr * 2., rr * 2.));
                let mut quad = gpui::fill(bounds, fill.to_hsla()).corner_radii(rr);
                if let Some((sc, w)) = stroke {
                    quad = quad.border_widths(gpui::px(w * z)).border_color(sc.to_hsla());
                }
                window.paint_quad(quad);
            }
            Shape::Text { x, y, text, size, color, anchor, display, upright } => {
                if !upright || text.is_empty() {
                    continue;
                }
                let family = if *display { ink.display_font.clone() } else { ink.ui_font.clone() };
                let run = TextRun { len: text.len(), font: gpui::font(family), color: color.to_hsla(), background_color: None, underline: None, strikethrough: None };
                let line = window.text_system().shape_line(SharedString::from(text.clone()), gpui::px(size * z), &[run], None);
                let width = f32::from(line.width) / z;
                let left = match anchor {
                    Anchor::Start => *x,
                    Anchor::Middle => x - width / 2.,
                    Anchor::End => x - width,
                };
                let ascent = line.ascent;
                let height = line.ascent + line.descent;
                let origin = gpui::point(origin.x + gpui::px(left * z), origin.y + gpui::px(y * z) - ascent);
                let _ = line.paint(origin, height, window, cx);
            }
        }
    }
}

/// Stat tiles: label, figure, change coloured by whether it is good news, and a sparkline.
fn stats_view(s: &super::spec::Stats, t: &Theme) -> AnyElement {
    let mut grid = div().flex().flex_wrap().gap(px(10.));
    for item in &s.items {
        let good = item.good();
        let line_colour: Hsla = if good { t.acc2.to_hsla() } else { RED.to_hsla() };
        let trend = item.trend.clone();
        let spark = (trend.len() > 1).then(|| {
            canvas(
                |_, _, _| (),
                move |bounds, _, window, _| {
                    let z = zoom();
                    let w = f32::from(bounds.size.width) / z;
                    let pts = scene::sparkline(&trend, 0., 0., w);
                    let mut path = PathBuilder::stroke(gpui::px(1.6 * z));
                    let mapped: Vec<Point<Pixels>> = pts.iter().map(|p| gpui::point(bounds.origin.x + gpui::px(p.0 * z), bounds.origin.y + gpui::px(p.1 * z))).collect();
                    path.add_polygon(&mapped, false);
                    if let Ok(path) = path.build() {
                        window.paint_path(path, line_colour);
                    }
                },
            )
            .w_full()
            .h(px(28.))
        });
        grid = grid.child(
            div()
                .flex_1()
                .flex_basis(px(150.))
                .min_w(px(150.))
                .flex()
                .flex_col()
                .gap(px(6.))
                .px(px(14.))
                .py(px(12.))
                .rounded(px(8.))
                .bg(t.line.to_hsla())
                .border_1()
                .border_color(t.line.to_hsla())
                .child(div().text_size(px(12.)).text_color(t.muted.to_hsla()).child(item.label.clone()))
                .child(
                    div()
                        .flex()
                        .items_baseline()
                        .gap(px(8.))
                        .child(div().font_family(t.display_font.clone()).text_size(px(28.)).line_height(px(28.)).child(item.value.clone()))
                        .when(!item.delta.is_empty(), |d| {
                            d.child(div().text_size(px(12.)).text_color(if good { GREEN.to_hsla() } else { RED.to_hsla() }).child(item.delta.clone()))
                        }),
                )
                .children(spark),
        );
    }
    grid.into_any_element()
}

/// The note in place of a chart or diagram that did not draw: what, and why.
fn draw_error(what: &str, error: &str, t: &Theme) -> AnyElement {
    div()
        .p(px(14.))
        .text_size(px(12.))
        .font_family(t.code_font.clone())
        .text_color(RED.to_hsla())
        .child(format!("Could not draw this {what}: {error}"))
        .into_any_element()
}

fn placeholder(text: &str, t: &Theme) -> AnyElement {
    div().h(px(160.)).flex().items_center().justify_center().text_size(px(12.)).text_color(t.muted.to_hsla()).child(text.to_string()).into_any_element()
}

fn capitalised(s: &str) -> String {
    let mut c = s.chars();
    c.next().map(|f| f.to_uppercase().chain(c).collect()).unwrap_or_default()
}

/// Capitals with a hair space between letters: GPUI has no letter-spacing.
fn spaced_caps(s: &str) -> String {
    s.chars().map(|c| c.to_uppercase().collect::<String>()).collect::<Vec<_>>().join("\u{200a}")
}

/// mermaid's SVG with a fixed size and the theme's background, so it draws the same outside a
/// browser. Returns the SVG and its size in px.
pub fn fit_svg(svg: &str, bg: Rgba) -> (String, (f32, f32)) {
    let Some(open_end) = svg.find('>') else { return (svg.to_string(), (720., 300.)) };
    let open = &svg[..open_end];
    let view_box: Vec<f32> = attr(open, "viewBox").map(|v| v.split([' ', ',']).filter_map(|n| n.trim().parse().ok()).collect()).unwrap_or_default();
    let (w, h) = match view_box.as_slice() {
        [_, _, w, h] if *w > 0. && *h > 0. => (*w, *h),
        _ => (attr(open, "width").and_then(|v| v.trim_end_matches("px").parse().ok()).unwrap_or(720.), attr(open, "height").and_then(|v| v.trim_end_matches("px").parse().ok()).unwrap_or(300.)),
    };
    // Drop the root's own size and style; set ours.
    let mut root = String::new();
    let mut rest = open;
    while let Some(i) = rest.find(|c: char| c.is_whitespace()) {
        root.push_str(&rest[..i]);
        rest = &rest[i..];
        let trimmed = rest.trim_start();
        let name_end = trimmed.find('=').unwrap_or(trimmed.len());
        let name = &trimmed[..name_end];
        let value_end = trimmed[name_end..]
            .strip_prefix("=\"")
            .and_then(|v| v.find('"').map(|e| name_end + 2 + e + 1))
            .unwrap_or(trimmed.len());
        if !matches!(name, "width" | "height" | "style") {
            root.push(' ');
            root.push_str(&trimmed[..value_end]);
        }
        rest = &trimmed[value_end..];
        if rest.trim().is_empty() {
            break;
        }
    }
    root.push_str(rest);
    let colour = scene::css(bg);
    let x0 = view_box.first().copied().unwrap_or(0.);
    let y0 = view_box.get(1).copied().unwrap_or(0.);
    let fixed = format!(
        r#"{root} width="{w}" height="{h}"><rect x="{x0}" y="{y0}" width="{w}" height="{h}" fill="{colour}"/>{}"#,
        &svg[open_end + 1..]
    );
    (fixed, (w, h))
}

fn attr<'a>(tag: &'a str, name: &str) -> Option<&'a str> {
    let key = format!(" {name}=\"");
    let start = tag.find(&key)? + key.len();
    let end = tag[start..].find('"')? + start;
    Some(&tag[start..end])
}

/// Puts PNG bytes on the system clipboard with `wl-copy` on Wayland, else `xclip`.
fn clipboard_png(png: &[u8]) -> Result<(), String> {
    use std::io::Write;
    let wayland = std::env::var_os("WAYLAND_DISPLAY").is_some();
    let (program, args): (&str, &[&str]) =
        if wayland { ("wl-copy", &["--type", "image/png"]) } else { ("xclip", &["-selection", "clipboard", "-t", "image/png", "-i"]) };
    let mut child = std::process::Command::new(program)
        .args(args)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(|e| format!("{program} is needed to copy images: {e}"))?;
    child.stdin.take().ok_or("no stdin")?.write_all(png).map_err(|e| e.to_string())?;
    // wl-copy and xclip stay behind to serve the clipboard; reap them on a thread.
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_diagram_gets_a_fixed_size_and_a_background() {
        let svg = r#"<svg id="m1" width="100%" xmlns="http://www.w3.org/2000/svg" style="max-width: 320px;" viewBox="-8 -8 320 140" role="graphics-document"><g/></svg>"#;
        let (fixed, size) = fit_svg(svg, Rgba::hex(0x0d0a08));
        assert_eq!(size, (320., 140.));
        assert!(!fixed.contains("100%") && !fixed.contains("max-width"), "{fixed}");
        assert!(fixed.contains(r#"width="320" height="140""#) && fixed.contains(r##"fill="#0d0a08""##) && fixed.contains("role=\"graphics-document\""), "{fixed}");
        assert!(scene::svg_to_png(&fixed).is_ok());
    }

    #[test]
    fn view_names_read_as_switch_labels() {
        assert_eq!(capitalised("donut"), "Donut");
        assert_eq!(spaced_caps("Bar"), "B\u{200a}A\u{200a}R");
    }
}

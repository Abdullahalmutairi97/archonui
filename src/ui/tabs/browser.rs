//! The Browser tab: a headless Chrome from `crate::preview`, drawn as frames under an address bar.
//! Spec: docs/design/tools-and-behaviour.md §A1. `WebView` is the page part, for the IDE tab to reuse.

use crate::{
    app::workspace::RightTab,
    preview::{self, Browser, BrowserEvent, Command, MouseKind},
    store,
    ui::{
        shell::Shell,
        tiles::keystroke_matches,
        widgets::{text_input, Button, TextInput, TextInputConfig, TextInputEvent},
    },
};
use gpui::{
    canvas, div, img, prelude::*, AnyElement, App, Bounds, Context, DispatchPhase, Entity, FocusHandle, ImageSource,
    Keystroke, KeyDownEvent, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, ObjectFit, Pixels, Point,
    RenderImage, ScrollWheelEvent, SharedString, StyledImage, Subscription, Task, Window,
};
use crate::ui::scale::px;
use std::{cell::Cell, io::Cursor, path::PathBuf, rc::Rc, sync::Arc};

const PLACEHOLDER: &str = "Websites your agent builds or starts open here. You can also type an address above.";

#[derive(Default)]
pub struct BrowserUi {
    view: WebView,
    address: Option<AddressBar>,
    /// The search engine prefix last given to `preview::set_search_engine`.
    search_prefix: &'static str,
}

/// The address bar's input, and the subscription that submits what is typed.
struct AddressBar {
    input: Entity<TextInput>,
    _submit: Subscription,
}

/// One headless Chrome and what its page shows. The Browser tab and the IDE tab each hold one.
#[derive(Default)]
pub(in crate::ui) struct WebView {
    browser: Option<Browser>,
    launching: bool,
    /// Commands to send once the browser is up, such as the first navigation.
    pending: Vec<Command>,
    url: String,
    title: String,
    loading: bool,
    /// Whether the page's history has a page before/after the current one.
    can_back: bool,
    can_forward: bool,
    pub(in crate::ui) error: Option<String>,
    /// Latest frame and its size in CSS pixels.
    frame: Option<(Arc<RenderImage>, f32, f32)>,
    /// Replaced frames whose GPU textures still need freeing.
    stale: Vec<Arc<RenderImage>>,
    /// Keystrokes go to the page once it has been clicked.
    page_focus: Option<FocusHandle>,
    /// Where the page area was last laid out, for mapping mouse positions.
    bounds: Rc<Cell<Bounds<Pixels>>>,
    /// Viewport last sent to the browser: width, height, scale.
    viewport: (u32, u32, f32),
    /// Visibility last sent to the browser, which starts hidden.
    sent_visible: Option<bool>,
    /// Last page point while the left button is held on the page.
    pressed: Option<(f32, f32)>,
    /// Launches the browser and forwards its events. Dropping it stops both.
    task: Option<Task<()>>,
}

impl WebView {
    /// Loads `url`, starting the browser if it isn't up. `select` finds this view on the shell again.
    /// `root` is the project folder: `file://` pages load only from inside it, and none load without one.
    pub(in crate::ui) fn navigate(
        &mut self,
        url: String,
        root: Option<PathBuf>,
        select: fn(&mut Shell) -> &mut WebView,
        cx: &mut Context<Shell>,
    ) {
        self.load(url, vec![Command::SetRoot(root)], select, cx);
    }

    /// `navigate`, with `cookie` (origin, name, value) set first. Pages that log in with a cookie need it
    /// before their first request.
    pub(in crate::ui) fn navigate_with_cookie(
        &mut self,
        url: String,
        cookie: (String, String, String),
        select: fn(&mut Shell) -> &mut WebView,
        cx: &mut Context<Shell>,
    ) {
        let (url_origin, name, value) = cookie;
        // code-server's page is served over http and never needs `file://`.
        self.load(url, vec![Command::SetRoot(None), Command::SetCookie { url: url_origin, name, value }], select, cx);
    }

    /// Sends `first` and then the navigation, or queues them until the browser is up.
    fn load(&mut self, url: String, first: Vec<Command>, select: fn(&mut Shell) -> &mut WebView, cx: &mut Context<Shell>) {
        self.url = url.clone();
        self.error = None;
        if let Some(browser) = &self.browser {
            for command in first {
                browser.send(command);
            }
            browser.send(Command::Navigate(url));
        } else {
            self.pending = first;
            self.pending.push(Command::Navigate(url));
            self.launch(select, cx);
        }
    }

    /// Stops the browser and forgets the page. Chrome shuts down in the background.
    pub(in crate::ui) fn stop(&mut self, cx: &mut Context<Shell>) {
        self.task = None;
        self.launching = false;
        self.pending.clear();
        self.url.clear();
        self.title.clear();
        self.loading = false;
        self.can_back = false;
        self.can_forward = false;
        self.error = None;
        self.pressed = None;
        self.sent_visible = None;
        if let Some(frame) = self.frame.take() {
            self.stale.push(frame.0);
        }
        if let Some(old) = self.browser.take() {
            cx.background_executor().spawn(async move { drop(old) }).detach();
        }
        cx.notify();
    }

    /// Starts Chrome in the background. Its pending commands go out once it is up.
    fn launch(&mut self, select: fn(&mut Shell) -> &mut WebView, cx: &mut Context<Shell>) {
        if self.launching || self.browser.is_some() {
            return;
        }
        self.launching = true;
        self.loading = true;
        self.error = None;
        self.task = Some(cx.spawn(async move |shell, cx| {
            let launched = cx.background_executor().spawn(async { Browser::launch() }).await;
            let events = shell
                .update(cx, |shell, cx| {
                    let view = select(shell);
                    view.launching = false;
                    cx.notify();
                    match launched {
                        Ok(browser) => {
                            // Size the page to the panel before the first load.
                            if view.viewport.0 > 0 {
                                let (width, height, scale) = view.viewport;
                                browser.send(Command::Resize { width, height, scale });
                            }
                            for command in view.pending.drain(..) {
                                browser.send(command);
                            }
                            view.sent_visible = None;
                            let events = browser.events.clone();
                            let copies = browser.copies.clone();
                            view.browser = Some(browser);
                            cx.spawn(async move |shell, cx| {
                                while let Ok(text) = copies.recv().await {
                                    let copied = shell.update(cx, |_, cx| {
                                        cx.write_to_clipboard(gpui::ClipboardItem::new_string(text))
                                    });
                                    if copied.is_err() {
                                        break;
                                    }
                                }
                            })
                            .detach();
                            Some(events)
                        }
                        Err(e) => {
                            view.loading = false;
                            view.error = Some(e);
                            None
                        }
                    }
                })
                .ok()
                .flatten();
            let Some(events) = events else { return };
            while let Ok(ev) = events.recv().await {
                if shell.update(cx, |shell, cx| select(shell).event(ev, cx)).is_err() {
                    break;
                }
            }
        }));
    }

    fn event(&mut self, ev: BrowserEvent, cx: &mut Context<Shell>) {
        match ev {
            BrowserEvent::Frame(frame, w, h) => {
                if let Some((old, _, _)) = self.frame.replace((frame, w, h)) {
                    self.stale.push(old);
                    if self.stale.len() > 8 {
                        self.stale.drain(..self.stale.len() - 8);
                    }
                }
            }
            BrowserEvent::Url(url) => {
                if url != "about:blank" {
                    self.url = url;
                }
                self.error = None;
            }
            BrowserEvent::Title(title) => self.title = title,
            BrowserEvent::History { back, forward } => {
                self.can_back = back;
                self.can_forward = forward;
            }
            BrowserEvent::Loading(loading) => self.loading = loading,
            BrowserEvent::Error(e) => self.error = Some(e),
            BrowserEvent::Closed => {
                self.sent_visible = None;
                // Chrome may take a moment to shut down; keep that off the UI thread.
                if let Some(old) = self.browser.take() {
                    cx.background_executor().spawn(async move { drop(old) }).detach();
                }
                self.loading = false;
                self.error = Some("The browser closed. Load a page to restart it.".into());
            }
        }
        cx.notify();
    }

    /// Per-frame upkeep: frees replaced frames, keeps the page sized to its area, and streams
    /// frames only while `visible`.
    pub(in crate::ui) fn sync(&mut self, visible: bool, window: &mut Window) {
        for frame in self.stale.drain(..) {
            let _ = window.drop_image(frame);
        }
        let b = self.bounds.get();
        let size = (
            f32::from(b.size.width).round() as u32,
            f32::from(b.size.height).round() as u32,
            window.scale_factor(),
        );
        if size.0 > 0 && size.1 > 0 && size != self.viewport {
            self.viewport = size;
            if let Some(browser) = &self.browser {
                browser.send(Command::Resize { width: size.0, height: size.1, scale: size.2 });
            }
        }
        if let Some(browser) = &self.browser {
            if self.sent_visible != Some(visible) {
                browser.send(Command::SetVisible(visible));
                self.sent_visible = Some(visible);
            }
        }
    }

    fn send(&self, command: Command) {
        if let Some(browser) = &self.browser {
            browser.send(command);
        }
    }

    /// Mouse position in the page's CSS pixels, if it is over the page.
    fn page_point(&self, pos: Point<Pixels>) -> Option<(f32, f32)> {
        let b = self.bounds.get();
        b.contains(&pos).then(|| (f32::from(pos.x - b.origin.x), f32::from(pos.y - b.origin.y)))
    }

    /// Page point for `pos`, pulled inside the page's edges.
    fn point_clamped(&self, pos: Point<Pixels>) -> (f32, f32) {
        let b = self.bounds.get();
        (
            f32::from(pos.x - b.origin.x).clamp(0., f32::from(b.size.width)),
            f32::from(pos.y - b.origin.y).clamp(0., f32::from(b.size.height)),
        )
    }

    /// Sends a mouse event at `pos`. Pages expect the pointer to arrive before it presses.
    fn mouse(&self, kind: MouseKind, pos: Point<Pixels>, right: bool, clicks: u32) {
        let Some((x, y)) = self.page_point(pos) else { return };
        if matches!(kind, MouseKind::Down) {
            self.send(Command::Mouse { kind: MouseKind::Move, x, y, right: false, clicks: 0 });
        }
        self.send(Command::Mouse { kind, x, y, right, clicks });
    }

    /// Keeps a press that started on the page moving after the pointer leaves it.
    fn drag_move(&mut self, pos: Point<Pixels>) {
        let (x, y) = self.point_clamped(pos);
        self.pressed = Some((x, y));
        self.send(Command::Mouse { kind: MouseKind::Move, x, y, right: false, clicks: 0 });
    }

    /// Ends a press that started on the page, wherever the button comes up.
    fn release(&mut self, pos: Point<Pixels>, clicks: u32) {
        if self.pressed.take().is_none() {
            return;
        }
        let (x, y) = self.point_clamped(pos);
        self.send(Command::Mouse { kind: MouseKind::Up, x, y, right: false, clicks });
    }

    /// Ends a held press where it is, for when the page loses the keyboard mid-press.
    pub(in crate::ui) fn cancel_press(&mut self) {
        if let Some((x, y)) = self.pressed.take() {
            self.send(Command::Mouse { kind: MouseKind::Up, x, y, right: false, clicks: 1 });
        }
    }
}

fn browser_view(shell: &mut Shell) -> &mut WebView {
    &mut shell.tabs.browser.view
}

fn address_config() -> TextInputConfig {
    TextInputConfig {
        placeholder: "Search or type an address".into(),
        font_size: px(12.5),
        line_height: px(18.),
        ..Default::default()
    }
}

impl Shell {
    pub(in crate::ui) fn render_browser(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let theme = self.theme.clone();
        let input = self.address_input(window, cx);
        // The address bar shows the page's address until the user types in it.
        let url = self.tabs.browser.view.url.clone();
        if !input.read(cx).focus_handle().is_focused(window) && input.read(cx).text() != url {
            input.update(cx, |input, cx| input.set_text(&url, cx));
        }

        let view = &self.tabs.browser.view;
        let has_page = view.browser.is_some();
        let status: Option<SharedString> = if view.launching {
            Some("Starting the browser…".into())
        } else if view.loading {
            Some("Loading…".into())
        } else if !view.title.is_empty() && view.title != view.url && !view.url.is_empty() {
            Some(view.title.clone().into())
        } else {
            None
        };
        let error = view.error.clone();

        let shell = cx.entity();
        let nav = |id: &'static str, glyph: &'static str, command: fn() -> Command, enabled: bool, reason: &'static str| {
            let button = Button::icon(id, glyph, &theme);
            if !has_page {
                return button.disabled("No page open");
            }
            if !enabled {
                return button.disabled(reason);
            }
            let shell = shell.clone();
            button.on_click(move |_, cx| shell.update(cx, |this, _| this.tabs.browser.view.send(command())))
        };
        let open = {
            let button = Button::icon("browser-open", "↗", &theme);
            if url.is_empty() {
                button.disabled("No page open")
            } else {
                button.on_click(move |_, cx| cx.open_url(&url))
            }
        };

        let controls = div()
            .flex()
            .items_center()
            .gap(px(6.))
            .px(px(12.))
            .py(px(10.))
            .child(nav("browser-back", "‹", || Command::Back, view.can_back, "No earlier page"))
            .child(nav("browser-forward", "›", || Command::Forward, view.can_forward, "No later page"))
            .child(nav("browser-reload", "↻", || Command::Reload, true, "No page open"))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .px(px(12.))
                    .py(px(7.))
                    .rounded(px(7.))
                    .bg(theme.line.to_hsla())
                    .child(div().text_size(px(11.)).text_color(theme.alt.to_hsla()).child("●"))
                    .child(div().flex_1().min_w_0().child(text_input(&input, &theme))),
            )
            .child(open);

        let page = self.web_page("browser-page", browser_view, PLACEHOLDER, cx);

        div()
            .size_full()
            .min_h_0()
            .flex()
            .flex_col()
            .child(controls)
            .children(status.map(|s| {
                div().px(px(16.)).pb(px(6.)).text_xs().text_color(theme.muted.to_hsla()).truncate().child(s)
            }))
            .children(error.map(|e| {
                div()
                    .mx(px(12.))
                    .mb(px(8.))
                    .px(px(10.))
                    .py(px(6.))
                    .rounded(px(6.))
                    .bg(theme.deep.to_hsla())
                    .text_xs()
                    // `on` is for text on the bright accent; on `deep` it is dark on dark, so the body colour reads.
                    .text_color(theme.fg.to_hsla())
                    .child(e)
            }))
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .flex_col()
                    .mx(px(12.))
                    .mb(px(12.))
                    .rounded(px(8.))
                    .overflow_hidden()
                    .bg(theme.bg.to_hsla())
                    .border_1()
                    .border_color(theme.line2.to_hsla())
                    .child(page),
            )
            .into_any_element()
    }

    /// A web page's area: its latest frame, or `placeholder` before one. Clicks, drags and wheel
    /// scrolls go to the page. Keys do too while it has focus (see `web_key`). `id` is unique in the panel.
    pub(in crate::ui) fn web_page(
        &mut self,
        id: &'static str,
        select: fn(&mut Shell) -> &mut WebView,
        placeholder: &'static str,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = self.theme.clone();
        let shell = cx.entity();
        let view = select(self);
        let focus = view.page_focus.get_or_insert_with(|| cx.focus_handle()).clone();
        let bounds = view.bounds.clone();
        let frame = view.frame.clone().filter(|_| !view.url.is_empty());
        let click_focus = focus.clone();

        div()
            .id(id)
            .track_focus(&focus)
            .flex_1()
            .min_h_0()
            .relative()
            .overflow_hidden()
            .map(|d| match frame {
                // Absolute, so the frame's size never feeds back into the page area's layout.
                Some((frame, w, h)) => d.child(
                    img(ImageSource::Render(frame))
                        .absolute()
                        .top_0()
                        .left_0()
                        // The frame's size is in window px (the page's CSS px), not design px.
                        .w(gpui::px(w))
                        .h(gpui::px(h))
                        .object_fit(ObjectFit::Fill),
                ),
                None => d.child(
                    div()
                        .size_full()
                        .flex()
                        .items_center()
                        .justify_center()
                        .p_6()
                        .child(
                            div()
                                .w_full()
                                .text_center()
                                .text_sm()
                                .text_color(theme.muted.to_hsla())
                                .child(placeholder),
                        ),
                ),
            })
            // Records where the page is, so mouse positions map into it and its viewport stays in sync.
            // Window-level listeners follow a press that leaves the page; they are registered on every paint.
            .child(
                canvas(
                    move |b, _, _| bounds.set(b),
                    move |_, _, window, _| {
                        let moving = shell.clone();
                        window.on_mouse_event(move |ev: &MouseMoveEvent, phase, _, cx| {
                            if phase == DispatchPhase::Bubble {
                                moving.update(cx, |this, cx| {
                                    let view = select(this);
                                    if view.pressed.is_some() {
                                        view.drag_move(ev.position);
                                        cx.notify();
                                    }
                                });
                            }
                        });
                        let releasing = shell.clone();
                        window.on_mouse_event(move |ev: &MouseUpEvent, phase, _, cx| {
                            if phase == DispatchPhase::Bubble && ev.button == MouseButton::Left {
                                releasing.update(cx, |this, cx| {
                                    let view = select(this);
                                    if view.pressed.is_some() {
                                        view.release(ev.position, ev.click_count as u32);
                                        cx.notify();
                                    }
                                });
                            }
                        });
                    },
                )
                .absolute()
                .top_0()
                .left_0()
                .size_full(),
            )
            .on_mouse_down(MouseButton::Left, cx.listener(move |this, ev: &MouseDownEvent, window, cx| {
                cx.stop_propagation();
                window.focus(&click_focus);
                let view = select(this);
                view.mouse(MouseKind::Down, ev.position, false, ev.click_count as u32);
                view.pressed = view.page_point(ev.position);
                cx.notify();
            }))
            .on_mouse_down(MouseButton::Right, cx.listener(move |this, ev: &MouseDownEvent, _, _| {
                select(this).mouse(MouseKind::Down, ev.position, true, 1);
            }))
            .on_mouse_up(MouseButton::Right, cx.listener(move |this, ev: &MouseUpEvent, _, _| {
                select(this).mouse(MouseKind::Up, ev.position, true, 1);
            }))
            .on_mouse_move(cx.listener(move |this, ev: &MouseMoveEvent, _, _| {
                let view = select(this);
                if view.pressed.is_none() && view.bounds.get().contains(&ev.position) {
                    view.mouse(MouseKind::Move, ev.position, false, 0);
                }
            }))
            .on_scroll_wheel(cx.listener(move |this, ev: &ScrollWheelEvent, _, _| {
                let view = select(this);
                let delta = ev.delta.pixel_delta(px(40.));
                if let (Some(browser), Some((x, y))) = (&view.browser, view.page_point(ev.position)) {
                    browser.send(Command::Wheel { x, y, dx: -f32::from(delta.x), dy: -f32::from(delta.y) });
                }
            }))
            .into_any_element()
    }

    /// Keys for a web page that has focus: text, editing shortcuts and navigation go to Chrome.
    /// Escape gives the keyboard back to the chat. Returns true when consumed.
    pub(in crate::ui) fn web_key(
        &mut self,
        select: fn(&mut Shell) -> &mut WebView,
        ev: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let root = self.focus.clone();
        let focus = select(self).page_focus.clone();
        if !focus.is_some_and(|f| f.is_focused(window)) {
            return false;
        }
        let ks = &ev.keystroke;
        let m = ks.modifiers;
        if ks.key == "escape" {
            window.focus(&root);
            return true;
        }
        if self.app_shortcut(ks) {
            return false;
        }
        let Some(browser) = &select(self).browser else { return false };
        // Copy before the key, so Ctrl+X copies the selection it is about to cut.
        if m.control && matches!(ks.key.as_str(), "c" | "x") {
            browser.send(Command::CopySelection);
        }
        if m.control && ks.key == "v" {
            if let Some(t) = cx.read_from_clipboard().and_then(|c| c.text()) {
                browser.send(Command::InsertText(t));
            }
        } else if let (Some(ch), false) = (&ks.key_char, m.control || m.alt || m.platform) {
            browser.send(if ks.key == "enter" {
                Command::Key { key: "enter".into(), ctrl: false, shift: m.shift, alt: false, meta: false }
            } else {
                Command::InsertText(ch.clone())
            });
        } else {
            browser.send(Command::Key {
                key: ks.key.clone(),
                ctrl: m.control,
                shift: m.shift,
                alt: m.alt,
                meta: m.platform,
            });
        }
        true
    }

    /// Whether `ks` is one of the app's own shortcuts (`settings.keys`) with Ctrl, Alt or Cmd. Those stay
    /// with the app. Plain keys such as Enter and Escape reach the page or the shell.
    pub(in crate::ui) fn app_shortcut(&self, ks: &Keystroke) -> bool {
        let m = ks.modifiers;
        (m.control || m.alt || m.platform) && self.state.settings.keys.values().any(|spec| keystroke_matches(spec, ks))
    }

    /// Shows `text` in the address bar as if typed; with `submit`, goes there as Enter would.
    pub(in crate::ui) fn autopilot_address(&mut self, text: &str, submit: bool, cx: &mut Context<Self>) {
        if let Some(bar) = &self.tabs.browser.address {
            let input = bar.input.clone();
            input.update(cx, |input, cx| input.set_text(text, cx));
        }
        if submit {
            let root = self.tool_folder();
            let cwd = root.clone().unwrap_or_else(|| self.default_folder());
            let url = preview::normalize_address(text, &cwd);
            self.tabs.browser.view.navigate(url, root, browser_view, cx);
        }
        cx.notify();
    }

    /// Scrolls the page by `dy` CSS px, as the wheel over its middle would.
    pub(in crate::ui) fn autopilot_wheel(&mut self, dy: f32) {
        self.tabs.browser.view.send(Command::Wheel { x: 260., y: 360., dx: 0., dy });
    }

    fn browser_visible(&self) -> bool {
        let ws = &self.state.workspace;
        ws.right_open && ws.right_tab == RightTab::Browser
    }

    /// The address bar's input, created on first render.
    fn address_input(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Entity<TextInput> {
        if let Some(bar) = &self.tabs.browser.address {
            return bar.input.clone();
        }
        let input = cx.new(|cx| TextInput::new(address_config(), window, cx));
        let submit = cx.subscribe(&input, |this, input, event: &TextInputEvent, cx| match event {
            TextInputEvent::Submit => {
                let typed = input.read(cx).text();
                let root = this.tool_folder();
                let cwd = root.clone().unwrap_or_else(|| this.default_folder());
                let url = preview::normalize_address(&typed, &cwd);
                input.update(cx, |input, cx| input.set_text(&url, cx));
                this.tabs.browser.view.navigate(url, root, browser_view, cx);
            }
            TextInputEvent::Changed => {}
            TextInputEvent::Blur => cx.notify(),
        });
        self.tabs.browser.address = Some(AddressBar { input: input.clone(), _submit: submit });
        input
    }

    fn address_focused(&self, window: &Window, cx: &App) -> bool {
        self.tabs.browser.address.as_ref().is_some_and(|bar| bar.input.read(cx).focus_handle().is_focused(window))
    }

    pub(in crate::ui) fn browser_key(&mut self, ev: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if !self.browser_visible() {
            return false;
        }
        // Escape leaves the address bar, which then shows the page's address again.
        if ev.keystroke.key == "escape" && self.address_focused(window, cx) {
            window.focus(&self.focus);
            cx.notify();
            return true;
        }
        self.web_key(browser_view, ev, window, cx)
    }

    pub(in crate::ui) fn browser_before_render(&mut self, window: &mut Window, _cx: &mut Context<Self>) {
        let prefix = self.state.settings.search_engine.url_prefix();
        if self.tabs.browser.search_prefix != prefix {
            preview::set_search_engine(prefix);
            self.tabs.browser.search_prefix = prefix;
        }
        let visible = self.browser_visible();
        self.tabs.browser.view.sync(visible, window);
    }

    /// Ends a press on the page when the keyboard leaves the Browser tab.
    pub(in crate::ui) fn browser_blur(&mut self, _cx: &mut Context<Self>) {
        self.tabs.browser.view.cancel_press();
    }

    /// Opens the Browser tab on `url`, as the agent does when it starts a page.
    pub(in crate::ui) fn browser_open(&mut self, url: &str, cx: &mut Context<Self>) {
        self.state.workspace.right_open = true;
        self.state.workspace.right_tab = RightTab::Browser;
        self.widths_changed(false, cx);
        let root = self.tool_folder();
        self.tabs.browser.view.navigate(url.to_string(), root, browser_view, cx);
    }

    /// Saves the page's latest frame as a PNG in the focused session's attachments. None without a page.
    pub(in crate::ui) fn browser_screenshot(&mut self, _cx: &mut Context<Self>) -> Option<PathBuf> {
        let session = self.state.workspace.layout.focus.clone()?;
        let (frame, _, _) = self.tabs.browser.view.frame.as_ref()?;
        let png = encode_png(frame)?;
        match store::store_bytes(&session, "browser.png", &png) {
            Ok(path) => Some(path),
            Err(e) => {
                eprintln!("could not save the browser screenshot: {e}");
                None
            }
        }
    }
}

/// The frame as PNG. GPUI frames are BGRA, so the red and blue channels are swapped first.
fn encode_png(frame: &RenderImage) -> Option<Vec<u8>> {
    let size = frame.size(0);
    let mut rgba = frame.as_bytes(0)?.to_vec();
    for pixel in rgba.chunks_exact_mut(4) {
        pixel.swap(0, 2);
    }
    let image = image::RgbaImage::from_raw(size.width.0 as u32, size.height.0 as u32, rgba)?;
    let mut png = Cursor::new(Vec::new());
    image.write_to(&mut png, image::ImageFormat::Png).ok()?;
    Some(png.into_inner())
}

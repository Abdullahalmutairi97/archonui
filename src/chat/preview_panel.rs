//! The right-hand panel: a live browser preview (see `crate::preview`), the
//! IDE (VS Code through code-server), and the chat's details, as tabs.

use super::{button, ChatView};
use crate::{
    ide::{find_code_server, IdeServer, COOKIE, NOT_INSTALLED},
    preview::{self, Browser, BrowserEvent, Command, MouseKind},
    store::{IdeKind, PanelTab},
};
use gpui::{
    canvas, div, img, prelude::*, px, rgb, Bounds, ClickEvent, ClipboardItem, Context, ImageSource, KeyDownEvent,
    MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, ObjectFit, Pixels, RenderImage,
    ScrollWheelEvent, SharedString, StyledImage, Task, Window,
};
use std::{cell::Cell, path::PathBuf, rc::Rc, sync::Arc};

const DEFAULT_WIDTH: f32 = 440.;
const MIN_WIDTH: f32 = 300.;
/// Room left for the sessions sidebar (230) and the chat when resizing.
const MIN_CHAT_WIDTH: f32 = 680.;

/// Which browser view an action is for: the Preview tab's page or the IDE.
#[derive(Clone, Copy, PartialEq)]
enum View {
    Page,
    Ide,
}

impl View {
    /// The panel tab that shows this view.
    fn tab(self) -> PanelTab {
        match self {
            View::Page => PanelTab::Preview,
            View::Ide => PanelTab::Ide,
        }
    }
}

/// The panel's browser state: the Preview tab's page and the IDE's VS Code,
/// each in its own headless Chrome, and the code-server behind the IDE.
#[derive(Default)]
pub struct Preview {
    page: BrowserView,
    ide: BrowserView,
    /// code-server, started at launch with `ide_autostart`, else the first time the IDE tab is shown.
    server: Option<IdeServer>,
    server_starting: bool,
    /// Starts code-server and then waits for it to exit. Dropping it stops both.
    ide_task: Option<Task<()>>,
    /// The `code_server_path` setting the server was started with.
    ide_started_with: Option<PathBuf>,
    /// Session cookie for `server`, which the IDE's browser sends.
    ide_cookie: String,
    /// The folder the IDE was last sent to.
    ide_folder: Option<PathBuf>,
    /// Why code-server isn't running, if it failed or stopped.
    ide_error: Option<String>,
    resizing: bool,
    /// Whether the panel fills the window beside the chat's minimum width. Only the normal width is saved.
    maximized: bool,
}

impl Preview {
    fn view(&self, v: View) -> &BrowserView {
        match v {
            View::Page => &self.page,
            View::Ide => &self.ide,
        }
    }

    fn view_mut(&mut self, v: View) -> &mut BrowserView {
        match v {
            View::Page => &mut self.page,
            View::Ide => &mut self.ide,
        }
    }
}

/// One headless Chrome and what its page shows.
#[derive(Default)]
struct BrowserView {
    browser: Option<Browser>,
    launching: bool,
    /// Commands to send once the browser is up, such as the first navigation.
    pending: Vec<Command>,
    url: String,
    title: String,
    loading: bool,
    error: Option<String>,
    /// Latest frame and its size in CSS pixels.
    frame: Option<(Arc<RenderImage>, f32, f32)>,
    /// Replaced frames whose GPU textures still need freeing.
    stale: Vec<Arc<RenderImage>>,
    /// Address bar contents while it is being edited.
    editing: Option<String>,
    /// Keystrokes go to the page (after clicking into it).
    page_focused: bool,
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

impl ChatView {
    /// Show the Preview tab.
    pub(super) fn show_preview(&mut self, cx: &mut Context<Self>) {
        self.settings.hide_panel = false;
        self.settings.panel_tab = PanelTab::Preview;
        self.save_settings();
        cx.notify();
    }

    /// Load `url` in the preview, starting the browser if needed.
    pub(super) fn open_preview(&mut self, url: String, cx: &mut Context<Self>) {
        self.show_preview(cx);
        let p = &mut self.preview.page;
        p.url = url.clone();
        p.error = None;
        p.editing = None;
        // `file://` pages load only from the chat's folder.
        let root = self.session().map(|s| s.cwd.clone());
        let p = &mut self.preview.page;
        match &p.browser {
            Some(b) => {
                b.send(Command::SetRoot(root));
                b.send(Command::Navigate(url));
            }
            None => {
                p.pending = vec![Command::SetRoot(root), Command::Navigate(url)];
                self.launch_browser(View::Page, cx);
            }
        }
    }

    /// Whether the panel shows `v`'s tab.
    fn view_visible(&self, v: View) -> bool {
        !self.settings.hide_panel
            && self.settings.panel_tab == v.tab()
            && (v != View::Ide || self.settings.ide == IdeKind::VsCode)
    }

    /// Starts `v`'s browser in the background. Its pending commands go out once it is up.
    fn launch_browser(&mut self, v: View, cx: &mut Context<Self>) {
        let view = self.preview.view_mut(v);
        if view.launching || view.browser.is_some() {
            return;
        }
        view.launching = true;
        view.loading = true;
        view.error = None;
        let task = cx.spawn(async move |this, cx| {
            let launched = cx.background_executor().spawn(async { Browser::launch() }).await;
            let events = this
                .update(cx, |this, cx| {
                    let visible = this.view_visible(v);
                    let view = this.preview.view_mut(v);
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
                            if visible {
                                browser.send(Command::SetVisible(true));
                                view.sent_visible = Some(true);
                            }
                            let events = browser.events.clone();
                            let copies = browser.copies.clone();
                            view.browser = Some(browser);
                            cx.spawn(async move |this, cx| {
                                while let Ok(text) = copies.recv().await {
                                    let copied = this.update(cx, |_, cx| {
                                        cx.write_to_clipboard(ClipboardItem::new_string(text))
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
                if this.update(cx, |this, cx| this.browser_event(v, ev, cx)).is_err() {
                    break;
                }
            }
        });
        self.preview.view_mut(v).task = Some(task);
    }

    fn browser_event(&mut self, v: View, ev: BrowserEvent, cx: &mut Context<Self>) {
        let p = self.preview.view_mut(v);
        match ev {
            BrowserEvent::Frame(frame, w, h) => {
                if let Some((old, _, _)) = p.frame.replace((frame, w, h)) {
                    p.stale.push(old);
                    if p.stale.len() > 8 {
                        p.stale.drain(..p.stale.len() - 8);
                    }
                }
            }
            BrowserEvent::Url(url) => {
                if url != "about:blank" {
                    p.url = url;
                }
                p.error = None;
            }
            BrowserEvent::Title(t) => p.title = t,
            BrowserEvent::History { .. } => {}
            BrowserEvent::Loading(l) => p.loading = l,
            BrowserEvent::Error(e) => p.error = Some(e),
            BrowserEvent::Closed => {
                p.sent_visible = None;
                // Chrome may take a moment to shut down; keep that off the UI thread.
                if let Some(old) = p.browser.take() {
                    cx.background_executor().spawn(async move { drop(old) }).detach();
                }
                p.loading = false;
                p.error = Some(match v {
                    View::Page => "The preview browser closed. Load a page to restart it.".into(),
                    View::Ide => "VS Code's browser closed. Retry to restart it.".into(),
                });
            }
        }
        cx.notify();
    }

    /// Per-frame housekeeping that needs the window: free replaced frames,
    /// keep each browser's viewport matched to its panel, and keep the IDE on
    /// the chat's folder.
    pub(super) fn preview_before_render(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        for v in [View::Page, View::Ide] {
            let visible = self.view_visible(v);
            let p = self.preview.view_mut(v);
            for frame in p.stale.drain(..) {
                let _ = window.drop_image(frame);
            }
            let b = p.bounds.get();
            let size = (f32::from(b.size.width).round() as u32, f32::from(b.size.height).round() as u32, window.scale_factor());
            if size.0 > 0 && size.1 > 0 && size != p.viewport {
                p.viewport = size;
                if let Some(browser) = &p.browser {
                    browser.send(Command::Resize { width: size.0, height: size.1, scale: size.2 });
                }
            }
            if let Some(browser) = &p.browser {
                if p.sent_visible != Some(visible) {
                    browser.send(Command::SetVisible(visible));
                    p.sent_visible = Some(visible);
                }
            }
        }
        // A new code-server path takes effect by restarting a running VS Code.
        if (self.preview.server.is_some() || self.preview.server_starting)
            && self.preview.ide_started_with != self.settings.code_server_path
        {
            self.restart_ide(cx);
        }
        self.ide_sync(cx);
    }

    /// Starts code-server if needed, and points the IDE at the chat's folder.
    /// Without `ide_autostart`, that only happens while the IDE tab is shown.
    fn ide_sync(&mut self, cx: &mut Context<Self>) {
        if self.settings.ide != IdeKind::VsCode || (!self.view_visible(View::Ide) && !self.settings.ide_autostart) {
            return;
        }
        let Some(server) = &self.preview.server else {
            if !self.preview.server_starting && self.preview.ide_error.is_none() {
                self.start_ide_server(cx);
            }
            return;
        };
        let Some(cwd) = self.session().map(|s| s.cwd.clone()) else { return };
        if self.preview.ide_folder.as_ref() == Some(&cwd) {
            return;
        }
        let url = server.url_for(&cwd);
        let base = server.base_url();
        let cookie = self.preview.ide_cookie.clone();
        self.preview.ide_folder = Some(cwd);
        let ide = self.preview.view_mut(View::Ide);
        match &ide.browser {
            Some(browser) => browser.send(Command::Navigate(url)),
            // The cookie has to be set before the first navigation.
            None => {
                ide.pending = vec![
                    Command::SetCookie { url: base, name: COOKIE.into(), value: cookie },
                    Command::Navigate(url),
                ];
                self.launch_browser(View::Ide, cx);
            }
        }
    }

    /// Starts code-server and logs in, in the background. Then it waits for code-server to exit.
    fn start_ide_server(&mut self, cx: &mut Context<Self>) {
        let configured = self.settings.code_server_path.clone();
        let Some(program) = find_code_server(configured.as_deref()) else {
            self.preview.ide_error = Some(NOT_INSTALLED.into());
            return;
        };
        let p = &mut self.preview;
        p.ide_started_with = configured;
        p.server_starting = true;
        p.ide_task = Some(cx.spawn(async move |this, cx| {
            let started = cx
                .background_executor()
                .spawn(async move {
                    let server = IdeServer::start(&program)?;
                    let cookie = server.login_cookie()?;
                    Ok::<_, String>((server, cookie))
                })
                .await;
            let exited = this
                .update(cx, |this, cx| {
                    let p = &mut this.preview;
                    p.server_starting = false;
                    let exited = match started {
                        Ok((server, cookie)) => {
                            let exited = server.exited();
                            p.server = Some(server);
                            p.ide_cookie = cookie;
                            Some(exited)
                        }
                        Err(e) => {
                            p.ide_error = Some(e);
                            None
                        }
                    };
                    cx.notify();
                    exited
                })
                .ok()
                .flatten();
            let Some(exited) = exited else { return };
            if let Ok(code) = exited.recv().await {
                this.update(cx, |this, cx| this.ide_exited(code, cx)).ok();
            }
        }));
    }

    /// Stops code-server after it exited on its own, and shows why, with Retry.
    fn ide_exited(&mut self, code: Option<i32>, cx: &mut Context<Self>) {
        self.stop_ide(cx);
        self.preview.ide_error = Some(match code {
            Some(code) => format!("VS Code stopped (exit code {code})."),
            None => "VS Code stopped (killed by a signal).".into(),
        });
        cx.notify();
    }

    /// Stops code-server and the IDE's browser, and forgets the page and folder they showed.
    fn stop_ide(&mut self, cx: &mut Context<Self>) {
        let p = &mut self.preview;
        p.server_starting = false;
        p.ide_folder = None;
        let ide = &mut p.ide;
        // Cancelling the browser's task also cancels a launch in progress, which would otherwise leave `launching` set.
        ide.task = None;
        ide.launching = false;
        ide.pending.clear();
        ide.url.clear();
        ide.title.clear();
        ide.loading = false;
        ide.error = None;
        let browser = ide.browser.take();
        let server = p.server.take();
        // Stopping them can take a moment; keep that off the UI thread.
        cx.background_executor().spawn(async move { drop((browser, server)) }).detach();
    }

    /// Keys for the address bar, or for the focused page or IDE. Returns true if consumed.
    pub(super) fn preview_key(&mut self, ev: &KeyDownEvent, cx: &mut Context<Self>) -> bool {
        let ks = &ev.keystroke;
        let m = ks.modifiers;
        if let Some(text) = &mut self.preview.page.editing {
            match ks.key.as_str() {
                "enter" => {
                    let typed = text.clone();
                    let cwd = self.session().map_or_else(|| self.default_cwd.clone(), |s| s.cwd.clone());
                    self.open_preview(preview::normalize_address(&typed, &cwd), cx);
                }
                "escape" => self.preview.page.editing = None,
                "backspace" => {
                    text.pop();
                }
                "v" if m.control => {
                    if let Some(t) = cx.read_from_clipboard().and_then(|c| c.text()) {
                        text.push_str(t.trim());
                    }
                }
                _ if m.control || m.alt || m.platform => {}
                _ => {
                    if let Some(ch) = &ks.key_char {
                        text.push_str(ch);
                    }
                }
            }
            return true;
        }
        if self.settings.hide_panel {
            return false;
        }
        let v = if self.preview.page.page_focused {
            View::Page
        } else if self.preview.ide.page_focused {
            View::Ide
        } else {
            return false;
        };
        // Escape gives the keyboard back to the chat without reaching the page.
        // In the IDE, Escape belongs to VS Code.
        if v == View::Page && ks.key == "escape" {
            self.preview.page.page_focused = false;
            cx.notify();
            return true;
        }
        // Chat shortcuts stay with the chat, except in the IDE, which needs them.
        if v == View::Page && m.control && matches!(ks.key.as_str(), "n" | "i") {
            return false;
        }
        let Some(browser) = &self.preview.view(v).browser else { return false };
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

    /// A click outside the pages and address bar gives the keyboard back to
    /// the chat.
    pub(super) fn preview_blur(&mut self, cx: &mut Context<Self>) {
        if self.preview.page.page_focused || self.preview.ide.page_focused || self.preview.page.editing.is_some() {
            self.set_page_focus(None);
            cx.notify();
        }
    }

    /// Gives the keyboard to `focus`'s page, or to the chat when `None`.
    fn set_page_focus(&mut self, focus: Option<View>) {
        for v in [View::Page, View::Ide] {
            let p = self.preview.view_mut(v);
            p.page_focused = focus == Some(v);
            p.editing = None;
        }
    }

    pub(super) fn panel_resize_move(&mut self, ev: &MouseMoveEvent, window: &mut Window, cx: &mut Context<Self>) {
        if !self.preview.resizing {
            return;
        }
        if ev.pressed_button != Some(MouseButton::Left) {
            self.panel_resize_end(cx);
            return;
        }
        let total = f32::from(window.viewport_size().width);
        let max = (total - MIN_CHAT_WIDTH).max(MIN_WIDTH);
        self.settings.panel_width = Some((total - f32::from(ev.position.x)).clamp(MIN_WIDTH, max));
        cx.notify();
    }

    pub(super) fn panel_resize_end(&mut self, cx: &mut Context<Self>) {
        if self.preview.resizing {
            self.preview.resizing = false;
            self.save_settings();
            cx.notify();
        }
    }

    pub(super) fn render_panel(&self, window: &Window, cx: &mut Context<Self>) -> impl IntoElement {
        let total = f32::from(window.viewport_size().width);
        let max_width = (total - MIN_CHAT_WIDTH).max(MIN_WIDTH);
        let width = if self.preview.maximized {
            max_width
        } else {
            self.settings.panel_width.unwrap_or(DEFAULT_WIDTH).clamp(MIN_WIDTH, max_width)
        };
        let tab = self.settings.panel_tab;
        let tab_button = |id: &'static str, label: &'static str, which: PanelTab| {
            div()
                .id(id)
                .px_3()
                .py_1()
                .rounded_md()
                .text_xs()
                .cursor_pointer()
                .when(tab == which, |d| d.bg(rgb(0x313244)))
                .hover(|d| d.bg(rgb(0x2a2b3c)))
                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                    this.settings.panel_tab = which;
                    this.save_settings();
                    // Files may have changed while the Git tab was hidden.
                    this.git_changed(cx);
                    cx.notify();
                }))
                .child(label)
        };
        div()
            .w(px(width))
            .flex_none()
            .h_full()
            .flex()
            .bg(rgb(0x181825))
            // Drag handle on the left edge.
            .child(
                div()
                    .id("panel-resize")
                    .w(px(5.))
                    .h_full()
                    .flex_none()
                    .cursor_col_resize()
                    .border_l_1()
                    .border_color(rgb(0x313244))
                    .hover(|d| d.bg(rgb(0x45475a)))
                    .when(self.preview.resizing, |d| d.bg(rgb(0x45475a)))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, _: &MouseDownEvent, _, _| {
                            this.preview.resizing = true;
                            this.preview.maximized = false;
                        }),
                    ),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .px_2()
                            .py_2()
                            .flex()
                            .gap_1()
                            .border_b_1()
                            .border_color(rgb(0x313244))
                            .child(tab_button("tab-preview", "Preview", PanelTab::Preview))
                            .child(tab_button("tab-details", "Details", PanelTab::Details))
                            .child(tab_button("tab-terminal", "Terminal", PanelTab::Terminal))
                            .child(tab_button("tab-ide", "IDE", PanelTab::Ide))
                            .child(tab_button("tab-git", "Git", PanelTab::Git))
                            .child(div().flex_1())
                            .child(
                                button("panel-maximize", "⤢")
                                    .when(self.preview.maximized, |b| b.bg(rgb(0x45475a)))
                                    .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                                        this.preview.maximized = !this.preview.maximized;
                                        cx.notify();
                                    })),
                            ),
                    )
                    .map(|d| match tab {
                        PanelTab::Preview => d.child(self.render_preview(cx)),
                        PanelTab::Details => d.child(self.render_details(cx)),
                        PanelTab::Terminal => d.child(self.render_terminal(window, cx)),
                        PanelTab::Ide => match self.settings.ide {
                            IdeKind::Builtin => d.child(self.render_editor(cx)),
                            IdeKind::VsCode => d.child(self.render_ide(cx)),
                        },
                        PanelTab::Git => d.child(self.render_git(window, cx)),
                    }),
            )
    }

    fn render_preview(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let p = &self.preview.page;
        let has_browser = p.browser.is_some();
        let nav = |id: &'static str, label: &'static str, command: fn() -> Command| {
            button(id, label).when(has_browser, |b| {
                b.on_click(cx.listener(move |this, _: &ClickEvent, _, _| {
                    if let Some(b) = &this.preview.page.browser {
                        b.send(command());
                    }
                }))
            })
        };
        let (address, address_color): (SharedString, _) = match &p.editing {
            Some(text) => (format!("{text}▏").into(), rgb(0xf2f2f7)),
            None if p.url.is_empty() => ("Search DuckDuckGo or type an address".into(), rgb(0x6c7086)),
            None => (p.url.clone().into(), rgb(0xbac2de)),
        };
        let toolbar = div()
            .px_2()
            .py_2()
            .flex()
            .items_center()
            .gap_1()
            .child(nav("pv-back", "◀", || Command::Back))
            .child(nav("pv-forward", "▶", || Command::Forward))
            .child(nav("pv-reload", "⟳", || Command::Reload))
            .child(
                div()
                    .id("pv-address")
                    .flex_1()
                    .min_w_0()
                    .px_2()
                    .py_1()
                    .rounded_md()
                    .text_xs()
                    .truncate()
                    .cursor_text()
                    .bg(rgb(0x2b2d3a))
                    .border_1()
                    .border_color(if p.editing.is_some() { rgb(0x89b4fa) } else { rgb(0x2b2d3a) })
                    .text_color(address_color)
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, _: &MouseDownEvent, window, cx| {
                            cx.stop_propagation();
                            this.set_page_focus(None);
                            this.preview.page.editing = Some(this.preview.page.url.clone());
                            this.term_blur(cx);
                            this.editor_blur(cx);
                            window.focus(&this.focus);
                            cx.notify();
                        }),
                    )
                    .child(address),
            )
            .when(!p.url.is_empty(), |d| {
                let url = p.url.clone();
                d.child(
                    button("pv-open", "↗")
                        .on_click(cx.listener(move |_, _: &ClickEvent, _, _| super::open_file(std::path::Path::new(&url)))),
                )
            });

        let status: Option<SharedString> = if p.launching {
            Some("Starting the preview browser…".into())
        } else if p.loading {
            Some("Loading…".into())
        } else if !p.title.is_empty() && p.title != p.url && !p.url.is_empty() {
            Some(p.title.clone().into())
        } else {
            None
        };

        div()
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .child(toolbar)
            .children(status.map(|s| {
                div().px_3().pb_1().text_xs().text_color(rgb(0x7f849c)).truncate().child(s)
            }))
            .children(p.error.clone().map(|e| {
                div().mx_2().mb_1().px_2().py_1().rounded_md().bg(rgb(0x4a2530)).text_xs().child(e)
            }))
            .child(self.render_page_area(View::Page, cx))
    }

    /// Restarts VS Code: stops code-server and its browser, then starts code-server again.
    /// Its browser loads the chat's folder once it's up.
    pub(super) fn restart_ide(&mut self, cx: &mut Context<Self>) {
        // Cancels a start in progress and the exit watch, so neither reports into the new server.
        self.preview.ide_task = None;
        self.preview.ide_error = None;
        self.stop_ide(cx);
        self.start_ide_server(cx);
        cx.notify();
    }

    /// One line describing the IDE server, for the settings page.
    pub(super) fn ide_status(&self) -> String {
        let p = &self.preview;
        if let Some(server) = &p.server {
            format!("Running · port {} · {}", server.port(), server.program().display())
        } else if p.server_starting {
            "Starting…".into()
        } else if let Some(error) = &p.ide_error {
            if error == NOT_INSTALLED {
                "Not installed".into()
            } else {
                format!("Stopped · {error}")
            }
        } else {
            "Stopped".into()
        }
    }

    /// The IDE tab: VS Code in the selected chat's folder.
    fn render_ide(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let p = &self.preview;
        let ide = &p.ide;
        let folder: SharedString = match self.session() {
            Some(s) => s.cwd.display().to_string().into(),
            None => "No chat selected".into(),
        };
        let error = p.ide_error.clone().or_else(|| ide.error.clone());
        let status: Option<SharedString> = if p.server_starting || ide.launching {
            Some("Starting VS Code…".into())
        } else if ide.loading {
            Some(match &p.ide_folder {
                Some(folder) => format!("Opening {}…", folder.display()).into(),
                None => "Loading…".into(),
            })
        } else {
            None
        };
        let toolbar = div()
            .px_2()
            .py_2()
            .flex()
            .items_center()
            .gap_1()
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .px_2()
                    .text_xs()
                    .text_color(rgb(0x7f849c))
                    .truncate()
                    .child(folder),
            )
            .child(button("ide-reload", "⟳").when(ide.browser.is_some(), |b| {
                b.on_click(cx.listener(|this, _: &ClickEvent, _, _| {
                    if let Some(browser) = &this.preview.ide.browser {
                        browser.send(Command::Reload);
                    }
                }))
            }));

        div()
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .child(toolbar)
            .children(status.map(|s| div().px_3().pb_1().text_xs().text_color(rgb(0x7f849c)).truncate().child(s)))
            .children(error.map(|e| {
                div()
                    .mx_2()
                    .mb_1()
                    .px_2()
                    .py_1()
                    .rounded_md()
                    .bg(rgb(0x4a2530))
                    .text_xs()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap_2()
                    .child(e)
                    .child(button("ide-retry", "Retry").on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                        this.restart_ide(cx);
                    })))
            }))
            .child(self.render_page_area(View::Ide, cx))
    }

    /// `v`'s page: its latest frame, which takes the mouse and keyboard.
    fn render_page_area(&self, v: View, cx: &mut Context<Self>) -> impl IntoElement {
        let p = self.preview.view(v);
        let bounds = p.bounds.clone();
        let placeholder = match v {
            View::Page => "Websites your agent builds or starts open here automatically. \
                           You can also type an address above or use /preview <url>.",
            View::Ide => "VS Code opens here in the selected chat's folder.",
        };
        div()
            .id(match v {
                View::Page => "pv-page",
                View::Ide => "ide-page",
            })
            .flex_1()
            .min_h_0()
            .relative()
            .overflow_hidden()
            .bg(rgb(0xffffff))
            .border_2()
            .border_color(if p.page_focused { rgb(0x89b4fa) } else { rgb(0x181825) })
            .map(|d| match &p.frame {
                Some((frame, w, h)) if !p.url.is_empty() => d.child(
                    img(ImageSource::Render(frame.clone()))
                        .w(px(*w))
                        .h(px(*h))
                        .object_fit(ObjectFit::Fill),
                ),
                _ => d.bg(rgb(0x1e1e2e)).child(
                    div()
                        .size_full()
                        .flex()
                        .items_center()
                        .justify_center()
                        .p_6()
                        .child(
                            // A full-width box so the text wraps.
                            div().w_full().text_center().text_xs().text_color(rgb(0x7f849c)).child(placeholder),
                        ),
                ),
            })
            // Records where the page is, so mouse positions can be mapped
            // into the page and its viewport kept in sync.
            .child(
                canvas(move |b, _, _| bounds.set(b), |_, _, _, _| {})
                    .absolute()
                    .top_0()
                    .left_0()
                    .size_full(),
            )
            .on_mouse_down(MouseButton::Left, cx.listener(move |this, ev: &MouseDownEvent, window, cx| {
                cx.stop_propagation();
                window.focus(&this.focus);
                this.term_blur(cx);
                this.editor_blur(cx);
                this.set_page_focus(Some(v));
                this.page_mouse(v, MouseKind::Down, ev.position, false, ev.click_count as u32);
                let pressed = this.page_point(v, ev.position);
                this.preview.view_mut(v).pressed = pressed;
                cx.notify();
            }))
            .on_mouse_down(MouseButton::Right, cx.listener(move |this, ev: &MouseDownEvent, _, _| {
                this.page_mouse(v, MouseKind::Down, ev.position, true, 1);
            }))
            .on_mouse_up(MouseButton::Left, cx.listener(|this, ev: &MouseUpEvent, _, _| {
                this.preview_mouse_release(ev.position, ev.click_count as u32);
            }))
            .on_mouse_up(MouseButton::Right, cx.listener(move |this, ev: &MouseUpEvent, _, _| {
                this.page_mouse(v, MouseKind::Up, ev.position, true, 1);
            }))
            .on_mouse_move(cx.listener(move |this, ev: &MouseMoveEvent, _, _| {
                // Held-button moves, including ones that leave the page, come from the panel root.
                let view = this.preview.view(v);
                if !this.preview.resizing && view.pressed.is_none() && view.bounds.get().contains(&ev.position) {
                    this.page_mouse(v, MouseKind::Move, ev.position, false, 0);
                }
            }))
            .on_scroll_wheel(cx.listener(move |this, ev: &ScrollWheelEvent, _, _| {
                let delta = ev.delta.pixel_delta(px(40.));
                if let (Some(b), Some((x, y))) = (&this.preview.view(v).browser, this.page_point(v, ev.position)) {
                    b.send(Command::Wheel { x, y, dx: -f32::from(delta.x), dy: -f32::from(delta.y) });
                }
            }))
    }

    /// Mouse position in `v`'s page (CSS) pixels, if it is over the page.
    fn page_point(&self, v: View, pos: gpui::Point<Pixels>) -> Option<(f32, f32)> {
        let b = self.preview.view(v).bounds.get();
        b.contains(&pos).then(|| (f32::from(pos.x - b.origin.x), f32::from(pos.y - b.origin.y)))
    }

    fn page_mouse(&self, v: View, kind: MouseKind, pos: gpui::Point<Pixels>, right: bool, clicks: u32) {
        let (Some(browser), Some((x, y))) = (&self.preview.view(v).browser, self.page_point(v, pos)) else { return };
        if matches!(kind, MouseKind::Down) {
            // Pages expect the pointer to arrive before it presses.
            browser.send(Command::Mouse { kind: MouseKind::Move, x, y, right: false, clicks: 0 });
        }
        browser.send(Command::Mouse { kind, x, y, right, clicks });
    }

    /// Page point for `pos`, pulled inside `v`'s page edges.
    fn page_point_clamped(&self, v: View, pos: gpui::Point<Pixels>) -> (f32, f32) {
        let b = self.preview.view(v).bounds.get();
        (
            f32::from(pos.x - b.origin.x).clamp(0., f32::from(b.size.width)),
            f32::from(pos.y - b.origin.y).clamp(0., f32::from(b.size.height)),
        )
    }

    /// Keeps a press that started on a page moving after the pointer leaves it.
    pub(super) fn preview_drag_move(&mut self, pos: gpui::Point<Pixels>) {
        for v in [View::Page, View::Ide] {
            if self.preview.view(v).pressed.is_none() {
                continue;
            }
            let (x, y) = self.page_point_clamped(v, pos);
            let p = self.preview.view_mut(v);
            p.pressed = Some((x, y));
            if let Some(browser) = &p.browser {
                browser.send(Command::Mouse { kind: MouseKind::Move, x, y, right: false, clicks: 0 });
            }
        }
    }

    /// Ends a press that started on a page, wherever the button comes up.
    pub(super) fn preview_mouse_release(&mut self, pos: gpui::Point<Pixels>, clicks: u32) {
        for v in [View::Page, View::Ide] {
            let (x, y) = self.page_point_clamped(v, pos);
            let p = self.preview.view_mut(v);
            if p.pressed.take().is_none() {
                continue;
            }
            if let Some(browser) = &p.browser {
                browser.send(Command::Mouse { kind: MouseKind::Up, x, y, right: false, clicks });
            }
        }
    }
}

//! Browser preview: a headless Chrome driven over the DevTools protocol.
//!
//! GPUI has no web view and Wayland can't host another toolkit's window, so
//! the page is rendered off-screen: Chrome streams JPEG frames of the page
//! (`Page.startScreencast`), which the panel shows as an image, and the
//! panel's mouse and keyboard input is replayed into the page.

use base64::{engine::general_purpose::STANDARD, Engine};
use gpui::RenderImage;
use serde_json::{json, Value};
use smol::channel::{unbounded, Receiver, Sender};
use std::{
    collections::HashMap,
    env,
    io::{self, BufRead, BufReader},
    net::TcpStream,
    os::unix::process::CommandExt,
    path::{Path, PathBuf},
    process::{self, Child, Stdio},
    sync::{
        atomic::{AtomicU32, Ordering},
        mpsc, Arc, RwLock,
    },
    thread,
    time::{Duration, Instant},
};
use tungstenite::{stream::MaybeTlsStream, Message, WebSocket};

pub enum BrowserEvent {
    /// A new frame of the page and its size in CSS pixels.
    Frame(Arc<RenderImage>, f32, f32),
    Url(String),
    Title(String),
    /// Whether the page can go back or forward, from its navigation history.
    History { back: bool, forward: bool },
    Loading(bool),
    Error(String),
    /// Chrome went away; the browser must be restarted.
    Closed,
}

#[derive(Clone, Copy)]
pub enum MouseKind {
    Down,
    Up,
    Move,
}

pub enum Command {
    Navigate(String),
    Back,
    Forward,
    Reload,
    /// Viewport in CSS pixels and the display's scale factor.
    Resize { width: u32, height: u32, scale: f32 },
    Mouse { kind: MouseKind, x: f32, y: f32, right: bool, clicks: u32 },
    Wheel { x: f32, y: f32, dx: f32, dy: f32 },
    /// A named key (GPUI's names, e.g. "enter", "left") with modifiers. `meta` is
    /// GPUI's platform modifier (Cmd on macOS, Super elsewhere).
    Key { key: String, ctrl: bool, shift: bool, alt: bool, meta: bool },
    InsertText(String),
    /// Whether the panel shows the page; frames stream only while it does.
    SetVisible(bool),
    /// Sets a cookie for requests to `url`.
    SetCookie { url: String, name: String, value: String },
    /// Sends the page's selected text to `Browser::copies`.
    CopySelection,
}

/// A running headless Chrome. Dropping it shuts Chrome down.
pub struct Browser {
    commands: mpsc::Sender<Command>,
    pub events: Receiver<BrowserEvent>,
    /// Text copied from the page by `Command::CopySelection`.
    pub copies: Receiver<String>,
    child: Child,
    profile: PathBuf,
}

impl Browser {
    /// Blocking: start Chrome and open a blank page.
    pub fn launch() -> Result<Self, String> {
        let root = profile_root();
        remove_stale_profiles(&root);
        let chrome = find_chrome().ok_or(
            "No Chrome or Chromium found. Install one, or set ARCHONUI_CHROME to its path.",
        )?;
        // Each browser needs its own profile: two Chromes can't share one.
        static LAUNCHES: AtomicU32 = AtomicU32::new(0);
        let n = LAUNCHES.fetch_add(1, Ordering::Relaxed);
        let profile = root.join(format!("chrome-{}-{n}", process::id()));
        crate::store::create_private_dir(&profile).map_err(|e| format!("Could not create a Chrome profile: {e}"))?;
        // Marks the directory as ours, so stale-profile cleanup may delete it.
        let _ = std::fs::write(profile.join(PROFILE_MARKER), process::id().to_string());
        let mut child = unsafe {
            process::Command::new(&chrome)
                .args([
                    "--headless=new",
                    "--remote-debugging-port=0",
                    "--no-first-run",
                    "--no-default-browser-check",
                    "--disable-background-networking",
                    "--mute-audio",
                    // Hides navigator.webdriver, an automation tell that sites check.
                    "--disable-blink-features=AutomationControlled",
                ])
                .arg(format!("--user-data-dir={}", profile.display()))
                .arg("about:blank")
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::piped())
                .process_group(0)
                // Chrome must not outlive ArchonUI, even if ArchonUI is killed.
                .pre_exec(|| {
                    libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL);
                    Ok(())
                })
                .spawn()
                .map_err(|e| format!("Could not start {}: {e}", chrome.display()))?
        };
        let ws = match connect_devtools(&mut child) {
            Ok(ws) => ws,
            Err(e) => {
                // Don't leave Chrome or its profile behind after a failed launch.
                unsafe { libc::kill(-(child.id() as i32), libc::SIGKILL) };
                let _ = child.wait();
                let _ = std::fs::remove_dir_all(&profile);
                return Err(e);
            }
        };
        let (cmd_tx, cmd_rx) = mpsc::channel();
        let (ev_tx, ev_rx) = unbounded();
        let (copy_tx, copy_rx) = unbounded();
        thread::spawn(move || Session::new(ws, ev_tx, copy_tx).run(cmd_rx));
        Ok(Self { commands: cmd_tx, events: ev_rx, copies: copy_rx, child, profile })
    }

    pub fn send(&self, command: Command) {
        let _ = self.commands.send(command);
    }
}

impl Drop for Browser {
    fn drop(&mut self) {
        stop_process_group(&mut self.child);
        let _ = std::fs::remove_dir_all(&self.profile);
    }
}

/// Stops a child and its process group: SIGTERM, then SIGKILL after 2 seconds.
pub(crate) fn stop_process_group(child: &mut Child) {
    let group = -(child.id() as i32);
    unsafe { libc::kill(group, libc::SIGTERM) };
    let deadline = Instant::now() + Duration::from_secs(2);
    while matches!(child.try_wait(), Ok(None)) && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(20));
    }
    if matches!(child.try_wait(), Ok(None)) {
        unsafe { libc::kill(group, libc::SIGKILL) };
        let _ = child.wait();
    }
}

/// Reads Chrome's DevTools address from its stderr and connects to it.
fn connect_devtools(child: &mut Child) -> Result<WebSocket<MaybeTlsStream<TcpStream>>, String> {
    let stderr = child.stderr.take().expect("piped stderr");
    let (url_tx, url_rx) = mpsc::channel();
    thread::spawn(move || {
        let mut url_tx = Some(url_tx);
        for line in BufReader::new(stderr).lines().map_while(Result::ok) {
            if let Some(url) = line.strip_prefix("DevTools listening on ") {
                if let Some(tx) = url_tx.take() {
                    let _ = tx.send(url.trim().to_string());
                }
            }
            // Keep draining so Chrome never blocks on a full pipe.
        }
    });
    let ws_url = url_rx
        .recv_timeout(Duration::from_secs(20))
        .map_err(|_| "Chrome started but did not open its debugging port.".to_string())?;
    let (ws, _) = tungstenite::connect(ws_url.as_str()).map_err(|e| format!("DevTools: {e}"))?;
    if let MaybeTlsStream::Plain(s) = ws.get_ref() {
        s.set_read_timeout(Some(Duration::from_millis(15))).map_err(|e| e.to_string())?;
    }
    Ok(ws)
}

/// Marks each Chrome profile directory ArchonUI created, so cleanup never deletes
/// a directory someone else made.
const PROFILE_MARKER: &str = ".archonui-chrome-profile";

/// The owner-only directory that holds Chrome profiles: `$XDG_RUNTIME_DIR/archonui`
/// when set, else a `browser` folder under ArchonUI's data directory. Never a
/// predictable path in a world-writable directory.
fn profile_root() -> PathBuf {
    if let Some(runtime) = env::var_os("XDG_RUNTIME_DIR") {
        let dir = PathBuf::from(runtime).join("archonui");
        if crate::store::create_private_dir(&dir).is_ok() {
            return dir;
        }
    }
    let dir = crate::store::dir().parent().map_or_else(|| env::temp_dir(), Path::to_path_buf).join("browser");
    let _ = crate::store::create_private_dir(&dir);
    dir
}

/// Removes profiles left behind by ArchonUI processes that no longer exist. Only
/// marked directories are touched, and a symlink is never followed.
fn remove_stale_profiles(root: &Path) {
    let Ok(entries) = std::fs::read_dir(root) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(meta) = path.symlink_metadata() else { continue };
        // `symlink_metadata` makes a symlink look like a file, so it is skipped.
        if !meta.file_type().is_dir() || !path.join(PROFILE_MARKER).is_file() {
            continue;
        }
        let Some(pid) = entry
            .file_name()
            .to_string_lossy()
            .strip_prefix("chrome-")
            .and_then(|p| p.split('-').next())
            .and_then(|p| p.parse::<u32>().ok())
        else {
            continue;
        };
        if !Path::new(&format!("/proc/{pid}")).exists() {
            let _ = std::fs::remove_dir_all(&path);
        }
    }
}

/// `ARCHONUI_CHROME`, Chrome/Chromium on `PATH`, then Playwright's Chromium.
fn find_chrome() -> Option<PathBuf> {
    if let Some(p) = env::var_os("ARCHONUI_CHROME") {
        return Some(p.into());
    }
    let path: Vec<PathBuf> =
        env::var_os("PATH").map(|p| env::split_paths(&p).collect()).unwrap_or_default();
    for name in ["google-chrome", "google-chrome-stable", "chromium", "chromium-browser", "chrome"] {
        if let Some(p) = path.iter().map(|d| d.join(name)).find(|p| p.is_file()) {
            return Some(p);
        }
    }
    let cache = PathBuf::from(env::var_os("HOME")?).join(".cache/ms-playwright");
    let mut dirs: Vec<PathBuf> = std::fs::read_dir(cache)
        .ok()?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.file_name().is_some_and(|n| n.to_string_lossy().starts_with("chromium-")))
        .collect();
    dirs.sort();
    dirs.iter().rev().map(|d| d.join("chrome-linux/chrome")).find(|p| p.is_file())
}

/// What a pending request's reply means to us.
enum Pending {
    Attached,
    Version,
    Navigate,
    Screenshot,
    Copy,
    History,
}

/// The page's selected text: a focused field's selection first (VS Code keeps
/// its editor selection in a hidden textarea), else the page's selection.
const SELECTED_TEXT_JS: &str = "(() => { const el = document.activeElement; if ((el.tagName === 'TEXTAREA' || el.tagName === 'INPUT') && el.selectionStart !== el.selectionEnd) return el.value.substring(el.selectionStart, el.selectionEnd); return String(window.getSelection()); })()";

/// Reports the page's clipboard writes to the `__archonuiCopy` binding, which
/// `Session` forwards to `copies`. Pages such as VS Code keep their selection
/// out of the DOM, so the selection can't be read; they write it to the
/// clipboard instead. Installed in every frame, once per window.
const COPY_HOOK_JS: &str = r#"(() => {
  if (window.__archonuiHooked) return;
  window.__archonuiHooked = true;
  const send = (text) => {
    if (typeof window.__archonuiCopy === 'function') window.__archonuiCopy(String(text));
  };
  // Only data set during a copy or cut is the clipboard's, not a drag's.
  let copying = false;
  for (const type of ['copy', 'cut']) {
    document.addEventListener(type, () => {
      copying = true;
      setTimeout(() => (copying = false));
    }, true);
  }
  const setData = DataTransfer.prototype.setData;
  DataTransfer.prototype.setData = function (type, data) {
    if (copying && ['text/plain', 'text'].includes(String(type).toLowerCase())) send(data);
    return setData.call(this, type, data);
  };
  const clipboard = navigator.clipboard;
  if (!clipboard) return;
  const writeText = clipboard.writeText.bind(clipboard);
  clipboard.writeText = (text) => {
    send(text);
    return writeText(text).catch(() => {});
  };
  const write = clipboard.write.bind(clipboard);
  clipboard.write = (items) => {
    for (const item of items) {
      if (item.types.includes('text/plain')) {
        item.getType('text/plain').then((blob) => blob.text()).then(send, () => {});
      }
    }
    return write(items).catch(() => {});
  };
})()"#;

/// The DevTools connection, run on its own thread.
struct Session {
    ws: WebSocket<MaybeTlsStream<TcpStream>>,
    events: Sender<BrowserEvent>,
    copies: Sender<String>,
    next_id: u64,
    pending: HashMap<u64, Pending>,
    /// Page session id once attached.
    page: Option<String>,
    /// Commands that arrived before the page was attached.
    queued: Vec<Command>,
    viewport: (u32, u32, f32),
    /// Whether the page is shown; the screencast only runs while it is.
    visible: bool,
    /// Mouse buttons held down, as CDP's bitmask: left 1, right 2.
    buttons: u8,
}

impl Session {
    fn new(ws: WebSocket<MaybeTlsStream<TcpStream>>, events: Sender<BrowserEvent>, copies: Sender<String>) -> Self {
        Self {
            ws,
            events,
            copies,
            next_id: 1,
            pending: HashMap::new(),
            page: None,
            queued: Vec::new(),
            viewport: (800, 600, 1.0),
            visible: true,
            buttons: 0,
        }
    }

    fn run(mut self, commands: mpsc::Receiver<Command>) {
        // Chrome reports its existing blank page; we attach to it there.
        self.call(None, "Target.setDiscoverTargets", json!({"discover": true}));
        loop {
            loop {
                match commands.try_recv() {
                    Ok(c) if self.ready() => self.command(c),
                    Ok(c) => self.queued.push(c),
                    Err(mpsc::TryRecvError::Empty) => break,
                    // The Browser was dropped.
                    Err(mpsc::TryRecvError::Disconnected) => return,
                }
            }
            match self.ws.read() {
                Ok(Message::Text(text)) => {
                    if let Ok(v) = serde_json::from_str::<Value>(text.as_str()) {
                        self.message(v);
                    }
                }
                Ok(_) => {}
                Err(tungstenite::Error::Io(e))
                    if matches!(e.kind(), io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut) => {}
                Err(_) => {
                    let _ = self.events.send_blocking(BrowserEvent::Closed);
                    return;
                }
            }
        }
    }

    /// Commands wait until the page is attached and its user agent is set.
    fn ready(&self) -> bool {
        self.page.is_some() && !self.pending.values().any(|p| matches!(p, Pending::Version))
    }

    fn call(&mut self, session: Option<&str>, method: &str, params: Value) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        let mut msg = json!({"id": id, "method": method, "params": params});
        if let Some(s) = session {
            msg["sessionId"] = json!(s);
        }
        let _ = self.ws.send(Message::Text(msg.to_string().into()));
        id
    }

    fn page_call(&mut self, method: &str, params: Value) -> u64 {
        let page = self.page.clone();
        self.call(page.as_deref(), method, params)
    }

    fn emit(&self, event: BrowserEvent) {
        let _ = self.events.send_blocking(event);
    }

    fn message(&mut self, v: Value) {
        if let Some(id) = v["id"].as_u64() {
            match self.pending.remove(&id) {
                Some(Pending::Attached) => {
                    if let Some(s) = v.pointer("/result/sessionId").and_then(Value::as_str) {
                        self.page = Some(s.to_string());
                        self.page_call("Page.enable", json!({}));
                        self.install_copy_hook();
                        let id = self.call(None, "Browser.getVersion", json!({}));
                        self.pending.insert(id, Pending::Version);
                        let (w, h, scale) = self.viewport;
                        self.resize(w, h, scale);
                    } else {
                        self.emit(BrowserEvent::Error("Could not open a browser tab.".into()));
                        self.emit(BrowserEvent::Closed);
                    }
                }
                Some(Pending::Version) => {
                    // Headless Chrome's user agent says it is automated, so present it as plain Chrome.
                    if let Some(ua) = v.pointer("/result/userAgent").and_then(Value::as_str) {
                        let ua = ua.replace("HeadlessChrome", "Chrome");
                        self.page_call(
                            "Network.setUserAgentOverride",
                            json!({"userAgent": ua, "acceptLanguage": "en-US,en;q=0.9", "platform": "Linux"}),
                        );
                    }
                    // Queued commands go out only after the override, even if the version lookup failed.
                    for c in std::mem::take(&mut self.queued) {
                        self.command(c);
                    }
                }
                Some(Pending::Navigate) => {
                    if let Some(err) = v.pointer("/result/errorText").and_then(Value::as_str) {
                        self.emit(BrowserEvent::Loading(false));
                        self.emit(BrowserEvent::Error(friendly_error(err)));
                    }
                }
                Some(Pending::Screenshot) => {
                    // The screencast may have stopped while this was in flight.
                    if let Some(frame) = v.pointer("/result/data").and_then(Value::as_str).and_then(decode_frame) {
                        if self.visible {
                            self.emit(BrowserEvent::Frame(frame, self.viewport.0 as f32, self.viewport.1 as f32));
                        }
                    }
                }
                Some(Pending::Copy) => {
                    // An empty selection has nothing to copy.
                    if let Some(text) = v.pointer("/result/result/value").and_then(Value::as_str).filter(|t| !t.is_empty()) {
                        let _ = self.copies.send_blocking(text.to_string());
                    }
                }
                Some(Pending::History) => {
                    let current = v.pointer("/result/currentIndex").and_then(Value::as_u64);
                    let entries = v.pointer("/result/entries").and_then(Value::as_array);
                    if let (Some(current), Some(entries)) = (current, entries) {
                        let last = entries.len().saturating_sub(1) as u64;
                        self.emit(BrowserEvent::History { back: current > 0, forward: current < last });
                        // Chrome keeps the committed entry's title current, so the history is ahead of
                        // the target-info event, which lags a navigation.
                        if let Some(title) = entries.get(current as usize).and_then(|e| e["title"].as_str()) {
                            if !title.is_empty() {
                                self.emit(BrowserEvent::Title(title.to_string()));
                            }
                        }
                    }
                }
                None => {}
            }
            return;
        }
        let params = &v["params"];
        match v["method"].as_str().unwrap_or("") {
            "Runtime.bindingCalled" if params["name"] == "__archonuiCopy" => {
                if let Some(text) = params["payload"].as_str().filter(|t| !t.is_empty()) {
                    let _ = self.copies.send_blocking(text.to_string());
                }
            }
            "Target.targetCreated"
                if self.page.is_none()
                    && params["targetInfo"]["type"] == "page"
                    && !self.pending.values().any(|p| matches!(p, Pending::Attached)) =>
            {
                let target = params["targetInfo"]["targetId"].clone();
                let id = self.call(None, "Target.attachToTarget", json!({"targetId": target, "flatten": true}));
                self.pending.insert(id, Pending::Attached);
            }
            "Target.targetInfoChanged" if params["targetInfo"]["type"] == "page" => {
                if let Some(t) = params["targetInfo"]["title"].as_str() {
                    self.emit(BrowserEvent::Title(t.to_string()));
                }
            }
            "Page.screencastFrame" => {
                let ack = params["sessionId"].clone();
                self.page_call("Page.screencastFrameAck", json!({"sessionId": ack}));
                let meta = &params["metadata"];
                let (w, h) = (
                    meta["deviceWidth"].as_f64().unwrap_or(0.) as f32,
                    meta["deviceHeight"].as_f64().unwrap_or(0.) as f32,
                );
                if let Some(frame) = params["data"].as_str().and_then(decode_frame) {
                    self.emit(BrowserEvent::Frame(frame, w, h));
                }
            }
            "Page.frameNavigated" if params["frame"]["parentId"].is_null() => {
                if let Some(u) = params["frame"]["url"].as_str() {
                    self.emit(BrowserEvent::Url(u.to_string()));
                }
                self.request_history();
            }
            "Page.navigatedWithinDocument" => {
                if let Some(u) = params["url"].as_str() {
                    self.emit(BrowserEvent::Url(u.to_string()));
                }
                self.request_history();
            }
            "Page.frameStartedLoading" => self.emit(BrowserEvent::Loading(true)),
            "Page.frameStoppedLoading" => {
                self.emit(BrowserEvent::Loading(false));
                self.request_history();
                if self.visible {
                    self.capture_screenshot();
                }
            }
            "Target.targetCrashed" => self.emit(BrowserEvent::Error("The page crashed. Reload to try again.".into())),
            _ => {}
        }
    }

    /// Lets the page write to the clipboard and reports those writes to `copies`.
    /// Replies are not awaited: commands are already held until the user agent is set.
    fn install_copy_hook(&mut self) {
        self.call(None, "Browser.grantPermissions", json!({"permissions": ["clipboardReadWrite", "clipboardSanitizedWrite"]}));
        self.page_call("Runtime.enable", json!({}));
        self.page_call("Runtime.addBinding", json!({"name": "__archonuiCopy"}));
        self.page_call("Page.addScriptToEvaluateOnNewDocument", json!({"source": COPY_HOOK_JS}));
        self.page_call("Runtime.evaluate", json!({"expression": COPY_HOOK_JS}));
    }

    fn resize(&mut self, width: u32, height: u32, scale: f32) {
        self.viewport = (width, height, scale);
        self.page_call(
            "Emulation.setDeviceMetricsOverride",
            json!({"width": width, "height": height, "deviceScaleFactor": scale, "mobile": false}),
        );
        self.restart_screencast();
    }

    /// Restart the screencast so frames match the viewport. Hidden pages stream nothing.
    fn restart_screencast(&mut self) {
        self.page_call("Page.stopScreencast", json!({}));
        if !self.visible {
            return;
        }
        let (width, height, scale) = self.viewport;
        self.page_call(
            "Page.startScreencast",
            json!({
                "format": "jpeg",
                "quality": 85,
                "maxWidth": (width as f32 * scale).round() as u32,
                "maxHeight": (height as f32 * scale).round() as u32,
                "everyNthFrame": 1,
            }),
        );
        // A static page repaints rarely, so the screencast alone may show nothing yet.
        self.capture_screenshot();
    }

    /// One frame now, for pages that have stopped repainting.
    fn capture_screenshot(&mut self) {
        let id = self.page_call("Page.captureScreenshot", json!({"format": "jpeg", "quality": 85}));
        self.pending.insert(id, Pending::Screenshot);
    }

    fn command(&mut self, c: Command) {
        match c {
            Command::Navigate(url) => {
                self.emit(BrowserEvent::Loading(true));
                let id = self.page_call("Page.navigate", json!({"url": url}));
                self.pending.insert(id, Pending::Navigate);
            }
            Command::Back => self.eval("history.back()"),
            Command::Forward => self.eval("history.forward()"),
            Command::Reload => {
                self.page_call("Page.reload", json!({"ignoreCache": true}));
            }
            Command::Resize { width, height, scale } => {
                if (width, height, scale) != self.viewport && width > 0 && height > 0 {
                    self.resize(width, height, scale);
                }
            }
            Command::Mouse { kind, x, y, right, clicks } => {
                let bit = if right { 2 } else { 1 };
                match kind {
                    MouseKind::Down => self.buttons |= bit,
                    MouseKind::Up => self.buttons &= !bit,
                    MouseKind::Move => {}
                }
                let (kind, button) = match kind {
                    MouseKind::Down => ("mousePressed", if right { "right" } else { "left" }),
                    MouseKind::Up => ("mouseReleased", if right { "right" } else { "left" }),
                    MouseKind::Move if self.buttons & 1 != 0 => ("mouseMoved", "left"),
                    MouseKind::Move => ("mouseMoved", "none"),
                };
                self.page_call(
                    "Input.dispatchMouseEvent",
                    json!({"type": kind, "x": x, "y": y, "button": button, "buttons": self.buttons, "clickCount": clicks}),
                );
            }
            Command::Wheel { x, y, dx, dy } => {
                self.page_call(
                    "Input.dispatchMouseEvent",
                    json!({"type": "mouseWheel", "x": x, "y": y, "deltaX": dx, "deltaY": dy}),
                );
            }
            Command::SetVisible(visible) => {
                self.visible = visible;
                self.restart_screencast();
            }
            Command::InsertText(text) => {
                self.page_call("Input.insertText", json!({"text": text}));
            }
            Command::SetCookie { url, name, value } => {
                self.page_call(
                    "Network.setCookie",
                    json!({"url": url, "name": name, "value": value, "httpOnly": true, "path": "/"}),
                );
            }
            Command::CopySelection => {
                let id = self.page_call("Runtime.evaluate", json!({"expression": SELECTED_TEXT_JS, "returnByValue": true}));
                self.pending.insert(id, Pending::Copy);
            }
            Command::Key { key, ctrl, shift, alt, meta } => self.key(&key, ctrl, shift, alt, meta),
        }
    }

    fn eval(&mut self, js: &str) {
        self.page_call("Runtime.evaluate", json!({"expression": js}));
    }

    /// Asks the page for its navigation history, for the back/forward buttons and the current title.
    fn request_history(&mut self) {
        let id = self.page_call("Page.getNavigationHistory", json!({}));
        self.pending.insert(id, Pending::History);
    }

    /// Replay a key press. Printable keys go through `Input.insertText`
    /// (by the caller); this handles editing, navigation and shortcut keys.
    fn key(&mut self, key: &str, ctrl: bool, shift: bool, alt: bool, meta: bool) {
        let Some(press) = key_press(key, shift) else { return };
        let modifiers = (alt as u8) | (ctrl as u8) << 1 | (meta as u8) << 2 | (shift as u8) << 3;
        // Editing shortcuts need an explicit command in headless Chrome.
        let commands: Vec<&str> = match (ctrl, press.code.as_str()) {
            (true, "KeyA") => vec!["selectAll"],
            (true, "KeyZ") => vec!["undo"],
            _ => vec![],
        };
        let down = json!({
            "type": if press.text.is_empty() { "rawKeyDown" } else { "keyDown" },
            "key": press.key,
            "code": press.code,
            "windowsVirtualKeyCode": press.vk,
            "text": press.text,
            "modifiers": modifiers,
            "commands": commands,
        });
        self.page_call("Input.dispatchKeyEvent", down);
        self.page_call(
            "Input.dispatchKeyEvent",
            json!({"type": "keyUp", "key": press.key, "code": press.code, "windowsVirtualKeyCode": press.vk, "modifiers": modifiers}),
        );
    }
}

/// A key as `Input.dispatchKeyEvent` describes it.
#[derive(Debug, PartialEq)]
struct KeyPress {
    /// The key's value, e.g. "s", "S" or "Enter".
    key: String,
    /// The physical key, e.g. "KeyS" or "Digit1".
    code: String,
    /// The Windows virtual key code pages read as `keyCode`.
    vk: u32,
    /// What the key types; empty for keys that only act.
    text: &'static str,
}

/// The CDP description of a key named as GPUI names it, or `None` for keys
/// a page can't take. Letters are uppercase when shift is held.
fn key_press(key: &str, shift: bool) -> Option<KeyPress> {
    let press = |key: &str, code: &str, vk: u32, text: &'static str| {
        Some(KeyPress { key: key.into(), code: code.into(), vk, text })
    };
    let mut chars = key.chars();
    if let (Some(c), None) = (chars.next(), chars.next()) {
        return match c {
            'a'..='z' => {
                let upper = c.to_ascii_uppercase();
                let value = if shift { upper } else { c };
                press(&value.to_string(), &format!("Key{upper}"), upper as u32, "")
            }
            '0'..='9' => press(key, &format!("Digit{c}"), c as u32, ""),
            '`' => press(key, "Backquote", 192, ""),
            '-' => press(key, "Minus", 189, ""),
            '=' => press(key, "Equal", 187, ""),
            '[' => press(key, "BracketLeft", 219, ""),
            ']' => press(key, "BracketRight", 221, ""),
            '\\' => press(key, "Backslash", 220, ""),
            ';' => press(key, "Semicolon", 186, ""),
            '\'' => press(key, "Quote", 222, ""),
            ',' => press(key, "Comma", 188, ""),
            '.' => press(key, "Period", 190, ""),
            '/' => press(key, "Slash", 191, ""),
            _ => None,
        };
    }
    let (name, code, vk, text) = match key {
        "enter" => ("Enter", "Enter", 13, "\r"),
        "backspace" => ("Backspace", "Backspace", 8, ""),
        "tab" => ("Tab", "Tab", 9, ""),
        "escape" => ("Escape", "Escape", 27, ""),
        "delete" => ("Delete", "Delete", 46, ""),
        "left" => ("ArrowLeft", "ArrowLeft", 37, ""),
        "up" => ("ArrowUp", "ArrowUp", 38, ""),
        "right" => ("ArrowRight", "ArrowRight", 39, ""),
        "down" => ("ArrowDown", "ArrowDown", 40, ""),
        "home" => ("Home", "Home", 36, ""),
        "end" => ("End", "End", 35, ""),
        "pageup" => ("PageUp", "PageUp", 33, ""),
        "pagedown" => ("PageDown", "PageDown", 34, ""),
        "space" => (" ", "Space", 32, ""),
        _ => {
            let n: u32 = key.strip_prefix('f')?.parse().ok()?;
            if !(1..=12).contains(&n) {
                return None;
            }
            let name = format!("F{n}");
            return press(&name, &name, 111 + n, "");
        }
    };
    press(name, code, vk, text)
}

fn friendly_error(err: &str) -> String {
    match err {
        "net::ERR_CONNECTION_REFUSED" => {
            "Nothing is running at that address. If an agent started a server, it may have stopped when the reply ended.".into()
        }
        "net::ERR_FILE_NOT_FOUND" => "That file doesn't exist.".into(),
        "net::ERR_NAME_NOT_RESOLVED" => "That address couldn't be found.".into(),
        e => format!("Could not load the page ({e})."),
    }
}

/// JPEG (base64) -> GPUI image, converting RGBA to the BGRA GPUI expects.
fn decode_frame(data: &str) -> Option<Arc<RenderImage>> {
    let bytes = STANDARD.decode(data).ok()?;
    let mut rgba = image::load_from_memory_with_format(&bytes, image::ImageFormat::Jpeg).ok()?.into_rgba8();
    for px in rgba.chunks_exact_mut(4) {
        px.swap(0, 2);
    }
    let frame = image::Frame::new(rgba);
    Some(Arc::new(RenderImage::new(smallvec::smallvec![frame])))
}

/// Turn what the user typed into something to load: web, file, about and
/// data addresses as-is, existing paths as `file://`, local hosts as `http://`,
/// other domains as `https://`. Anything else, including schemes that must
/// never load (`javascript:`, `chrome://`, …), is searched on DuckDuckGo.
pub fn normalize_address(input: &str, cwd: &Path) -> String {
    let s = input.trim();
    if s.is_empty() {
        return "about:blank".into();
    }
    let lower = s.to_ascii_lowercase();
    if ["http://", "https://", "file://", "about:", "data:"].iter().any(|p| lower.starts_with(p)) {
        return s.to_string();
    }
    let other_scheme = s
        .split_once("://")
        .is_some_and(|(scheme, _)| scheme.chars().all(|c| c.is_ascii_alphanumeric() || "+-.".contains(c)));
    let blocked = ["javascript:", "chrome:", "chrome-extension:", "view-source:", "devtools:"];
    if other_scheme || blocked.iter().any(|p| lower.starts_with(p)) {
        return search_url(s);
    }
    let explicit_path = if let Some(rest) = s.strip_prefix("~/") {
        env::var_os("HOME").map(|home| PathBuf::from(home).join(rest))
    } else if ["/", "./", "../"].iter().any(|p| s.starts_with(p)) {
        Some(PathBuf::from(s))
    } else {
        None
    };
    if let Some(path) = explicit_path {
        return file_url(&path);
    }
    let in_cwd = cwd.join(s);
    if in_cwd.is_file() {
        return file_url(&in_cwd);
    }
    let authority = s.split(['/', '?', '#']).next().unwrap_or_default();
    let host = match authority.rsplit_once(':') {
        Some((host, port)) if !port.is_empty() && port.bytes().all(|b| b.is_ascii_digit()) => host,
        _ => authority,
    };
    if host.eq_ignore_ascii_case("localhost") || host == "[::1]" || is_ipv4(host) {
        return format!("http://{s}");
    }
    let labels: Vec<&str> = host.split('.').collect();
    let is_domain = !s.chars().any(char::is_whitespace)
        && labels.len() >= 2
        && labels.iter().all(|label| !label.is_empty())
        && labels.last().is_some_and(|tld| tld.len() >= 2 && tld.bytes().all(|b| b.is_ascii_alphabetic()));
    if is_domain {
        return format!("https://{s}");
    }
    search_url(s)
}

fn is_ipv4(host: &str) -> bool {
    let parts: Vec<&str> = host.split('.').collect();
    parts.len() == 4 && parts.iter().all(|p| p.bytes().all(|b| b.is_ascii_digit()) && p.parse::<u8>().is_ok())
}

/// Where the address bar's searches go; the settings page changes it.
static SEARCH_PREFIX: RwLock<&'static str> = RwLock::new("https://duckduckgo.com/?q=");

/// Sends the address bar's searches to `prefix`, which the form-encoded query is appended to.
pub fn set_search_engine(prefix: &'static str) {
    *SEARCH_PREFIX.write().unwrap_or_else(|e| e.into_inner()) = prefix;
}

/// A search URL for `query` on the chosen search engine, form-encoded.
fn search_url(query: &str) -> String {
    let mut url = String::from(*SEARCH_PREFIX.read().unwrap_or_else(|e| e.into_inner()));
    for b in query.bytes() {
        match b {
            b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => url.push(b as char),
            b' ' => url.push('+'),
            _ => url.push_str(&format!("%{b:02X}")),
        }
    }
    url
}

/// A `file://` URL for `path`, with the characters that would change how the
/// path parses percent-encoded.
fn file_url(path: &Path) -> String {
    let mut url = String::from("file://");
    for c in path.to_string_lossy().chars() {
        match c {
            ' ' | '#' | '?' | '%' => url.push_str(&format!("%{:02X}", c as u8)),
            _ => url.push(c),
        }
    }
    url
}

/// The last local web address or HTML file an agent's reply points at, if
/// any: `http(s)://localhost|127.0.0.1|0.0.0.0|[::1]…`, `file://…`, or a
/// path ending in `.html`/`.htm` that exists (relative to `cwd`).
/// Public URLs are ignored so documentation links don't hijack the preview.
pub fn find_preview_target(text: &str, cwd: &Path) -> Option<String> {
    // Strip quotes, brackets and markdown around a token, and sentence
    // punctuation after it, in any order (e.g. "`http://x/`.").
    let trim = |t: &str| {
        let mut t = t;
        loop {
            let next = t
                .trim_matches(|c: char| "`'\"()[]<>{},;*".contains(c))
                .trim_end_matches(['.', ':', '!', '?']);
            if next == t {
                return t.to_string();
            }
            t = next;
        }
    };
    let mut found = None;
    for token in text.split(|c: char| c.is_whitespace()).map(trim).filter(|t| !t.is_empty()) {
        if let Some(rest) = token.strip_prefix("http://").or_else(|| token.strip_prefix("https://")) {
            let host = rest.split(['/', ':', '?', '#']).next().unwrap_or("");
            if matches!(host, "localhost" | "127.0.0.1" | "0.0.0.0" | "[::1]") || rest.starts_with("[::1]") {
                found = Some(token.replacen("0.0.0.0", "localhost", 1));
            }
        } else if token.starts_with("file://") {
            found = Some(token);
        } else if token.ends_with(".html") || token.ends_with(".htm") {
            let path = if token.starts_with('/') { PathBuf::from(&token) } else { cwd.join(&token) };
            if path.is_file() {
                found = Some(file_url(&path.canonicalize().unwrap_or(path)));
            }
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stale_profiles_are_removed_without_following_symlinks() {
        let root = env::temp_dir().join(format!("archonui-preview-profiles-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        // A marked profile for a pid that cannot exist.
        let ours = root.join("chrome-4000000000-0");
        std::fs::create_dir_all(&ours).unwrap();
        std::fs::write(ours.join(PROFILE_MARKER), b"").unwrap();
        // An unmarked directory that must be left alone.
        let foreign = root.join("chrome-4000000001-0");
        std::fs::create_dir_all(&foreign).unwrap();
        std::fs::write(foreign.join("data"), b"keep").unwrap();
        // A profile name that is really a symlink to a directory we must not delete.
        let victim = root.join("victim");
        std::fs::create_dir_all(&victim).unwrap();
        std::fs::write(victim.join("keep"), b"keep").unwrap();
        std::os::unix::fs::symlink(&victim, root.join("chrome-4000000002-0")).unwrap();

        remove_stale_profiles(&root);
        assert!(!ours.exists(), "our stale profile is removed");
        assert!(foreign.join("data").exists(), "unmarked directories are kept");
        assert!(victim.join("keep").exists(), "symlinks are not followed");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn finds_local_targets_only() {
        let cwd = Path::new("/nonexistent");
        let t = "Docs at https://react.dev. Server running at `http://localhost:5173/`.";
        assert_eq!(find_preview_target(t, cwd).as_deref(), Some("http://localhost:5173/"));
        assert_eq!(find_preview_target("see http://0.0.0.0:8000", cwd).as_deref(), Some("http://localhost:8000"));
        assert_eq!(find_preview_target("read https://example.com/page.html", cwd), None);
        assert_eq!(find_preview_target("I made index.html", cwd), None, "missing files are ignored");
    }

    #[test]
    fn finds_existing_html_files() {
        let dir = env::temp_dir().join(format!("archonui-preview-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("index.html"), "<p>hi</p>").unwrap();
        let got = find_preview_target("Created **index.html** with a button.", &dir).unwrap();
        assert!(got.starts_with("file://") && got.ends_with("/index.html"), "{got}");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn normalizes_addresses() {
        let cwd = Path::new("/tmp");
        assert_eq!(normalize_address("", cwd), "about:blank");
        assert_eq!(normalize_address("localhost:3000", cwd), "http://localhost:3000");
        assert_eq!(normalize_address("127.0.0.1:8080/x", cwd), "http://127.0.0.1:8080/x");
        assert_eq!(normalize_address("example.com", cwd), "https://example.com");
        assert_eq!(normalize_address("github.com:443", cwd), "https://github.com:443");
        assert_eq!(
            normalize_address("news.ycombinator.com/item?id=1", cwd),
            "https://news.ycombinator.com/item?id=1"
        );
        assert_eq!(normalize_address("https://a.b", cwd), "https://a.b");
        assert_eq!(normalize_address("/srv/x.html", cwd), "file:///srv/x.html");
    }

    #[test]
    fn search_defaults_to_duckduckgo() {
        assert_eq!(normalize_address("rust gpui", Path::new("/tmp")), "https://duckduckgo.com/?q=rust+gpui");
    }

    #[test]
    fn searches_everything_else() {
        let cwd = Path::new("/tmp");
        assert_eq!(
            normalize_address("rust gpui tutorial", cwd),
            "https://duckduckgo.com/?q=rust+gpui+tutorial"
        );
        assert_eq!(normalize_address("c++ & rust", cwd), "https://duckduckgo.com/?q=c%2B%2B+%26+rust");
        assert_eq!(normalize_address("v1.2", cwd), "https://duckduckgo.com/?q=v1.2");
        assert_eq!(
            normalize_address("javascript:alert(1)", cwd),
            "https://duckduckgo.com/?q=javascript%3Aalert%281%29"
        );
        assert_eq!(normalize_address("chrome://settings", cwd), "https://duckduckgo.com/?q=chrome%3A%2F%2Fsettings");
    }

    #[test]
    fn percent_encodes_file_paths() {
        assert_eq!(
            normalize_address("/srv/100% my #1/x.html", Path::new("/tmp")),
            "file:///srv/100%25%20my%20%231/x.html"
        );
    }

    #[test]
    fn maps_shortcut_keys() {
        let press = |key: &str, code: &str, vk: u32, text: &'static str| {
            Some(KeyPress { key: key.into(), code: code.into(), vk, text })
        };
        assert_eq!(key_press("s", false), press("s", "KeyS", 83, ""));
        assert_eq!(key_press("s", true), press("S", "KeyS", 83, ""));
        assert_eq!(key_press("f5", false), press("F5", "F5", 116, ""));
        assert_eq!(key_press("f13", false), None);
        assert_eq!(key_press("1", false), press("1", "Digit1", 49, ""));
        assert_eq!(key_press("`", false), press("`", "Backquote", 192, ""));
        assert_eq!(key_press("space", true), press(" ", "Space", 32, ""));
        assert_eq!(key_press("enter", false), press("Enter", "Enter", 13, "\r"));
        assert_eq!(key_press("shift", false), None);
    }

    /// Clipboard writes from the page reach `copies`. Needs Chrome:
    /// `cargo test copy_hook -- --ignored`.
    #[test]
    #[ignore = "launches headless Chrome"]
    fn copy_hook() {
        use std::io::{Read, Write};
        // `navigator.clipboard` exists only in secure contexts, which `data:` URLs aren't,
        // so the page is served from localhost.
        const PAGE: &str = r#"<button id=b style='position:fixed;left:0;top:0;width:200px;height:100px'>copy</button>
            <script>
            navigator.clipboard.writeText('from-writeText');
            document.addEventListener('copy', e => { e.clipboardData.setData('text/plain', 'from-copy-event'); e.preventDefault(); });
            document.getElementById('b').addEventListener('click', () => document.execCommand('copy'));
            </script>"#;
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let page = format!("http://{}/", listener.local_addr().unwrap());
        thread::spawn(move || {
            for mut stream in listener.incoming().flatten() {
                let mut request = [0; 4096];
                let _ = stream.read(&mut request);
                let reply = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{PAGE}",
                    PAGE.len()
                );
                let _ = stream.write_all(reply.as_bytes());
            }
        });
        let browser = Browser::launch().expect("Chrome launches");
        browser.send(Command::Navigate(page));

        // Click once the page has loaded, so the button is laid out.
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut started = false;
        loop {
            match browser.events.try_recv() {
                Ok(BrowserEvent::Loading(true)) => started = true,
                Ok(BrowserEvent::Loading(false)) if started => break,
                Ok(BrowserEvent::Error(e)) => panic!("{e}"),
                _ => {}
            }
            assert!(Instant::now() < deadline, "page did not load");
            thread::sleep(Duration::from_millis(20));
        }
        for kind in [MouseKind::Down, MouseKind::Up] {
            browser.send(Command::Mouse { kind, x: 100.0, y: 50.0, right: false, clicks: 1 });
        }

        let deadline = Instant::now() + Duration::from_secs(10);
        let mut copied = Vec::new();
        while !(copied.iter().any(|t| t == "from-writeText") && copied.iter().any(|t| t == "from-copy-event")) {
            match browser.copies.try_recv() {
                Ok(text) => copied.push(text),
                Err(_) => {
                    assert!(Instant::now() < deadline, "copies arrived: {copied:?}");
                    thread::sleep(Duration::from_millis(20));
                }
            }
        }
    }
}

//! Embedded terminal: a shell on a pseudo-terminal, parsed by `vt100`.
//!
//! A reader thread feeds the shell's output to a `vt100::Parser` and sends
//! `TermEvent::Changed` when the screen changed. The UI takes a `Screen`
//! snapshot to draw, so it never holds the parser's lock while painting.
//!
//! With tmux on PATH, the shell runs in a tmux session on ArchonUI's own tmux
//! server, so it outlives the `Terminal` and this process.

use crate::store;
use portable_pty::{native_pty_system, Child, CommandBuilder, MasterPty, PtySize};
use smol::channel::{unbounded, Receiver, Sender};
use std::{
    env,
    ffi::OsStr,
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex, MutexGuard, OnceLock, PoisonError,
    },
    thread,
    time::Duration,
};

/// Lines of history kept above the visible screen.
const SCROLLBACK: usize = 5000;

/// Config for ArchonUI's tmux server, written by `tmux_conf`. Mouse reporting stays off,
/// so the wheel scrolls through tmux's copy mode (`tmux_scroll`).
const TMUX_CONF: &str = r##"set -g default-terminal "tmux-256color"
set -ga terminal-overrides ",xterm-256color:Tc"
set -g mouse off
set -g history-limit 10000
set -g escape-time 10
set -g status-style "bg=#181825,fg=#a6adc8"
set -g status-left "[#S] "
set -g status-left-length 40
set -g set-titles on
set -g set-titles-string "#{pane_current_path}"
"##;

pub enum TermEvent {
    /// New output was parsed. At most one is queued until `Terminal::screen` runs.
    Changed,
    /// The shell exited with this code, if it reported one.
    Exited(Option<u32>),
}

/// A shell running on a pseudo-terminal. Dropping it hangs up the shell, or only
/// detaches from a tmux session, which keeps the shell running.
pub struct Terminal {
    master: Box<dyn MasterPty + Send>,
    writer: Mutex<Box<dyn Write + Send>>,
    parser: Arc<Mutex<vt100::Parser<WindowTitle>>>,
    /// Set while a `Changed` event is queued and `screen` hasn't run since.
    changed_queued: Arc<AtomicBool>,
    /// Set once the reader has reaped the shell.
    exited: Arc<AtomicBool>,
    /// The shell's pid, which is also its process group id.
    pid: Option<u32>,
    /// The tmux session the shell runs in, if it runs in one.
    tmux: Option<String>,
}

/// A snapshot of the visible screen.
#[derive(Debug, Clone)]
pub struct Screen {
    pub rows: Vec<Vec<Span>>,
    /// `(row, col)` of the cursor, or `None` when it is hidden or the view is scrolled back.
    pub cursor: Option<(u16, u16)>,
    pub title: String,
    pub alternate: bool,
}

/// Consecutive cells with the same colors and attributes.
#[derive(Debug, Clone)]
pub struct Span {
    pub text: String,
    /// `0xRRGGBB`, or `None` for the default color.
    pub fg: Option<u32>,
    pub bg: Option<u32>,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub inverse: bool,
}

/// Keeps the window title the shell sets with an OSC 0 or 2 sequence.
#[derive(Default)]
struct WindowTitle(String);

impl vt100::Callbacks for WindowTitle {
    fn set_window_title(&mut self, _: &mut vt100::Screen, title: &[u8]) {
        self.0 = String::from_utf8_lossy(title).into_owned();
    }
}

impl Terminal {
    /// Starts the user's shell (`$SHELL`, else `/bin/bash`) in `cwd` at `cols` x `rows`.
    /// With `tmux_session` and tmux on PATH, the shell runs in that tmux session, attached
    /// if it already exists. Otherwise it is a plain shell.
    pub fn spawn(
        cwd: &Path,
        cols: u16,
        rows: u16,
        tmux_session: Option<&str>,
    ) -> Result<(Self, Receiver<TermEvent>), String> {
        let pair = native_pty_system()
            .openpty(pty_size(cols, rows))
            .map_err(|e| format!("Could not open a terminal: {e}"))?;
        let shell = env::var("SHELL").ok().filter(|s| !s.is_empty()).unwrap_or_else(|| "/bin/bash".into());
        let tmux = tmux_session.and_then(|name| Some((tmux_bin()?, tmux_conf()?, name)));
        let tmux_name = tmux.map(|(_, _, name)| name.to_string());
        let mut command = match tmux {
            Some((bin, conf, name)) => {
                let socket = tmux_socket();
                let mut command = CommandBuilder::new(bin);
                command.args(["-L", socket.as_str(), "-f"]);
                command.arg(conf);
                command.args(["new-session", "-A", "-s", name, "-c"]);
                command.arg(cwd);
                // tmux refuses to start a session inside another tmux, whatever the socket.
                command.env_remove("TMUX");
                command.env_remove("TMUX_PANE");
                command
            }
            None => CommandBuilder::new(&shell),
        };
        command.cwd(cwd);
        command.env("TERM", "xterm-256color");
        command.env("COLORTERM", "truecolor");
        // Handles first, so a failure here doesn't leave a shell running unattended.
        let reader = pair.master.try_clone_reader().map_err(|e| e.to_string())?;
        let writer = pair.master.take_writer().map_err(|e| e.to_string())?;
        let child = pair
            .slave
            .spawn_command(command)
            .map_err(|e| format!("Could not start the shell: {e}"))?;
        // Only the shell keeps the slave open, so the reader sees EOF when it exits.
        drop(pair.slave);
        let pid = child.process_id();
        let parser = Arc::new(Mutex::new(vt100::Parser::new_with_callbacks(
            rows,
            cols,
            SCROLLBACK,
            WindowTitle::default(),
        )));
        let changed_queued = Arc::new(AtomicBool::new(false));
        let exited = Arc::new(AtomicBool::new(false));
        let (events_tx, events_rx) = unbounded();
        let term = Self {
            master: pair.master,
            writer: Mutex::new(writer),
            parser: parser.clone(),
            changed_queued: changed_queued.clone(),
            exited: exited.clone(),
            pid,
            tmux: tmux_name,
        };
        thread::spawn(move || read_output(reader, child, parser, changed_queued, exited, events_tx));
        Ok((term, events_rx))
    }

    /// The tmux session the shell runs in, if it runs in one.
    pub fn tmux(&self) -> Option<&str> {
        self.tmux.as_deref()
    }

    /// Sends input to the shell.
    pub fn write(&self, bytes: &[u8]) {
        let _ = lock(&self.writer).write_all(bytes);
    }

    /// Resizes the pty and the parser. Ignored when the size is unchanged or zero.
    pub fn resize(&self, cols: u16, rows: u16) {
        if cols == 0 || rows == 0 {
            return;
        }
        let mut parser = lock(&self.parser);
        if parser.screen().size() == (rows, cols) {
            return;
        }
        parser.screen_mut().set_size(rows, cols);
        let _ = self.master.resize(pty_size(cols, rows));
    }

    /// A snapshot of the visible screen, taken without holding the parser's lock.
    pub fn screen(&self) -> Screen {
        let parser = lock(&self.parser);
        self.changed_queued.store(false, Ordering::Release);
        snapshot(&parser)
    }

    /// Scrolls the view `lines` up into scrollback, clamped to what exists. 0 is the live screen.
    pub fn set_scrollback(&self, lines: usize) {
        lock(&self.parser).screen_mut().set_scrollback(lines);
    }

    pub fn scrollback(&self) -> usize {
        lock(&self.parser).screen().scrollback()
    }

    /// Whether the shell has turned on bracketed paste mode.
    pub fn bracketed_paste(&self) -> bool {
        lock(&self.parser).screen().bracketed_paste()
    }

    /// Whether the app asked for application cursor keys (vim, less, ...).
    pub fn application_cursor(&self) -> bool {
        lock(&self.parser).screen().application_cursor()
    }
}

impl Drop for Terminal {
    fn drop(&mut self) {
        if self.exited.load(Ordering::Acquire) {
            return;
        }
        let Some(pid) = self.pid else { return };
        let group = -(pid as i32);
        unsafe { libc::kill(group, libc::SIGHUP) };
        // Escalate on a detached thread so dropping never blocks the caller.
        let exited = self.exited.clone();
        thread::spawn(move || {
            thread::sleep(Duration::from_secs(1));
            if !exited.load(Ordering::Acquire) {
                unsafe { libc::kill(group, libc::SIGKILL) };
            }
        });
    }
}

fn pty_size(cols: u16, rows: u16) -> PtySize {
    PtySize { rows, cols, pixel_width: 0, pixel_height: 0 }
}

/// Locks a mutex, ignoring poisoning: a panic on the reader thread must not panic the UI too.
fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Feeds the shell's output to the parser until the pty closes, then reports the exit.
fn read_output(
    mut reader: Box<dyn Read + Send>,
    mut child: Box<dyn Child + Send + Sync>,
    parser: Arc<Mutex<vt100::Parser<WindowTitle>>>,
    changed_queued: Arc<AtomicBool>,
    exited: Arc<AtomicBool>,
    events: Sender<TermEvent>,
) {
    let mut buf = [0u8; 4096];
    loop {
        match reader.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => {
                lock(&parser).process(&buf[..n]);
                if !changed_queued.swap(true, Ordering::AcqRel) {
                    let _ = events.send_blocking(TermEvent::Changed);
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
            Err(_) => break,
        }
    }
    let code = child.wait().ok().map(|status| status.exit_code());
    exited.store(true, Ordering::Release);
    let _ = events.send_blocking(TermEvent::Exited(code));
}

/// Copies the visible screen, merging neighboring cells with the same style.
fn snapshot(parser: &vt100::Parser<WindowTitle>) -> Screen {
    let screen = parser.screen();
    let (rows, cols) = screen.size();
    let scrolled_back = screen.scrollback() > 0;
    Screen {
        rows: (0..rows).map(|row| row_spans(screen, row, cols)).collect(),
        cursor: (!screen.hide_cursor() && !scrolled_back).then(|| screen.cursor_position()),
        title: parser.callbacks().0.clone(),
        alternate: screen.alternate_screen(),
    }
}

fn row_spans(screen: &vt100::Screen, row: u16, cols: u16) -> Vec<Span> {
    let mut spans: Vec<Span> = Vec::new();
    for col in 0..cols {
        let Some(cell) = screen.cell(row, col) else { continue };
        // The second half of a wide character has no text of its own.
        if cell.is_wide_continuation() {
            continue;
        }
        let text = if cell.has_contents() { cell.contents() } else { " " };
        let span = Span {
            text: text.to_string(),
            fg: color(cell.fgcolor(), cell.bold()),
            bg: color(cell.bgcolor(), false),
            bold: cell.bold(),
            italic: cell.italic(),
            underline: cell.underline(),
            inverse: cell.inverse(),
        };
        match spans.last_mut() {
            Some(last) if same_style(last, &span) => last.text.push_str(text),
            _ => spans.push(span),
        }
    }
    spans
}

fn same_style(a: &Span, b: &Span) -> bool {
    a.fg == b.fg
        && a.bg == b.bg
        && a.bold == b.bold
        && a.italic == b.italic
        && a.underline == b.underline
        && a.inverse == b.inverse
}

/// `0xRRGGBB` for a cell color. Bold brightens a dim ANSI color, as terminals do for text.
fn color(color: vt100::Color, bold: bool) -> Option<u32> {
    match color {
        vt100::Color::Default => None,
        vt100::Color::Idx(n) => Some(palette(if bold && n < 8 { n + 8 } else { n })),
        vt100::Color::Rgb(r, g, b) => Some(rgb(r, g, b)),
    }
}

/// The xterm 256-color palette: 16 ANSI colors, a 6x6x6 cube, then greys.
fn palette(index: u8) -> u32 {
    const ANSI: [u32; 16] = [
        0x000000, 0xcd3131, 0x0dbc79, 0xe5e510, 0x2472c8, 0xbc3fbc, 0x11a8cd, 0xe5e5e5, 0x666666, 0xf14c4c,
        0x23d18b, 0xf5f543, 0x3b8eea, 0xd670d6, 0x29b8db, 0xffffff,
    ];
    const LEVELS: [u8; 6] = [0, 95, 135, 175, 215, 255];
    match index {
        0..=15 => ANSI[usize::from(index)],
        16..=231 => {
            let i = usize::from(index - 16);
            rgb(LEVELS[i / 36], LEVELS[i / 6 % 6], LEVELS[i % 6])
        }
        232..=255 => {
            let grey = 8 + 10 * (index - 232);
            rgb(grey, grey, grey)
        }
    }
}

fn rgb(r: u8, g: u8, b: u8) -> u32 {
    u32::from(r) << 16 | u32::from(g) << 8 | u32::from(b)
}

/// The tmux session of a chat: `archonui-` and the chat id, with each character tmux
/// rejects in a session name (such as `.` and `:`) replaced by `_`.
pub fn tmux_session_name(chat_id: &str) -> String {
    let id = chat_id.strip_prefix("archonui-").unwrap_or(chat_id);
    let safe: String = id
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
        .collect();
    format!("archonui-{safe}")
}

/// The tmux server's socket name: `ARCHONUI_TMUX_SOCKET`, else `archonui`. It keeps ArchonUI's
/// sessions off the user's own tmux server.
pub fn tmux_socket() -> String {
    env::var("ARCHONUI_TMUX_SOCKET").ok().filter(|s| !s.is_empty()).unwrap_or_else(|| "archonui".into())
}

/// Kills a tmux session, and the shell in it. Waits for tmux, which is quick, so a new
/// session can start right after. Errors are ignored: the session may already be gone.
pub fn tmux_kill(name: &str) {
    let Some(bin) = tmux_bin() else { return };
    let _ = tmux_command(bin, &["kill-session", "-t", &format!("={name}")]).status();
}

/// The names of the sessions on ArchonUI's tmux server. Empty when there is no server.
pub fn tmux_sessions() -> Vec<String> {
    let Some(bin) = tmux_bin() else { return Vec::new() };
    let Ok(output) = Command::new(bin)
        .arg("-L")
        .arg(tmux_socket())
        .args(["list-sessions", "-F", "#{session_name}"])
        .stderr(Stdio::null())
        .output()
    else {
        return Vec::new();
    };
    if !output.status.success() {
        return Vec::new();
    }
    String::from_utf8_lossy(&output.stdout).lines().map(str::to_owned).collect()
}

/// Scrolls a tmux session's pane `lines` up or down, entering copy mode first. Doesn't
/// wait for tmux, so the wheel never stalls the UI. The `:` makes `=name` a pane target.
pub fn tmux_scroll(name: &str, up: bool, lines: u32) {
    let Some(bin) = tmux_bin() else { return };
    let target = format!("={name}:");
    let lines = lines.to_string();
    let direction = if up { "scroll-up" } else { "scroll-down" };
    let mut enter = tmux_command(bin, &["copy-mode", "-e", "-t", &target]);
    let mut scroll = tmux_command(bin, &["send-keys", "-t", &target, "-X", "-N", &lines, direction]);
    thread::spawn(move || {
        let _ = enter.status();
        let _ = scroll.status();
    });
}

/// Leaves copy mode, if the session is in it. Waits, so that a key sent next isn't
/// taken by copy mode.
pub fn tmux_cancel(name: &str) {
    let Some(bin) = tmux_bin() else { return };
    let _ = tmux_command(bin, &["send-keys", "-t", &format!("={name}:"), "-X", "cancel"]).status();
}

/// A `tmux` command on ArchonUI's server, with its output discarded.
fn tmux_command(bin: &Path, args: &[&str]) -> Command {
    let mut command = Command::new(bin);
    command.arg("-L").arg(tmux_socket()).args(args).stdout(Stdio::null()).stderr(Stdio::null());
    command
}

/// The `tmux` on PATH, looked up once.
fn tmux_bin() -> Option<&'static Path> {
    static BIN: OnceLock<Option<PathBuf>> = OnceLock::new();
    BIN.get_or_init(|| find_in(&env::var_os("PATH")?, "tmux")).as_deref()
}

/// The first file named `name` in the `:`-separated `dirs`.
fn find_in(dirs: &OsStr, name: &str) -> Option<PathBuf> {
    // An empty entry (a stray `:` in PATH) would look in the working directory, so skip it, as `ide` does.
    env::split_paths(dirs).filter(|dir| !dir.as_os_str().is_empty()).map(|dir| dir.join(name)).find(|path| path.is_file())
}

/// ArchonUI's tmux config, written next to the session store the first time it's needed.
/// `None` if it can't be written, and then shells are plain.
fn tmux_conf() -> Option<&'static Path> {
    static CONF: OnceLock<Option<PathBuf>> = OnceLock::new();
    CONF.get_or_init(|| {
        let path = store::dir().with_file_name("tmux.conf");
        fs::create_dir_all(path.parent()?).ok()?;
        fs::write(&path, TMUX_CONF).ok()?;
        Some(path)
    })
    .as_deref()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn palette_maps_indices() {
        assert_eq!(palette(1), 0xcd3131);
        assert_eq!(palette(196), 0xff0000);
        assert_eq!(palette(244), 0x808080);
        assert_eq!(color(vt100::Color::Idx(1), true), Some(0xf14c4c));
        assert_eq!(color(vt100::Color::Idx(1), false), Some(0xcd3131));
        assert_eq!(color(vt100::Color::Rgb(1, 2, 3), false), Some(0x010203));
        assert_eq!(color(vt100::Color::Default, false), None);
    }

    #[test]
    fn snapshot_merges_spans_of_equal_style() {
        let mut parser = vt100::Parser::new_with_callbacks(2, 20, 0, WindowTitle::default());
        parser.process(b"ab\x1b[31mcd\x1b[0me");
        let screen = snapshot(&parser);
        let spans = &screen.rows[0];
        assert_eq!(spans.len(), 3);
        assert_eq!(spans[0].text, "ab");
        assert_eq!(spans[1].text, "cd");
        assert_eq!(spans[1].fg, Some(0xcd3131));
        assert_eq!(spans[2].text, format!("e{}", " ".repeat(15)));
        assert_eq!(spans[2].fg, None);
    }

    #[test]
    fn tmux_session_names_are_tmux_safe() {
        assert_eq!(tmux_session_name("archonui-18dd2-38e6ad"), "archonui-18dd2-38e6ad");
        assert_eq!(tmux_session_name("a.b:c d"), "archonui-a_b_c_d");
    }

    #[test]
    fn tmux_is_found_only_where_it_is() {
        let dir = env::temp_dir().join(format!("archonui-terminal-test-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        assert_eq!(find_in(dir.as_os_str(), "tmux"), None);
        fs::write(dir.join("tmux"), b"").unwrap();
        assert_eq!(find_in(dir.as_os_str(), "tmux"), Some(dir.join("tmux")));
        fs::remove_dir_all(&dir).unwrap();
    }
}

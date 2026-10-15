//! The share modal: exports the focused session as Markdown or JSON. Links, visibility and invites need a
//! server ArchonUI doesn't have, so they render disabled. Spec: docs/design/layout.md (share scrim 0.6, radius 12).

use super::{
    shell::Shell,
    theme::Theme,
    widgets::{
        popover::{tooltip, TooltipColors},
        toggle::Toggle,
    },
};
use crate::{
    engine,
    store::{Message, Role, Session, ToolKind},
};
use gpui::{
    div, hsla, point, prelude::*, AnyElement, BoxShadow, ClipboardItem, Context, Div, FontWeight, KeyDownEvent,
    MouseButton, SharedString, Stateful, Window,
};
use crate::ui::scale::px;
use std::fmt::Write as _;
use std::path::Path;

/// Why the link, visibility and invite controls are disabled.
const NO_SERVER: &str = "Sharing links needs a server; export instead";

/// Share modal state, not persisted.
pub struct ShareUi {
    /// The session the open modal is about.
    session: Option<String>,
    /// "Include file diffs", on by default as the prototype has it.
    diffs: bool,
    /// "Include terminal output", off by default as the prototype has it.
    terminal: bool,
}

impl Default for ShareUi {
    fn default() -> Self {
        Self { session: None, diffs: true, terminal: false }
    }
}

/// The export formats: the file extension and how the session is written.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Format {
    Markdown,
    Json,
}

impl Format {
    fn ext(self) -> &'static str {
        match self {
            Format::Markdown => "md",
            Format::Json => "json",
        }
    }
}

/// Session facts for the Markdown header that the session doesn't store itself, as display names.
pub struct Details {
    pub harness: String,
    pub model: String,
    pub effort: String,
    pub project: Option<String>,
}

/// The extra content the share toggles add to an export: file diffs and terminal output. `None`
/// means the matching toggle is off (or nothing was recorded), so the section is left out.
#[derive(Default)]
pub struct Extras {
    /// One `(file, diff)` per file the session's edit/write calls touched.
    pub diffs: Option<Vec<(String, String)>>,
    /// The session's tmux pane history.
    pub terminal: Option<String>,
}

/// The session as Markdown: a title, a metadata list, one section per message, then the included
/// diff and terminal sections.
pub fn markdown(s: &Session, details: &Details, extras: &Extras) -> String {
    let mut out = format!("# {}\n\n", title(s));
    let mut meta = vec![
        ("Harness", details.harness.clone()),
        ("Model", details.model.clone()),
        ("Effort", details.effort.clone()),
        ("Mode", s.mode.name().to_string()),
    ];
    if let Some(project) = &details.project {
        meta.push(("Project", project.clone()));
    }
    meta.push(("Folder", s.cwd.display().to_string()));
    meta.push(("Created", stamp(s.created)));
    meta.push(("Updated", stamp(s.updated)));
    for (key, value) in meta {
        let _ = writeln!(out, "- **{key}:** {value}");
    }
    out.push('\n');
    for message in &s.messages {
        out.push_str(&message_section(message, &details.harness));
    }
    if let Some(diffs) = &extras.diffs {
        out.push_str(&diffs_section(diffs));
    }
    if let Some(terminal) = &extras.terminal {
        out.push_str(&terminal_section(terminal));
    }
    out
}

/// The file diffs as "## File diffs" with a "### <file>" and a `diff` fence per entry. Empty when
/// no entry has text.
fn diffs_section(diffs: &[(String, String)]) -> String {
    let present: Vec<_> = diffs.iter().filter(|(_, diff)| !diff.trim().is_empty()).collect();
    if present.is_empty() {
        return String::new();
    }
    let mut out = String::from("## File diffs\n\n");
    for (file, diff) in present {
        let diff = diff.trim_end();
        let fence = fence_for(diff);
        let _ = write!(out, "### {}\n\n{fence}diff\n{diff}\n{fence}\n\n", one_line(file));
    }
    out
}

/// The terminal output as "## Terminal output" and one `text` fence. Empty when there is none.
fn terminal_section(output: &str) -> String {
    let output = output.trim_end();
    if output.is_empty() {
        return String::new();
    }
    let fence = fence_for(output);
    format!("## Terminal output\n\n{fence}text\n{output}\n{fence}\n\n")
}

/// The longest run of backticks in `content`.
fn longest_ticks(content: &str) -> usize {
    let (mut longest, mut run) = (0, 0);
    for c in content.chars() {
        run = if c == '`' { run + 1 } else { 0 };
        longest = longest.max(run);
    }
    longest
}

/// A backtick fence longer than any backtick run in `content` (at least three), so the content can't close it.
fn fence_for(content: &str) -> String {
    "`".repeat(longest_ticks(content).max(2) + 1)
}

/// `text` as inline code whose delimiters can't be closed by backticks inside it.
fn inline_code(text: &str) -> String {
    let text = one_line(text);
    let ticks = "`".repeat(longest_ticks(&text) + 1);
    let pad = if text.starts_with('`') || text.ends_with('`') { " " } else { "" };
    format!("{ticks}{pad}{text}{pad}{ticks}")
}

/// `text` with line breaks turned into spaces, for headings and list items.
fn one_line(text: &str) -> String {
    text.replace(['\r', '\n'], " ")
}

/// One message: a heading naming the speaker, its text, then its tool calls and attachments as lists.
/// Thinking is not part of a message, so it isn't written.
fn message_section(m: &Message, harness: &str) -> String {
    let who = match m.role {
        Role::User => "You",
        Role::Agent => harness,
        Role::Error => "Error",
    };
    let mut out = format!("## {who}\n\n");
    let text = m.text.trim();
    if !text.is_empty() {
        out.push_str(text);
        out.push_str("\n\n");
    }
    if !m.tools.is_empty() {
        out.push_str("Tool calls:\n\n");
        for tool in &m.tools {
            let _ = write!(out, "- {:?} {}", tool.kind, inline_code(&tool.target));
            if let Some(detail) = &tool.detail {
                let _ = write!(out, ", {}", one_line(detail));
            }
            out.push_str(match tool.ok {
                Some(true) => " (ok)",
                Some(false) => " (failed)",
                None => " (running)",
            });
            out.push('\n');
        }
        out.push('\n');
    }
    if !m.attachments.is_empty() {
        out.push_str("Attachments:\n\n");
        for file in &m.attachments {
            let _ = writeln!(out, "- {}", file.name);
        }
        out.push('\n');
    }
    out
}

/// The text an export writes: Markdown, or the session as it's stored on disk. JSON adds
/// `file_diffs` and `terminal_output` for whichever sections are included.
fn export_body(format: Format, s: &Session, details: &Details, extras: &Extras) -> Result<String, serde_json::Error> {
    match format {
        Format::Markdown => Ok(markdown(s, details, extras)),
        Format::Json => {
            let mut value = serde_json::to_value(s)?;
            if let Some(object) = value.as_object_mut() {
                if let Some(diffs) = extras.diffs.as_deref().filter(|d| !d.is_empty()) {
                    let list: Vec<serde_json::Value> = diffs
                        .iter()
                        .map(|(file, diff)| serde_json::json!({ "file": file, "diff": diff }))
                        .collect();
                    object.insert("file_diffs".into(), serde_json::Value::Array(list));
                }
                if let Some(terminal) = extras.terminal.as_deref().filter(|t| !t.trim().is_empty()) {
                    object.insert("terminal_output".into(), serde_json::Value::String(terminal.to_string()));
                }
            }
            serde_json::to_string_pretty(&value)
        }
    }
}

/// The export toggles' content for `s`: the files its edit/write calls touched, and its tmux pane
/// history. `None` when the matching toggle is off or nothing was recorded.
fn extras(s: &Session, diffs: bool, terminal: bool) -> Extras {
    Extras {
        diffs: diffs.then(|| file_diffs(s)).filter(|d| !d.is_empty()),
        terminal: terminal.then(|| terminal_output(&s.id)).filter(|t| !t.trim().is_empty()),
    }
}

/// Whether `detail` contains diff text (e.g. unified diff headers or hunks) rather than a count
/// summary like "+24 −8" or "128 passed".
fn is_diff_text(detail: &str) -> bool {
    let text = detail.trim();
    if text.is_empty() {
        return false;
    }
    text.contains("@@")
        || text.starts_with("diff ")
        || text.starts_with("--- ")
        || (text.contains('\n') && text.lines().any(|l| l.starts_with('+') || l.starts_with('-')))
}

/// One entry per edit/write tool call, oldest first, duplicate files collapsed: recorded diff text
/// if a tool call has it, else the file's `git diff` in the session's folder. Files whose diff is
/// empty are left out.
fn file_diffs(s: &Session) -> Vec<(String, String)> {
    let mut files: Vec<String> = Vec::new();
    for tool in s.messages.iter().flat_map(|m| &m.tools) {
        if matches!(tool.kind, ToolKind::Edit | ToolKind::Write) && !files.contains(&tool.target) {
            files.push(tool.target.clone());
        }
    }
    files
        .into_iter()
        .filter_map(|file| {
            let recorded: Vec<&str> = s
                .messages
                .iter()
                .flat_map(|m| &m.tools)
                .filter(|t| matches!(t.kind, ToolKind::Edit | ToolKind::Write) && t.target == file)
                .filter_map(|t| t.detail.as_deref())
                .filter(|d| is_diff_text(d))
                .collect();
            let diff = if !recorded.is_empty() {
                recorded.join("\n\n")
            } else {
                git_diff(&s.cwd, &file)
            };
            (!diff.trim().is_empty()).then(|| (file, diff))
        })
        .collect()
}

/// The uncommitted diff of `path` in `cwd` as git reports it, or an empty string when git is
/// unavailable, the folder isn't a repository, or the file has no changes.
fn git_diff(cwd: &Path, path: &str) -> String {
    let diff = std::process::Command::new("git")
        .arg("-C")
        .arg(cwd)
        .args(["diff", "HEAD", "--", path])
        .output()
        .ok()
        .filter(|out| out.status.success())
        .map(|out| String::from_utf8_lossy(&out.stdout).trim_end().to_string())
        .unwrap_or_default();
    if !diff.is_empty() {
        return diff;
    }
    std::process::Command::new("git")
        .arg("-C")
        .arg(cwd)
        .args(["diff", "--", path])
        .output()
        .ok()
        .filter(|out| out.status.success())
        .map(|out| String::from_utf8_lossy(&out.stdout).trim_end().to_string())
        .unwrap_or_default()
}

/// The session's tmux pane history, or an empty string when it has no tmux session or tmux isn't
/// available. The socket and the session name come from `terminal`.
fn terminal_output(session_id: &str) -> String {
    let name = crate::terminal::tmux_session_name(session_id);
    std::process::Command::new("tmux")
        .arg("-L")
        .arg(crate::terminal::tmux_socket())
        .args(["capture-pane", "-p", "-S", "-", "-t", &name])
        .output()
        .ok()
        .filter(|out| out.status.success())
        .map(|out| String::from_utf8_lossy(&out.stdout).trim_end().to_string())
        .unwrap_or_default()
}

/// The session's title, or "New session" for one without a title, as the sidebar shows it.
fn title(s: &Session) -> &str {
    if s.title.trim().is_empty() {
        "New session"
    } else {
        s.title.trim()
    }
}

/// A file name stem for a title: lowercase words joined by `-`, at most 60 characters.
fn slug(title: &str) -> String {
    let mut out = String::new();
    for c in title.chars().flat_map(char::to_lowercase) {
        if c.is_ascii_alphanumeric() {
            out.push(c);
        } else if !out.is_empty() && !out.ends_with('-') {
            out.push('-');
        }
    }
    out.truncate(60);
    let slug = out.trim_end_matches('-');
    if slug.is_empty() {
        "session".into()
    } else {
        slug.to_string()
    }
}

/// Seconds since the epoch as "YYYY-MM-DD HH:MM UTC".
fn stamp(ts: u64) -> String {
    // Howard Hinnant's civil_from_days, for days since 1970-01-01.
    let days = (ts / 86_400) as i64;
    let secs = ts % 86_400;
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02} {:02}:{:02} UTC", secs / 3600, secs % 3600 / 60)
}

impl Shell {
    /// Opens the share modal for the focused session. Toasts when no session has focus.
    pub(super) fn share_open(&mut self, cx: &mut Context<Self>) {
        match self.state.workspace.layout.focus.clone() {
            Some(id) => self.share_open_for(&id, cx),
            None => self.toast("Open a session to share it", cx),
        }
    }

    /// Opens the share modal for session `id`. Toasts when that session is gone.
    pub(super) fn share_open_for(&mut self, id: &str, cx: &mut Context<Self>) {
        if self.state.session(id).is_none() {
            self.toast("Open a session to share it", cx);
            return;
        }
        self.share.session = Some(id.to_string());
        cx.notify();
    }

    fn share_close(&mut self, cx: &mut Context<Self>) {
        self.share.session = None;
        cx.notify();
    }

    /// The session the open modal is about.
    fn shared(&self) -> Option<&Session> {
        self.share.session.as_deref().and_then(|id| self.state.session(id))
    }

    /// Keys while the modal is open: Escape closes it, and every other key stays inside it. Returns true when consumed.
    pub(super) fn share_key(&mut self, ev: &KeyDownEvent, _window: &mut Window, cx: &mut Context<Self>) -> bool {
        if self.share.session.is_none() {
            return false;
        }
        if ev.keystroke.key == "escape" {
            self.share_close(cx);
        }
        true
    }

    /// The header details for `s`: the harness name, the model and effort labels the composer shows, and the project.
    fn share_details(&self, s: &Session) -> Details {
        let caps = self.caps.get(s.agent.as_str()).and_then(Option::as_ref);
        let model = match s.model.as_deref() {
            Some(id) => caps
                .and_then(|c| c.models.iter().find(|m| m.id == id))
                .map_or_else(|| id.to_string(), |m| m.label.clone()),
            None => caps
                .and_then(|c| c.default_model.as_deref())
                .map_or_else(|| "Default model".into(), |d| format!("Default ({d})")),
        };
        Details {
            harness: engine::agent_name(&s.agent).to_string(),
            model,
            effort: s.effort.clone().unwrap_or_else(|| "default".into()),
            project: s.project.as_deref().and_then(|p| self.state.project(p)).map(|p| p.name.clone()),
        }
    }

    /// Asks where to save the session, writes it off the UI thread, then toasts where it went.
    fn export(&mut self, format: Format, cx: &mut Context<Self>) {
        let Some(s) = self.shared() else { return };
        let extras = extras(s, self.share.diffs, self.share.terminal);
        let details = self.share_details(s);
        let body = match export_body(format, s, &details, &extras) {
            Ok(body) => body,
            Err(e) => {
                self.toast(format!("Export failed: {e}"), cx);
                return;
            }
        };
        let name = format!("{}.{}", slug(title(s)), format.ext());
        let dir = if s.cwd.is_dir() { s.cwd.clone() } else { self.default_folder() };
        let asked = cx.prompt_for_new_path(&dir, Some(&name));
        cx.spawn(async move |this, cx| {
            let path = match asked.await {
                Ok(Ok(Some(path))) => path,
                Ok(Err(e)) => {
                    let _ = this.update(cx, |this, cx| this.toast(format!("Export failed: {e}"), cx));
                    return;
                }
                Ok(Ok(None)) | Err(_) => return,
            };
            let target = path.clone();
            let saved = cx.background_executor().spawn(async move { std::fs::write(&target, body) }).await;
            let _ = this.update(cx, |this, cx| {
                let message = match saved {
                    Ok(()) => format!("Exported to {}", path.display()),
                    Err(e) => format!("Export failed: {e}"),
                };
                this.toast(message, cx);
            });
        })
        .detach();
    }

    fn copy_markdown(&mut self, cx: &mut Context<Self>) {
        let Some(s) = self.shared() else { return };
        let extras = extras(s, self.share.diffs, self.share.terminal);
        let body = markdown(s, &self.share_details(s), &extras);
        cx.write_to_clipboard(ClipboardItem::new_string(body));
        self.toast("Copied as Markdown", cx);
    }

    /// The modal, when a session is shared (drawn by `render_overlays`).
    pub(super) fn render_share(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let s = self.shared()?;
        let theme = &self.theme;
        let (muted, line, line2) = (theme.muted.to_hsla(), theme.line.to_hsla(), theme.line2.to_hsla());
        let header = div()
            .flex()
            .items_center()
            .gap(px(12.))
            .pl(px(22.))
            .pr(px(16.))
            .py(px(16.))
            .border_b_1()
            .border_color(line)
            .child(
                div()
                    .flex_1()
                    .font_family(theme.display_font.clone())
                    .text_size(px(26.))
                    .line_height(px(26.))
                    .child("Share session"),
            )
            .child(
                div()
                    .id("share-close")
                    .w(px(30.))
                    .h(px(30.))
                    .rounded(px(6.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_color(muted)
                    .cursor_pointer()
                    .hover(move |d| d.bg(line2))
                    .on_click(cx.listener(|this, _, _, cx| this.share_close(cx)))
                    .child(div().text_size(px(17.)).child("✕")),
            );
        let body = div()
            .id("share-body")
            .flex_1()
            .min_h(px(0.))
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .gap(px(10.))
            .px(px(22.))
            .pt(px(20.))
            .pb(px(24.))
            .child(
                div()
                    .font_family(theme.display_font.clone())
                    .text_size(px(22.))
                    .line_height(px(25.))
                    .child(title(s).to_string()),
            )
            .child(unavailable(visibility_track(theme), theme))
            .child(div().text_size(px(12.)).text_color(muted).child("Only you can open this session."))
            .child(unavailable(link_row(theme), theme))
            .child(share_toggle_row("share-diffs", "Include file diffs", self.share.diffs, theme, |this| {
                this.share.diffs = !this.share.diffs;
            }, cx))
            .child(share_toggle_row("share-term", "Include terminal output", self.share.terminal, theme, |this| {
                this.share.terminal = !this.share.terminal;
            }, cx))
            .child(section_label("PEOPLE", theme))
            .child(unavailable(invite_row(theme), theme))
            .child(section_label("EXPORT", theme))
            .child(
                div()
                    .flex()
                    .gap(px(6.))
                    .child(
                        export_button("share-markdown", "Markdown", theme)
                            .on_click(cx.listener(|this, _, _, cx| this.export(Format::Markdown, cx))),
                    )
                    .child(
                        export_button("share-json", "JSON", theme)
                            .on_click(cx.listener(|this, _, _, cx| this.export(Format::Json, cx))),
                    ),
            )
            .child(
                export_button("share-copy-markdown", "Copy as Markdown", theme)
                    .on_click(cx.listener(|this, _, _, cx| this.copy_markdown(cx))),
            );
        let dialog = div()
            .flex()
            .flex_col()
            .w(px(520.))
            .max_h(px(820.))
            .rounded(px(12.))
            .bg(theme.pop.to_hsla())
            .border_1()
            .border_color(line2)
            .shadow(vec![BoxShadow {
                color: hsla(0., 0., 0., 0.6),
                offset: point(px(0.), px(30.)),
                blur_radius: px(80.),
                spread_radius: px(0.),
            }])
            .overflow_hidden()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(header)
            .child(body);
        Some(
            div()
                .absolute()
                .top_0()
                .left_0()
                .size_full()
                .occlude()
                .flex()
                .justify_center()
                .items_start()
                .pt(px(120.))
                .bg(hsla(0., 0., 0., 0.6))
                .on_mouse_down(MouseButton::Left, cx.listener(|this, _, _, cx| this.share_close(cx)))
                .child(dialog)
                .into_any_element(),
        )
    }
}

/// Dims a control ArchonUI can't run without a server, and says why on hover.
fn unavailable(control: Stateful<Div>, theme: &Theme) -> Stateful<Div> {
    control.opacity(0.5).tooltip(tooltip(NO_SERVER, TooltipColors::of(theme)))
}

/// A share export toggle row (prototype: `padding:6px 2px`, 13px label, 34×18 switch). The whole
/// row toggles, as the prototype has it; `flip` applies the toggle to the `ShareUi` flag.
fn share_toggle_row(
    id: &'static str,
    label: &'static str,
    on: bool,
    theme: &Theme,
    flip: impl Fn(&mut Shell) + 'static,
    cx: &mut Context<Shell>,
) -> impl IntoElement {
    div()
        .id(id)
        .flex()
        .items_center()
        .justify_between()
        .gap(px(10.))
        .px(px(2.))
        .py(px(6.))
        .cursor_pointer()
        .on_click(cx.listener(move |this, _, _, cx| {
            flip(this);
            cx.notify();
        }))
        .child(div().text_size(px(13.)).child(label))
        .child(Toggle::new(SharedString::from(format!("{id}-switch")), on, theme))
}

/// Private / Team / Public. Only Private describes a session that is never shared.
fn visibility_track(theme: &Theme) -> Stateful<Div> {
    let mut track = div().id("share-visibility").flex().gap(px(4.)).p(px(4.)).rounded(px(8.)).bg(theme.line.to_hsla());
    for (ix, option) in ["Private", "Team", "Public"].into_iter().enumerate() {
        track = track.child(
            div()
                .flex_1()
                .flex()
                .justify_center()
                .py(px(7.))
                .rounded(px(6.))
                .text_size(px(12.))
                .when(ix == 0, |d| d.bg(theme.line2.to_hsla()).text_color(theme.fg.to_hsla()))
                .when(ix != 0, |d| d.text_color(theme.muted.to_hsla()))
                .child(option),
        );
    }
    track
}

fn link_row(theme: &Theme) -> Stateful<Div> {
    div()
        .id("share-link")
        .flex()
        .items_center()
        .gap(px(6.))
        .pl(px(12.))
        .pr(px(4.))
        .py(px(4.))
        .rounded(px(8.))
        .border_1()
        .border_color(theme.line2.to_hsla())
        .child(div().flex_1().min_w_0().truncate().text_size(px(12.)).text_color(theme.muted.to_hsla()).child("No link"))
        .child(
            div()
                .px(px(12.))
                .py(px(7.))
                .rounded(px(6.))
                .bg(theme.acc.to_hsla())
                .text_color(theme.on.to_hsla())
                .text_size(px(12.))
                .font_weight(FontWeight::BOLD)
                .child("Copy"),
        )
}

fn invite_row(theme: &Theme) -> Stateful<Div> {
    div()
        .id("share-invite")
        .flex()
        .gap(px(6.))
        .pl(px(12.))
        .pr(px(4.))
        .py(px(4.))
        .rounded(px(8.))
        .border_1()
        .border_color(theme.line2.to_hsla())
        .child(div().flex_1().min_w_0().text_size(px(12.5)).text_color(theme.muted.to_hsla()).child("name@team.dev"))
        .child(
            div()
                .px(px(12.))
                .py(px(7.))
                .rounded(px(6.))
                .bg(theme.line2.to_hsla())
                .text_size(px(12.))
                .child("Invite"),
        )
}

fn export_button(id: &'static str, label: &'static str, theme: &Theme) -> Stateful<Div> {
    let line2 = theme.line2.to_hsla();
    div()
        .id(id)
        .flex_1()
        .flex()
        .justify_center()
        .py(px(9.))
        .rounded(px(7.))
        .border_1()
        .border_color(line2)
        .text_size(px(12.))
        .cursor_pointer()
        .hover(move |d| d.bg(line2))
        .child(label)
}

fn section_label(text: &'static str, theme: &Theme) -> impl IntoElement {
    div().mt(px(8.)).text_size(px(10.5)).text_color(theme.muted.to_hsla()).child(text)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        engine::attach::{Attachment, Kind},
        store::{ToolCall, ToolKind},
    };
    use std::path::PathBuf;

    fn details() -> Details {
        Details {
            harness: "Claude Code".into(),
            model: "Sonnet 5.5".into(),
            effort: "high".into(),
            project: Some("archonui-core".into()),
        }
    }

    fn session() -> Session {
        let mut s = Session::new("claude", "/work/archonui".into());
        s.id = "archonui-test".into();
        s.title = "Refactor auth middleware".into();
        s.created = 1_000_000_000;
        s.updated = 1_000_000_600;
        s.messages = vec![
            Message {
                role: Role::User,
                text: "Cache the token.".into(),
                attachments: Vec::new(),
                tools: Vec::new(),
                thought_secs: Some(3),
                took_secs: None,
                agent: None,
                final_at: None,
                usage: None,
                todos: Vec::new(),
            },
            Message {
                role: Role::Agent,
                text: "Cached, **128 pass**.\n".into(),
                attachments: vec![Attachment {
                    path: PathBuf::from("/tmp/x/shot.png"),
                    name: "shot.png".into(),
                    kind: Kind::Image,
                    size: 10,
                }],
                tools: vec![
                    ToolCall { kind: ToolKind::Edit, target: "src/cache.ts".into(), detail: Some("+41".into()), ok: Some(true) },
                    ToolCall { kind: ToolKind::Run, target: "pnpm test".into(), detail: None, ok: Some(false) },
                ],
                thought_secs: Some(14),
                took_secs: Some(31),
                agent: None,
                final_at: None,
                usage: None,
                todos: Vec::new(),
            },
        ];
        s
    }

    #[test]
    fn markdown_has_title_metadata_and_messages_in_order() {
        let md = markdown(&session(), &details(), &Extras::default());
        assert!(md.starts_with("# Refactor auth middleware\n\n- **Harness:** Claude Code\n"));
        for line in [
            "- **Model:** Sonnet 5.5",
            "- **Effort:** high",
            "- **Mode:** Auto",
            "- **Project:** archonui-core",
            "- **Folder:** /work/archonui",
            "- **Created:** 2001-09-09 01:46 UTC",
            "- **Updated:** 2001-09-09 01:56 UTC",
        ] {
            assert!(md.contains(line), "missing {line:?} in:\n{md}");
        }
        let you = md.find("## You\n\nCache the token.").expect("user section");
        let agent = md.find("## Claude Code\n\nCached, **128 pass**.").expect("agent section");
        assert!(you < agent);
        assert!(md.contains("Tool calls:\n\n- Edit `src/cache.ts`, +41 (ok)\n- Run `pnpm test` (failed)\n"));
        assert!(md.contains("Attachments:\n\n- shot.png\n"));
        assert!(!md.contains("14"), "thinking time must not be written:\n{md}");
    }

    #[test]
    fn markdown_omits_project_when_none_and_names_untitled_sessions() {
        let mut s = session();
        s.title = "  ".into();
        let md = markdown(&s, &Details { project: None, ..details() }, &Extras::default());
        assert!(md.starts_with("# New session\n"));
        assert!(!md.contains("Project"));
    }

    #[test]
    fn json_export_is_the_session_as_stored() {
        let body = export_body(Format::Json, &session(), &details(), &Extras::default()).unwrap();
        let value: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(value["title"], "Refactor auth middleware");
        assert_eq!(value["messages"][1]["tools"][0]["kind"], "edit");
        assert!(value.get("file_diffs").is_none(), "a disabled toggle adds no fields");
        let back: Session = serde_json::from_str(&body).unwrap();
        assert_eq!(back.messages.len(), 2);
    }

    #[test]
    fn markdown_appends_the_included_diff_and_terminal_sections() {
        let extras = Extras {
            diffs: Some(vec![
                ("src/cache.ts".into(), "+41".into()),
                ("src/new.ts".into(), "diff --git a/src/new.ts b/src/new.ts\n+new".into()),
            ]),
            terminal: Some("$ pnpm test\n128 passed".into()),
        };
        let md = markdown(&session(), &details(), &extras);
        assert!(md.contains("## File diffs\n\n### src/cache.ts\n\n```diff\n+41\n```\n"));
        assert!(md.contains("### src/new.ts\n\n```diff\ndiff --git a/src/new.ts b/src/new.ts\n+new\n```\n"));
        assert!(md.contains("## Terminal output\n\n```text\n$ pnpm test\n128 passed\n```\n"));
        let agent = md.find("## Claude Code").expect("agent section");
        let diffs = md.find("## File diffs").expect("diffs section");
        let terminal = md.find("## Terminal output").expect("terminal section");
        assert!(agent < diffs && diffs < terminal, "sections follow the messages");
    }

    #[test]
    fn markdown_leaves_out_empty_or_disabled_sections() {
        let md = markdown(&session(), &details(), &Extras::default());
        assert!(!md.contains("File diffs"));
        assert!(!md.contains("Terminal output"));
        let blank = Extras { diffs: Some(Vec::new()), terminal: Some("  \n".into()) };
        let md = markdown(&session(), &details(), &blank);
        assert!(!md.contains("File diffs"));
        assert!(!md.contains("Terminal output"));
    }

    #[test]
    fn json_export_adds_the_included_sections() {
        let extras = Extras {
            diffs: Some(vec![("src/cache.ts".into(), "+41".into())]),
            terminal: Some("$ pnpm test\n128 passed".into()),
        };
        let body = export_body(Format::Json, &session(), &details(), &extras).unwrap();
        let value: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(value["file_diffs"][0]["file"], "src/cache.ts");
        assert_eq!(value["file_diffs"][0]["diff"], "+41");
        assert_eq!(value["terminal_output"], "$ pnpm test\n128 passed");
        let back: Session = serde_json::from_str(&body).unwrap();
        assert_eq!(back.messages.len(), 2);
    }

    /// A temp folder holding a git repository with `src/cache.ts` committed, then edited, so the
    /// file has one uncommitted change.
    fn temp_repo() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("archonui-share-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("src")).unwrap();
        let git = |args: &[&str]| {
            let out = std::process::Command::new("git").arg("-C").arg(&dir).args(args).output().unwrap();
            assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
        };
        git(&["init", "-q"]);
        git(&["config", "user.name", "ArchonUI"]);
        git(&["config", "user.email", "archonui@localhost"]);
        std::fs::write(dir.join("src/cache.ts"), "get(key) {}\n").unwrap();
        git(&["add", "src/cache.ts"]);
        git(&["commit", "-qm", "cache"]);
        std::fs::write(dir.join("src/cache.ts"), "get(key) {\n  return null;\n}\n").unwrap();
        dir
    }

    #[test]
    fn file_diffs_come_from_git_for_edited_files_only() {
        let dir = temp_repo();
        let mut s = session();
        s.cwd = dir.clone();
        let diffs = file_diffs(&s);
        assert_eq!(diffs.len(), 1, "{diffs:?}");
        assert_eq!(diffs[0].0, "src/cache.ts");
        assert!(diffs[0].1.contains("diff --git a/src/cache.ts b/src/cache.ts"), "{}", diffs[0].1);
        assert!(diffs[0].1.contains("+  return null;"), "{}", diffs[0].1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn file_diffs_are_empty_when_git_reports_nothing() {
        // The fixture's `pnpm test` call is not a file edit, and `/work/archonui` is no repository.
        assert!(file_diffs(&session()).is_empty());
    }

    #[test]
    fn file_names_come_from_the_title() {
        assert_eq!(slug("Refactor auth middleware"), "refactor-auth-middleware");
        assert_eq!(slug("  Fix: the (weird) bug!! "), "fix-the-weird-bug");
        assert_eq!(slug(""), "session");
        assert_eq!(slug("日本語"), "session");
    }

    #[test]
    fn stamps_are_utc_calendar_dates() {
        assert_eq!(stamp(0), "1970-01-01 00:00 UTC");
        assert_eq!(stamp(1_000_000_000), "2001-09-09 01:46 UTC");
        assert_eq!(stamp(1_709_164_800), "2024-02-29 00:00 UTC");
    }

    #[test]
    fn diff_text_detection_distinguishes_diffs_from_summaries() {
        assert!(!is_diff_text("+41"));
        assert!(!is_diff_text("+24 −8"));
        assert!(!is_diff_text("128 passed"));
        assert!(!is_diff_text(""));
        assert!(is_diff_text("@@ -1,3 +1,4 @@\n-old\n+new"));
        assert!(is_diff_text("diff --git a/a.ts b/a.ts\n+new"));
        assert!(is_diff_text("--- a/a.ts\n+++ b/a.ts"));
        assert!(is_diff_text("-line1\n+line2"));
    }

    #[test]
    fn file_diffs_prefers_recorded_diff_text_over_git() {
        let mut s = session();
        s.cwd = PathBuf::from("/nonexistent-share-test-dir");
        s.messages[1].tools.push(ToolCall {
            kind: ToolKind::Edit,
            target: "src/recorded.ts".into(),
            detail: Some("@@ -1 +1 @@\n-old\n+new".into()),
            ok: Some(true),
        });
        let diffs = file_diffs(&s);
        assert_eq!(diffs.len(), 1);
        assert_eq!(diffs[0].0, "src/recorded.ts");
        assert_eq!(diffs[0].1, "@@ -1 +1 @@\n-old\n+new");
    }

    #[test]
    fn fences_and_inline_code_survive_backticks_in_content() {
        let extras = Extras {
            diffs: Some(vec![("a.md".into(), "+```\n+x".into())]),
            terminal: Some("```\nrm -rf\n```".into()),
        };
        let md = markdown(&session(), &details(), &extras);
        assert!(md.contains("````diff\n+```\n+x\n````\n"), "{md}");
        assert!(md.contains("````text\n```\nrm -rf\n```\n````\n"), "{md}");
        assert_eq!(inline_code("a`b"), "``a`b``");
        assert_eq!(inline_code("`a"), "`` `a ``");
        assert_eq!(inline_code("a\nb"), "`a b`");
    }

    #[test]
    fn terminal_output_returns_empty_when_no_session() {
        assert_eq!(terminal_output("nonexistent-archonui-session-id-12345"), "");
    }
}

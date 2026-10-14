//! The Git tab: the folder's changes with staging checkboxes, a commit message, push, init and recent
//! commits. Spec: docs/design/tools-and-behaviour.md §A4. The snapshot read here also feeds the Files
//! tab's change markers.

use crate::{
    git::{self, Commit, FileChange, Status},
    ui::{
        shell::Shell,
        theme::{Rgba, Theme},
        widgets::{
            button::Button,
            text_input::{text_input, TextInput, TextInputConfig},
        },
    },
};
use gpui::{
    div, prelude::*, transparent_black, AnyElement, ClickEvent, Context, Entity, Subscription, Window,
};
use crate::ui::scale::px;
use std::{
    collections::{HashMap, HashSet},
    fs,
    path::{Path, PathBuf},
};

/// Commits listed in the history.
const HISTORY_LEN: usize = 10;

/// Diff lines shown before the Show all action appears.
const DIFF_PREVIEW_LINES: usize = 400;

/// View state of the Git tab, not persisted.
#[derive(Default)]
pub struct GitUi {
    /// The folder the tab shows. `None` until the first read starts.
    pub(super) folder: Option<PathBuf>,
    pub(super) snapshot: Option<Snapshot>,
    /// Counts reads, so that a slow read can't replace a newer one.
    seq: u64,
    /// Commit messages of the folders not shown now.
    drafts: HashMap<PathBuf, String>,
    message: Option<Entity<TextInput>>,
    /// The file whose inline diff is open, if any.
    selected: Option<Selected>,
    /// The diff text of `selected`.
    diff: Option<Result<String, String>>,
    /// Whether the diff shows every line rather than the first [`DIFF_PREVIEW_LINES`].
    diff_all: bool,
    /// Counts diff reads, so that a slow read can't replace a newer one.
    diff_seq: u64,
    _subs: Vec<Subscription>,
}

/// The file whose inline diff is open: how the diff is taken.
#[derive(Clone, PartialEq, Debug)]
struct Selected {
    path: String,
    /// Diff the index against `HEAD` rather than the working tree against the index.
    staged: bool,
    /// The file is untracked, so it is diffed against `/dev/null`.
    untracked: bool,
}

/// The repository state the Git and Files tabs share, read in the background.
pub(super) struct Snapshot {
    pub(super) status: Status,
    /// The change markers, keyed by path relative to the repository root.
    marks: HashMap<PathBuf, ChangeMark>,
    /// The folders under the repository root that hold a change.
    dirs: HashSet<PathBuf>,
    commits: Vec<Commit>,
    top: PathBuf,
}

/// How git changed one file: its letter in the design, and its line counts.
#[derive(Clone, Copy)]
pub(super) struct ChangeMark {
    pub(super) letter: char,
    pub(super) added: u32,
    pub(super) removed: u32,
}

impl ChangeMark {
    /// The letter's colour in the design: `A` alt, `M` acc2, anything else muted.
    pub(super) fn colour(&self, theme: &Theme) -> Rgba {
        match self.letter {
            'A' => theme.alt,
            'M' => theme.acc2,
            _ => theme.muted,
        }
    }
}

impl Shell {
    /// Reads the git state of the tool folder in the background. A read that a newer one has overtaken
    /// is dropped. A change of folder keeps the old folder's commit message as a draft.
    pub(in crate::ui) fn refresh_git(&mut self, cx: &mut Context<Self>) {
        let folder = self.tool_root();
        let ui = &mut self.tabs.git;
        if ui.folder.as_deref() != Some(folder.as_path()) {
            if let (Some(old), Some(input)) = (ui.folder.take(), ui.message.clone()) {
                ui.drafts.insert(old, input.read(cx).text());
            }
            if let Some(input) = ui.message.clone() {
                let draft = ui.drafts.remove(&folder).unwrap_or_default();
                input.update(cx, |input, cx| input.set_text(&draft, cx));
            }
        }
        ui.folder = Some(folder.clone());
        ui.seq += 1;
        let seq = ui.seq;
        cx.spawn(async move |this, cx| {
            let snapshot = cx.background_executor().spawn(async move { read_snapshot(&folder) }).await;
            this.update(cx, |this, cx| {
                if this.tabs.git.seq == seq {
                    this.tabs.git.snapshot = snapshot;
                    // A file that left the change list closes its diff.
                    let stale = this.tabs.git.selected.as_ref().is_some_and(|selected| {
                        !this.tabs.git.snapshot.as_ref().is_some_and(|snapshot| listed(&snapshot.status, selected))
                    });
                    if stale {
                        this.tabs.git.selected = None;
                        this.tabs.git.diff = None;
                        this.tabs.git.diff_all = false;
                    }
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
    }

    /// The change marker of `path`, when git reports a change to it.
    pub(super) fn git_mark(&self, path: &Path) -> Option<ChangeMark> {
        let snap = self.tabs.git.snapshot.as_ref()?;
        let rel = path.strip_prefix(&snap.top).ok()?;
        snap.marks.get(rel).copied()
    }

    /// Whether a change lies under the folder `dir`.
    pub(super) fn git_dir_changed(&self, dir: &Path) -> bool {
        let Some(snap) = self.tabs.git.snapshot.as_ref() else { return false };
        dir.strip_prefix(&snap.top).is_ok_and(|rel| snap.dirs.contains(rel))
    }

    pub(in crate::ui) fn render_git(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let theme = self.theme.clone();
        if self.tabs.git.folder.as_deref() != Some(self.tool_root().as_path()) {
            self.refresh_git(cx);
        }
        if self.tabs.git.message.is_none() {
            let input = cx.new(|cx| TextInput::new(message_config(), window, cx));
            let observe = cx.observe(&input, |_, _, cx| cx.notify());
            self.tabs.git.message = Some(input);
            self.tabs.git._subs.push(observe);
        }
        let body = match (self.tabs.git.folder.is_some(), self.tabs.git.snapshot.as_ref()) {
            (false, _) => message_text("Reading the repository…", &theme),
            (true, None) => message_text("Could not read git status.", &theme),
            (true, Some(snap)) if !snap.status.repo => self.git_not_repo(&theme, cx),
            (true, Some(_)) => self.git_panel(&theme, cx),
        };
        div()
            .id("git-panel")
            .size_full()
            .min_h_0()
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .font_family(theme.ui_font.clone())
            .text_color(theme.fg.to_hsla())
            .child(body)
            .into_any_element()
    }

    /// The Git tab takes no keys of its own; typing goes to the commit message field.
    pub(in crate::ui) fn git_key(&mut self, _ev: &gpui::KeyDownEvent, _window: &mut Window, _cx: &mut Context<Self>) -> bool {
        false
    }

    pub(in crate::ui) fn git_blur(&mut self, _cx: &mut Context<Self>) {}

    /// The folder is not a repository: a line of text and the Init button.
    fn git_not_repo(&self, theme: &Theme, cx: &mut Context<Self>) -> AnyElement {
        let view = cx.entity();
        div()
            .flex()
            .flex_col()
            .items_start()
            .gap(px(10.))
            .px(px(16.))
            .pt(px(14.))
            .child(div().text_size(px(12.5)).text_color(theme.muted.to_hsla()).child("Not a git repository."))
            .child(Button::new("git-init", "Init repository", theme).on_click(move |_, cx| {
                view.update(cx, |this, cx| this.git_run(cx, git::init));
            }))
            .into_any_element()
    }

    /// The repository view: the branch header, the changes, the commit box and the history.
    fn git_panel(&self, theme: &Theme, cx: &mut Context<Self>) -> AnyElement {
        let Some(snap) = self.tabs.git.snapshot.as_ref() else { return div().into_any_element() };
        let status = &snap.status;
        let muted = theme.muted.to_hsla();
        let branch = status.branch.clone().unwrap_or_else(|| "detached".to_string());
        let sync = match status.upstream.as_deref().and_then(|up| up.split('/').next()) {
            Some(remote) => format!("↑{} ↓{} · {remote}", status.ahead, status.behind),
            None => "no upstream".to_string(),
        };
        let header = div()
            .flex()
            .items_center()
            .gap(px(10.))
            .pt(px(14.))
            .px(px(16.))
            .pb(px(8.))
            .child(
                div()
                    .text_size(px(13.))
                    .px(px(10.))
                    .py(px(5.))
                    .rounded(px(6.))
                    .bg(theme.line2.to_hsla())
                    .text_color(theme.alt.to_hsla())
                    .child(format!("⎇ {branch}")),
            )
            .child(div().text_size(px(12.)).text_color(muted).child(sync))
            .child(div().flex_1())
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .child(Self::git_header_button("git-pull", "Pull", git::pull, theme, cx))
                    .child(Self::git_header_button("git-merge", "Auto Merge", git::auto_merge, theme, cx))
                    .child(Self::git_header_button("git-push", "Push", git::push, theme, cx)),
            );

        let staged = status.files.iter().filter(|file| file.staged.is_some()).count();
        let mut changes = div().flex().flex_col().px(px(8.));
        if status.files.is_empty() {
            changes = changes.child(
                div().px(px(8.)).py(px(10.)).text_size(px(12.5)).text_color(muted).child("Working tree clean."),
            );
        }
        for (ix, file) in status.files.iter().enumerate() {
            changes = changes.child(self.git_change_row(ix, file, snap, theme, cx));
        }

        let commit = self.git_commit_box(staged, status.files.len(), theme, cx);
        let mut history = div().flex().flex_col().px(px(16.)).pb(px(12.));
        for (ix, commit) in snap.commits.iter().enumerate() {
            history = history.child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .py(px(7.))
                    .border_b_1()
                    .border_color(theme.line.to_hsla())
                    .text_size(px(12.))
                    .child(
                        div().w(px(14.)).flex_none().flex().justify_center().child(
                            div()
                                .w(px(8.))
                                .h(px(8.))
                                .rounded_full()
                                .border_2()
                                .border_color(if ix == 0 { theme.acc2 } else { theme.faint }.to_hsla()),
                        ),
                    )
                    .child(
                        div()
                            .w(px(64.))
                            .flex_none()
                            .text_color(theme.alt.to_hsla())
                            .child(commit.hash.chars().take(7).collect::<String>()),
                    )
                    .child(div().flex_1().min_w_0().truncate().child(commit.subject.clone()))
                    .child(div().flex_none().text_size(px(11.)).text_color(muted).child(commit.date.clone())),
            );
        }

        div()
            .flex()
            .flex_col()
            .child(header)
            .child(self.git_changes_label(status, theme, cx))
            .child(changes)
            .child(commit)
            .child(section_label("HISTORY", theme))
            .child(history)
            .into_any_element()
    }

    /// One changed file: its staging checkbox, letter, path and line counts. Clicking the path opens or
    /// closes the file's inline diff below the row.
    fn git_change_row(&self, ix: usize, file: &FileChange, snap: &Snapshot, theme: &Theme, cx: &mut Context<Self>) -> AnyElement {
        let staged = file.staged.is_some();
        let (added, removed) = snap
            .marks
            .get(Path::new(&file.path))
            .map_or((0, 0), |mark| (mark.added, mark.removed));
        let letter = snap.marks.get(Path::new(&file.path)).map_or(' ', |mark| mark.letter);
        let path = file.path.clone();
        let target = diff_target(file);
        let open = self.tabs.git.selected.as_ref() == Some(&target);
        let checkbox = div()
            .id(("git-check", ix))
            .flex_none()
            .w(px(14.))
            .h(px(14.))
            .rounded(px(3.))
            .border_1()
            .border_color(if staged { theme.acc } else { theme.faint }.to_hsla())
            .bg(if staged { theme.acc.to_hsla() } else { transparent_black() })
            .flex()
            .items_center()
            .justify_center()
            .text_size(px(10.))
            .text_color(theme.on.to_hsla())
            .cursor_pointer()
            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| this.git_toggle(path.clone(), staged, cx)))
            .child(if staged { "✓" } else { "" });
        let toggle = target;
        let row = div()
            .id(("git-change", ix))
            .flex()
            .items_center()
            .gap(px(10.))
            .px(px(8.))
            .py(px(7.))
            .rounded(px(6.))
            .text_size(px(12.5))
            .when(open, |d| d.bg(theme.line2.to_hsla()))
            .hover(|d| d.bg(theme.line2.to_hsla()))
            .child(checkbox)
            .child(div().w(px(12.)).flex_none().text_color(theme.acc2.to_hsla()).child(letter.to_string()))
            .child(
                div()
                    .id(("git-name", ix))
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .cursor_pointer()
                    .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| this.git_select(toggle.clone(), cx)))
                    .child(file.path.clone()),
            )
            .child(div().flex_none().text_color(theme.alt.to_hsla()).child(format!("+{added}")))
            .child(div().flex_none().text_color(theme.muted.to_hsla()).child(format!("−{removed}")));

        let mut column = div().flex().flex_col().child(row);
        if open {
            column = column.child(self.git_diff_body(ix, theme, cx));
        }
        column.into_any_element()
    }

    /// The `CHANGES · N` label with the Stage all and Unstage all text actions at its right, matching the
    /// section-label style.
    fn git_changes_label(&self, status: &Status, theme: &Theme, cx: &mut Context<Self>) -> AnyElement {
        let muted = theme.muted.to_hsla();
        let fg = theme.fg.to_hsla();
        let action = |id: &'static str, label: &'static str| {
            div()
                .id(id)
                .text_size(px(10.5))
                .text_color(muted)
                .cursor_pointer()
                .hover(move |d| d.text_color(fg))
                .child(label)
        };
        let can_stage = status.files.iter().any(|file| file.staged.is_none() || file.unstaged.is_some());
        let can_unstage = status.files.iter().any(|file| file.staged.is_some());
        let mut actions = div().flex().items_center().gap(px(12.));
        if can_stage {
            actions = actions.child(
                action("git-stage-all", "STAGE ALL")
                    .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.git_stage_all(cx))),
            );
        }
        if can_unstage {
            actions = actions.child(
                action("git-unstage-all", "UNSTAGE ALL")
                    .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.git_unstage_all(cx))),
            );
        }
        div()
            .flex()
            .items_center()
            .justify_between()
            .px(px(16.))
            .pt(px(10.))
            .pb(px(6.))
            .child(
                div()
                    .text_size(px(10.5))
                    .text_color(theme.muted.to_hsla())
                    .child(format!("CHANGES · {}", status.files.len())),
            )
            .child(actions)
            .into_any_element()
    }

    /// A header action in the design's outlined style: the same shape as Push, Pull, Refresh.
    fn git_header_button(
        id: &'static str,
        label: &'static str,
        job: fn(&Path) -> Result<String, String>,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        div()
            .id(id)
            .text_size(px(12.))
            .px(px(10.))
            .py(px(6.))
            .rounded(px(6.))
            .border_1()
            .border_color(theme.line2.to_hsla())
            .cursor_pointer()
            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| this.git_run(cx, job)))
            .child(label)
    }

    /// The inline diff of the open file: the code font, numbered rows, added lines in `alt` and removed
    /// lines on the IDE's `soft` band. Shows [`DIFF_PREVIEW_LINES`] lines with a Show all action.
    fn git_diff_body(&self, ix: usize, theme: &Theme, cx: &mut Context<Self>) -> AnyElement {
        let ui = &self.tabs.git;
        let muted = theme.muted.to_hsla();
        let fg = theme.fg.to_hsla();
        let note = |text: String, colour: gpui::Hsla| {
            div().px(px(2.)).py(px(4.)).text_size(px(12.5)).text_color(colour).child(text)
        };
        let body = match &ui.diff {
            None => note("Reading diff…".into(), muted).into_any_element(),
            Some(Err(e)) => note(e.clone(), theme.acc.to_hsla()).into_any_element(),
            Some(Ok(text)) if text.trim().is_empty() => note("No textual changes.".into(), muted).into_any_element(),
            Some(Ok(text)) => {
                let lines: Vec<&str> = text.lines().collect();
                let shown = if ui.diff_all { lines.len() } else { lines.len().min(DIFF_PREVIEW_LINES) };
                let mut column = div().flex().flex_col();
                for (n, line) in lines.iter().take(shown).enumerate() {
                    column = column.child(diff_row(n + 1, line, theme));
                }
                if lines.len() > shown {
                    column = column.child(
                        div()
                            .id(("git-diff-all", ix))
                            .mt(px(4.))
                            .text_size(px(11.))
                            .text_color(muted)
                            .cursor_pointer()
                            .hover(move |d| d.text_color(fg))
                            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                                this.tabs.git.diff_all = true;
                                cx.notify();
                            }))
                            .child(format!("Show all · {} lines", lines.len())),
                    );
                }
                column.into_any_element()
            }
        };
        div()
            .id(("git-diff", ix))
            .ml(px(46.))
            .mr(px(8.))
            .mb(px(8.))
            .px(px(8.))
            .py(px(6.))
            .rounded(px(6.))
            .bg(theme.line.to_hsla())
            .font_family(theme.code_font.clone())
            .text_size(px(12.5))
            .max_h(px(320.))
            .overflow_y_scroll()
            .child(body)
            .into_any_element()
    }

    /// The commit message field, the staged count and the Commit button. Commit is dimmed until there is
    /// something staged and a message.
    fn git_commit_box(&self, staged: usize, changes: usize, theme: &Theme, cx: &mut Context<Self>) -> AnyElement {
        let Some(input) = self.tabs.git.message.as_ref() else { return div().into_any_element() };
        let ready = staged > 0 && !input.read(cx).text().trim().is_empty();
        let commit = div()
            .id("git-commit")
            .px(px(14.))
            .py(px(7.))
            .rounded(px(6.))
            .text_size(px(12.))
            .font_weight(gpui::FontWeight::BOLD)
            .bg(theme.acc.to_hsla())
            .text_color(theme.on.to_hsla())
            .opacity(if ready { 1. } else { 0.4 })
            .cursor_pointer()
            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.git_commit(cx)))
            .child("Commit");
        div()
            .mx(px(16.))
            .mt(px(12.))
            .rounded(px(8.))
            .border_1()
            .border_color(theme.line2.to_hsla())
            .bg(theme.line.to_hsla())
            .child(text_input(input, theme))
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .pl(px(12.))
                    .pr(px(8.))
                    .pt(px(6.))
                    .pb(px(8.))
                    .child(
                        div()
                            .text_size(px(11.))
                            .text_color(theme.muted.to_hsla())
                            .child(format!("{staged} of {changes} staged")),
                    )
                    .child(commit),
            )
            .into_any_element()
    }

    /// Stages or unstages one file, as its checkbox says.
    fn git_toggle(&mut self, path: String, staged: bool, cx: &mut Context<Self>) {
        self.git_run(cx, move |dir| {
            let paths = [path];
            if staged {
                git::unstage(dir, &paths)
            } else {
                git::stage(dir, &paths)
            }
            .map(|()| String::new())
        });
    }

    /// Stages every change in the repository.
    fn git_stage_all(&mut self, cx: &mut Context<Self>) {
        self.git_run(cx, |dir| git::stage_all(dir).map(|()| String::new()));
    }

    /// Unstages every staged change, keeping the working-tree changes.
    fn git_unstage_all(&mut self, cx: &mut Context<Self>) {
        self.git_run(cx, |dir| git::unstage_all(dir).map(|()| String::new()));
    }

    /// Opens `selected`'s inline diff below its row, or closes it when it is already open.
    fn git_select(&mut self, selected: Selected, cx: &mut Context<Self>) {
        let ui = &mut self.tabs.git;
        if ui.selected.as_ref() == Some(&selected) {
            ui.selected = None;
            ui.diff = None;
            ui.diff_all = false;
            cx.notify();
            return;
        }
        let Some(folder) = ui.folder.clone() else { return };
        ui.selected = Some(selected.clone());
        ui.diff = None;
        ui.diff_all = false;
        ui.diff_seq += 1;
        let seq = ui.diff_seq;
        cx.spawn(async move |this, cx| {
            let job = selected.clone();
            let diff = cx
                .background_executor()
                .spawn(async move { git::diff(&folder, &job.path, job.staged, job.untracked) })
                .await;
            this.update(cx, |this, cx| {
                if this.tabs.git.diff_seq == seq && this.tabs.git.selected.as_ref() == Some(&selected) {
                    this.tabs.git.diff = Some(diff);
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
    }

    /// Commits the staged files with the message. The message is cleared once the commit succeeds.
    fn git_commit(&mut self, cx: &mut Context<Self>) {
        let (Some(folder), Some(input)) = (self.tabs.git.folder.clone(), self.tabs.git.message.clone()) else { return };
        let message = input.read(cx).text().trim().to_string();
        let staged = self.tabs.git.snapshot.as_ref().is_some_and(|snap| snap.status.files.iter().any(|f| f.staged.is_some()));
        if message.is_empty() || !staged {
            return;
        }
        cx.spawn(async move |this, cx| {
            let result = cx.background_executor().spawn(async move { git::commit(&folder, &message) }).await;
            this.update(cx, |this, cx| {
                match result {
                    Ok(summary) => {
                        if let Some(input) = this.tabs.git.message.clone() {
                            input.update(cx, |input, cx| input.set_text("", cx));
                        }
                        if !summary.is_empty() {
                            this.toast(summary, cx);
                        }
                    }
                    Err(e) => this.toast(e, cx),
                }
                this.refresh_git(cx);
            })
            .ok();
        })
        .detach();
    }

    /// Runs a git command for the shown folder in the background. Its output or error is toasted, then the
    /// snapshot is read again.
    fn git_run(&mut self, cx: &mut Context<Self>, job: impl FnOnce(&Path) -> Result<String, String> + Send + 'static) {
        let Some(folder) = self.tabs.git.folder.clone() else { return };
        cx.spawn(async move |this, cx| {
            let result = cx.background_executor().spawn(async move { job(&folder) }).await;
            this.update(cx, |this, cx| {
                match result {
                    Ok(text) if !text.is_empty() => this.toast(text, cx),
                    Ok(_) => {}
                    Err(e) => this.toast(e, cx),
                }
                this.refresh_git(cx);
            })
            .ok();
        })
        .detach();
    }
}

/// Reads the repository state for `folder`. `None` when git itself fails.
fn read_snapshot(folder: &Path) -> Option<Snapshot> {
    let status = git::status(folder).ok()?;
    if !status.repo {
        return Some(Snapshot { status, marks: HashMap::new(), dirs: HashSet::new(), commits: Vec::new(), top: folder.to_path_buf() });
    }
    let top = git::toplevel(folder).ok()?;
    let mut counts: HashMap<String, (u32, u32)> = HashMap::new();
    for staged in [false, true] {
        for (path, added, removed) in git::numstat(folder, staged).unwrap_or_default() {
            let total = counts.entry(path).or_default();
            total.0 += added;
            total.1 += removed;
        }
    }
    let mut marks = HashMap::new();
    let mut dirs = HashSet::new();
    for file in &status.files {
        let (mut added, removed) = counts.get(&file.path).copied().unwrap_or((0, 0));
        if file.untracked {
            added = fs::read_to_string(top.join(&file.path)).map_or(0, |text| text.lines().count() as u32);
        }
        let letter = match file.unstaged.or(file.staged) {
            _ if file.untracked => 'A',
            Some('A') => 'A',
            Some('D') => 'D',
            _ => 'M',
        };
        let rel = PathBuf::from(&file.path);
        let mut dir = rel.parent();
        while let Some(parent) = dir.filter(|parent| !parent.as_os_str().is_empty()) {
            if !dirs.insert(parent.to_path_buf()) {
                break;
            }
            dir = parent.parent();
        }
        marks.insert(rel, ChangeMark { letter, added, removed });
    }
    let commits = git::log(folder, HISTORY_LEN).unwrap_or_default();
    Some(Snapshot { status, marks, dirs, commits, top })
}

/// Which diff will be taken for `file`: the working tree against the index (or `/dev/null` for an
/// untracked file), or the index against `HEAD` when only the staged change remains.
fn diff_target(file: &FileChange) -> Selected {
    let (staged, untracked) = if file.untracked {
        (false, true)
    } else if file.unstaged.is_some() {
        (false, false)
    } else {
        (true, false)
    };
    Selected { path: file.path.clone(), staged, untracked }
}

/// Whether `selected` is still in `status`'s change list.
fn listed(status: &Status, selected: &Selected) -> bool {
    status.files.iter().any(|file| {
        file.path == selected.path
            && if selected.untracked {
                file.untracked
            } else if selected.staged {
                file.staged.is_some()
            } else {
                file.unstaged.is_some()
            }
    })
}

/// One diff row: its number, and the line coloured by what it adds or removes.
fn diff_row(number: usize, line: &str, theme: &Theme) -> AnyElement {
    let band = line.starts_with('-') && !line.starts_with("---");
    let colour = if line.starts_with("@@") || line.starts_with("+++") || line.starts_with("---") {
        theme.muted
    } else if line.starts_with('+') {
        theme.alt
    } else {
        theme.fg
    };
    let text = if line.is_empty() { " ".to_string() } else { line.to_string() };
    div()
        .flex()
        .items_start()
        .when(band, |d| d.bg(theme.soft.to_hsla()))
        .child(
            div()
                .w(px(34.))
                .flex_none()
                .text_right()
                .pr(px(8.))
                .text_color(theme.faint.to_hsla())
                .child(number.to_string()),
        )
        .child(div().flex_1().min_w_0().whitespace_nowrap().text_color(colour.to_hsla()).child(text))
        .into_any_element()
}

fn message_config() -> TextInputConfig {
    TextInputConfig {
        multiline: true,
        min_lines: 2,
        max_lines: Some(6),
        placeholder: "Commit message".into(),
        enter_submits: false,
        font_size: px(12.5),
        line_height: px(18.75),
        pad_top: px(10.),
        pad_x: px(12.),
        pad_bottom: px(10.),
        ..Default::default()
    }
}

/// The uppercase label above a section.
fn section_label(text: &str, theme: &Theme) -> AnyElement {
    div()
        .px(px(16.))
        .pt(px(10.))
        .pb(px(6.))
        .text_size(px(10.5))
        .text_color(theme.muted.to_hsla())
        .child(text.to_string())
        .into_any_element()
}

/// A message filling the tab, in the muted text style.
fn message_text(text: &str, theme: &Theme) -> AnyElement {
    div()
        .px(px(8.))
        .py(px(10.))
        .text_size(px(12.5))
        .text_color(theme.muted.to_hsla())
        .child(text.to_string())
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(path: &str, staged: Option<char>, unstaged: Option<char>, untracked: bool) -> FileChange {
        FileChange { path: path.to_string(), staged, unstaged, untracked }
    }

    #[test]
    fn diff_target_follows_the_remaining_change() {
        let selected = |path: &str, staged, untracked| Selected { path: path.to_string(), staged, untracked };
        assert_eq!(diff_target(&file("a", None, Some('M'), false)), selected("a", false, false));
        assert_eq!(diff_target(&file("b", Some('A'), None, false)), selected("b", true, false));
        assert_eq!(diff_target(&file("c", None, None, true)), selected("c", false, true));
        // Staged and changed again: the working-tree change is the newest.
        assert_eq!(diff_target(&file("d", Some('M'), Some('M'), false)), selected("d", false, false));
    }

    #[test]
    fn listed_tracks_each_diff_kind() {
        let status = Status {
            files: vec![file("a", None, Some('M'), false), file("b", Some('A'), None, false), file("c", None, None, true)],
            ..Default::default()
        };
        assert!(listed(&status, &Selected { path: "a".into(), staged: false, untracked: false }));
        assert!(listed(&status, &Selected { path: "b".into(), staged: true, untracked: false }));
        assert!(listed(&status, &Selected { path: "c".into(), staged: false, untracked: true }));
        assert!(!listed(&status, &Selected { path: "a".into(), staged: true, untracked: false }));
        assert!(!listed(&status, &Selected { path: "gone".into(), staged: false, untracked: false }));
    }
}

//! The Git tab: the chat folder's repository status, diffs and commits.
//!
//! Each chat's tab state lives in a thread-local map keyed by chat id, since `ChatView` has no
//! field for it. GPUI runs all UI code on one thread, so a `RefCell` is enough. Background git
//! work only sends its results back, and they are applied here on that thread.

use super::{button, ChatView};
use crate::{git, store::PanelTab};
use gpui::{
    div, font, prelude::*, px, rgb, ClickEvent, Context, Div, KeyDownEvent, MouseButton, MouseDownEvent, SharedString,
    Stateful, Window,
};
use std::{
    cell::RefCell,
    collections::HashMap,
    path::{Path, PathBuf},
};

const FONT_FAMILY: &str = "DejaVu Sans Mono";
/// Commits listed under the changes.
const RECENT_COMMITS: usize = 10;

thread_local! {
    static STATES: RefCell<HashMap<String, GitState>> = RefCell::new(HashMap::new());
}

/// Runs `f` on a chat's Git tab state, creating it on first use.
fn with_state<R>(id: &str, f: impl FnOnce(&mut GitState) -> R) -> R {
    STATES.with(|states| f(states.borrow_mut().entry(id.to_string()).or_default()))
}

/// What the Git tab shows for one chat.
#[derive(Default)]
struct GitState {
    /// Bumped by each refresh; only the newest refresh's result is applied.
    seq: u64,
    /// Whether the tab has read the repository, which it does once when first shown.
    loaded: bool,
    status: Option<git::Status>,
    commits: Vec<git::Commit>,
    selected: Option<Selected>,
    diff: Option<Result<String, String>>,
    message: String,
    /// The commit box has the keyboard.
    editing: bool,
    /// The action running, e.g. "Running git push…"; further actions wait for it.
    busy: Option<&'static str>,
    /// Result of the last action, or why a refresh failed. `Err` shows in red.
    note: Option<Result<String, String>>,
}

/// The file whose diff is shown.
#[derive(Clone, PartialEq)]
struct Selected {
    path: String,
    staged: bool,
    untracked: bool,
}

impl ChatView {
    /// Keys for the commit message box. Returns true if consumed.
    pub(super) fn git_key(&mut self, ev: &KeyDownEvent, cx: &mut Context<Self>) -> bool {
        let ks = &ev.keystroke;
        let m = ks.modifiers;
        if !self.git_visible() || m.platform {
            return false;
        }
        let Some(id) = self.session().map(|s| s.id.clone()) else { return false };
        if !with_state(&id, |g| g.editing) {
            return false;
        }
        // Chat shortcuts stay with the chat.
        if m.control && matches!(ks.key.as_str(), "n" | "i" | ",") {
            return false;
        }
        match ks.key.as_str() {
            "enter" => self.git_commit(cx),
            "escape" => with_state(&id, |g| g.editing = false),
            "backspace" => with_state(&id, |g| {
                g.message.pop();
            }),
            "space" => with_state(&id, |g| g.message.push(' ')),
            "v" if m.control => {
                if let Some(text) = cx.read_from_clipboard().and_then(|c| c.text()) {
                    with_state(&id, |g| g.message.push_str(&text.replace(['\r', '\n'], " ")));
                }
            }
            _ if m.control || m.alt => {}
            _ => {
                if let Some(ch) = &ks.key_char {
                    with_state(&id, |g| g.message.push_str(ch));
                }
            }
        }
        cx.notify();
        true
    }

    /// A click outside the commit box gives the keyboard back to the chat.
    pub(super) fn git_blur(&mut self, cx: &mut Context<Self>) {
        let Some(id) = self.session().map(|s| s.id.clone()) else { return };
        if with_state(&id, |g| std::mem::take(&mut g.editing)) {
            cx.notify();
        }
    }

    /// Drops a deleted chat's Git tab state.
    pub(super) fn forget_git(&mut self, id: &str) {
        STATES.with(|states| states.borrow_mut().remove(id));
    }

    /// Files may have changed (e.g. an agent reply finished): refresh if shown.
    pub(super) fn git_changed(&mut self, cx: &mut Context<Self>) {
        if self.git_visible() {
            self.git_refresh(cx);
        }
    }

    pub(super) fn render_git(&self, _window: &Window, cx: &mut Context<Self>) -> impl IntoElement {
        let Some((id, dir)) = self.git_target() else {
            return div()
                .id("git-page")
                .flex_1()
                .min_h_0()
                .p_4()
                .text_xs()
                .text_color(rgb(0x7f849c))
                .child("No chat selected.");
        };
        // The tab reads the repository the first time it is shown for a chat.
        if !with_state(&id, |g| std::mem::replace(&mut g.loaded, true)) {
            Self::git_load(id.clone(), dir, cx);
        }
        with_state(&id, |g| self.git_page(g, cx))
    }

    /// The whole tab for a chat's state, top to bottom. The page scrolls; the diff scrolls itself.
    fn git_page(&self, g: &GitState, cx: &mut Context<Self>) -> Stateful<Div> {
        let mut page = div()
            .id("git-page")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .p_2()
            .flex()
            .flex_col()
            .gap_2()
            .text_xs()
            .child(self.git_header(g, cx));
        match &g.status {
            None => page = page.child(muted("Loading…")),
            Some(status) if !status.repo => {
                page = page.child(muted("This folder isn't a git repository.")).child(
                    button("git-init", "Initialize repository")
                        .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.git_init(cx))),
                );
            }
            Some(status) => {
                page = page
                    .child(self.git_commit_box(g, cx))
                    .child(self.git_list(true, &staged_files(status), g, cx))
                    .child(self.git_list(false, &changed_files(status), g, cx));
                if let Some(diff) = self.git_diff_view(g) {
                    page = page.child(diff);
                }
                page = page.child(git_commits(g, status.has_commits));
            }
        }
        // The last action's result, or a failed refresh.
        let note = g.note.as_ref().map(|result| match result {
            Ok(text) => (text.clone(), 0xa6adc8),
            Err(text) => (text.clone(), 0xf38ba8),
        });
        page.children(note.filter(|(text, _)| !text.is_empty()).map(|(text, color)| {
            div().text_color(rgb(color)).child(text)
        }))
    }

    /// The title line, the tracking counts and the Refresh, Pull and Push buttons.
    fn git_header(&self, g: &GitState, cx: &mut Context<Self>) -> Div {
        let refresh = button("git-refresh", "Refresh")
            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.git_refresh(cx)));
        let Some(status) = g.status.as_ref().filter(|s| s.repo) else {
            return div().flex().items_center().gap_1().child(refresh);
        };
        let name = status.branch.clone().unwrap_or_else(|| "detached".to_string());
        let tracking = if status.upstream.is_some() {
            format!("  ↑{} ↓{}", status.ahead, status.behind)
        } else {
            String::new()
        };
        let title = if status.has_commits { format!("{name}{tracking}") } else { format!("{name} · no commits yet") };
        let has_upstream = status.upstream.is_some();
        let pull = button("git-pull", "Pull")
            .when(!has_upstream, |b| b.text_color(rgb(0x6c7086)))
            .when(has_upstream, |b| b.on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.git_pull(cx))));
        let push = button("git-push", "Push")
            .when(!has_upstream, |b| b.text_color(rgb(0x6c7086)))
            .when(has_upstream, |b| b.on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.git_push(cx))));
        div()
            .flex()
            .flex_col()
            .gap_1()
            .child(div().truncate().child(title))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_1()
                    .child(refresh)
                    .child(pull)
                    .child(push)
                    .when(!has_upstream, |d| d.child(muted("no upstream")))
                    .children(g.busy.map(muted)),
            )
    }

    /// The commit message box and the Commit button.
    fn git_commit_box(&self, g: &GitState, cx: &mut Context<Self>) -> Div {
        let (text, color): (SharedString, u32) = if g.editing {
            (format!("{}▏", g.message).into(), 0xf2f2f7)
        } else if g.message.is_empty() {
            ("Commit message".into(), 0x6c7086)
        } else {
            (g.message.clone().into(), 0xf2f2f7)
        };
        let message = div()
            .id("git-message")
            .flex_1()
            .min_w_0()
            .px_2()
            .py_1()
            .rounded_md()
            .truncate()
            .cursor_text()
            .bg(rgb(0x2b2d3a))
            .border_1()
            .border_color(if g.editing { rgb(0x89b4fa) } else { rgb(0x2b2d3a) })
            .text_color(rgb(color))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _: &MouseDownEvent, window, cx| {
                    cx.stop_propagation();
                    window.focus(&this.focus);
                    this.preview_blur(cx);
                    this.term_blur(cx);
                    this.editor_blur(cx);
                    if let Some((id, _)) = this.git_target() {
                        with_state(&id, |g| g.editing = true);
                    }
                    cx.notify();
                }),
            )
            .child(text);
        div()
            .flex()
            .items_center()
            .gap_1()
            .child(message)
            .child(
                button("git-commit", "Commit")
                    .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.git_commit(cx))),
            )
    }

    /// The staged or the changed files under a heading with its Stage all or Unstage all button.
    fn git_list(&self, staged: bool, files: &[&git::FileChange], g: &GitState, cx: &mut Context<Self>) -> Div {
        let title = format!("{} ({})", if staged { "Staged changes" } else { "Changes" }, files.len());
        let bulk = (!files.is_empty()).then(|| {
            if staged {
                button("git-unstage-all", "Unstage all")
                    .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.git_unstage_all(cx)))
            } else {
                button("git-stage-all", "Stage all")
                    .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.git_stage_all(cx)))
            }
        });
        let empty = muted(if staged { "Nothing staged." } else { "No changes." });
        let rows: Vec<Stateful<Div>> = files
            .iter()
            .enumerate()
            .map(|(i, file)| {
                let picked = g.selected.as_ref().is_some_and(|sel| sel.path == file.path && sel.staged == staged);
                self.git_row(i, file, staged, picked, cx)
            })
            .collect();
        div()
            .flex()
            .flex_col()
            .gap_1()
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .text_color(rgb(0xa6adc8))
                    .child(title)
                    .children(bulk),
            )
            .children(files.is_empty().then_some(empty))
            .children(rows)
    }

    /// One file in a list: its status letter, its path, and the button that moves it to the other
    /// list. Clicking the row shows its diff.
    fn git_row(
        &self,
        i: usize,
        file: &git::FileChange,
        staged: bool,
        picked: bool,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let (row_id, move_id, move_label) = if staged {
            ("git-staged", "git-unstage", "−")
        } else {
            ("git-change", "git-stage", "+")
        };
        let letter = list_letter(file, staged);
        let path = file.path.clone();
        let sel = Selected { path: file.path.clone(), staged, untracked: file.untracked && !staged };
        div()
            .id((row_id, i))
            .flex()
            .items_center()
            .gap_1()
            .px_1()
            .rounded_md()
            .cursor_pointer()
            .when(picked, |d| d.bg(rgb(0x313244)))
            .hover(|d| d.bg(rgb(0x2a2b3c)))
            .child(
                div()
                    .w(px(12.))
                    .flex_none()
                    .font(font(FONT_FAMILY))
                    .text_color(rgb(letter_color(letter)))
                    .child(letter.to_string()),
            )
            .child(div().flex_1().min_w_0().truncate().child(file.path.clone()))
            .child(button((move_id, i), move_label).on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                cx.stop_propagation();
                if staged {
                    this.git_unstage(vec![path.clone()], cx);
                } else {
                    this.git_stage(vec![path.clone()], cx);
                }
            })))
            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| this.git_select(sel.clone(), cx)))
    }

    /// The diff of the picked file, or `None` when no file is picked.
    fn git_diff_view(&self, g: &GitState) -> Option<Div> {
        let sel = g.selected.as_ref()?;
        let label = if sel.staged {
            "staged"
        } else if sel.untracked {
            "untracked"
        } else {
            "unstaged"
        };
        let body = match &g.diff {
            None => muted("Loading…").into_any_element(),
            Some(Err(e)) => div().text_color(rgb(0xf38ba8)).child(e.clone()).into_any_element(),
            Some(Ok(text)) if text.is_empty() => muted("No textual changes.").into_any_element(),
            Some(Ok(text)) => div()
                .id("git-diff")
                .max_h(px(320.))
                .overflow_y_scroll()
                .font(font(FONT_FAMILY))
                .flex()
                .flex_col()
                .children(text.lines().map(diff_line))
                .into_any_element(),
        };
        Some(
            div()
                .flex()
                .flex_col()
                .gap_1()
                .child(muted(format!("{} · {label}", sel.path)).truncate())
                .child(body),
        )
    }

    /// Reads the repository in the background and shows it, unless a newer read has started.
    fn git_load(id: String, dir: PathBuf, cx: &mut Context<Self>) {
        let seq = with_state(&id, |g| {
            g.seq += 1;
            g.seq
        });
        let reading = dir.clone();
        cx.spawn(async move |this, cx| {
            let read = cx.background_executor().spawn(async move { read_repo(&reading) }).await;
            let _ = this.update(cx, |_, cx| {
                let picked = with_state(&id, |g| {
                    if g.seq != seq {
                        return None;
                    }
                    match read {
                        Ok((status, commits)) => {
                            if g.selected.as_ref().is_some_and(|sel| !listed(&status, sel)) {
                                g.selected = None;
                                g.diff = None;
                            }
                            g.status = Some(status);
                            g.commits = commits;
                        }
                        Err(e) => g.note = Some(Err(e)),
                    }
                    g.selected.clone()
                });
                if let Some(sel) = picked {
                    Self::git_diff(id, dir, sel, cx);
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Reads the diff of a picked file in the background. Dropped if another file is picked meanwhile.
    fn git_diff(id: String, dir: PathBuf, sel: Selected, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            let picked = sel.clone();
            let diff = cx
                .background_executor()
                .spawn(async move { git::diff(&dir, &picked.path, picked.staged, picked.untracked) })
                .await;
            let _ = this.update(cx, |_, cx| {
                with_state(&id, |g| {
                    if g.selected.as_ref() == Some(&sel) {
                        g.diff = Some(diff);
                    }
                });
                cx.notify();
            });
        })
        .detach();
    }

    /// Runs a git action for the current chat in the background, then refreshes. Ignored while
    /// another action runs. `on_ok` changes the state when the action succeeds.
    fn git_action(
        &mut self,
        label: &'static str,
        work: impl FnOnce(&Path) -> Result<String, String> + Send + 'static,
        on_ok: fn(&mut GitState),
        cx: &mut Context<Self>,
    ) {
        let Some((id, dir)) = self.git_target() else { return };
        let started = with_state(&id, |g| {
            if g.busy.is_some() {
                return false;
            }
            g.busy = Some(label);
            g.note = None;
            true
        });
        if !started {
            return;
        }
        cx.notify();
        let refreshing = dir.clone();
        cx.spawn(async move |this, cx| {
            let result = cx.background_executor().spawn(async move { work(&dir) }).await;
            let _ = this.update(cx, |_, cx| {
                with_state(&id, |g| {
                    g.busy = None;
                    if result.is_ok() {
                        on_ok(g);
                    }
                    g.note = Some(result);
                });
                Self::git_load(id, refreshing, cx);
                cx.notify();
            });
        })
        .detach();
    }

    /// Reads the current chat's repository again.
    fn git_refresh(&mut self, cx: &mut Context<Self>) {
        if let Some((id, dir)) = self.git_target() {
            Self::git_load(id, dir, cx);
        }
    }

    /// Shows the diff of `sel`.
    fn git_select(&mut self, sel: Selected, cx: &mut Context<Self>) {
        let Some((id, dir)) = self.git_target() else { return };
        with_state(&id, |g| {
            g.selected = Some(sel.clone());
            g.diff = None;
        });
        Self::git_diff(id, dir, sel, cx);
        cx.notify();
    }

    /// Commits the staged changes with the commit box's message. Does nothing without a message.
    fn git_commit(&mut self, cx: &mut Context<Self>) {
        let Some((id, _)) = self.git_target() else { return };
        let (message, staged) = with_state(&id, |g| {
            let staged = g.status.as_ref().is_some_and(|s| s.files.iter().any(|f| f.staged.is_some()));
            (g.message.trim().to_string(), staged)
        });
        if message.is_empty() || with_state(&id, |g| g.busy.is_some()) {
            return;
        }
        if !staged {
            with_state(&id, |g| g.note = Some(Err("Stage files first".to_string())));
            cx.notify();
            return;
        }
        self.git_action("Committing…", move |dir| git::commit(dir, &message), |g| g.message.clear(), cx);
    }

    fn git_stage(&mut self, paths: Vec<String>, cx: &mut Context<Self>) {
        self.git_action("Staging…", move |dir| git::stage(dir, &paths).map(|()| String::new()), |_| {}, cx);
    }

    fn git_stage_all(&mut self, cx: &mut Context<Self>) {
        self.git_action("Staging…", |dir| git::stage_all(dir).map(|()| String::new()), |_| {}, cx);
    }

    fn git_unstage(&mut self, paths: Vec<String>, cx: &mut Context<Self>) {
        self.git_action("Unstaging…", move |dir| git::unstage(dir, &paths).map(|()| String::new()), |_| {}, cx);
    }

    fn git_unstage_all(&mut self, cx: &mut Context<Self>) {
        self.git_action("Unstaging…", |dir| git::unstage_all(dir).map(|()| String::new()), |_| {}, cx);
    }

    fn git_init(&mut self, cx: &mut Context<Self>) {
        self.git_action("Initializing…", git::init, |_| {}, cx);
    }

    fn git_push(&mut self, cx: &mut Context<Self>) {
        if self.git_has_upstream() {
            self.git_action("Running git push…", git::push, |_| {}, cx);
        }
    }

    fn git_pull(&mut self, cx: &mut Context<Self>) {
        if self.git_has_upstream() {
            self.git_action("Running git pull…", git::pull, |_| {}, cx);
        }
    }

    /// Whether the current chat's branch tracks a remote branch.
    fn git_has_upstream(&self) -> bool {
        self.git_target().is_some_and(|(id, _)| {
            with_state(&id, |g| g.status.as_ref().is_some_and(|s| s.upstream.is_some()))
        })
    }

    /// The current chat's id and folder.
    fn git_target(&self) -> Option<(String, PathBuf)> {
        self.session().map(|s| (s.id.clone(), s.cwd.clone()))
    }

    /// Whether the Git tab is on screen.
    fn git_visible(&self) -> bool {
        !self.settings.hide_panel && self.settings.panel_tab == PanelTab::Git
    }
}

/// The repository's state and recent commits, read off the UI thread.
fn read_repo(dir: &Path) -> Result<(git::Status, Vec<git::Commit>), String> {
    let status = git::status(dir)?;
    let commits = if status.repo { git::log(dir, RECENT_COMMITS)? } else { Vec::new() };
    Ok((status, commits))
}

/// Files with changes in the index.
fn staged_files(status: &git::Status) -> Vec<&git::FileChange> {
    status.files.iter().filter(|f| f.staged.is_some()).collect()
}

/// Files changed in the working tree since they were staged, including untracked files.
fn changed_files(status: &git::Status) -> Vec<&git::FileChange> {
    status.files.iter().filter(|f| f.unstaged.is_some() || f.untracked).collect()
}

/// Whether `sel` is still listed in `status`.
fn listed(status: &git::Status, sel: &Selected) -> bool {
    status.files.iter().any(|f| {
        f.path == sel.path && if sel.staged { f.staged.is_some() } else { f.unstaged.is_some() || f.untracked }
    })
}

/// The status letter shown for a file in the staged or the changes list.
fn list_letter(file: &git::FileChange, staged: bool) -> char {
    let letter = if staged {
        file.staged
    } else if file.untracked {
        Some('?')
    } else {
        file.unstaged
    };
    letter.unwrap_or('?')
}

/// The colour of a status letter: yellow for modified, green for added or untracked, red for
/// deleted, blue for renamed.
fn letter_color(letter: char) -> u32 {
    match letter {
        'M' => 0xf9e2af,
        'A' | '?' => 0xa6e3a1,
        'D' => 0xf38ba8,
        'R' => 0x89b4fa,
        _ => 0x9399b2,
    }
}

/// One line of a diff, coloured by what it adds, removes or where it is.
fn diff_line(line: &str) -> Div {
    let color = if line.starts_with("@@") {
        Some(0x89b4fa)
    } else if line.starts_with('+') {
        Some(0xa6e3a1)
    } else if line.starts_with('-') {
        Some(0xf38ba8)
    } else {
        None
    };
    let text = if line.is_empty() { " ".to_string() } else { line.to_string() };
    div().when_some(color, |d, color| d.text_color(rgb(color))).child(text)
}

/// The recent commits, one line each, or a note when there are none.
fn git_commits(g: &GitState, has_commits: bool) -> Div {
    let mut section = div().flex().flex_col().gap_1().child(muted("Recent commits"));
    if !has_commits {
        return section.child(muted("No commits yet."));
    }
    for commit in &g.commits {
        section = section.child(
            div()
                .flex()
                .items_center()
                .gap_1()
                .child(div().flex_none().text_color(rgb(0x6c7086)).child(commit.hash.clone()))
                .child(div().flex_1().min_w_0().truncate().child(commit.subject.clone()))
                .child(
                    div()
                        .flex_none()
                        .text_color(rgb(0x7f849c))
                        .child(format!("· {} · {}", commit.author, commit.date)),
                ),
        );
    }
    section
}

/// Muted text for headings and hints.
fn muted(text: impl Into<SharedString>) -> Div {
    div().text_color(rgb(0x7f849c)).child(text.into())
}

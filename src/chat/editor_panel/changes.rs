//! The agent's changes to the chat's folder: the baseline each changed file is compared against, how
//! open files reload or get flagged, and how the editor follows the agent.

use super::{Buffer, ChatView, EditorSet};
use crate::{
    git,
    workspace::{Change, ChangeKind},
};
use gpui::Context;
use std::{
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

/// The text a changed file held before the agent's first change: the open buffer's saved text, else
/// the file's `HEAD` text. A file that did not exist before is empty. `None` when neither is known.
pub(super) fn baseline_for(open_saved: Option<&str>, head: Option<String>, kind: ChangeKind) -> Option<String> {
    open_saved
        .map(str::to_string)
        .or(head)
        .or_else(|| (kind == ChangeKind::Created).then(String::new))
}

/// Records a change to `path`. A path already in `changes` keeps its place and takes the newest kind
/// and time. Returns whether the path is new to `changes`.
pub(super) fn record(
    changes: &mut Vec<(PathBuf, ChangeKind, Instant)>,
    path: &Path,
    kind: ChangeKind,
    at: Instant,
) -> bool {
    match changes.iter_mut().find(|(p, _, _)| p == path) {
        Some(entry) => {
            entry.1 = kind;
            entry.2 = at;
            false
        }
        None => {
            changes.push((path.to_path_buf(), kind, at));
            true
        }
    }
}

impl ChatView {
    /// Handles one change the watcher reports under chat `id`'s folder. The first change to a file
    /// records its baseline. An open file reloads or is flagged, and the editor follows the change.
    pub(super) fn agent_change(&mut self, id: &str, change: Change, cx: &mut Context<Self>) {
        let Some(set) = self.editors.sets.get_mut(id) else { return };
        set.tree.changed(&change.path, change.kind);
        let first = record(&mut set.changes, &change.path, change.kind, Instant::now());
        let open_saved = set
            .files
            .iter()
            .find(|f| f.buffer.path() == Some(change.path.as_path()))
            .map(|f| f.saved.clone());
        if first && !set.baselines.contains_key(&change.path) {
            match open_saved.clone() {
                Some(saved) => {
                    set.baselines.insert(change.path.clone(), saved);
                }
                None => self.fetch_head_baseline(id, &change.path, change.kind, cx),
            }
        }

        if let Some(index) = self.file_index(id, &change.path) {
            if let Some(file) = self.editors.sets.get_mut(id).and_then(|s| s.files.get_mut(index)) {
                file.take_disk_change(change.kind);
            }
            self.refresh_marks(id, index, Duration::ZERO, cx);
        }

        if self.editors.follow && change.kind != ChangeKind::Removed && change.path.is_file() {
            if self.editors.focused {
                if let Some(set) = self.editors.sets.get_mut(id) {
                    set.notice = Some(change.path.clone());
                }
            } else {
                self.open_in(id, &change.path, true, cx);
            }
        }
        cx.notify();
    }

    /// Reads the `HEAD` text of `path` off the UI thread, then records it as the baseline. A `Clear`
    /// meanwhile makes the result stale, and it is dropped.
    fn fetch_head_baseline(&mut self, id: &str, path: &Path, kind: ChangeKind, cx: &mut Context<Self>) {
        let Some(set) = self.editors.sets.get(id) else { return };
        let (root, generation) = (set.root.clone(), set.generation);
        let (id, path) = (id.to_string(), path.to_path_buf());
        let read_path = path.clone();
        cx.spawn(async move |this, cx| {
            let head = cx.background_executor().spawn(async move { git::head_version(&root, &read_path) }).await;
            this.update(cx, |this, cx| this.set_baseline(&id, &path, kind, generation, head, cx)).ok();
        })
        .detach();
    }

    /// Stores the baseline of `path` read for `generation`, unless the baseline is known or the changes
    /// were cleared since. Then the file's markers are recomputed against it.
    fn set_baseline(
        &mut self,
        id: &str,
        path: &Path,
        kind: ChangeKind,
        generation: u64,
        head: Option<String>,
        cx: &mut Context<Self>,
    ) {
        let Some(set) = self.editors.sets.get_mut(id) else { return };
        if set.generation != generation || set.baselines.contains_key(path) {
            return;
        }
        if let Some(text) = baseline_for(None, head, kind) {
            set.baselines.insert(path.to_path_buf(), text);
        }
        if let Some(index) = self.file_index(id, path) {
            self.refresh_marks(id, index, Duration::ZERO, cx);
        }
    }

    /// Clears the agent's changes, their baselines, the notice, and the change markers of the open files.
    pub(super) fn clear_changes(&mut self, cx: &mut Context<Self>) {
        let Some(id) = self.current.clone() else { return };
        let Some(set) = self.editors.sets.get_mut(&id) else { return };
        set.changes.clear();
        set.baselines.clear();
        set.notice = None;
        set.generation += 1;
        for file in &mut set.files {
            file.marks.clear();
            file.marks_task = None;
            file.reveal_change = false;
        }
        cx.notify();
    }

    /// Opens a file from the changes list, scrolled to its first change.
    pub(super) fn open_change(&mut self, path: &Path, cx: &mut Context<Self>) {
        let Some(id) = self.current.clone() else { return };
        self.open_in(&id, path, true, cx);
    }

    /// Opens the file the agent changed while the editor had the keyboard.
    pub(super) fn view_notice(&mut self, cx: &mut Context<Self>) {
        let Some(id) = self.current.clone() else { return };
        let Some(path) = self.editors.sets.get_mut(&id).and_then(|s| s.notice.take()) else { return };
        self.open_in(&id, &path, true, cx);
    }

    /// Hides the notice about a file the agent changed.
    pub(super) fn dismiss_notice(&mut self, cx: &mut Context<Self>) {
        if let Some(set) = self.editor_set_mut() {
            set.notice = None;
        }
        cx.notify();
    }

    /// Replaces the active file's text with the file on disk, dropping its unsaved edits.
    pub(super) fn reload_from_disk(&mut self, cx: &mut Context<Self>) {
        let Some(id) = self.current.clone() else { return };
        let Some(set) = self.editors.sets.get_mut(&id) else { return };
        let index = set.active;
        let Some(file) = set.files.get_mut(index) else { return };
        let Some(path) = file.buffer.path().map(Path::to_path_buf) else { return };
        match Buffer::open(&path) {
            Ok(disk) => file.reload(&disk.text()),
            Err(e) => file.error = Some(format!("Could not read {}: {e}", path.display())),
        }
        self.refresh_marks(&id, index, Duration::ZERO, cx);
        cx.notify();
    }

    /// Keeps the active file's unsaved edits over the changed file on disk.
    pub(super) fn keep_mine(&mut self, cx: &mut Context<Self>) {
        if let Some(file) = self.editor_set_mut().and_then(EditorSet::active_file) {
            file.changed_on_disk = false;
        }
        cx.notify();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn baseline_prefers_the_open_buffer_then_head_then_empty_for_new_files() {
        assert_eq!(baseline_for(Some("open"), Some("head".into()), ChangeKind::Modified), Some("open".to_string()));
        assert_eq!(baseline_for(None, Some("head".into()), ChangeKind::Modified), Some("head".to_string()));
        assert_eq!(baseline_for(None, None, ChangeKind::Created), Some(String::new()));
        assert_eq!(baseline_for(None, None, ChangeKind::Modified), None);
        assert_eq!(baseline_for(None, None, ChangeKind::Removed), None);
    }

    #[test]
    fn repeated_changes_to_a_path_are_recorded_once() {
        let mut changes = Vec::new();
        let now = Instant::now();
        assert!(record(&mut changes, Path::new("/w/a.rs"), ChangeKind::Created, now));
        assert!(!record(&mut changes, Path::new("/w/a.rs"), ChangeKind::Modified, now));
        assert!(record(&mut changes, Path::new("/w/b.rs"), ChangeKind::Modified, now));
        assert_eq!(changes.len(), 2);
        assert_eq!(changes[0].0.as_path(), Path::new("/w/a.rs"));
        assert_eq!(changes[0].1, ChangeKind::Modified, "the newest kind wins");
    }
}

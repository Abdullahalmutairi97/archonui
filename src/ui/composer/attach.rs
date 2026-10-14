//! The attach popover and the ways files reach a message: the popover's rows, the file picker, drops
//! and pasted images. Spec: chat.md §3, §4.3.

use super::{
    picker::{popover_shadow, Pop},
    Shell,
};
use crate::{
    app::workspace::RightTab,
    engine::attach::{Attachment, MAX_FILES},
    store,
};
use gpui::{
    div, prelude::*, AnyElement, ClickEvent, ClipboardEntry, Context, ImageFormat, PathPromptOptions,
    SharedString,
};
use crate::ui::scale::px;
use std::{
    fs,
    path::{Path, PathBuf},
    time::SystemTime,
};

/// How many recently changed files the popover offers.
const RECENT_FILES: usize = 4;
/// Folders below the session's folder that the recent-file scan reads.
const SCAN_DEPTH: usize = 4;
/// Entries the recent-file scan reads at most, so a huge folder cannot stall opening the popover.
const SCAN_ENTRIES: usize = 5000;

impl Shell {
    /// The attach button: opens the attach popover, or closes it if it is open.
    pub(super) fn toggle_attach(&mut self, id: &str, cx: &mut Context<Self>) {
        if self.composers.pop(id) == Some(Pop::Attach) {
            self.close_popover(id, cx);
            return;
        }
        let Some(cwd) = self.state.session(id).map(|s| s.cwd.clone()) else { return };
        self.composers.set_recent(id, recent_files(&cwd));
        self.open_popover(id, Pop::Attach, cx);
    }

    /// Adds `paths` to the files session `id`'s next message carries. Files past the limit, files
    /// already attached and files that cannot be read are skipped, with a toast for each problem.
    pub(in crate::ui) fn add_files(&mut self, id: &str, paths: &[PathBuf], cx: &mut Context<Self>) {
        let pending = self.state.pending(id);
        let room = MAX_FILES.saturating_sub(pending.len());
        let mut taken: Vec<PathBuf> = pending.iter().map(|a| a.path.clone()).collect();
        let (mut files, mut problems, mut full) = (Vec::new(), Vec::new(), false);
        for path in paths {
            if taken.contains(path) {
                continue;
            }
            if files.len() == room {
                full = true;
                break;
            }
            match Attachment::inspect(path) {
                Ok(file) => {
                    taken.push(path.clone());
                    files.push(file);
                }
                Err(problem) => problems.push(problem),
            }
        }
        if !files.is_empty() {
            self.state.add_pending(id, files);
        }
        if full {
            self.toast(format!("A message takes at most {MAX_FILES} files."), cx);
        }
        if !problems.is_empty() {
            self.toast(problems.join(" "), cx);
        }
        cx.notify();
    }

    /// Ctrl+V with an image on the clipboard: saves it into the session's attachments and attaches it.
    /// Returns false when the clipboard holds no image, so the text paste goes ahead.
    pub(super) fn paste_image(&mut self, id: &str, cx: &mut Context<Self>) -> bool {
        let Some(item) = cx.read_from_clipboard() else { return false };
        let Some(image) = item.entries().iter().find_map(|e| match e {
            ClipboardEntry::Image(image) => Some(image),
            ClipboardEntry::String(_) => None,
        }) else {
            return false;
        };
        let ext = match image.format {
            ImageFormat::Png => "png",
            ImageFormat::Jpeg => "jpg",
            ImageFormat::Webp => "webp",
            ImageFormat::Gif => "gif",
            _ => {
                self.toast("That image format can't be attached", cx);
                return true;
            }
        };
        let name = format!("pasted-{}.{ext}", store::now());
        match store::store_bytes(id, &name, image.bytes()) {
            Ok(path) => self.add_files(id, &[path], cx),
            Err(problem) => self.toast(format!("Couldn't save the pasted image: {problem}"), cx),
        }
        true
    }

    /// The attach popover: recent files, then the browser and terminal captures, then the file picker.
    pub(super) fn render_attach_popover(&self, id: &str, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = &self.theme;
        let mut rows: Vec<AnyElement> = Vec::new();
        for (i, path) in self.composers.recent(id).iter().enumerate() {
            let (id, path) = (id.to_string(), path.clone());
            let name = path.file_name().map_or_else(|| path.display().to_string(), |n| n.to_string_lossy().into_owned());
            let folder = path.parent().map(|p| p.display().to_string()).unwrap_or_default();
            rows.push(self.attach_row(
                format!("composer-recent-{id}-{i}"),
                name,
                folder,
                cx,
                move |this, cx| {
                    this.add_files(&id, std::slice::from_ref(&path), cx);
                    this.close_popover(&id, cx);
                },
            ));
        }
        let (browser_id, terminal_id, upload_id) = (id.to_string(), id.to_string(), id.to_string());
        rows.push(self.attach_row(
            format!("composer-browser-{id}"),
            "Browser screenshot".into(),
            "Attach what the browser shows".into(),
            cx,
            move |this, cx| {
                this.capture_browser_screenshot(&browser_id, cx);
                this.close_popover(&browser_id, cx);
            },
        ));
        rows.push(self.attach_row(
            format!("composer-terminal-{id}"),
            "Terminal output".into(),
            "Attach the terminal's recent output".into(),
            cx,
            move |this, cx| {
                this.capture_terminal_output(&terminal_id, cx);
                this.close_popover(&terminal_id, cx);
            },
        ));
        rows.push(self.attach_row(
            format!("composer-upload-{id}"),
            "Upload from disk…".into(),
            "Choose files to attach".into(),
            cx,
            move |this, cx| {
                this.close_popover(&upload_id, cx);
                this.pick_files(&upload_id, cx);
            },
        ));

        div()
            .w(px(300.))
            .p(px(6.))
            .rounded(px(10.))
            .bg(theme.pop.to_hsla())
            .border_1()
            .border_color(theme.line2.to_hsla())
            .shadow(popover_shadow())
            .occlude()
            .flex()
            .flex_col()
            .gap(px(2.))
            .child(
                div()
                    .px(px(8.))
                    .py(px(6.))
                    .text_size(px(10.5))
                    .text_color(theme.muted.to_hsla())
                    .child("ATTACH"),
            )
            .children(rows)
    }

    /// One row of the attach popover: a label over a detail line. `choose` runs when it is clicked.
    fn attach_row(
        &self,
        row_id: String,
        label: String,
        detail: String,
        cx: &mut Context<Self>,
        choose: impl Fn(&mut Shell, &mut Context<Shell>) + 'static,
    ) -> AnyElement {
        let theme = &self.theme;
        div()
            .id(SharedString::from(row_id))
            .flex()
            .flex_col()
            .gap(px(2.))
            .px(px(10.))
            .py(px(7.))
            .rounded(px(6.))
            .cursor_pointer()
            .hover(|h| h.bg(theme.line2.to_hsla()))
            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| choose(this, cx)))
            .child(div().text_size(px(13.)).text_color(theme.fg.to_hsla()).truncate().child(label))
            .child(div().text_size(px(11.)).text_color(theme.muted.to_hsla()).truncate().child(detail))
            .into_any_element()
    }

    /// Asks for files, then attaches the ones picked to session `id`. A cancelled picker does nothing.
    fn pick_files(&mut self, id: &str, cx: &mut Context<Self>) {
        let id = id.to_string();
        let picked = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: true,
            prompt: Some("Attach".into()),
        });
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(paths))) = picked.await else { return };
            this.update(cx, |this, cx| this.add_files(&id, &paths, cx)).ok();
        })
        .detach();
    }

    /// Attaches what the Browser tab shows to session `id`'s next message.
    fn capture_browser_screenshot(&mut self, id: &str, cx: &mut Context<Self>) {
        match self.browser_screenshot(cx) {
            Some(path) => self.add_files(id, &[path], cx),
            None => {
                self.show_tool_tab(RightTab::Browser, cx);
                self.toast("Open a page in the Browser tab first", cx);
            }
        }
    }

    /// Attaches the visible text of the focused session's terminal to session `id`'s next message.
    fn capture_terminal_output(&mut self, id: &str, cx: &mut Context<Self>) {
        let Some(text) = self.terminal_capture().filter(|t| !t.trim().is_empty()) else {
            self.show_tool_tab(RightTab::Terminal, cx);
            self.toast("The terminal has no output yet", cx);
            return;
        };
        match crate::store::store_bytes(id, "terminal.log", text.as_bytes()) {
            Ok(path) => self.add_files(id, &[path], cx),
            Err(e) => self.toast(format!("Could not save the terminal output: {e}"), cx),
        }
    }
}

/// The most recently changed files in `root` and the folders below it, newest first. Hidden entries
/// and `target` and `node_modules` folders are skipped.
fn recent_files(root: &Path) -> Vec<PathBuf> {
    let mut found: Vec<(SystemTime, PathBuf)> = Vec::new();
    let mut dirs = vec![(root.to_path_buf(), 0usize)];
    let mut seen = 0;
    while let Some((dir, depth)) = dirs.pop() {
        let Ok(entries) = fs::read_dir(&dir) else { continue };
        for entry in entries.flatten() {
            seen += 1;
            if seen > SCAN_ENTRIES {
                dirs.clear();
                break;
            }
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if name.starts_with('.') || matches!(name.as_ref(), "target" | "node_modules") {
                continue;
            }
            let Ok(file_type) = entry.file_type() else { continue };
            if file_type.is_dir() {
                if depth + 1 < SCAN_DEPTH {
                    dirs.push((entry.path(), depth + 1));
                }
            } else if file_type.is_file() {
                if let Ok(modified) = entry.metadata().and_then(|m| m.modified()) {
                    found.push((modified, entry.path()));
                }
            }
        }
    }
    found.sort_by(|a, b| b.0.cmp(&a.0));
    found.into_iter().take(RECENT_FILES).map(|(_, path)| path).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recent_files_skip_hidden_and_build_folders_and_stop_at_the_depth_limit() {
        let root = std::env::temp_dir().join(format!("archonui-recent-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        for dir in ["target", "node_modules", "src/a/b/c/d"] {
            fs::create_dir_all(root.join(dir)).unwrap();
        }
        fs::write(root.join("main.rs"), "").unwrap();
        fs::write(root.join(".env"), "").unwrap();
        fs::write(root.join("target/out"), "").unwrap();
        fs::write(root.join("node_modules/dep.js"), "").unwrap();
        fs::write(root.join("src/a/b/c/d/deep.txt"), "").unwrap();

        let names: Vec<String> = recent_files(&root)
            .iter()
            .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        let _ = fs::remove_dir_all(&root);

        assert!(names.contains(&"main.rs".to_string()), "{names:?}");
        assert!(!names.iter().any(|n| n == ".env" || n == "out" || n == "dep.js"), "{names:?}");
        assert!(!names.contains(&"deep.txt".to_string()), "{names:?}");
    }
}

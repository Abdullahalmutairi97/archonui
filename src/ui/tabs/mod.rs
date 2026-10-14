//! Contents of the right tools panel's tabs. They act on the focused session's folder.
//! Spec: docs/design/tools-and-behaviour.md §A1–A5.

mod browser;
mod files;
mod git;
mod ide;
mod terminal;

use super::shell::Shell;
use crate::app::workspace::RightTab;
use gpui::{AnyElement, Context, KeyDownEvent, Window};
use std::path::PathBuf;

/// View state of all tabs, not persisted.
#[derive(Default)]
pub struct TabsUi {
    pub(super) browser: browser::BrowserUi,
    pub(super) ide: ide::IdeUi,
    pub(super) files: files::FilesUi,
    pub(super) git: git::GitUi,
    pub(super) terminal: terminal::TerminalUi,
}

impl Shell {
    /// The folder tools act on: the focused session's, else the current project's, else the default.
    pub(super) fn tool_folder(&self) -> PathBuf {
        let ws = &self.state.workspace;
        if let Some(s) = ws.layout.focus.as_deref().and_then(|id| self.state.session(id)) {
            return s.cwd.clone();
        }
        if let Some(p) = ws.current_project.as_deref().and_then(|id| self.state.project(id)) {
            return p.path.clone();
        }
        self.default_folder()
    }

    pub(super) fn render_tab(&mut self, tab: RightTab, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        match tab {
            RightTab::Browser => self.render_browser(window, cx),
            RightTab::Ide => self.render_ide(window, cx),
            RightTab::Files => self.render_files(window, cx),
            RightTab::Git => self.render_git(window, cx),
            RightTab::Terminal => self.render_terminal(window, cx),
        }
    }

    /// Keys for whichever tab has the keyboard. Returns true when consumed.
    pub(super) fn tab_key(&mut self, ev: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) -> bool {
        self.browser_key(ev, window, cx)
            || self.ide_key(ev, window, cx)
            || self.files_key(ev, window, cx)
            || self.git_key(ev, window, cx)
            || self.terminal_key(ev, window, cx)
    }

    /// Per-frame housekeeping every tab may need (browser frames, terminal resize, file watching).
    pub(super) fn tabs_before_render(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.browser_before_render(window, cx);
        self.ide_before_render(window, cx);
        self.terminal_before_render(window, cx);
    }

    /// A click outside the tools panel gives the keyboard back to the chat.
    pub(super) fn tabs_blur(&mut self, cx: &mut Context<Self>) {
        self.browser_blur(cx);
        self.ide_blur(cx);
        self.files_blur(cx);
        self.git_blur(cx);
        self.terminal_blur(cx);
    }
}

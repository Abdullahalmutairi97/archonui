//! The session switcher (Ctrl+K): a palette over every session, searched by title and message text.
//! Enter opens a session in the focused tile and Alt+Enter in a new split, as a sidebar click does.

use super::{
    shell::Shell,
    sigil::sigil_element,
    sidebar::sessions::dot,
    widgets::text_input::{text_input, TextInput, TextInputConfig, TextInputEvent},
};
use crate::{
    app::sections::ago,
    store::{self, Session},
};
use gpui::{
    div, hsla, point, prelude::*, AnyElement, App, BoxShadow, ClickEvent, Context, ElementId, Entity, Keystroke,
    MouseButton, ScrollHandle, Subscription, Window,
};
use crate::ui::scale::px;
use std::cmp::Reverse;

/// Switcher state, not persisted.
#[derive(Default)]
pub struct SwitcherUi {
    pub(super) open: bool,
    /// Routes the navigation keys to the palette before the search field's bindings take them.
    keys: Option<Subscription>,
    /// The search field, made on first render.
    input: Option<(Entity<TextInput>, Subscription)>,
    /// Clear the search and focus it on the next frame.
    focus_input: bool,
    /// Index into the rows of the current search.
    selected: usize,
    scroll: ScrollHandle,
}

/// One line of the palette.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Row {
    New,
    Session(String),
}

/// The sessions matching `query`, best first. A title that starts with the query comes first, then a title
/// that contains it, then sessions whose title or messages contain every word of it. Within a rank the most
/// recently used come first. An empty query lists every session by recency.
pub fn rank<'a>(sessions: &'a [Session], query: &str) -> Vec<&'a Session> {
    let query = query.trim().to_lowercase();
    let words: Vec<&str> = query.split_whitespace().collect();
    let mut hits: Vec<(u8, &Session)> = sessions
        .iter()
        .filter_map(|s| {
            if words.is_empty() {
                return Some((0, s));
            }
            let title = s.title.to_lowercase();
            if title.starts_with(&query) {
                Some((0, s))
            } else if title.contains(&query) {
                Some((1, s))
            } else {
                let found = words.iter().all(|w| {
                    title.contains(w) || s.messages.iter().any(|m| m.text.to_lowercase().contains(w))
                });
                found.then_some((2, s))
            }
        })
        .collect();
    hits.sort_by_key(|(tier, s)| (*tier, Reverse(s.updated)));
    hits.into_iter().map(|(_, s)| s).collect()
}

/// The row selected when the palette opens or the search changes: the most recent session when the search is
/// empty (just below New session), else the best match.
fn default_row(rows: &[Row]) -> usize {
    usize::from(rows.len() > 1 && rows[0] == Row::New)
}

impl Shell {
    /// Opens the switcher with an empty search. The search field takes focus on the next frame.
    pub(super) fn switcher_open(&mut self, cx: &mut Context<Self>) {
        if self.switcher.keys.is_none() {
            // The search field binds up, down and enter itself, and bound keys never reach key listeners. Keystroke
            // interceptors run before bindings, so the palette takes its keys there.
            let shell = cx.weak_entity();
            self.switcher.keys = Some(cx.intercept_keystrokes(move |ev, window, cx| {
                if !is_nav(&ev.keystroke) {
                    return;
                }
                let Some(shell) = shell.upgrade() else { return };
                if shell.update(cx, |this, cx| this.switcher_key(&ev.keystroke, window, cx)) {
                    cx.stop_propagation();
                }
            }));
        }
        self.switcher.open = true;
        self.switcher.focus_input = true;
        cx.notify();
    }

    /// The rows for `query`: New session first when the query is empty, then the matching sessions.
    fn switcher_rows(&self, query: &str) -> Vec<Row> {
        let mut rows = Vec::new();
        if query.trim().is_empty() {
            rows.push(Row::New);
        }
        let visible = rank(self.state.sessions(), query).into_iter().filter(|s| s.visible());
        rows.extend(visible.map(|s| Row::Session(s.id.clone())));
        rows
    }

    fn switcher_query(&self, cx: &App) -> String {
        self.switcher.input.as_ref().map(|(input, _)| input.read(cx).text()).unwrap_or_default()
    }

    fn switcher_close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.switcher.open = false;
        window.focus(&self.focus);
        cx.notify();
    }

    fn switcher_move(&mut self, row: usize, cx: &mut Context<Self>) {
        self.switcher.selected = row;
        self.switcher.scroll.scroll_to_item(row);
        cx.notify();
    }

    /// Runs a row: closes the switcher, then starts a session or opens one as a sidebar click would.
    fn switcher_activate(&mut self, row: Row, alt: bool, window: &mut Window, cx: &mut Context<Self>) {
        self.switcher_close(window, cx);
        match row {
            Row::New => self.new_session(window, cx),
            Row::Session(id) => self.open_session_click(&id, alt, window, cx),
        }
    }

    /// Keys while the switcher is open: arrows move, Enter opens, Escape or Ctrl+K closes. Other keys stay in the
    /// search field. Returns true when the palette is open, so the shell's own shortcuts don't run.
    pub(super) fn switcher_key(&mut self, keystroke: &Keystroke, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if !self.switcher.open {
            return false;
        }
        let rows = self.switcher_rows(&self.switcher_query(cx));
        let last = rows.len().saturating_sub(1);
        let current = self.switcher.selected.min(last);
        match keystroke.key.as_str() {
            "escape" => self.switcher_close(window, cx),
            "k" if keystroke.modifiers.control => self.switcher_close(window, cx),
            "down" => self.switcher_move((current + 1).min(last), cx),
            "up" => self.switcher_move(current.saturating_sub(1), cx),
            "enter" => {
                if let Some(row) = rows.get(current).cloned() {
                    self.switcher_activate(row, keystroke.modifiers.control && keystroke.modifiers.shift, window, cx);
                }
            }
            _ => {}
        }
        true
    }

    /// The name a session row shows after its title: its project, else its folder.
    fn switcher_project(&self, s: &Session) -> String {
        s.project
            .as_deref()
            .and_then(|p| self.state.project(p))
            .map(|p| p.name.clone())
            .or_else(|| s.cwd.file_name().map(|name| name.to_string_lossy().into_owned()))
            .unwrap_or_default()
    }

    /// One palette row: sigil, title, harness badge, project and relative time. The selected row has the `line2` fill.
    fn switcher_row(&self, ix: usize, row: &Row, selected: bool, now: u64, cx: &mut Context<Self>) -> AnyElement {
        let theme = &self.theme;
        let (fg, muted) = (theme.fg.to_hsla(), theme.muted.to_hsla());
        let key = match row {
            Row::New => "switch-new".to_string(),
            Row::Session(id) => format!("switch-{id}"),
        };
        let row_click = row.clone();
        let base = div()
            .id(ElementId::Name(key.into()))
            .flex()
            .items_center()
            .gap(px(11.))
            .px(px(10.))
            .py(px(8.))
            .rounded(px(8.))
            .cursor_pointer()
            .when(selected, |d| d.bg(theme.line2.to_hsla()))
            .on_mouse_move(cx.listener(move |this, _, _, cx| {
                if this.switcher.selected != ix {
                    this.switcher.selected = ix;
                    cx.notify();
                }
            }))
            .on_click(cx.listener(move |this, ev: &ClickEvent, window, cx| {
                this.switcher_activate(row_click.clone(), ev.modifiers().alt, window, cx)
            }));
        let Row::Session(id) = row else {
            return base
                .child(
                    div()
                        .w(px(20.))
                        .flex_none()
                        .flex()
                        .justify_center()
                        .text_size(px(16.))
                        .text_color(theme.acc2.to_hsla())
                        .child("+"),
                )
                .child(div().flex_1().text_size(px(13.5)).text_color(fg).child("New session"))
                .child(div().text_size(px(11.)).text_color(muted).child("⏎"))
                .into_any_element();
        };
        let Some(s) = self.state.session(id) else { return div().into_any_element() };
        let focused = self.state.workspace.layout.focus.as_deref() == Some(s.id.as_str());
        let open = self.state.workspace.layout.contains(&s.id);
        let colour = if focused {
            theme.acc2.to_hsla()
        } else if open {
            fg
        } else {
            muted
        };
        let title = if s.title.trim().is_empty() { "New session" } else { s.title.trim() };
        base.child(
            div()
                .relative()
                .w(px(20.))
                .h(px(20.))
                .flex_none()
                .flex()
                .items_center()
                .justify_center()
                .child(sigil_element(s.icon, px(16.), colour))
                .child(
                    div()
                        .absolute()
                        .right(px(-2.))
                        .bottom(px(-2.))
                        .child(dot(theme, self.session_state(s), theme.tile.to_hsla())),
                ),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .overflow_hidden()
                .whitespace_nowrap()
                .text_ellipsis()
                .text_size(px(13.5))
                .text_color(fg)
                .child(title.to_string()),
        )
        .child(
            div()
                .size(px(20.))
                .flex_none()
                .rounded(px(5.))
                .flex()
                .items_center()
                .justify_center()
                // On the selected row the `line2` fill is the row's own, so the badge uses the popover surface
                // to read as a cutout against it (the sidebar badge sits on a `soft` row instead).
                .bg(if selected { theme.pop } else { theme.line }.to_hsla())
                .child(crate::ui::composer::icons::harness_icon(
                    &s.agent,
                    12.,
                    1.6,
                    if selected { theme.acc2 } else { theme.alt }.to_hsla(),
                )),
        )
        .child(
            div()
                .flex_none()
                .max_w(px(140.))
                .truncate()
                .text_size(px(11.5))
                .text_color(muted)
                .child(self.switcher_project(s)),
        )
        .child(div().flex_none().w(px(30.)).text_right().text_size(px(11.)).text_color(muted).child(ago(s.updated, now)))
        .into_any_element()
    }

    /// The palette, when open (drawn by `render_overlays`). Creates the search field on first use.
    pub(super) fn render_switcher(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        if !self.switcher.open {
            return None;
        }
        let input = match &self.switcher.input {
            Some((input, _)) => input.clone(),
            None => {
                let input = cx.new(|cx| TextInput::new(search_config(), window, cx));
                let changed = cx.subscribe(&input, |this, input, event: &TextInputEvent, cx| {
                    if let TextInputEvent::Changed = event {
                        let rows = this.switcher_rows(&input.read(cx).text());
                        this.switcher.selected = default_row(&rows);
                        cx.notify();
                    }
                });
                self.switcher.input = Some((input.clone(), changed));
                input
            }
        };
        if std::mem::take(&mut self.switcher.focus_input) {
            input.update(cx, |input, cx| input.set_text("", cx));
            input.read(cx).focus(window);
            self.switcher.selected = default_row(&self.switcher_rows(""));
        }

        let query = input.read(cx).text();
        let rows = self.switcher_rows(&query);
        self.switcher.selected = self.switcher.selected.min(rows.len().saturating_sub(1));
        let now = store::now();
        let theme = &self.theme;
        let line = theme.line.to_hsla();
        let items: Vec<AnyElement> = rows
            .iter()
            .enumerate()
            .map(|(ix, row)| self.switcher_row(ix, row, ix == self.switcher.selected, now, cx))
            .collect();
        let list = if rows.is_empty() {
            div()
                .px(px(12.))
                .py(px(14.))
                .text_size(px(12.5))
                .text_color(theme.muted.to_hsla())
                .child("No matches")
                .into_any_element()
        } else {
            div()
                .id("switcher-list")
                .flex()
                .flex_col()
                .gap(px(2.))
                .p(px(6.))
                .max_h(px(380.))
                .overflow_y_scroll()
                .track_scroll(&self.switcher.scroll)
                .children(items)
                .into_any_element()
        };
        let search = div()
            .flex()
            .items_center()
            .gap(px(10.))
            .px(px(16.))
            .py(px(12.))
            .border_b_1()
            .border_color(line)
            .child(div().flex_none().text_size(px(16.)).text_color(theme.muted.to_hsla()).child("⌕"))
            .child(div().flex_1().min_w_0().child(text_input(&input, theme)));
        let footer = div()
            .flex()
            .gap(px(14.))
            .px(px(16.))
            .py(px(9.))
            .border_t_1()
            .border_color(line)
            .text_size(px(11.))
            .text_color(theme.muted.to_hsla())
            .child("↑↓ move")
            .child("⏎ open")
            .child("ctrl ⇧ ⏎ new split")
            .child("esc close");
        let dialog = div()
            .flex()
            .flex_col()
            .w(px(560.))
            .rounded(px(12.))
            .bg(theme.pop.to_hsla())
            .border_1()
            .border_color(theme.line2.to_hsla())
            .shadow(vec![BoxShadow {
                color: hsla(0., 0., 0., 0.6),
                offset: point(px(0.), px(30.)),
                blur_radius: px(80.),
                spread_radius: px(0.),
            }])
            .overflow_hidden()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(search)
            .child(list)
            .child(footer);
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
                .bg(hsla(0., 0., 0., 0.45))
                .on_mouse_down(MouseButton::Left, cx.listener(|this, _, window, cx| this.switcher_close(window, cx)))
                .child(dialog)
                .into_any_element(),
        )
    }
}

/// The keys the palette takes while open. Ctrl+K toggles it, as it opens it.
fn is_nav(keystroke: &Keystroke) -> bool {
    matches!(keystroke.key.as_str(), "up" | "down" | "enter" | "escape")
        || (keystroke.key == "k" && keystroke.modifiers.control)
}

fn search_config() -> TextInputConfig {
    TextInputConfig {
        placeholder: "Search sessions and messages".into(),
        font_size: px(14.),
        line_height: px(20.),
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::{Message, Role};

    fn session(id: &str, title: &str, updated: u64, texts: &[&str]) -> Session {
        let mut s = Session::new("claude", "/".into());
        s.id = id.into();
        s.title = title.into();
        s.updated = updated;
        s.messages = texts
            .iter()
            .map(|text| Message {
                role: Role::Agent,
                text: (*text).into(),
                attachments: Vec::new(),
                tools: Vec::new(),
                thought_secs: None,
                took_secs: None,
            })
            .collect();
        s
    }

    fn ids(sessions: &[&Session]) -> Vec<String> {
        sessions.iter().map(|s| s.id.clone()).collect()
    }

    #[test]
    fn empty_query_lists_every_session_by_recency() {
        let all = [session("a", "Old", 1, &[]), session("b", "New", 3, &[]), session("c", "Mid", 2, &[])];
        assert_eq!(ids(&rank(&all, "  ")), ["b", "c", "a"]);
    }

    #[test]
    fn title_prefix_ranks_above_title_substring_above_message_match() {
        let all = [
            session("msg", "Other", 9, &["fix the upload"]),
            session("sub", "Big upload fix", 8, &[]),
            session("pre", "Upload fix", 1, &[]),
        ];
        assert_eq!(ids(&rank(&all, "upload")), ["pre", "sub", "msg"]);
    }

    #[test]
    fn words_may_match_the_title_and_different_messages() {
        let all = [
            session("a", "Retry queue", 5, &["Backoff capped at five"]),
            session("b", "Retry queue", 4, &["nothing relevant"]),
        ];
        assert_eq!(ids(&rank(&all, "backoff retry")), ["a"]);
    }

    #[test]
    fn search_ignores_case_and_surrounding_space() {
        let all = [session("a", "Fix Flaky Upload", 1, &[])];
        assert_eq!(ids(&rank(&all, "  FLAKY  ")), ["a"]);
        assert_eq!(ids(&rank(&all, "upload")), ["a"]);
    }

    #[test]
    fn no_match_is_empty() {
        let all = [session("a", "Fix flaky upload", 1, &["Retries added."])];
        assert!(rank(&all, "settings").is_empty());
    }

    #[test]
    fn selection_starts_below_new_session_when_sessions_exist() {
        assert_eq!(default_row(&[Row::New, Row::Session("a".into())]), 1);
        assert_eq!(default_row(&[Row::New]), 0);
        assert_eq!(default_row(&[Row::Session("a".into())]), 0);
    }
}

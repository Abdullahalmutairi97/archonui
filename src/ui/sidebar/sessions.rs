//! The Sessions view (layout.md §2.1 rows 3–3.4): the New session button, the search box and the
//! TODAY / EARLIER rows. Also the open and new-session actions the rail shares.

use super::{Shell, Target};
use crate::{
    app::{
        sections::{ago, Section},
        settings::Action,
        state::Placement,
    },
    engine,
    store::{self, Role, Session},
    ui::{
        sigil,
        theme::Theme,
        tiling,
        widgets::{
            popover::{tooltip, TooltipColors},
            text_input::{text_input, TextInput},
        },
    },
};
use gpui::{
    div, prelude::*, transparent_black, AnyElement, ClickEvent, Context, ElementId, Entity, FontWeight,
    Hsla, MouseButton, MouseDownEvent, Window,
};
use crate::ui::scale::px;
use std::collections::HashMap;

/// A session row or rail button, as the sidebar draws it.
pub(super) struct Entry {
    pub id: String,
    pub title: String,
    pub icon: u16,
    pub harness: String,
    pub ago: String,
    pub state: sigil::SessionState,
    /// Shown in a tile.
    pub open: bool,
    /// The focused tile shows this session.
    pub focused: bool,
}

/// The harness badge's two letters (layout.md §2.1). Unknown ids show `?`.
pub(in crate::ui) fn badge_label(harness: &str) -> &'static str {
    match harness {
        "claude" => "Cc",
        "codex" => "Cx",
        "gemini" => "Gm",
        "opencode" => "Oc",
        "pi" => "Pi",
        _ => "?",
    }
}

/// A status dot for a session's state. `surface` is the ring that separates it from the sigil's
/// background: the tile colour in a row, the panel colour in the rail.
pub(in crate::ui) fn dot(theme: &Theme, state: sigil::SessionState, surface: Hsla) -> impl IntoElement {
    let colours = sigil::StatusDotColours {
        running: theme.acc2.to_hsla(),
        done: theme.alt.to_hsla(),
        idle: theme.faint.to_hsla(),
        surface,
    };
    sigil::status_dot_sized(state, &colours, 5., 1.5)
}

impl Shell {
    /// The whole Sessions view: the button, the search box and the groups.
    pub(super) fn session_list(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let search = self.sidebar.search.as_ref().map(|(input, _)| input.clone());
        let query = search.as_ref().map(|input| input.read(cx).text()).unwrap_or_default();
        let groups = self.session_entries(&query, store::now());
        let rename = self.sidebar.rename.as_ref().map(|r| (r.target.clone(), r.input.clone()));
        let theme = self.theme.clone();

        let mut list = div().flex().flex_col().gap(px(3.));
        if groups.is_empty() {
            let empty = if query.is_empty() { "No sessions yet" } else { "No matches" };
            list = list.child(div().px(px(10.)).py(px(8.)).text_size(px(12.)).text_color(theme.muted.to_hsla()).child(empty));
        }
        for (section, entries) in &groups {
            list = list.child(group_label(*section, &theme));
            for entry in entries {
                let renaming = rename.as_ref().and_then(|(target, input)| match target {
                    Target::Session(id) if *id == entry.id => Some(input),
                    _ => None,
                });
                list = list.child(self.session_row(entry, renaming, cx));
            }
        }
        div()
            .flex()
            .flex_col()
            .gap(px(10.))
            .child(self.new_session_button(cx))
            .when_some(search, |col, input| col.child(search_box(&input, &theme)))
            .child(list)
            .into_any_element()
    }

    /// Sessions grouped as TODAY then EARLIER, filtered by the search query as the prototype does.
    pub(super) fn session_entries(&self, query: &str, now: u64) -> Vec<(Section, Vec<Entry>)> {
        let needle = query.trim().to_lowercase();
        let focus = self.state.workspace.layout.focus.as_deref();
        self.state
            .sections(now)
            .into_iter()
            .filter_map(|(section, sessions)| {
                let entries: Vec<Entry> = sessions
                    .into_iter()
                    .filter(|s| {
                        let project = s.project.as_deref().and_then(|id| self.state.project(id)).map(|p| p.name.as_str());
                        needle.is_empty() || matches_query(s, &needle, project)
                    })
                    .map(|s| Entry {
                        id: s.id.clone(),
                        title: match (s.title.trim().is_empty(), s.draft.trim().is_empty()) {
                            (false, _) => s.title.clone(),
                            (true, false) => crate::store::title_from(s.draft.trim()),
                            (true, true) => "New session".into(),
                        },
                        icon: s.icon,
                        harness: s.agent.clone(),
                        ago: ago(s.updated, now),
                        state: self.session_state(s),
                        open: self.state.workspace.layout.contains(&s.id),
                        focused: focus == Some(s.id.as_str()),
                    })
                    .collect();
                (!entries.is_empty()).then_some((section, entries))
            })
            .collect()
    }

    /// Running while a turn runs; done when the agent has the last word; idle otherwise.
    pub(in crate::ui) fn session_state(&self, s: &Session) -> sigil::SessionState {
        if self.state.busy(&s.id) {
            sigil::SessionState::Running
        } else if s.messages.last().is_some_and(|m| m.role == Role::Agent) {
            sigil::SessionState::Done
        } else {
            sigil::SessionState::Idle
        }
    }

    /// The accent "New session" button with its shortcut hint (the New session binding, none when
    /// unbound). Tighter than layout.md §2.1 row
    /// 3.1's spec padding/text size, by request.
    fn new_session_button(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = &self.theme;
        let hint = new_session_hint(&self.state.settings.keys);
        div()
            .id("new-session")
            .flex()
            .justify_between()
            .items_center()
            .px(px(12.))
            .py(px(8.))
            .rounded(px(8.))
            .text_size(px(13.))
            .bg(theme.acc.to_hsla())
            .text_color(theme.on.to_hsla())
            .font_weight(FontWeight::BOLD)
            .cursor_pointer()
            .on_click(cx.listener(|this, _: &ClickEvent, window, cx| this.new_session(window, cx)))
            .child("New session")
            .when_some(hint, |d, hint| d.child(div().text_size(px(10.)).opacity(0.75).child(hint)))
    }

    fn session_row(&self, entry: &Entry, rename: Option<&Entity<TextInput>>, cx: &mut Context<Self>) -> AnyElement {
        let theme = &self.theme;
        let (fg, muted, soft, line2) = (theme.fg.to_hsla(), theme.muted.to_hsla(), theme.soft.to_hsla(), theme.line2.to_hsla());
        let colour = if entry.focused {
            theme.acc2.to_hsla()
        } else if entry.open {
            fg
        } else {
            muted
        };
        let id = entry.id.clone();
        let title: AnyElement = match rename {
            Some(input) => text_input(input, theme).into_any_element(),
            // No `whitespace_nowrap`: GPUI only truncates text it wraps, so the nowrap variant
            // keeps the untruncated cached layout and clips mid-glyph instead of ending in "…".
            None => div()
                .flex_1()
                .min_w_0()
                .overflow_hidden()
                .text_ellipsis()
                .line_clamp(1)
                .text_size(px(13.5))
                .text_color(fg)
                .child(entry.title.clone())
                .into_any_element(),
        };
        let row = div()
            .id(ElementId::Name(format!("session-{}", entry.id).into()))
            .flex()
            .items_center()
            .gap(px(11.))
            .px(px(10.))
            .py(px(8.))
            .rounded(px(8.))
            .border_1()
            .border_color(if entry.focused { line2 } else { transparent_black() })
            .when(entry.focused, |d| d.bg(soft))
            .hover(move |s| s.bg(line2))
            .on_mouse_down(
                MouseButton::Right,
                cx.listener({
                    let id = id.clone();
                    move |this, ev: &MouseDownEvent, _, cx| this.open_menu(Target::Session(id.clone()), ev.position, cx)
                }),
            )
            .child(
                div()
                    .relative()
                    .w(px(20.))
                    .h(px(20.))
                    .flex_none()
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(sigil::sigil_element(entry.icon, px(16.), colour))
                    .child(div().absolute().right(px(-2.)).bottom(px(-2.)).child(dot(theme, entry.state, theme.tile.to_hsla()))),
            )
            .child(title)
            // Badge and time in fixed 18 + 30 px columns, so they line up on every row.
            .child(
                div()
                    .flex_none()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .child(self.badge(entry))
                    .child(
                        div()
                            .w(px(30.))
                            .flex_none()
                            .text_right()
                            .whitespace_nowrap()
                            .text_size(px(11.))
                            .text_color(muted)
                            .child(entry.ago.clone()),
                    ),
            );
        if rename.is_some() {
            return row.into_any_element();
        }
        row.cursor_pointer()
            .on_click(cx.listener(move |this, ev: &ClickEvent, window, cx| {
                this.open_session_click(&id, ev.modifiers().alt, window, cx)
            }))
            .into_any_element()
    }

    /// The harness badge; its tooltip is "<harness name> · <project>".
    fn badge(&self, entry: &Entry) -> impl IntoElement {
        let theme = &self.theme;
        let project = self
            .state
            .session(&entry.id)
            .and_then(|s| s.project.as_deref())
            .and_then(|p| self.state.project(p))
            .map_or("No project", |p| p.name.as_str());
        let label = format!("{} · {}", engine::agent_name(&entry.harness), project);
        let (bg, colour) = if entry.focused { (theme.line2, theme.acc2) } else { (theme.line, theme.alt) };
        div()
            .id(ElementId::Name(format!("badge-{}", entry.id).into()))
            .h(px(18.))
            .min_w(px(18.))
            .px(px(4.))
            .flex_none()
            .rounded(px(5.))
            .flex()
            .items_center()
            .justify_center()
            .text_size(px(9.5))
            .font_weight(FontWeight::BOLD)
            .text_color(colour.to_hsla())
            .bg(bg.to_hsla())
            .tooltip(tooltip(label, TooltipColors::of(theme)))
            .child(crate::ui::composer::icons::harness_icon(&entry.harness, 11., 1.6, colour.to_hsla()))
    }

    /// Opens a session in the focused tile, or in a new split when `new_split` is set (Alt+click).
    pub(in crate::ui) fn open_session_click(&mut self, id: &str, new_split: bool, window: &Window, cx: &mut Context<Self>) {
        self.finish_rename(true, cx);
        self.sidebar.menu = None;
        if new_split {
            self.open_in_split(id, window);
        } else {
            self.state.workspace.layout.open_in_focused(id);
        }
        self.save(cx);
    }

    /// Opens `id` in a new split of the centre. Falls back to the focused tile when there's no room.
    pub(super) fn open_in_split(&mut self, id: &str, window: &Window) {
        let (area, gap) = self.centre(window);
        if !self.state.workspace.layout.open_in_new_split(id, area, gap) {
            self.state.workspace.layout.open_in_focused(id);
        }
    }

    /// Starts a chat with the default agent in the current project's folder, split beside the
    /// focused tile. Alt+Enter does the same.
    pub(in crate::ui) fn new_session(&mut self, window: &Window, cx: &mut Context<Self>) {
        let agent = crate::ui::shell::pick_harness(self.state.settings.default_harness.as_deref(), &self.agents);
        let Some(agent) = agent else {
            self.no_agent(cx);
            return;
        };
        let cwd = self
            .state
            .workspace
            .current_project
            .as_deref()
            .and_then(|id| self.state.project(id))
            .map(|p| p.path.clone())
            .unwrap_or_else(|| self.default_folder());
        let (area, gap) = self.centre(window);
        let id = self.state.new_session(agent, cwd, Placement::Split { area, gap });
        let (model, effort) = self.agent_defaults(agent);
        self.state.set_options(&id, model, effort, self.state.settings.default_mode);
        self.save(cx);
    }

    /// The centre region in tiling units, and the tile gap.
    fn centre(&self, window: &Window) -> (tiling::Rect, f32) {
        let regions = self.regions(window);
        let c = regions.centre;
        (tiling::Rect { x: c.x, y: c.y, w: c.w, h: c.h }, regions.gap)
    }
}

/// Whether `s` matches the lowercased `needle` in its title or project name, which is the
/// prototype's filter (`x.name.toLowerCase().includes(q) || x.project.includes(q)`): the title is
/// compared case-insensitively, the project as written. Message text is not searched.
fn matches_query(s: &Session, needle: &str, project: Option<&str>) -> bool {
    let title = if s.title.trim().is_empty() { "New session" } else { s.title.as_str() };
    title.to_lowercase().contains(needle) || project.is_some_and(|p| p.contains(needle))
}

/// "TODAY" / "EARLIER" (layout.md §2.1 row 3.3).
fn group_label(section: Section, theme: &Theme) -> impl IntoElement {
    div()
        .px(px(4.))
        .pt(px(8.))
        .pb(px(4.))
        .text_size(px(10.5))
        .text_color(theme.muted.to_hsla())
        .child(section.label())
}

/// The search field: a ⌕ glyph and a borderless input (layout.md §2.1 row 3.2).
fn search_box(input: &Entity<TextInput>, theme: &Theme) -> impl IntoElement {
    div()
        .flex()
        .items_center()
        .gap(px(8.))
        .px(px(12.))
        .py(px(9.))
        .rounded(px(8.))
        .border_1()
        .border_color(theme.line2.to_hsla())
        .child(div().flex_none().text_color(theme.muted.to_hsla()).child("⌕"))
        .child(div().flex_1().min_w_0().child(text_input(input, theme)))
}

/// The shortcut the New session button shows: whatever New session is bound to, or nothing when unbound.
fn new_session_hint(keys: &HashMap<Action, String>) -> Option<String> {
    keys.get(&Action::NewSession).filter(|k| !k.is_empty()).map(|k| crate::ui::settings::key_label(k))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_session_hint_follows_the_binding() {
        let mut keys = crate::app::settings::default_keys();
        assert_eq!(new_session_hint(&keys).as_deref(), Some("Ctrl N"));
        keys.insert(Action::NewSession, "alt-shift-n".into());
        assert_eq!(new_session_hint(&keys).as_deref(), Some("Alt ⇧ N"));
        keys.insert(Action::NewSession, String::new());
        assert_eq!(new_session_hint(&keys), None);
    }
    use crate::store::Message;

    fn session(title: &str, texts: &[&str]) -> Session {
        let mut s = Session::new("claude", "/".into());
        s.title = title.into();
        for text in texts {
            s.messages.push(Message { role: Role::Agent, text: (*text).into(), attachments: Vec::new(), tools: Vec::new(), thought_secs: None, took_secs: None, agent: None, final_at: None, usage: None, todos: Vec::new() });
        }
        s
    }

    #[test]
    fn badges_match_the_prototype() {
        assert_eq!(badge_label("claude"), "Cc");
        assert_eq!(badge_label("codex"), "Cx");
        assert_eq!(badge_label("pi"), "Pi");
        assert_eq!(badge_label("unknown"), "?");
    }

    #[test]
    fn search_matches_the_title_in_any_case_and_the_project_as_written() {
        let s = session("Fix flaky upload", &["The Progress bar races."]);
        assert!(matches_query(&s, "upload", None));
        assert!(matches_query(&s, "fix flaky", Some("archonui-core")));
        assert!(matches_query(&s, "core", Some("archonui-core")));
        assert!(!matches_query(&s, "core", Some("ArchonUI-Core")), "the project is compared as written");
        assert!(!matches_query(&s, "progress", Some("archonui-core")), "message text is not searched");
        assert!(!matches_query(&s, "settings", None));
        let untitled = session("", &[]);
        assert!(matches_query(&untitled, "new", None), "untitled sessions match their display title");
        assert!(!matches_query(&untitled, "upload", None));
    }
}

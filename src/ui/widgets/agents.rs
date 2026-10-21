//! Agent chips: which installed agents a skill, MCP server, tool or extension reaches. Every installed
//! agent gets a chip, lit when the item reaches it and faded when it does not, so a gap shows at a glance.

use gpui::{div, prelude::*, AnyElement, SharedString};

use crate::ui::{
    composer::icons::harness_icon,
    scale::px,
    theme::Theme,
    widgets::popover::{tooltip, TooltipColors},
};

/// The short name a chip shows for agent `id`.
pub fn short_name(id: &str) -> &str {
    match id {
        "claude" => "Claude",
        "codex" => "Codex",
        "gemini" => "Gemini",
        "opencode" => "OpenCode",
        "pi" => "Pi",
        other => other,
    }
}

/// One chip per agent in `installed`, in that order. A chip is lit when `reaches` holds for its id.
pub fn agent_chips(id: impl Into<SharedString>, installed: &[&str], reaches: impl Fn(&str) -> bool, theme: &Theme) -> AnyElement {
    let marks: Vec<AgentMark> = installed.iter().map(|agent| AgentMark { agent, lit: reaches(agent), struck: false, note: None }).collect();
    agent_marks(id, &marks, theme)
}

/// How one agent's chip is drawn.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AgentMark<'a> {
    pub agent: &'a str,
    /// The item reaches this agent: the chip is lit.
    pub lit: bool,
    /// The agent ignores a setting the user chose: the faded chip is also struck through.
    pub struck: bool,
    /// Shown as the chip's tooltip.
    pub note: Option<String>,
}

/// One chip per mark, in order: lit, faded, or faded and struck through, each with its note as a tooltip.
pub fn agent_marks(id: impl Into<SharedString>, marks: &[AgentMark], theme: &Theme) -> AnyElement {
    let id: SharedString = id.into();
    let mut row = div().id(id.clone()).flex().flex_wrap().items_center().gap(px(4.));
    for mark in marks {
        let on = mark.lit;
        let colour = if on { theme.fg } else { theme.faint };
        let chip = div()
            .id(SharedString::from(format!("{id}-{}", mark.agent)))
            .flex()
            .items_center()
            .gap(px(4.))
            .px(px(6.))
            .py(px(2.))
            .rounded(px(5.))
            .border_1()
            .border_color(if on { theme.line2.to_hsla() } else { theme.line.to_hsla() })
            .when(on, |d| d.bg(theme.line.to_hsla()))
            .opacity(if on { 1.0 } else { 0.5 })
            .child(harness_icon(mark.agent, 11., 1.5, colour.to_hsla()))
            .child(
                div()
                    .text_size(px(11.))
                    .text_color(colour.to_hsla())
                    .when(mark.struck, |d| d.line_through())
                    .child(short_name(mark.agent).to_string()),
            )
            .when_some(mark.note.clone(), |d, note| d.tooltip(tooltip(note, TooltipColors::of(theme))));
        row = row.child(chip);
    }
    row.into_any_element()
}

/// One 22 px icon per mark, as the Built-in tools rows show: full strength when lit, faded when not,
/// each with the agent's name and note as its tooltip.
pub fn agent_icons(id: impl Into<SharedString>, marks: &[AgentMark], theme: &Theme) -> AnyElement {
    let id: SharedString = id.into();
    let mut row = div().id(id.clone()).flex().items_center().gap(px(4.));
    for mark in marks {
        let tip = match &mark.note {
            Some(note) => format!("{}: {note}", short_name(mark.agent)),
            None => short_name(mark.agent).to_string(),
        };
        row = row.child(
            div()
                .id(SharedString::from(format!("{id}-{}", mark.agent)))
                .size(px(22.))
                .rounded(px(6.))
                .border_1()
                .border_color(theme.line.to_hsla())
                .flex()
                .items_center()
                .justify_center()
                .opacity(if mark.lit { 1.0 } else { 0.25 })
                .child(harness_icon(mark.agent, 12., 1.4, theme.fg.to_hsla()))
                .tooltip(tooltip(tip, TooltipColors::of(theme))),
        );
    }
    row.into_any_element()
}

#[cfg(test)]
mod tests {
    use super::short_name;

    #[test]
    fn every_harness_has_a_short_name() {
        for (id, name) in [("claude", "Claude"), ("codex", "Codex"), ("gemini", "Gemini"), ("opencode", "OpenCode"), ("pi", "Pi")] {
            assert_eq!(short_name(id), name);
        }
        assert_eq!(short_name("other"), "other");
    }
}

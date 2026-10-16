//! Agent chips: which installed agents a skill, MCP server, tool or extension reaches. Every installed
//! agent gets a chip, lit when the item reaches it and faded when it does not, so a gap shows at a glance.

use gpui::{div, prelude::*, AnyElement, SharedString};

use crate::ui::{composer::icons::harness_icon, scale::px, theme::Theme};

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
    let mut row = div().id(id.into()).flex().flex_wrap().items_center().gap(px(4.));
    for agent in installed {
        let on = reaches(agent);
        let colour = if on { theme.fg } else { theme.faint };
        row = row.child(
            div()
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
                .child(harness_icon(agent, 11., 1.5, colour.to_hsla()))
                .child(div().text_size(px(11.)).text_color(colour.to_hsla()).child(short_name(agent).to_string())),
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

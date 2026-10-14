//! Pure composer rules: placeholders, mode cycling and the effort scale. Spec: chat.md §4.2, §4.4, §5.4.

use crate::engine::Mode;

/// The composer placeholder for `mode`. `app_name` replaces `archonui`; a blank name falls back to it.
pub fn placeholder(mode: Mode, app_name: &str) -> String {
    let name = match app_name.trim() {
        "" => "archonui",
        name => name,
    };
    match mode {
        Mode::Auto => format!("Message {name}…"),
        Mode::Plan => format!("Plan with {name}…"),
        Mode::Ask => "Ask about the code…".into(),
    }
}

/// The mode after `current` among the `supported` modes, wrapping around. A `current` the agent
/// does not support moves to the first supported mode. `None` when there are no supported modes.
pub fn next_mode(current: Mode, supported: &[Mode]) -> Option<Mode> {
    let first = *supported.first()?;
    Some(match supported.iter().position(|&m| m == current) {
        Some(i) => supported[(i + 1) % supported.len()],
        None => first,
    })
}

/// Where level `index` sits on the 0–1 scale (chat.md §5.4 `effFrac`). Lists that start with `Off`
/// run from 0 to 1; other lists have no zero, so level `k` of `n` sits at `(k + 1) / n`.
pub fn effort_fraction(levels: &[String], index: usize) -> f32 {
    match levels.len() {
        0 | 1 => 0.0,
        n if levels[0] == "Off" => index as f32 / (n - 1) as f32,
        n => (index + 1) as f32 / n as f32,
    }
}

/// The index of the level whose fraction is closest to `fraction`. Ties go to the lower level.
pub fn nearest_effort(levels: &[String], fraction: f32) -> Option<usize> {
    (0..levels.len()).min_by(|&a, &b| {
        let da = (effort_fraction(levels, a) - fraction).abs();
        let db = (effort_fraction(levels, b) - fraction).abs();
        da.total_cmp(&db)
    })
}

/// The line under the effort moons (chat.md §5.4 `EDESC`), for the level called `name` at `fraction`.
pub fn effort_description(name: &str, fraction: f32) -> String {
    let budget = name.strip_suffix('k').is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()));
    if budget {
        return format!("Thinking budget of {name} tokens.");
    }
    let text = if name == "Auto" {
        "The model picks a thinking budget each turn."
    } else if fraction == 0.0 {
        "No extended thinking. Fastest replies."
    } else if fraction <= 0.34 {
        "Light thinking for small, contained changes."
    } else if fraction <= 0.6 {
        "Reads around the change before editing."
    } else if fraction < 1.0 {
        "Weighs alternatives and runs tests."
    } else {
        "Thinks as long as the task needs."
    };
    text.into()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn levels(names: &[&str]) -> Vec<String> {
        names.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn placeholders_follow_mode_and_app_name() {
        assert_eq!(placeholder(Mode::Auto, "archonui"), "Message archonui…");
        assert_eq!(placeholder(Mode::Plan, "Forge"), "Plan with Forge…");
        assert_eq!(placeholder(Mode::Ask, "Forge"), "Ask about the code…");
        assert_eq!(placeholder(Mode::Auto, "  "), "Message archonui…");
    }

    #[test]
    fn modes_cycle_through_supported_only() {
        assert_eq!(next_mode(Mode::Auto, &Mode::ALL), Some(Mode::Plan));
        assert_eq!(next_mode(Mode::Ask, &Mode::ALL), Some(Mode::Auto));
        assert_eq!(next_mode(Mode::Auto, &[Mode::Auto, Mode::Ask]), Some(Mode::Ask));
        assert_eq!(next_mode(Mode::Plan, &[Mode::Auto, Mode::Ask]), Some(Mode::Auto));
        assert_eq!(next_mode(Mode::Auto, &[]), None);
    }

    #[test]
    fn effort_fractions_match_the_prototype_scale() {
        let four = levels(&["Low", "Medium", "High", "Max"]);
        let fractions: Vec<f32> = (0..4).map(|k| effort_fraction(&four, k)).collect();
        assert_eq!(fractions, vec![0.25, 0.5, 0.75, 1.0]);
        let off_first = levels(&["Off", "Low", "Medium", "High", "Max"]);
        assert_eq!(effort_fraction(&off_first, 0), 0.0);
        assert_eq!(effort_fraction(&off_first, 4), 1.0);
        assert_eq!(effort_fraction(&levels(&["Only"]), 0), 0.0);
    }

    #[test]
    fn nearest_effort_keeps_the_relative_position() {
        let four = levels(&["Low", "Medium", "High", "Max"]);
        // Half way on a 2-level list lands on the 4-level list's Medium.
        assert_eq!(nearest_effort(&four, 0.5), Some(1));
        // A new model with fewer levels maps the top position to its top level.
        assert_eq!(nearest_effort(&levels(&["Off", "On"]), 1.0), Some(1));
        // Exact ties keep the lower level.
        assert_eq!(nearest_effort(&four, 0.375), Some(0));
        assert_eq!(nearest_effort(&[], 0.5), None);
    }

    #[test]
    fn effort_descriptions_follow_the_level_name_then_fraction() {
        assert_eq!(effort_description("32k", 1.0), "Thinking budget of 32k tokens.");
        assert_eq!(effort_description("Auto", 0.0), "The model picks a thinking budget each turn.");
        assert_eq!(effort_description("Off", 0.0), "No extended thinking. Fastest replies.");
        assert_eq!(effort_description("Low", 0.25), "Light thinking for small, contained changes.");
        assert_eq!(effort_description("Medium", 0.5), "Reads around the change before editing.");
        assert_eq!(effort_description("High", 0.75), "Weighs alternatives and runs tests.");
        assert_eq!(effort_description("Max", 1.0), "Thinks as long as the task needs.");
    }
}

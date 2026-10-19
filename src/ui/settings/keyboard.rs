//! The Keyboard page: rebindable shortcuts, read-only keys, and conflict handling. Spec: settings.md §4.

use super::controls::{binding_label, kbd_row, section};
use crate::{
    app::settings::Action,
    ui::{shell::Shell, widgets::{kbd::Kbd, section::SettingRow}},
};
use gpui::{div, prelude::*, AnyElement, Context};
use std::collections::HashMap;
use crate::ui::scale::px;

/// Tile shortcuts, read by the tiles key handler.
const TILING: [(Action, &str); 4] = [
    (Action::SplitNewSession, "Split new session"),
    (Action::CloseTile, "Close tile"),
    (Action::FocusNext, "Focus next tile"),
    (Action::FocusPrev, "Focus previous tile"),
];

/// App shortcuts, read by the global key handler. `Next theme` comes first on the page, then the two
/// keys the prototype groups under APP (settings.md §4); the rest are ArchonUI's own bindings.
const APP: [(Action, &str); 5] = [
    (Action::NextTheme, "Next theme"),
    (Action::ToggleSidebar, "Toggle sidebar"),
    (Action::TogglePanel, "Toggle tools panel"),
    (Action::NewSession, "New session"),
    (Action::SessionSwitcher, "Session switcher"),
];

/// The composer key the prototype lists on its own (settings.md §4). The composer hard-codes it, so it
/// can't be rebound yet.
const COMPOSER: [(&str, &str); 1] = [("Cycle agent mode: Auto, Plan, Ask", "⇧ Tab")];

/// The composer and dialog keys the prototype groups under APP (settings.md §4); hard-coded, not rebindable.
const HARD_KEYS: [(&str, &str); 2] = [("Send / new line", "⏎ / ⇧⏎"), ("Close dialog", "Esc")];

/// What Swap changes to resolve the clash between `action` and `other`: the action to rebind and its new
/// key. Both share one key, so trading their current keys would change nothing. When one of the two was
/// just rebound onto the shared key, the other one takes the key it gave up, which is the swap the user
/// means. Otherwise `other` falls back to its own default when that is free, else is left unbound.
fn swap_binding(keys: &HashMap<Action, String>, action: Action, other: Action, displaced: Option<(Action, String)>) -> (Action, String) {
    let taken_by_another = |target: Action, spec: &str| keys.iter().any(|(a, k)| *a != target && k.as_str() == spec);
    if let Some((rebound, before)) = displaced {
        let target = if rebound == action { Some(other) } else if rebound == other { Some(action) } else { None };
        if let Some(target) = target.filter(|t| !before.is_empty() && !taken_by_another(*t, &before)) {
            return (target, before);
        }
    }
    let default = other.default_key();
    (other, if taken_by_another(other, default) { String::new() } else { default.to_string() })
}

impl Shell {
    pub(super) fn keyboard_page(&mut self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let theme = self.theme.clone();
        let mut tiling = Vec::new();
        for (action, name) in TILING {
            tiling.push(self.binding_row(action, name, cx));
        }
        let mut app = vec![self.binding_row(Action::NextTheme, "Next theme", cx)];
        app.extend(HARD_KEYS.iter().map(|(name, keys)| kbd_row(name, keys, &theme)));
        for (action, name) in APP.iter().skip(1) {
            app.push(self.binding_row(*action, name, cx));
        }
        let composer: Vec<SettingRow> = COMPOSER.iter().map(|(name, keys)| kbd_row(name, keys, &theme)).collect();

        let hint = match self.settings.capture {
            Some(_) => "Press new shortcut, Esc cancels, Backspace clears",
            None => "Click to change, Escape closes",
        };
        vec![
            div().text_size(px(12.)).text_color(theme.muted.to_hsla()).child(hint).into_any_element(),
            section("TILING", tiling, &theme),
            section("COMPOSER", composer, &theme),
            section("APP", app, &theme),
        ]
    }

    /// One rebindable row: the shortcut badge, plus a conflict note and Swap when another action has the same keys.
    fn binding_row(&mut self, action: Action, name: &str, cx: &mut Context<Self>) -> SettingRow {
        let theme = self.theme.clone();
        let capturing = self.settings.capture == Some(action);
        let keys = &self.state.settings.keys;
        let spec = keys.get(&action).cloned().unwrap_or_default();
        let other = (!spec.is_empty())
            .then(|| {
                [TILING.as_slice(), APP.as_slice()]
                    .concat()
                    .into_iter()
                    .find(|(b, _)| *b != action && keys.get(b).is_some_and(|k| *k == spec))
            })
            .flatten();
        let badge = if capturing {
            div()
                .px(px(10.))
                .py(px(3.))
                .rounded(px(5.))
                .border_1()
                .border_color(theme.acc.to_hsla())
                .text_size(px(12.))
                .text_color(theme.acc2.to_hsla())
                .child("Press a key…")
                .into_any_element()
        } else {
            let label = binding_label(keys, action);
            div().child(Kbd::new(label, &theme)).into_any_element()
        };
        let mut control = div()
            .flex()
            .items_center()
            .gap(px(10.))
            .id(("binding", action as usize))
            .cursor_pointer()
            .on_click(cx.listener(move |this, _, _, cx| {
                this.settings.capture = Some(action);
                cx.notify();
            }))
            .child(badge);
        if let Some((other, other_name)) = other {
            let other_name = other_name.to_string();
            control = control
                .child(
                    div()
                        .text_size(px(11.5))
                        .text_color(theme.acc2.to_hsla())
                        .child(format!("Also bound to {other_name}")),
                )
                .child(
                    div()
                        .id(("swap", action as usize))
                        .px(px(10.))
                        .py(px(5.))
                        .rounded(px(6.))
                        .border_1()
                        .border_color(theme.line2.to_hsla())
                        .text_size(px(12.))
                        .cursor_pointer()
                        .hover(|s| s.bg(theme.line2.to_hsla()))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            let displaced = this.settings.displaced.take();
                            let (target, spec) = swap_binding(&this.state.settings.keys, action, other, displaced);
                            this.state.settings.keys.insert(target, spec);
                            this.save(cx);
                        }))
                        .child("Swap"),
                );
        }
        SettingRow::new(name.to_string(), String::new(), &theme).control(control)
    }
}

#[cfg(test)]
mod tests {
    use super::swap_binding;
    use crate::app::settings::{default_keys, Action};

    #[test]
    fn swap_gives_the_clashing_action_the_key_that_was_given_up() {
        let mut keys = default_keys();
        // Next theme was rebound onto Close tile's key; it used to be alt-t.
        keys.insert(Action::NextTheme, "alt-q".into());
        let displaced = || Some((Action::NextTheme, "alt-t".to_string()));
        // From either row, Close tile takes alt-t and Next theme keeps alt-q.
        assert_eq!(swap_binding(&keys, Action::NextTheme, Action::CloseTile, displaced()), (Action::CloseTile, "alt-t".into()));
        assert_eq!(swap_binding(&keys, Action::CloseTile, Action::NextTheme, displaced()), (Action::CloseTile, "alt-t".into()));
    }

    #[test]
    fn swap_without_history_uses_a_free_default_or_unbinds() {
        let mut keys = default_keys();
        keys.insert(Action::NextTheme, "alt-q".into());
        // Close tile's default is the shared key itself, so it is left unbound rather than still clashing.
        assert_eq!(swap_binding(&keys, Action::NextTheme, Action::CloseTile, None), (Action::CloseTile, String::new()));
        let mut keys = default_keys();
        keys.insert(Action::CloseTile, "ctrl-b".into());
        // Close tile was moved onto Toggle sidebar's key; its default alt-q is free again.
        assert_eq!(swap_binding(&keys, Action::ToggleSidebar, Action::CloseTile, None), (Action::CloseTile, "alt-q".into()));
    }
}

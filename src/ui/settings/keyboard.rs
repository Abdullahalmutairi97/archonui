//! The Keyboard page: rebindable shortcuts, read-only keys, and conflict handling. Spec: settings.md §4.

use super::controls::{key_label, listen, row_button};
use crate::{
    app::settings::Action,
    ui::{
        shell::Shell,
        theme::Theme,
        widgets::section::{card, section_head},
    },
};
use gpui::{div, prelude::*, AnyElement, Context};
use std::collections::HashMap;
use crate::ui::scale::px;

/// Tile shortcuts, read by the tiles key handler, and Next theme.
const TILING: [(Action, &str); 5] = [
    (Action::SplitNewSession, "Split new session"),
    (Action::CloseTile, "Close tile"),
    (Action::FocusNext, "Focus next"),
    (Action::FocusPrev, "Focus previous"),
    (Action::NextTheme, "Next theme"),
];

/// App shortcuts, read by the global key handler.
const APP: [(Action, &str); 4] = [
    (Action::NewSession, "New session"),
    (Action::SessionSwitcher, "Session switcher"),
    (Action::ToggleSidebar, "Toggle sidebar"),
    (Action::TogglePanel, "Toggle panel"),
];

/// The message box keys. It handles them itself, so they can't be rebound yet.
const COMPOSER: [(&str, &[&str]); 3] = [("Send", &["Enter"]), ("New line", &["Shift", "Enter"]), ("Cycle mode", &["Shift", "Tab"])];

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
        let tiling: Vec<AnyElement> = TILING.iter().map(|(action, name)| self.binding_row(*action, name, cx)).collect();
        let composer: Vec<AnyElement> = COMPOSER.iter().map(|(name, keys)| key_row(name, None, keys_box(keys, None, &theme).into_any_element(), &theme)).collect();
        let mut app: Vec<AnyElement> = APP.iter().map(|(action, name)| self.binding_row(*action, name, cx)).collect();
        app.push(key_row("Close dialog", None, keys_box(&["Esc"], None, &theme).into_any_element(), &theme));
        let group = |title: &str, hint: Option<&str>, rows: Vec<AnyElement>| {
            div()
                .flex()
                .flex_col()
                .child(section_head(title.to_string(), hint.map(|h| h.to_string().into()), Vec::new(), &theme))
                .child(card(rows, &theme))
                .into_any_element()
        };
        vec![
            group("Tiling", Some("Click a shortcut to change it"), tiling),
            group("Composer", None, composer),
            group("App", None, app),
        ]
    }

    /// One rebindable row: its keys in a dashed box that starts a rebinding when clicked, and a
    /// conflict note with Swap when another action has the same keys.
    fn binding_row(&mut self, action: Action, name: &str, cx: &mut Context<Self>) -> AnyElement {
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
        let label = key_label(&spec);
        let parts: Vec<&str> = if spec.is_empty() { Vec::new() } else { label.split(' ').collect() };
        let state = if capturing { Some("Press new shortcut") } else if spec.is_empty() { Some("Unset") } else { None };
        let boxed = keys_box(&parts, state, &theme)
            .id(("binding", action as usize))
            .cursor_pointer()
            .when(capturing, |d| d.border_color(theme.acc2.to_hsla()))
            .on_click(cx.listener(move |this, _, _, cx| {
                this.settings.capture = if this.settings.capture == Some(action) { None } else { Some(action) };
                cx.notify();
            }));
        let mut control = div().flex().items_center().gap(px(10.));
        if let Some((other, other_name)) = other.filter(|_| !capturing) {
            control = control
                .child(div().text_size(px(12.)).text_color(theme.acc2.to_hsla()).child(format!("Also bound to {other_name}")))
                .child(row_button(("swap", action as usize), "Swap", &theme, listen(cx, move |this, cx| {
                    let displaced = this.settings.displaced.take();
                    let (target, spec) = swap_binding(&this.state.settings.keys, action, other, displaced);
                    this.state.settings.keys.insert(target, spec);
                    this.save(cx);
                })));
        }
        key_row(name, capturing.then_some("Esc cancels · Backspace clears"), control.child(boxed).into_any_element(), &theme)
    }
}

/// A shortcut row: the action, an optional note in the second accent, and its keys at the end.
fn key_row(name: &str, note: Option<&str>, keys: AnyElement, theme: &Theme) -> AnyElement {
    div()
        .flex()
        .items_center()
        .gap(px(16.))
        .px(px(18.))
        .py(px(10.))
        .bg(theme.tile.to_hsla())
        .child(
            div()
                .flex_1()
                .flex()
                .flex_col()
                .gap(px(2.))
                .child(name.to_string())
                .when_some(note, |d, note| d.child(div().text_size(px(12.)).text_color(theme.acc2.to_hsla()).child(note.to_string()))),
        )
        .child(keys)
        .into_any_element()
}

/// Key chips in a dashed box, or `state` (such as "Unset") in place of the keys.
fn keys_box(keys: &[&str], state: Option<&str>, theme: &Theme) -> gpui::Div {
    let mut b = div()
        .flex()
        .items_center()
        .gap(px(4.))
        .p(px(3.))
        .rounded(px(7.))
        .border_1()
        .border_dashed()
        .border_color(gpui::transparent_black());
    match state {
        Some(text) => {
            let colour = if text == "Unset" { theme.muted } else { theme.acc2 };
            b = b.child(div().px(px(10.)).py(px(3.)).text_size(px(12.)).text_color(colour.to_hsla()).child(text.to_string()));
        }
        None => {
            for key in keys {
                b = b.child(
                    div()
                        .min_w(px(24.))
                        .flex()
                        .justify_center()
                        .px(px(8.))
                        .py(px(3.))
                        .rounded(px(5.))
                        .border_1()
                        .border_color(theme.line2.to_hsla())
                        .bg(theme.line.to_hsla())
                        .font_family(theme.code_font.clone())
                        .text_size(px(12.))
                        .child(key.to_string()),
                );
            }
        }
    }
    b
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

//! The Keyboard page: rebindable shortcuts, read-only keys, and conflict handling. Spec: settings.md §4.

use super::controls::{binding_label, kbd_row, section};
use crate::{
    app::settings::Action,
    ui::{shell::Shell, widgets::{kbd::Kbd, section::SettingRow}},
};
use gpui::{div, prelude::*, AnyElement, Context};
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
            Some(_) => "Press the new shortcut. Esc cancels, Backspace clears.",
            None => "Click a shortcut to change it. Escape closes this window.",
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
                            let keys = &mut this.state.settings.keys;
                            let mine = keys.get(&action).cloned().unwrap_or_default();
                            let theirs = keys.get(&other).cloned().unwrap_or_default();
                            keys.insert(action, theirs);
                            keys.insert(other, mine);
                            this.save(cx);
                        }))
                        .child("Swap"),
                );
        }
        SettingRow::new(name.to_string(), String::new(), &theme).control(control)
    }
}

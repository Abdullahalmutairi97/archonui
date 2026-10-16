//! Pieces shared by the settings pages: handlers that write to the shell, and the row builders.

use super::Slide;
use crate::{
    app::settings::Action,
    ui::{
        shell::Shell,
        theme::Theme,
        widgets::{
            button::{Button, ButtonStyle},
            kbd::Kbd,
            section::{Section, SettingRow},
            segmented::Segmented,
            slider::{slider, Slider, SliderRange},
            toggle::Toggle,
        },
    },
};
use gpui::{div, prelude::*, AnyElement, App, Context, Entity, IntoElement, SharedString, WeakEntity, Window};
use crate::ui::scale::px;

/// A handler that runs `apply` on the shell with the control's new value, then redraws.
pub(super) fn edit<T: 'static>(
    shell: &WeakEntity<Shell>,
    apply: impl Fn(&mut Shell, T, &mut Context<Shell>) + 'static,
) -> impl Fn(T, &mut Window, &mut App) + 'static {
    let shell = shell.clone();
    move |value, _, cx| {
        shell.update(cx, |this, cx| apply(this, value, cx)).ok();
    }
}

/// A click handler that runs `apply` on the shell.
pub(super) fn act(
    shell: &WeakEntity<Shell>,
    apply: impl Fn(&mut Shell, &mut Context<Shell>) + 'static,
) -> impl Fn(&mut Window, &mut App) + 'static {
    let shell = shell.clone();
    move |_, cx| {
        shell.update(cx, |this, cx| apply(this, cx)).ok();
    }
}

/// A button handler that runs `apply` on the shell. Buttons take no event, so this is `cx.listener` without one.
pub(super) fn listen(cx: &mut Context<Shell>, apply: impl Fn(&mut Shell, &mut Context<Shell>) + 'static) -> impl Fn(&mut Window, &mut App) + 'static {
    act(&cx.entity().downgrade(), apply)
}

/// A toggle row bound to a boolean setting.
pub(super) fn toggle_row(
    id: &'static str,
    name: &str,
    desc: &str,
    on: bool,
    shell: &WeakEntity<Shell>,
    theme: &Theme,
    apply: impl Fn(&mut Shell, bool, &mut Context<Shell>) + 'static,
    disabled: Option<&'static str>,
) -> SettingRow {
    let mut toggle = Toggle::new(id, on, theme).on_change(edit(shell, apply));
    if let Some(reason) = disabled {
        toggle = toggle.disabled(reason);
    }
    SettingRow::new(name.to_string(), desc.to_string(), theme).control(toggle)
}

/// A segmented row. `selected` is an index into `options`; an out-of-range index highlights nothing.
pub(super) fn segmented_row(
    id: impl Into<SharedString>,
    name: &str,
    desc: &str,
    options: &[&str],
    selected: usize,
    shell: &WeakEntity<Shell>,
    theme: &Theme,
    apply: impl Fn(&mut Shell, usize, &mut Context<Shell>) + 'static,
    disabled: Option<&'static str>,
) -> SettingRow {
    let mut seg = Segmented::new(id, options.iter().map(|o| o.to_string()), selected, theme).on_select(edit(shell, apply));
    if let Some(reason) = disabled {
        seg = seg.disabled(reason);
    }
    SettingRow::new(name.to_string(), desc.to_string(), theme).control(seg)
}

/// A neutral or destructive button row.
pub(super) fn button_row(
    id: &'static str,
    name: &str,
    desc: &str,
    label: &str,
    style: ButtonStyle,
    shell: &WeakEntity<Shell>,
    theme: &Theme,
    apply: impl Fn(&mut Shell, &mut Context<Shell>) + 'static,
) -> SettingRow {
    let button = Button::new(id, label.to_string(), theme).style(style).on_click(act(shell, apply));
    SettingRow::new(name.to_string(), desc.to_string(), theme).control(button)
}

/// A row whose control is a shortcut badge.
pub(super) fn kbd_row(name: &str, keys: &str, theme: &Theme) -> SettingRow {
    SettingRow::new(name.to_string(), String::new(), theme).control(Kbd::new(keys.to_string(), theme))
}

/// A titled group of setting rows.
pub(super) fn section(title: &str, rows: Vec<SettingRow>, theme: &Theme) -> AnyElement {
    rows.into_iter().fold(Section::new(title.to_string(), theme), |s, row| s.child(row)).into_any_element()
}

/// A small muted caption above a grid of cards.
pub(super) fn caption(text: &str, theme: &Theme) -> AnyElement {
    div()
        .pt(px(6.))
        .text_size(px(12.))
        .text_color(theme.muted.to_hsla())
        .child(text.to_string())
        .into_any_element()
}

/// A section heading with an optional note on the right, as the Appearance page and Tools use.
pub(super) fn heading(title: &str, note: Option<&str>, theme: &Theme) -> AnyElement {
    let mut row = div().flex().items_center().gap(px(16.)).child(
        div().flex_1().text_size(px(11.)).text_color(theme.muted.to_hsla()).child(title.to_string()),
    );
    if let Some(note) = note {
        row = row.child(div().text_size(px(12.)).text_color(theme.muted.to_hsla()).child(note.to_string()));
    }
    row.into_any_element()
}

/// A slider bound to a layout or effect value. The entity is kept in `sliders` so drags stay smooth.
pub(super) fn slider_control(
    which: Slide,
    value: f32,
    range: SliderRange,
    unit: &'static str,
    shell: &WeakEntity<Shell>,
    theme: &Theme,
    sliders: &mut std::collections::HashMap<Slide, Entity<Slider>>,
    cx: &mut Context<Shell>,
) -> AnyElement {
    let entity = match sliders.get(&which) {
        Some(entity) => entity.clone(),
        None => {
            let on_change = edit(shell, move |this: &mut Shell, v: f32, cx| this.set_slide(which, v, cx));
            let slider = Slider::new(value, range, unit, on_change);
            let entity = cx.new(|_| if which.commits_on_release() { slider.commit_on_release() } else { slider });
            sliders.insert(which, entity.clone());
            entity
        }
    };
    entity.update(cx, |s, cx| {
        // A drag preview stays put until it is committed; the setting catches up after that.
        if !s.is_pending() && (s.value() - value).abs() > f32::EPSILON {
            s.set_value(value, cx);
        }
    });
    div().flex().items_center().flex_none().child(slider(&entity, theme)).into_any_element()
}

/// The display label for a keystroke in the GPUI form (`ctrl-shift-w`): `Ctrl ⇧ W`, `⇧ Tab`, `Ctrl ⇧ ←`.
pub(crate) fn key_label(spec: &str) -> String {
    // Peel modifiers off the front, so a key that is itself `-` (`ctrl--`) isn't lost to a split on dashes.
    let mut rest = spec;
    let mut out: Vec<String> = Vec::new();
    while let Some((m, tail)) = rest.split_once('-') {
        if tail.is_empty() {
            break;
        }
        out.push(match m {
            "ctrl" => "Ctrl".to_string(),
            "alt" => "Alt".to_string(),
            "shift" => "⇧".to_string(),
            "cmd" => "⌘".to_string(),
            "super" | "win" => "Super".to_string(),
            "fn" => "Fn".to_string(),
            other => other.to_string(),
        });
        rest = tail;
    }
    out.push(match rest {
        "enter" => "⏎".to_string(),
        "tab" => "Tab".to_string(),
        "left" => "←".to_string(),
        "right" => "→".to_string(),
        "up" => "↑".to_string(),
        "down" => "↓".to_string(),
        "escape" => "Esc".to_string(),
        "space" => "Space".to_string(),
        "backspace" => "Backspace".to_string(),
        k if k.chars().count() == 1 => k.to_uppercase(),
        k => k.to_string(),
    });
    out.join(" ")
}

/// The keystroke a binding shows, or the words for an unbound action.
pub(super) fn binding_label(keys: &std::collections::HashMap<Action, String>, action: Action) -> String {
    match keys.get(&action).map(String::as_str) {
        Some("") | None => "Unbound".to_string(),
        Some(spec) => key_label(spec),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_labels_read_like_the_prototype() {
        assert_eq!(key_label("ctrl-shift-w"), "Ctrl ⇧ W");
        assert_eq!(key_label("shift-tab"), "⇧ Tab");
        assert_eq!(key_label("ctrl-shift-left"), "Ctrl ⇧ ←");
        assert_eq!(key_label("super-k"), "Super K");
        assert_eq!(key_label("ctrl--"), "Ctrl -");
        assert_eq!(key_label("-"), "-");
        assert_eq!(key_label("ctrl-f5"), "Ctrl f5");
    }

    #[test]
    fn unbound_actions_say_so() {
        let mut keys = std::collections::HashMap::new();
        assert_eq!(binding_label(&keys, Action::CloseTile), "Unbound");
        keys.insert(Action::CloseTile, String::new());
        assert_eq!(binding_label(&keys, Action::CloseTile), "Unbound");
        keys.insert(Action::CloseTile, "alt-q".into());
        assert_eq!(binding_label(&keys, Action::CloseTile), "Alt Q");
    }
}

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

/// The prototype's icons, on a 16-unit grid, drawn with a 1.4 stroke.
pub(super) mod icon {
    pub const PIN: &str = "M9.8 2.5l3.7 3.7-1.6.9-2.6 2.6.2 2.5-.9.9-2.9-2.9-3.4 3.4M7 5.2l3.8 3.8";
    pub const FOLDER: &str = "M2 4.5h4l1.5 1.5H14v6.5H2z";
    pub const CLOUD: &str = "M4.5 12.5a3 3 0 010-6 4 4 0 017.7 1 2.5 2.5 0 01-.7 5z";
    pub const DESKTOP: &str = "M2 3h12v8H2zM6 14h4M8 11v3";
    pub const PHONE: &str = "M5 1.5h6v13H5zM7.5 12.5h1";
    pub const SHARE: &str = "M12 3a1.5 1.5 0 110 3 1.5 1.5 0 010-3zM4 6.5a1.5 1.5 0 110 3 1.5 1.5 0 010-3zM12 10a1.5 1.5 0 110 3 1.5 1.5 0 010-3zM5.4 7.3l5.2-3M5.4 8.7l5.2 3";
    pub const PLUS: &str = "M8 3v10M3 8h10";
    pub const X: &str = "M4.5 4.5l7 7M11.5 4.5l-7 7";
    pub const EDIT: &str = "M3 13l.7-3L11 2.7 13.3 5 6 12.3zM9.5 4.2l2.3 2.3";
    pub const SEARCH: &str = "M7 2.5a4.5 4.5 0 110 9 4.5 4.5 0 010-9zM10.5 10.5L14 14";
    pub const CHEV: &str = "M4 6l4 4 4-4";
}

/// A path in the code font, ellipsed: in the text colour when set, muted when it is a placeholder.
pub(super) fn path_label(text: &str, set: bool, theme: &Theme) -> AnyElement {
    div()
        .max_w(px(240.))
        .min_w(px(0.))
        .overflow_hidden()
        .text_ellipsis()
        .whitespace_nowrap()
        .font_family(theme.code_font.clone())
        .text_size(px(12.))
        .text_color(if set { theme.fg.to_hsla() } else { theme.muted.to_hsla() })
        .child(text.to_string())
        .into_any_element()
}

/// A small button for a section's title row: outline, with an optional icon.
pub(super) fn act_button(
    id: impl Into<gpui::ElementId>,
    label: &str,
    glyph: Option<&'static str>,
    theme: &Theme,
    on_click: impl Fn(&mut Window, &mut App) + 'static,
) -> AnyElement {
    let fg = theme.fg.to_hsla();
    div()
        .id(id.into())
        .flex()
        .items_center()
        .gap(px(6.))
        .px(px(11.))
        .py(px(5.))
        .rounded(px(7.))
        .text_size(px(12.))
        .border_1()
        .border_color(theme.line2.to_hsla())
        .text_color(fg)
        .cursor_pointer()
        .hover(|s| s.bg(theme.line.to_hsla()))
        .on_click(move |_, window, cx| on_click(window, cx))
        .when_some(glyph, |b, d| b.child(crate::ui::composer::icons::stroke_icon(d, 12., 1.4, fg)))
        .child(label.to_string())
        .into_any_element()
}

/// A dashed box with a message, for an empty list.
pub(super) fn empty_box(glyph: Option<&'static str>, text: &str, theme: &Theme) -> AnyElement {
    let (radius, _) = crate::ui::widgets::section::card_style();
    let muted = theme.muted.to_hsla();
    div()
        .flex()
        .flex_col()
        .items_center()
        .gap(px(10.))
        .py(px(34.))
        .rounded(px(radius))
        .border_1()
        .border_dashed()
        .border_color(theme.line2.to_hsla())
        .text_color(muted)
        .when_some(glyph, |b, d| b.child(crate::ui::composer::icons::stroke_icon(d, 20., 1.4, muted)))
        .child(text.to_string())
        .into_any_element()
}

/// A coloured dot and a muted status word, as agents, servers and devices show their state.
pub(super) fn status(dot: crate::ui::theme::Rgba, text: impl Into<SharedString>, theme: &Theme) -> AnyElement {
    div()
        .flex()
        .items_center()
        .gap(px(6.))
        .text_size(px(12.))
        .text_color(theme.muted.to_hsla())
        .child(div().flex_none().size(px(7.)).rounded_full().bg(dot.to_hsla()))
        .child(text.into())
        .into_any_element()
}

/// The search field above a long list.
pub(super) fn search_box(input: &Entity<crate::ui::widgets::text_input::TextInput>, theme: &Theme) -> AnyElement {
    let (radius, border) = crate::ui::widgets::section::card_style();
    let muted = theme.muted.to_hsla();
    div()
        .flex()
        .items_center()
        .gap(px(10.))
        .px(px(14.))
        .py(px(9.))
        .mb(px(10.))
        .rounded(px(radius))
        .bg(theme.tile.to_hsla())
        .when(border, |d| d.border_1().border_color(theme.line.to_hsla()))
        .child(crate::ui::composer::icons::stroke_icon(icon::SEARCH, 14., 1.4, muted))
        .child(div().flex_1().min_w(px(0.)).child(crate::ui::widgets::text_input::text_input(input, theme)))
        .into_any_element()
}

/// Filter chips: one per option, the picked one outlined in the accent.
pub(super) fn chips(
    id: &'static str,
    options: &[String],
    picked: usize,
    theme: &Theme,
    on_pick: impl Fn(usize, &mut Window, &mut App) + 'static,
) -> AnyElement {
    let on_pick = std::rc::Rc::new(on_pick);
    let mut row = div().flex().flex_wrap().gap(px(6.)).pb(px(10.));
    for (i, name) in options.iter().enumerate() {
        let on = i == picked;
        let on_pick = on_pick.clone();
        row = row.child(
            div()
                .id((id, i))
                .px(px(11.))
                .py(px(4.))
                .rounded_full()
                .text_size(px(12.))
                .border_1()
                .border_color(if on { theme.acc.to_hsla() } else { theme.line2.to_hsla() })
                .when(on, |d| d.bg(theme.line2.to_hsla()).text_color(theme.fg.to_hsla()))
                .when(!on, |d| d.text_color(theme.muted.to_hsla()))
                .cursor_pointer()
                .on_click(move |_, window, cx| on_pick(i, window, cx))
                .child(name.clone()),
        );
    }
    row.into_any_element()
}

/// The "Show N more · M left" row at the foot of a paged list.
pub(super) fn more_row(
    id: &'static str,
    step: usize,
    left: usize,
    theme: &Theme,
    on_click: impl Fn(&mut Window, &mut App) + 'static,
) -> AnyElement {
    let hover = crate::ui::theme::mix(theme.bg, theme.fg, 0.12).to_hsla();
    div()
        .id(id)
        .flex()
        .justify_center()
        .p(px(11.))
        .text_size(px(12.))
        .bg(theme.tile.to_hsla())
        .text_color(theme.acc2.to_hsla())
        .cursor_pointer()
        .hover(move |s| s.bg(hover))
        .on_click(move |_, window, cx| on_click(window, cx))
        .child(format!("Show {} more · {left} left", step.min(left)))
        .into_any_element()
}

/// A text link in the second accent.
pub(super) fn link(
    id: impl Into<gpui::ElementId>,
    label: impl Into<SharedString>,
    colour: crate::ui::theme::Rgba,
    on_click: impl Fn(&mut Window, &mut App) + 'static,
) -> AnyElement {
    div()
        .id(id.into())
        .text_size(px(12.))
        .text_color(colour.to_hsla())
        .cursor_pointer()
        .hover(|s| s.opacity(0.8))
        .on_click(move |_, window, cx| on_click(window, cx))
        .child(label.into())
        .into_any_element()
}

/// A row of a long list: the name with an optional note, and its controls at the end.
pub(super) fn list_row(name: impl Into<SharedString>, note: Option<SharedString>, end: Vec<AnyElement>, theme: &Theme) -> gpui::Div {
    div()
        .flex()
        .flex_wrap()
        .items_center()
        .gap_x(px(16.))
        .gap_y(px(8.))
        .px(px(18.))
        .py(px(11.))
        .bg(theme.tile.to_hsla())
        .child(
            div()
                .flex_1()
                .flex_basis(px(180.))
                .min_w(px(0.))
                .flex()
                .flex_col()
                .gap(px(2.))
                .child(name.into())
                .when_some(note, |d, note| d.child(div().text_size(px(12.)).text_color(theme.muted.to_hsla()).child(note))),
        )
        .child(div().ml_auto().flex().items_center().gap(px(14.)).children(end))
}

/// An outline button for a list row, such as Install or Remove.
pub(super) fn row_button(
    id: impl Into<gpui::ElementId>,
    label: impl Into<SharedString>,
    theme: &Theme,
    on_click: impl Fn(&mut Window, &mut App) + 'static,
) -> AnyElement {
    let line = theme.line.to_hsla();
    div()
        .id(id.into())
        .px(px(12.))
        .py(px(5.))
        .rounded(px(7.))
        .text_size(px(12.))
        .border_1()
        .border_color(theme.line2.to_hsla())
        .cursor_pointer()
        .hover(move |s| s.bg(line))
        .on_click(move |_, window, cx| on_click(window, cx))
        .child(label.into())
        .into_any_element()
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

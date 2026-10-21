//! Settings section and row layout from the settings prototype: a small muted title with optional
//! hint and actions at the end, then a rounded card of rows separated by 1px hairlines. Each row is a
//! label with an optional short note, and its control at the end; they wrap when the row is narrow.

use gpui::{div, prelude::*, AnyElement, App, Hsla, IntoElement, RenderOnce, SharedString, Window};
use std::cell::Cell;
use crate::ui::scale::px;

use crate::ui::theme::Theme;

thread_local! {
    /// The card corner radius and whether cards have a border: the user's Corner radius and Borders.
    static CARD: Cell<(f32, bool)> = const { Cell::new((10., true)) };
}

/// Sets the corner radius and border the cards drawn after this use.
pub fn set_card_style(radius: f32, border: bool) {
    CARD.with(|c| c.set((radius, border)));
}

/// The corner radius and border cards are drawn with.
pub fn card_style() -> (f32, bool) {
    CARD.with(Cell::get)
}

#[derive(Clone, Copy)]
struct Colors {
    fg: Hsla,
    muted: Hsla,
    line: Hsla,
    tile: Hsla,
    panel: Hsla,
}

impl Colors {
    fn of(theme: &Theme) -> Self {
        Self {
            fg: theme.fg.to_hsla(),
            muted: theme.muted.to_hsla(),
            line: theme.line.to_hsla(),
            tile: theme.tile.to_hsla(),
            panel: theme.panel.to_hsla(),
        }
    }
}

/// A section's title row: the title, then an optional hint and action buttons at the end.
pub fn section_head(title: impl Into<SharedString>, hint: Option<SharedString>, acts: Vec<AnyElement>, theme: &Theme) -> AnyElement {
    let muted = theme.muted.to_hsla();
    div()
        .flex()
        .items_center()
        .gap(px(12.))
        .pt(px(26.))
        .pb(px(10.))
        .px(px(2.))
        .min_h(px(20.))
        .child(div().text_size(px(12.)).text_color(muted).child(title.into()))
        .child(div().flex_1())
        .when_some(hint, |row, hint| row.child(div().text_size(px(12.)).text_color(muted).child(hint)))
        .children(acts)
        .into_any_element()
}

/// A rounded card whose children are separated by 1px hairlines, as the settings rows sit in.
pub fn card(children: impl IntoIterator<Item = AnyElement>, theme: &Theme) -> gpui::Div {
    let (radius, border) = card_style();
    div()
        .flex()
        .flex_col()
        .gap(px(1.))
        .rounded(px(radius))
        .overflow_hidden()
        .bg(theme.panel.to_hsla())
        .when(border, |d| d.border_1().border_color(theme.line.to_hsla()))
        .children(children)
}

/// A titled group of [`SettingRow`]s.
#[derive(IntoElement)]
pub struct Section {
    title: SharedString,
    hint: Option<SharedString>,
    acts: Vec<AnyElement>,
    children: Vec<AnyElement>,
    colors: Colors,
    theme: Theme,
}

impl Section {
    pub fn new(title: impl Into<SharedString>, theme: &Theme) -> Self {
        Self {
            title: title.into(),
            hint: None,
            acts: Vec::new(),
            children: Vec::new(),
            colors: Colors::of(theme),
            theme: theme.clone(),
        }
    }

    pub fn child(mut self, child: impl IntoElement) -> Self {
        self.children.push(child.into_any_element());
        self
    }

    /// A short note at the end of the title row.
    pub fn hint(mut self, hint: impl Into<SharedString>) -> Self {
        self.hint = Some(hint.into());
        self
    }

    /// A button at the end of the title row.
    pub fn act(mut self, button: impl IntoElement) -> Self {
        self.acts.push(button.into_any_element());
        self
    }
}

impl RenderOnce for Section {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        let _ = self.colors;
        div()
            .flex()
            .flex_col()
            .child(section_head(self.title, self.hint, self.acts, &self.theme))
            .when(!self.children.is_empty(), |d| d.child(card(self.children, &self.theme)))
    }
}

/// One setting: a label with an optional note, and its control at the end.
#[derive(IntoElement)]
pub struct SettingRow {
    name: SharedString,
    description: SharedString,
    control: Option<AnyElement>,
    detail: Option<AnyElement>,
    below: Option<AnyElement>,
    colors: Colors,
}

impl SettingRow {
    pub fn new(
        name: impl Into<SharedString>,
        description: impl Into<SharedString>,
        theme: &Theme,
    ) -> Self {
        Self {
            name: name.into(),
            description: description.into(),
            control: None,
            detail: None,
            below: None,
            colors: Colors::of(theme),
        }
    }

    pub fn control(mut self, control: impl IntoElement) -> Self {
        self.control = Some(control.into_any_element());
        self
    }

    /// Content under the label, such as a status line.
    pub fn detail(mut self, detail: impl IntoElement) -> Self {
        self.detail = Some(detail.into_any_element());
        self
    }

    /// Content across the whole row under the label and control, such as thumbnail tiles.
    pub fn below(mut self, content: impl IntoElement) -> Self {
        self.below = Some(content.into_any_element());
        self
    }
}

impl RenderOnce for SettingRow {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        let c = self.colors;
        div()
            .flex()
            .flex_wrap()
            .items_center()
            .gap_x(px(24.))
            .gap_y(px(10.))
            .px(px(18.))
            .py(px(14.))
            .bg(c.tile)
            .child(
                div()
                    .flex_1()
                    .flex_basis(px(200.))
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap(px(2.))
                    .child(div().text_size(px(13.5)).text_color(c.fg).child(self.name))
                    .when(!self.description.is_empty(), |col| {
                        col.child(div().text_size(px(12.)).text_color(c.muted).child(self.description))
                    })
                    .when_some(self.detail, |col, detail| col.child(div().pt(px(3.)).child(detail))),
            )
            .when_some(self.control, |row, control| {
                row.child(div().ml_auto().flex().items_center().justify_end().gap(px(10.)).flex_wrap().child(control))
            })
            .when_some(self.below, |row, below| row.child(div().w_full().pt(px(4.)).child(below)))
    }
}

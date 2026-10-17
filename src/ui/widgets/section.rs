//! Settings section and row layout from settings §1.5: a muted section title, then rows with a
//! label and description on the left and one control on the right.
//!
//! The spec's 0.16em tracking on section titles isn't applied: GPUI 0.2.2 has no letter-spacing.

use gpui::{div, prelude::*, AnyElement, App, Hsla, IntoElement, RenderOnce, SharedString, Window};
use crate::ui::scale::px;

use crate::ui::theme::Theme;

#[derive(Clone, Copy)]
struct Colors {
    fg: Hsla,
    muted: Hsla,
    line: Hsla,
}

impl Colors {
    fn of(theme: &Theme) -> Self {
        Self { fg: theme.fg.to_hsla(), muted: theme.muted.to_hsla(), line: theme.line.to_hsla() }
    }
}

/// A titled group of [`SettingRow`]s.
#[derive(IntoElement)]
pub struct Section {
    title: SharedString,
    children: Vec<AnyElement>,
    colors: Colors,
}

impl Section {
    pub fn new(title: impl Into<SharedString>, theme: &Theme) -> Self {
        Self { title: title.into(), children: Vec::new(), colors: Colors::of(theme) }
    }

    pub fn child(mut self, child: impl IntoElement) -> Self {
        self.children.push(child.into_any_element());
        self
    }
}

impl RenderOnce for Section {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        div()
            .flex()
            .flex_col()
            .child(
                div()
                    .pt(px(14.))
                    .pb(px(2.))
                    .text_size(px(11.))
                    .line_height(px(14.))
                    .text_color(self.colors.muted)
                    .child(self.title),
            )
            .children(self.children)
    }
}

/// One settings row: name and description on the left, `control` on the right, with a hairline below.
#[derive(IntoElement)]
pub struct SettingRow {
    name: SharedString,
    description: SharedString,
    control: Option<AnyElement>,
    /// Shown under the description, such as the agents a setting reaches.
    detail: Option<AnyElement>,
    /// Full-width content under the name and control, such as a row of image tiles.
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

    /// An element under the description.
    pub fn detail(mut self, detail: impl IntoElement) -> Self {
        self.detail = Some(detail.into_any_element());
        self
    }

    /// Content laid out under the row's name, across its full width.
    pub fn below(mut self, content: impl IntoElement) -> Self {
        self.below = Some(content.into_any_element());
        self
    }
}

impl RenderOnce for SettingRow {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        let c = self.colors;
        let top = div()
            .flex()
            .items_center()
            .gap(px(24.))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap(px(3.))
                    .child(div().text_size(px(14.)).line_height(px(16.)).text_color(c.fg).child(self.name))
                    // An empty description would still take a line and push the name above the control's centre.
                    .when(!self.description.is_empty(), |col| {
                        col.child(
                            div()
                                .text_size(px(12.))
                                .line_height(px(14.))
                                .text_color(c.muted)
                                .child(self.description),
                        )
                    })
                    .when_some(self.detail, |col, detail| col.child(div().pt(px(3.)).child(detail))),
            )
            .when_some(self.control, |row, control| row.child(control));
        div()
            .flex()
            .flex_col()
            .gap(px(10.))
            .py(px(14.))
            .border_b_1()
            .border_color(c.line)
            .child(top)
            .when_some(self.below, |row, below| row.child(below))
    }
}

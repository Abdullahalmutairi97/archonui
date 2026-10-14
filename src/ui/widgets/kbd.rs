//! Keyboard shortcut badge from settings §1.6. It's display only, with no click handler.

use gpui::{div, prelude::*, App, Hsla, IntoElement, RenderOnce, SharedString, Window};
use crate::ui::scale::px;

use crate::ui::theme::Theme;

#[derive(IntoElement)]
pub struct Kbd {
    label: SharedString,
    colors: Colors,
}

#[derive(Clone, Copy)]
struct Colors {
    line2: Hsla,
    alt: Hsla,
}

impl Kbd {
    pub fn new(label: impl Into<SharedString>, theme: &Theme) -> Self {
        Self {
            label: label.into(),
            colors: Colors { line2: theme.line2.to_hsla(), alt: theme.alt.to_hsla() },
        }
    }
}

impl RenderOnce for Kbd {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        div()
            .flex_none()
            .px(px(10.))
            .py(px(4.))
            .rounded(px(5.))
            .border_1()
            .border_color(self.colors.line2)
            .text_color(self.colors.alt)
            .text_size(px(12.))
            .child(self.label)
    }
}

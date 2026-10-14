//! Buttons from settings §1.6 and chat §4.3: outline, filled accent, danger, ghost and icon.
//! A disabled button keeps its look, takes no clicks, and shows its reason as a tooltip.

use std::rc::Rc;

use gpui::{
    div, prelude::*, AnyElement, App, ElementId, Hsla, IntoElement, RenderOnce, SharedString,
    Window,
};
use crate::ui::scale::px;

use super::popover::{tooltip, TooltipColors};
use crate::ui::theme::Theme;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ButtonStyle {
    /// Neutral border and text with a `line2` hover fill. Settings' Export and Reset.
    Outline,
    /// Accent fill with `on` text.
    Filled,
    /// Outline in accent red. Settings' destructive "Clear history".
    Danger,
    /// No border or fill until hovered.
    Ghost,
}

#[derive(Clone, Copy)]
struct Colors {
    fg: Hsla,
    muted: Hsla,
    on: Hsla,
    acc: Hsla,
    line: Hsla,
    line2: Hsla,
}

/// A button. Text buttons use the settings-row metrics; [`Button::icon`] is a 34×34 square.
#[derive(IntoElement)]
pub struct Button {
    id: ElementId,
    child: AnyElement,
    style: ButtonStyle,
    icon: bool,
    disabled: Option<SharedString>,
    on_click: Option<Rc<dyn Fn(&mut Window, &mut App)>>,
    colors: Colors,
    tooltip_colors: TooltipColors,
}

impl Button {
    /// An outline text button labelled `label`.
    pub fn new(id: impl Into<ElementId>, label: impl Into<SharedString>, theme: &Theme) -> Self {
        let label: SharedString = label.into();
        Self::build(id.into(), div().child(label).into_any_element(), false, theme)
    }

    /// A ghost 34×34 icon button. `glyph` is the icon element.
    pub fn icon(id: impl Into<ElementId>, glyph: impl IntoElement, theme: &Theme) -> Self {
        let mut button = Self::build(id.into(), glyph.into_any_element(), true, theme);
        button.style = ButtonStyle::Ghost;
        button
    }

    fn build(id: ElementId, child: AnyElement, icon: bool, theme: &Theme) -> Self {
        Self {
            id,
            child,
            style: ButtonStyle::Outline,
            icon,
            disabled: None,
            on_click: None,
            colors: Colors {
                fg: theme.fg.to_hsla(),
                muted: theme.muted.to_hsla(),
                on: theme.on.to_hsla(),
                acc: theme.acc.to_hsla(),
                line: theme.line.to_hsla(),
                line2: theme.line2.to_hsla(),
            },
            tooltip_colors: TooltipColors::of(theme),
        }
    }

    pub fn style(mut self, style: ButtonStyle) -> Self {
        self.style = style;
        self
    }

    /// Disables the button and shows `reason` as its tooltip.
    pub fn disabled(mut self, reason: impl Into<SharedString>) -> Self {
        self.disabled = Some(reason.into());
        self
    }

    pub fn on_click(mut self, handler: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.on_click = Some(Rc::new(handler));
        self
    }
}

impl RenderOnce for Button {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        let c = self.colors;
        let (text, border, fill, hover) = match self.style {
            ButtonStyle::Outline => (c.fg, Some(c.line2), None, c.line2),
            ButtonStyle::Filled => (c.on, None, Some(c.acc), c.acc),
            ButtonStyle::Danger => (c.acc, Some(c.acc), None, c.line),
            ButtonStyle::Ghost => (c.muted, None, None, c.line2),
        };

        let mut el = div()
            .id(self.id)
            .flex()
            .items_center()
            .justify_center()
            .rounded(px(7.))
            .text_size(px(12.5))
            .text_color(text)
            .when(self.icon, |d| d.size(px(34.)))
            .when(!self.icon, |d| d.px(px(14.)).py(px(8.)))
            .when_some(fill, |d, fill| d.bg(fill))
            .when_some(border, |d, border| d.border_1().border_color(border))
            .child(self.child);

        if let Some(reason) = self.disabled {
            el = el.opacity(0.45).cursor_default().tooltip(tooltip(reason, self.tooltip_colors));
        } else {
            el = el.cursor_pointer().hover(move |style| style.bg(hover));
            if let Some(handler) = self.on_click {
                el = el.on_click(move |_, window, cx| handler(window, cx));
            }
        }
        el
    }
}

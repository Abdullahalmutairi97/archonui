//! Segmented control from settings §1.6: a `line` well with one filled `acc` pill for the selected
//! option. The selected pill is the only fill.

use std::rc::Rc;

use gpui::{div, prelude::*, App, ElementId, Hsla, IntoElement, RenderOnce, SharedString, Window};
use crate::ui::scale::px;

use super::popover::{tooltip, TooltipColors};
use crate::ui::theme::Theme;

#[derive(IntoElement)]
pub struct Segmented {
    id: SharedString,
    options: Vec<SharedString>,
    selected: usize,
    on_select: Option<Rc<dyn Fn(usize, &mut Window, &mut App)>>,
    disabled: Option<SharedString>,
    colors: Colors,
    tooltip_colors: TooltipColors,
}

#[derive(Clone, Copy)]
struct Colors {
    line: Hsla,
    acc: Hsla,
    on: Hsla,
    muted: Hsla,
}

impl Segmented {
    /// `id` names the control; each option gets an id derived from it, so it must be unique per window.
    pub fn new(
        id: impl Into<SharedString>,
        options: impl IntoIterator<Item = impl Into<SharedString>>,
        selected: usize,
        theme: &Theme,
    ) -> Self {
        Self {
            id: id.into(),
            options: options.into_iter().map(Into::into).collect(),
            selected,
            on_select: None,
            disabled: None,
            colors: Colors {
                line: theme.line.to_hsla(),
                acc: theme.line2.to_hsla(),
                on: theme.fg.to_hsla(),
                muted: theme.muted.to_hsla(),
            },
            tooltip_colors: TooltipColors::of(theme),
        }
    }

    /// Disables the control and shows `reason` as its tooltip. Options take no clicks.
    pub fn disabled(mut self, reason: impl Into<SharedString>) -> Self {
        self.disabled = Some(reason.into());
        self
    }

    /// Called with the index of the option that was clicked.
    pub fn on_select(mut self, handler: impl Fn(usize, &mut Window, &mut App) + 'static) -> Self {
        self.on_select = Some(Rc::new(handler));
        self
    }
}

impl RenderOnce for Segmented {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        let c = self.colors;
        let selected = self.selected;
        let on_select = if self.disabled.is_some() { None } else { self.on_select };
        let mut well = div()
            .id(ElementId::Name(self.id.clone()))
            .flex()
            .flex_none()
            .gap(px(2.))
            .p(px(3.))
            .rounded(px(8.))
            .bg(c.line)
            .when_some(self.disabled, |d, reason| d.opacity(0.45).tooltip(tooltip(reason, self.tooltip_colors)));
        for (index, label) in self.options.into_iter().enumerate() {
            let is_selected = index == selected;
            let mut option = div()
                .id(ElementId::NamedInteger(self.id.clone(), index as u64))
                .px(px(12.))
                .py(px(5.))
                .rounded(px(6.))
                .text_size(px(12.))
                .line_height(px(14.))
                .cursor_pointer()
                .when(is_selected, |d| d.bg(c.acc).text_color(c.on))
                .when(!is_selected, |d| d.text_color(c.muted))
                .child(label);
            if let Some(handler) = on_select.clone() {
                option = option.on_click(move |_, window, cx| handler(index, window, cx));
            }
            well = well.child(option);
        }
        well
    }
}

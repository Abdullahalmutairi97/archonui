//! On/off switch from settings §1.6: a 34×18 track with a 14px knob. The owner keeps `on` and
//! receives the wanted state from `on_change`. The track's `background` and the knob's `left` follow
//! the prototype's 0.2 s ease (layout.md §6 #8); the knob's colour changes at once.

use std::rc::Rc;

use gpui::{div, prelude::*, App, ElementId, Hsla, IntoElement, RenderOnce, SharedString, Window};
use crate::ui::scale::px;

use super::popover::{tooltip, TooltipColors};
use crate::ui::{
    anim::{self, Lerp, TOGGLE},
    theme::Theme,
};

#[derive(IntoElement)]
pub struct Toggle {
    id: ElementId,
    on: bool,
    on_change: Option<Rc<dyn Fn(bool, &mut Window, &mut App)>>,
    disabled: Option<SharedString>,
    colors: Colors,
    tooltip_colors: TooltipColors,
}

#[derive(Clone, Copy)]
struct Colors {
    acc: Hsla,
    faint: Hsla,
    on: Hsla,
    fg: Hsla,
}

impl Toggle {
    pub fn new(id: impl Into<ElementId>, on: bool, theme: &Theme) -> Self {
        Self {
            id: id.into(),
            on,
            on_change: None,
            disabled: None,
            colors: Colors {
                acc: theme.acc.to_hsla(),
                faint: theme.faint.to_hsla(),
                on: theme.on.to_hsla(),
                fg: theme.fg.to_hsla(),
            },
            tooltip_colors: TooltipColors::of(theme),
        }
    }

    /// Disables the switch and shows `reason` as its tooltip. It takes no clicks and keeps its state.
    pub fn disabled(mut self, reason: impl Into<SharedString>) -> Self {
        self.disabled = Some(reason.into());
        self
    }

    /// Called with the state after the click, which is the opposite of `on`.
    pub fn on_change(mut self, handler: impl Fn(bool, &mut Window, &mut App) + 'static) -> Self {
        self.on_change = Some(Rc::new(handler));
        self
    }
}

impl RenderOnce for Toggle {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        let c = self.colors;
        let on = self.on;
        let id = self.id;
        let tooltip_colors = self.tooltip_colors;
        let disabled = self.disabled;
        let on_change = self.on_change;
        anim::animated((id.clone(), "toggle"), TOGGLE, if on { 1.0 } else { 0.0 }, move |t| {
            let mut track = div()
                .id(id.clone())
                .relative()
                .flex_none()
                .w(px(34.))
                .h(px(18.))
                .rounded(px(9.))
                .bg(Hsla::lerp(&c.faint, &c.acc, t))
                .cursor_pointer()
                .child(
                    div()
                        .absolute()
                        .top(px(2.))
                        .left(px(2. + 16. * t))
                        .size(px(14.))
                        .rounded(px(7.))
                        .bg(if on { c.on } else { c.fg }),
                );
            if let Some(reason) = disabled.clone() {
                return track.opacity(0.45).cursor_default().tooltip(tooltip(reason, tooltip_colors)).into_any_element();
            }
            if let Some(handler) = on_change.clone() {
                track = track.on_click(move |_, window, cx| handler(!on, window, cx));
            }
            track.into_any_element()
        })
    }
}

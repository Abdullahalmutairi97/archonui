//! Anchored popovers and plain-text tooltips (layout.md §1.4: popovers sit 6px from their trigger,
//! `pop` fill, `line2` border, radius 10, padding 6).
//!
//! [`Popover`] is stateless. The caller owns `open` and gets every requested change through
//! `on_open_change(next)`. A press on the trigger, a press outside the panel and Escape all go
//! through that one callback with a target state, so their order doesn't matter.

use std::rc::Rc;

use gpui::{
    anchored, canvas, deferred, div, point, prelude::*, AnyElement, AnyView, App, Corner,
    DispatchPhase, Hsla, IntoElement, KeyDownEvent, MouseButton, RenderOnce,
    Render, SharedString, Window, BoxShadow, hsla,
};
use crate::ui::scale::px;

use crate::ui::theme::Theme;

/// Where the panel opens relative to the trigger. Both are left-aligned with it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Placement {
    /// The panel's bottom edge sits 6px above the trigger's top edge.
    Above,
    /// The panel's top edge sits 6px below the trigger's bottom edge.
    Below,
}

/// Colours for tooltips and popover panels, from the theme.
#[derive(Clone, Copy)]
pub struct TooltipColors {
    pub pop: Hsla,
    pub line2: Hsla,
    pub fg: Hsla,
}

impl TooltipColors {
    pub fn of(theme: &Theme) -> Self {
        Self { pop: theme.pop.to_hsla(), line2: theme.line2.to_hsla(), fg: theme.fg.to_hsla() }
    }
}

/// A tooltip builder for `Div::tooltip`: plain 12px text in a `pop` box with a `line2` border.
pub fn tooltip(
    text: impl Into<SharedString>,
    colors: TooltipColors,
) -> impl Fn(&mut Window, &mut App) -> AnyView + 'static {
    let text: SharedString = text.into();
    move |_window, cx| cx.new(|_| TooltipText { text: text.clone(), colors }).into()
}

struct TooltipText {
    text: SharedString,
    colors: TooltipColors,
}

impl Render for TooltipText {
    fn render(&mut self, _window: &mut Window, _cx: &mut gpui::Context<Self>) -> impl IntoElement {
        div()
            .px(px(10.))
            .py(px(6.))
            .rounded(px(6.))
            .bg(self.colors.pop)
            .border_1()
            .border_color(self.colors.line2)
            .text_color(self.colors.fg)
            .text_size(px(12.))
            .child(self.text.clone())
    }
}

type OpenChange = Rc<dyn Fn(bool, &mut Window, &mut App)>;

/// A panel anchored to `trigger`. Render it only where the trigger should show; `open` controls the panel.
#[derive(IntoElement)]
pub struct Popover {
    trigger: AnyElement,
    content: Option<AnyElement>,
    placement: Placement,
    open: bool,
    on_open_change: Option<OpenChange>,
    colors: PanelColors,
}

#[derive(Clone, Copy)]
struct PanelColors {
    pop: Hsla,
    line2: Hsla,
    shadow: Hsla,
}

impl Popover {
    pub fn new(trigger: impl IntoElement, theme: &Theme) -> Self {
        Self {
            trigger: trigger.into_any_element(),
            content: None,
            placement: Placement::Below,
            open: false,
            on_open_change: None,
            colors: PanelColors {
                pop: theme.pop.to_hsla(),
                line2: theme.line2.to_hsla(),
                shadow: hsla(0., 0., 0., 0.4),
            },
        }
    }

    pub fn placement(mut self, placement: Placement) -> Self {
        self.placement = placement;
        self
    }

    pub fn open(mut self, open: bool) -> Self {
        self.open = open;
        self
    }

    /// The panel's contents, shown while `open` is true.
    pub fn content(mut self, content: impl IntoElement) -> Self {
        self.content = Some(content.into_any_element());
        self
    }

    /// Called with the wanted state: `true` to open, `false` to close. Fires when the trigger is
    /// pressed, when the mouse is pressed outside the panel, and on Escape. The owner stores the value.
    pub fn on_open_change(mut self, handler: impl Fn(bool, &mut Window, &mut App) + 'static) -> Self {
        self.on_open_change = Some(Rc::new(handler));
        self
    }
}

impl RenderOnce for Popover {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        let open = self.open;
        let colors = self.colors;
        let placement = self.placement;
        let on_change = self.on_open_change.clone();

        // The trigger press is handled on the trigger's own wrapper. A window-level mouse listener
        // with bounds checks doesn't work here: this canvas can't cover the trigger, because it
        // sits after it in the flow. Clicking the trigger while open also fires the panel's
        // outside-press, which sends the same `false`, so the two can't disagree.
        let press = on_change.clone();
        let trigger = div()
            .when_some(press, |d, press| {
                d.on_mouse_down(MouseButton::Left, move |_, window, cx| press(!open, window, cx))
            })
            .child(self.trigger);

        // Escape is a window-level key listener, registered only while open. The canvas is absolute
        // with explicit insets so it takes no flow space and doesn't move the anchor slots.
        let escape = {
            let on_change = on_change.clone();
            canvas(
                |_, _, _| (),
                move |_, _, window, _cx| {
                    let Some(on_change) = on_change.clone() else {
                        return;
                    };
                    if open {
                        window.on_key_event(move |event: &KeyDownEvent, phase, window, cx| {
                            if phase == DispatchPhase::Bubble && event.keystroke.key == "escape" {
                                on_change(false, window, cx);
                            }
                        });
                    }
                },
            )
            .absolute()
            .top_0()
            .left_0()
            .size_full()
        };

        let panel = self.content.filter(|_| open).map(|content| {
            let (anchor, offset) = match placement {
                Placement::Above => (Corner::BottomLeft, point(px(0.), px(-6.))),
                Placement::Below => (Corner::TopLeft, point(px(0.), px(6.))),
            };
            let dismiss = on_change.clone();
            let panel = div()
                .flex()
                .flex_col()
                .min_w(px(200.))
                .p(px(6.))
                .rounded(px(10.))
                .bg(colors.pop)
                .border_1()
                .border_color(colors.line2)
                .shadow(vec![BoxShadow {
                    color: colors.shadow,
                    offset: point(px(0.), px(10.)),
                    blur_radius: px(30.),
                    spread_radius: px(0.),
                }])
                .when_some(dismiss, |d, dismiss| {
                    d.on_mouse_down_out(move |_, window, cx| dismiss(false, window, cx))
                })
                .child(content);
            // A zero-size slot placed before (Above) or after (Below) the trigger gives the anchor
            // point at the trigger's top or bottom edge.
            deferred(
                anchored()
                    .anchor(anchor)
                    .offset(offset)
                    .snap_to_window_with_margin(px(8.))
                    .child(panel),
            )
        });

        let (above, below) = match placement {
            Placement::Above => (panel, None),
            Placement::Below => (None, panel),
        };
        div()
            .relative()
            .when_some(above, |root, panel| root.child(panel))
            .child(trigger)
            .child(escape)
            .when_some(below, |root, panel| root.child(panel))
    }
}

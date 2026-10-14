//! Toast from layout.md §1.4 and §6: a `pop` box with a `line2` border, centred 36px above the bottom
//! edge, that hides after 1800 ms. Hold it as `Entity<Toasts>` and call `show` through `update`.

use std::time::Duration;

use gpui::{
    div, hsla, point, prelude::*, BoxShadow, Context, Hsla, IntoElement, Render, SharedString,
    Window,
};
use crate::ui::scale::px;

use crate::ui::theme::Theme;

/// How long a toast stays up, in milliseconds. layout.md §1.4.
pub const TOAST_MS: u64 = 1800;

pub struct Toasts {
    current: Option<Toast>,
    /// Bumped on every `show`, so a timer from an earlier toast can't hide a newer one.
    last_id: u64,
    colors: Colors,
}

struct Toast {
    id: u64,
    message: SharedString,
}

#[derive(Clone, Copy)]
struct Colors {
    pop: Hsla,
    line2: Hsla,
    fg: Hsla,
}

impl Toasts {
    pub fn new(theme: &Theme) -> Self {
        Self {
            current: None,
            last_id: 0,
            colors: Colors {
                pop: theme.pop.to_hsla(),
                line2: theme.line2.to_hsla(),
                fg: theme.fg.to_hsla(),
            },
        }
    }

    /// Shows `message`, replacing any toast already up, and hides it after [`TOAST_MS`].
    pub fn show(&mut self, message: impl Into<SharedString>, cx: &mut Context<Self>) {
        self.last_id += 1;
        let id = self.last_id;
        self.current = Some(Toast { id, message: message.into() });
        cx.notify();
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(Duration::from_millis(TOAST_MS)).await;
            this.update(cx, |toasts, cx| {
                if toasts.current.as_ref().is_some_and(|toast| toast.id == id) {
                    toasts.current = None;
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
    }

    /// The message on screen, if any. Used by tests.
    pub fn visible(&self) -> Option<&str> {
        self.current.as_ref().map(|toast| toast.message.as_ref())
    }
}

impl Render for Toasts {
    /// Fills the width at the bottom of its parent. The parent should be full-size and relatively positioned.
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let c = self.colors;
        div()
            .absolute()
            .bottom(px(36.))
            .left_0()
            .right_0()
            .flex()
            .justify_center()
            .when_some(self.current.as_ref(), |row, toast| {
                row.child(
                    div()
                        .px(px(18.))
                        .py(px(11.))
                        .rounded(px(8.))
                        .bg(c.pop)
                        .border_1()
                        .border_color(c.line2)
                        .text_size(px(13.))
                        .text_color(c.fg)
                        .shadow(vec![BoxShadow {
                            color: hsla(0., 0., 0., 0.4),
                            offset: point(px(0.), px(10.)),
                            blur_radius: px(30.),
                            spread_radius: px(0.),
                        }])
                        .child(toast.message.clone()),
                )
            })
    }
}

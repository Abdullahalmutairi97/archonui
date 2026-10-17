//! A gallery of the shared widgets in the Blood Moon theme. [`open_demo`] opens it in its own window.
//! Not wired into `main.rs`: call it from a debug entry point when you want to look at the widgets.

use std::collections::HashMap;

use gpui::{div, prelude::*, App, Context, Entity, IntoElement, Render, Subscription, Window, WindowOptions};
use crate::ui::scale::px;

use super::badge::{AttachmentCard, CardVariant};
use super::button::{Button, ButtonStyle};
use super::kbd::Kbd;
use super::popover::{Placement, Popover};
use super::section::{Section, SettingRow};
use super::segmented::Segmented;
use super::slider::{slider, Slider, SliderRange};
use super::text_input::{init as init_text_input, text_input, TextInput, TextInputConfig, TextInputEvent};
use super::toast::Toasts;
use super::toggle::Toggle;
use crate::ui::fonts::FontChoice;
use crate::ui::theme::{resolve, Theme};

/// Opens the widget gallery in a new window, in the Blood Moon theme. Binds the text input keys first.
pub fn open_demo(cx: &mut App) {
    init_text_input(cx);
    let theme = resolve(crate::ui::theme::preset(crate::ui::theme::DEFAULT_THEME), &HashMap::new(), FontChoice::default());
    cx.open_window(WindowOptions::default(), move |window, cx| {
        cx.new(|cx| DemoView::new(theme, window, cx))
    })
    .unwrap_or_else(|e| crate::ui::shell::window_failed(e));
}

struct DemoView {
    theme: Theme,
    toasts: Entity<Toasts>,
    single: Entity<TextInput>,
    multi: Entity<TextInput>,
    steps: Entity<Slider>,
    toggle_on: bool,
    mode: usize,
    top_menu_open: bool,
    popover_open: bool,
    _subscriptions: Vec<Subscription>,
}

impl DemoView {
    fn new(theme: Theme, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let toasts = cx.new(|_| Toasts::new(&theme));
        let single = cx.new(|cx| {
            TextInput::new(
                TextInputConfig { placeholder: "Search sessions".into(), max_len: Some(80), ..Default::default() },
                window,
                cx,
            )
        });
        let multi = cx.new(|cx| {
            TextInput::new(
                TextInputConfig {
                    multiline: true,
                    min_lines: 2,
                    max_lines: Some(6),
                    placeholder: "Message the agent".into(),
                    font_size: px(13.),
                    line_height: px(20.),
                    pad_top: px(8.),
                    pad_bottom: px(8.),
                    pad_x: px(10.),
                    ..Default::default()
                },
                window,
                cx,
            )
        });
        let steps = {
            let toasts = toasts.clone();
            cx.new(|_| {
                Slider::new(
                    40.,
                    SliderRange { min: 10., max: 100., step: 5. },
                    "",
                    move |value, _window, cx| {
                        toasts.update(cx, |toasts, cx| toasts.show(format!("Max steps: {value}"), cx));
                    },
                )
            })
        };
        let submit_toasts = toasts.clone();
        let subscriptions = vec![cx.subscribe(&single, move |_, input, event: &TextInputEvent, cx| {
            if *event == TextInputEvent::Submit {
                let text = input.read(cx).text();
                submit_toasts.update(cx, |toasts, cx| toasts.show(format!("Submitted: {text}"), cx));
            }
        })];
        Self {
            theme,
            toasts,
            single,
            multi,
            steps,
            toggle_on: true,
            mode: 0,
            top_menu_open: false,
            popover_open: false,
            _subscriptions: subscriptions,
        }
    }
}

impl Render for DemoView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = &self.theme;
        let view = cx.weak_entity();
        let toasts = self.toasts.clone();

        let buttons = div()
            .flex()
            .flex_wrap()
            .gap(px(8.))
            .child(Button::new("outline", "Export", theme))
            .child({
                let toasts = toasts.clone();
                Button::new("filled", "Save", theme)
                    .style(ButtonStyle::Filled)
                    .on_click(move |_, cx| toasts.update(cx, |t, cx| t.show("Saved", cx)))
            })
            .child({
                let toasts = toasts.clone();
                Button::new("danger", "Clear history", theme)
                    .style(ButtonStyle::Danger)
                    .on_click(move |_, cx| toasts.update(cx, |t, cx| t.show("History cleared", cx)))
            })
            .child(Button::new("ghost", "Reset", theme).style(ButtonStyle::Ghost))
            .child(Button::icon("icon", "⋯", theme))
            .child(Button::new("disabled", "Export all", theme).disabled("Nothing to export yet"));

        let toggle_view = view.clone();
        let mode_view = view.clone();
        let controls = Section::new("CONTROLS", theme)
            .child(SettingRow::new("Restore sessions on launch", "Reopen the tiles you had open.", theme).control(
                Toggle::new("restore", self.toggle_on, theme).on_change(move |next, _, cx| {
                    toggle_view.update(cx, |this, cx| {
                        this.toggle_on = next;
                        cx.notify();
                    })
                    .ok();
                }),
            ))
            .child(SettingRow::new("Default mode", "Auto edits and runs commands.", theme).control(
                Segmented::new("mode", ["Auto", "Plan", "Ask"], self.mode, theme).on_select(move |index, _, cx| {
                    mode_view.update(cx, |this, cx| {
                        this.mode = index;
                        cx.notify();
                    })
                    .ok();
                }),
            ))
            .child(SettingRow::new("Max steps per turn", "The agent stops after this many tool calls.", theme)
                .control(slider(&self.steps, theme)))
            .child(SettingRow::new("Send", "Shortcut shown in the composer.", theme).control(Kbd::new("⌘ ↵", theme)));

        let input = Section::new("INPUT", theme)
            .child(div().py(px(10.)).child(text_input(&self.single, theme)))
            .child(
                div()
                    .py(px(10.))
                    .rounded(px(8.))
                    .border_1()
                    .border_color(theme.line2.to_hsla())
                    .child(text_input(&self.multi, theme)),
            );

        let popover_view = view.clone();
        let list = || ["Session one", "Session two", "Session three"].into_iter().fold(
            div().flex().flex_col().gap(px(2.)),
            |list, name| {
                list.child(div().px(px(8.)).py(px(6.)).rounded(px(6.)).text_size(px(12.5)).child(name))
            },
        );
        let attachments = Section::new("ATTACHMENTS", theme)
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap(px(8.))
                    .py(px(10.))
                    .child(AttachmentCard::new("att-rs", "src/server/main.rs", "src/server · 23 lines", CardVariant::Composer, theme).on_remove(|_, _| {}))
                    .child(AttachmentCard::new("att-png", "bonfire-frame-240.png", "Image · 1920×1080", CardVariant::Composer, theme).on_remove(|_, _| {}))
                    .child(AttachmentCard::new("att-pdf", "docs/spec.pdf", "PDF · 4 pages", CardVariant::Composer, theme).on_remove(|_, _| {})),
            )
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap(px(6.))
                    .py(px(10.))
                    .child(AttachmentCard::new("sent-browser", "browser.png", "Browser · 1440×900", CardVariant::Sent, theme))
                    .child(AttachmentCard::new("sent-log", "terminal.log", "Terminal · 42 lines", CardVariant::Sent, theme)),
            );

        // Near the top of the window, opening downwards like a toolbar menu.
        let top_view = view.clone();
        let top_menu = Popover::new(Button::new("top-trigger", "Open menu", theme), theme)
            .placement(Placement::Below)
            .open(self.top_menu_open)
            .content(list())
            .on_open_change(move |next, _, cx| {
                top_view
                    .update(cx, |this, cx| {
                        this.top_menu_open = next;
                        cx.notify();
                    })
                    .ok();
            });

        // Near the bottom, opening upwards like the composer's model picker.
        let menu = Popover::new(Button::new("popover-trigger", "Open list", theme), theme)
            .placement(Placement::Above)
            .open(self.popover_open)
            .content(list())
            .on_open_change(move |next, _, cx| {
                popover_view
                    .update(cx, |this, cx| {
                        this.popover_open = next;
                        cx.notify();
                    })
                    .ok();
            });

        div()
            .relative()
            .size_full()
            .bg(theme.bg.to_hsla())
            .text_color(theme.fg.to_hsla())
            .font_family(theme.ui_font.clone())
            .p(px(24.))
            .flex()
            .flex_col()
            .gap(px(12.))
            .child(top_menu)
            .child(buttons)
            .child(controls)
            .child(input)
            .child(attachments)
            .child(menu)
            .child(toasts)
    }
}

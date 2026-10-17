//! The Extensions page: search, installed and browsable plugins, each marked with its agent. Spec: settings.md §9.

use super::controls::{edit, heading, listen};
use crate::{
    engine::{self, plugins::Extension},
    ui::{
        shell::Shell,
        theme::Theme,
        widgets::{
            agents::agent_chips,
            button::{Button, ButtonStyle},
            text_input::text_input,
            toggle::Toggle,
        },
    },
};
use gpui::{div, prelude::*, AnyElement, Context, SharedString};
use crate::ui::scale::px;

/// Whether `ext` matches the search text (already trimmed and lowercased). An empty query matches everything.
fn matches(ext: &Extension, query: &str) -> bool {
    query.is_empty() || format!("{} {} {}", ext.name, ext.description, ext.author).to_lowercase().contains(query)
}

/// The two-letter sigil for an extension, from its name.
fn sigil(name: &str) -> String {
    name.chars().take(2).collect()
}

impl Shell {
    pub(super) fn extensions_page(&mut self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let theme = self.theme.clone();
        let weak = cx.entity().downgrade();
        let query = self
            .settings
            .search
            .as_ref()
            .map(|(input, _)| input.read(cx).text().trim().to_lowercase())
            .unwrap_or_default();
        let search_input = self.settings.search.as_ref().map(|(input, _)| input.clone());

        let search = div()
            .flex()
            .items_center()
            .gap(px(8.))
            .px(px(12.))
            .py(px(9.))
            .rounded(px(8.))
            .border_1()
            .border_color(theme.line2.to_hsla())
            .child(div().text_size(px(14.)).text_color(theme.muted.to_hsla()).child("⌕"))
            .when_some(search_input, |d, input| d.child(div().flex_1().min_w(px(0.)).child(text_input(&input, &theme))));

        let Some((installed, available)) = self.settings.plugins.clone() else {
            return vec![
                search.into_any_element(),
                div().text_size(px(12.5)).text_color(theme.muted.to_hsla()).child("Loading extensions…").into_any_element(),
            ];
        };

        let mut list = div()
            .flex()
            .flex_col()
            .rounded(px(10.))
            .border_1()
            .border_color(theme.line2.to_hsla())
            .overflow_hidden();
        let shown: Vec<&Extension> = installed.iter().filter(|e| matches(e, &query)).collect();
        if shown.is_empty() {
            list = list.child(
                div()
                    .px(px(16.))
                    .py(px(14.))
                    .text_size(px(12.5))
                    .text_color(theme.muted.to_hsla())
                    .child("No installed extensions match."),
            );
        }
        for ext in shown {
            let (id, name) = (ext.id.clone(), ext.name.clone());
            // A switch its CLI can't honour would only toast an error, so those extensions just get Remove.
            let toggle = engine::plugins::can_toggle(&ext.harness).then(|| {
                Toggle::new(SharedString::from(format!("ext-{}", ext.id)), ext.enabled, &theme).on_change(edit(
                    &weak,
                    move |this: &mut Shell, on: bool, cx| {
                        let id = id.clone();
                        let done = format!("{} {name}", if on { "Enabled" } else { "Disabled" });
                        this.change_extension(done, move || engine::plugins::set_enabled(&id, on), cx);
                    },
                ))
            });
            list = list.child(extension_row(ext, &theme, toggle, cx));
        }

        let mut browse = div().grid().grid_cols(super::columns(self.settings.body_w, 220., 12., 3)).gap(px(12.));
        let candidates: Vec<&Extension> = available.iter().filter(|e| !e.installed && matches(e, &query)).collect();
        if candidates.is_empty() {
            browse = browse.col_span_full().child(
                div().text_size(px(12.5)).text_color(theme.muted.to_hsla()).child("Nothing else to install."),
            );
        }
        for ext in candidates {
            browse = browse.child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(10.))
                    .p(px(16.))
                    .rounded(px(10.))
                    .border_1()
                    .border_color(theme.line2.to_hsla())
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(12.))
                            .child(sigil_tile(ext, &theme, theme.muted.to_hsla()))
                            .child(name_meta(ext, &theme)),
                    )
                    .child(
                        div()
                            .flex_1()
                            .text_size(px(12.5))
                            .text_color(theme.muted.to_hsla())
                            .child(ext.description.clone()),
                    )
                    .child(
                        // The prototype's BROWSE button is inline and auto-width, not stretched across the card.
                        div().flex().child(
                            Button::new(SharedString::from(format!("install-{}", ext.id)), "Install", &theme)
                                .style(ButtonStyle::Filled)
                                .on_click(listen(cx, {
                                    let (id, name) = (ext.id.clone(), ext.name.clone());
                                    move |this, cx| {
                                        let id = id.clone();
                                        this.change_extension(format!("Installed {name}"), move || engine::plugins::install(&id), cx);
                                    }
                                })),
                        ),
                    ),
            );
        }

        vec![
            search.into_any_element(),
            div()
                .flex()
                .flex_col()
                .gap(px(12.))
                .child(heading("INSTALLED", None, &theme))
                .child(list)
                .into_any_element(),
            div()
                .flex()
                .flex_col()
                .gap(px(12.))
                .child(heading("BROWSE", None, &theme))
                .child(browse)
                .into_any_element(),
        ]
    }

    /// Runs one extension change off the UI thread, toasts the outcome, then reloads the extension lists.
    fn change_extension(&mut self, done: String, change: impl FnOnce() -> Result<(), String> + Send + 'static, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            let result = cx.background_executor().spawn(async move { change() }).await;
            let _ = this.update(cx, |this, cx| {
                match result {
                    Ok(()) => this.toast(done, cx),
                    Err(e) => this.toast(e, cx),
                }
                this.load_catalogues(cx);
            });
        })
        .detach();
    }
}

/// The square sigil tile for an extension.
fn sigil_tile(ext: &Extension, theme: &Theme, colour: gpui::Hsla) -> AnyElement {
    div()
        .w(px(34.))
        .h(px(34.))
        .rounded(px(8.))
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .bg(theme.line2.to_hsla())
        .text_size(px(12.5))
        .font_weight(gpui::FontWeight::BOLD)
        .text_color(colour)
        .child(sigil(&ext.name))
        .into_any_element()
}

/// The name with its author and version beneath.
fn name_meta(ext: &Extension, theme: &Theme) -> AnyElement {
    let credit = [ext.author.as_str(), ext.version.as_str()].into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join(" · ");
    div()
        .flex()
        .flex_col()
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(8.))
                .child(div().text_size(px(14.)).child(ext.name.clone()))
                // An extension belongs to one agent: its own CLI installs and loads it.
                .child(agent_chips(SharedString::from(format!("ext-agent-{}", ext.id)), &[ext.harness.as_str()], |_| true, theme)),
        )
        .when(!credit.is_empty(), |d| {
            d.child(div().text_size(px(11.5)).text_color(theme.muted.to_hsla()).child(credit))
        })
        .into_any_element()
}

/// An installed extension: sigil, name, description, Remove, and its toggle when it has one.
fn extension_row(ext: &Extension, theme: &Theme, toggle: Option<Toggle>, cx: &mut Context<Shell>) -> AnyElement {
    let (id, name) = (ext.id.clone(), ext.name.clone());
    div()
        .flex()
        .items_center()
        .gap(px(14.))
        .px(px(16.))
        .py(px(13.))
        .border_b_1()
        .border_color(theme.line.to_hsla())
        .child(sigil_tile(ext, theme, theme.acc2.to_hsla()))
        .child(
            div()
                .flex_1()
                .min_w(px(0.))
                .flex()
                .flex_col()
                .child(name_meta(ext, theme))
                .child(div().text_size(px(12.)).text_color(theme.muted.to_hsla()).child(ext.description.clone())),
        )
        .child(
            div()
                .id(SharedString::from(format!("remove-{}", ext.id)))
                .px(px(10.))
                .py(px(6.))
                .rounded(px(6.))
                .text_size(px(12.))
                .text_color(theme.muted.to_hsla())
                .cursor_pointer()
                .hover(|s| s.bg(theme.line2.to_hsla()).text_color(theme.fg.to_hsla()))
                .on_click(cx.listener(move |this, _, _, cx| {
                    let id = id.clone();
                    this.change_extension(format!("Removed {name}"), move || engine::plugins::remove(&id), cx);
                }))
                .child("Remove"),
        )
        // Without a switch, an empty slot of the switch's width keeps Remove lined up with the other rows.
        .child(match toggle {
            Some(toggle) => toggle.into_any_element(),
            None => div().w(px(34.)).flex_none().into_any_element(),
        })
        .into_any_element()
}

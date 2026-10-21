//! The Extensions page: search, installed and browsable plugins, each marked with its agent. Spec: settings.md §9.

use super::controls::{chips, edit, empty_box, list_row, listen, more_row, row_button, search_box};
use crate::{
    engine::{self, plugins::Extension},
    ui::{
        shell::Shell,
        widgets::{
            section::{card, section_head},
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

impl Shell {
    pub(super) fn extensions_page(&mut self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let theme = self.theme.clone();
        let weak = cx.entity().downgrade();
        let query = self.query("ext", cx);
        let chip = self.settings.chip.get("ext").copied().unwrap_or(0).min(2);
        let catalogue = self.settings.plugins.clone();
        let (installed, available) = catalogue.clone().unwrap_or_default();
        let mut all: Vec<(Extension, bool)> = installed.into_iter().map(|e| (e, true)).collect();
        all.extend(available.into_iter().filter(|e| !e.installed).map(|e| (e, false)));
        let n_installed = all.iter().filter(|(_, i)| *i).count();
        let shown: Vec<(Extension, bool)> = all
            .into_iter()
            .filter(|(_, inst)| match chip {
                1 => *inst,
                2 => !*inst,
                _ => true,
            })
            .filter(|(e, _)| matches(e, &query))
            .collect();
        let title = if catalogue.is_some() { format!("Extensions · {}", shown.len()) } else { "Extensions".to_string() };
        let head = section_head(title, catalogue.is_some().then(|| format!("{n_installed} installed").into()), Vec::new(), &theme);
        let search = self.settings.searches.get("ext").map(|(input, _)| search_box(input, &theme));
        let names = ["All", "Installed", "Available"].map(String::from);
        let chips = chips("ext-chip", &names, chip, &theme, edit(&weak, |this: &mut Shell, i: usize, cx| this.pick_chip("ext", i, cx)));
        let limit = self.limit("ext", 8);
        let left = shown.len().saturating_sub(limit);
        let body = if catalogue.is_none() {
            empty_box(None, "Looking for extensions…", &theme)
        } else if shown.is_empty() {
            empty_box(None, if query.is_empty() && chip == 0 { "No extensions yet" } else { "No matching extensions" }, &theme)
        } else {
            let mut rows = Vec::new();
            for (ext, inst) in shown.iter().take(limit) {
                let agent = engine::harness(&ext.harness).map_or(ext.harness.as_str(), |h| h.name());
                let note = if ext.description.is_empty() { agent.to_string() } else { format!("{agent} · {}", ext.description) };
                let mut end = Vec::new();
                let (id, name) = (ext.id.clone(), ext.name.clone());
                if *inst {
                    end.push(row_button(SharedString::from(format!("remove-{id}")), "Remove", &theme, listen(cx, {
                        let (id, name) = (id.clone(), name.clone());
                        move |this, cx| {
                            let id = id.clone();
                            this.change_extension(format!("Removed {name}"), move || engine::plugins::remove(&id), cx);
                        }
                    })));
                    // A switch its CLI can't honour would only toast an error, so those just get Remove.
                    if engine::plugins::can_toggle(&ext.harness) {
                        end.push(
                            Toggle::new(SharedString::from(format!("ext-{id}")), ext.enabled, &theme)
                                .on_change(edit(&weak, move |this: &mut Shell, on: bool, cx| {
                                    let id = id.clone();
                                    let done = format!("{} {name}", if on { "Enabled" } else { "Disabled" });
                                    this.change_extension(done, move || engine::plugins::set_enabled(&id, on), cx);
                                }))
                                .into_any_element(),
                        );
                    } else {
                        end.push(div().w(px(34.)).flex_none().into_any_element());
                    }
                } else {
                    end.push(row_button(SharedString::from(format!("install-{id}")), "Install", &theme, listen(cx, move |this, cx| {
                        let id = id.clone();
                        this.change_extension(format!("Installed {name}"), move || engine::plugins::install(&id), cx);
                    })));
                }
                rows.push(list_row(ext.name.clone(), Some(note.into()), end, &theme).into_any_element());
            }
            if left > 0 {
                rows.push(more_row("ext-more", 20, left, &theme, listen(cx, |this, cx| this.show_more("ext", 8, 20, cx))));
            }
            card(rows, &theme).into_any_element()
        };
        vec![div().flex().flex_col().child(head).children(search).child(chips).child(body).into_any_element()]
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

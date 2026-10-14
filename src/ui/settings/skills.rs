//! The Skills page: discovered skills, each switched on or off, plus import and create. Spec: settings.md §8.

use super::controls::{edit, heading, listen};
use crate::{
    engine,
    ui::{
        shell::Shell,
        widgets::{
            button::{Button, ButtonStyle},
            toggle::Toggle,
        },
    },
};
use gpui::{div, prelude::*, AnyElement, Context, PathPromptOptions, SharedString};
use crate::ui::scale::px;

impl Shell {
    pub(super) fn skills_page(&mut self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let theme = self.theme.clone();
        let weak = cx.entity().downgrade();
        let skills = self.settings.skills.clone();
        let total = skills.as_ref().map_or(0, Vec::len);
        let on = skills
            .iter()
            .flatten()
            .filter(|k| self.state.settings.skills_enabled.get(&k.id).copied().unwrap_or(true))
            .count();

        let header = div()
            .flex()
            .items_center()
            .gap(px(8.))
            .child(heading(&format!("SKILLS · {on} OF {total} ON"), None, &theme))
            .child(div().flex_1())
            .child(
                Button::new("import-skill", "Import folder", &theme)
                    .style(ButtonStyle::Outline)
                    .on_click(listen(cx, |this, cx| this.import_skill(cx))),
            )
            .child(
                Button::new("new-skill", "+ New skill", &theme)
                    .style(ButtonStyle::Filled)
                    .on_click(listen(cx, |this, cx| this.new_skill(cx))),
            );

        let body = match skills {
            None => div().text_size(px(12.5)).text_color(theme.muted.to_hsla()).child("Loading skills…").into_any_element(),
            Some(list) if list.is_empty() => div()
                .text_size(px(12.5))
                .text_color(theme.muted.to_hsla())
                .child("No skills found. Import a folder that has a SKILL.md, or create one.")
                .into_any_element(),
            Some(list) => {
                let mut grid = div().grid().grid_cols(2).gap(px(12.));
                for skill in list {
                    let enabled = self.state.settings.skills_enabled.get(&skill.id).copied().unwrap_or(true);
                    let id = skill.id.clone();
                    let toggle = Toggle::new(SharedString::from(format!("skill-{}", skill.id)), enabled, &theme).on_change(edit(
                        &weak,
                        move |this: &mut Shell, on: bool, cx| {
                            this.state.settings.skills_enabled.insert(id.clone(), on);
                            this.save(cx);
                        },
                    ));
                    let dim = if enabled { 1.0 } else { 0.55 };
                    grid = grid.child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(8.))
                            .p(px(16.))
                            .rounded(px(10.))
                            .border_1()
                            .border_color(if enabled { theme.line2.to_hsla() } else { theme.line.to_hsla() })
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(8.))
                                    .child(
                                        div()
                                            .flex_1()
                                            .text_size(px(14.))
                                            .opacity(dim)
                                            .child(skill.name.clone()),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(11.))
                                            .px(px(8.))
                                            .py(px(3.))
                                            .rounded(px(5.))
                                            .bg(theme.line.to_hsla())
                                            .text_color(theme.muted.to_hsla())
                                            .child(skill.source.clone()),
                                    )
                                    .child(toggle),
                            )
                            .child(
                                div()
                                    .text_size(px(12.5))
                                    .text_color(theme.muted.to_hsla())
                                    .opacity(dim)
                                    .child(skill.description.clone()),
                            )
                            .child(div().text_size(px(12.)).text_color(theme.acc2.to_hsla()).opacity(dim).child(skill.command.clone())),
                    );
                }
                grid.into_any_element()
            }
        };

        vec![
            div().flex().flex_col().gap(px(12.)).child(header).child(body).into_any_element(),
        ]
    }

    /// Copies a folder that holds a SKILL.md into the skills dir, then rediscovers skills.
    fn import_skill(&mut self, cx: &mut Context<Self>) {
        let picked = cx.prompt_for_paths(PathPromptOptions { files: false, directories: true, multiple: false, prompt: None });
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(paths))) = picked.await else { return };
            let Some(folder) = paths.into_iter().next() else { return };
            let imported = cx.background_executor().spawn(async move { engine::skills::import(&folder) }).await;
            let _ = this.update(cx, |this, cx| {
                match imported {
                    Ok(path) => this.toast(format!("Imported {}", path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()), cx),
                    Err(e) => this.toast(format!("Import failed: {e}"), cx),
                }
                this.reload_skills(cx);
            });
        })
        .detach();
    }

    /// Creates a skill folder with a starter SKILL.md, then rediscovers skills.
    fn new_skill(&mut self, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            let created = cx.background_executor().spawn(async move { engine::skills::create("new-skill") }).await;
            let _ = this.update(cx, |this, cx| {
                match created {
                    Ok(path) => this.toast(format!("Created {}", path.display()), cx),
                    Err(e) => this.toast(format!("Could not create skill: {e}"), cx),
                }
                this.reload_skills(cx);
            });
        })
        .detach();
    }

    /// Searches for skills again off the UI thread.
    fn reload_skills(&mut self, cx: &mut Context<Self>) {
        let cwd = self.default_folder();
        cx.spawn(async move |this, cx| {
            let skills = cx.background_executor().spawn(async move { engine::skills::discover(&cwd) }).await;
            let _ = this.update(cx, |this, cx| {
                this.settings.skills = Some(skills);
                cx.notify();
            });
        })
        .detach();
    }
}

//! The Skills page: discovered skills, each switched on or off and marked with the agents that load
//! it, plus sync, import and create. Spec: settings.md §8.

use super::controls::{edit, heading, listen};
use crate::{
    engine,
    ui::{
        shell::Shell,
        widgets::{
            agents::{agent_chips, short_name},
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

        // Agents turned off in settings get no chip and no count.
        let installed: Vec<&'static str> = self.agents.iter().filter(|a| a.enabled).map(|a| a.harness.id()).collect();
        // How many skills each installed agent loads, so a gap between agents shows before syncing. Nothing
        // is counted while the search runs, rather than a row of misleading zeros.
        let counts = skills.as_ref().map(|list| {
            installed
                .iter()
                .map(|agent| format!("{} {}", short_name(agent), list.iter().filter(|k| k.agents.contains(agent)).count()))
                .collect::<Vec<_>>()
                .join(" · ")
        });
        let title = if skills.is_some() { format!("SKILLS · {on} OF {total} ON") } else { "SKILLS".to_string() };

        // The buttons wrap under the title and counts when the page is narrow.
        let header = div()
            .flex()
            .flex_wrap()
            .items_center()
            .gap(px(8.))
            .child(heading(&title, None, &theme))
            .when_some(counts, |d, counts| d.child(div().text_size(px(12.)).text_color(theme.muted.to_hsla()).child(counts)))
            .child(div().flex_1())
            .child(
                Button::new("sync-skills", "Sync to all agents", &theme)
                    .style(ButtonStyle::Outline)
                    .on_click(listen(cx, |this, cx| this.sync_skills(cx))),
            )
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
                .child("No skills found.")
                .into_any_element(),
            Some(list) => {
                let mut grid = div().grid().grid_cols(super::columns(self.settings.body_w, 300., 12., 2)).gap(px(12.));
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
                            .min_w(px(0.))
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
                                            .min_w(px(0.))
                                            .overflow_hidden()
                                            .text_ellipsis()
                                            .whitespace_nowrap()
                                            .text_size(px(14.))
                                            .opacity(dim)
                                            .child(skill.name.clone()),
                                    )
                                    .child(
                                        div()
                                            .flex_none()
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
                            .child(div().text_size(px(12.)).text_color(theme.acc2.to_hsla()).opacity(dim).child(skill.command.clone()))
                            .child(agent_chips(
                                SharedString::from(format!("skill-agents-{}", skill.id)),
                                &installed,
                                |agent| skill.agents.contains(&agent),
                                &theme,
                            )),
                    );
                }
                grid.into_any_element()
            }
        };

        vec![
            div().flex().flex_col().gap(px(12.)).child(header).child(body).into_any_element(),
        ]
    }

    /// Links every skill into each agent's skills folder, toasts what changed, then rediscovers skills.
    /// Skills turned off here are left out, and so are the folders of agents turned off.
    fn sync_skills(&mut self, cx: &mut Context<Self>) {
        let disabled: Vec<String> =
            self.state.settings.skills_enabled.iter().filter(|(_, on)| !**on).map(|(id, _)| id.clone()).collect();
        let agents: Vec<&'static str> = self.agents.iter().filter(|a| a.enabled).map(|a| a.harness.id()).collect();
        cx.spawn(async move |this, cx| {
            let synced = cx.background_executor().spawn(async move { engine::skills::sync(&disabled, &agents) }).await;
            let _ = this.update(cx, |this, cx| {
                match synced {
                    Ok(report) => this.toast(sync_summary(&report), cx),
                    Err(e) => this.toast(format!("Sync failed: {e}"), cx),
                }
                this.reload_skills(cx);
            });
        })
        .detach();
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

    /// Searches for skills again off the UI thread. Only the latest search's result is kept.
    pub(super) fn reload_skills(&mut self, cx: &mut Context<Self>) {
        let cwd = self.default_folder();
        self.settings.skills_search += 1;
        let search = self.settings.skills_search;
        cx.spawn(async move |this, cx| {
            let skills = cx.background_executor().spawn(async move { engine::skills::discover(&cwd) }).await;
            let _ = this.update(cx, |this, cx| {
                if this.settings.skills_search == search {
                    this.settings.skills = Some(skills);
                    cx.notify();
                }
            });
        })
        .detach();
    }
}

/// One line for the toast after a sync: what was linked, skipped and removed. Skills kept as the
/// user's own copies, or turned off, are the steady state: a sync with only those changed nothing.
fn sync_summary(report: &engine::skills::SyncReport) -> String {
    let (linked, skipped, pruned) = (report.linked.len(), report.skipped.len(), report.pruned.len());
    let plural = |n: usize, word: &str| format!("{n} {word}{}", if n == 1 { "" } else { "s" });
    let mut steady = Vec::new();
    if !report.kept.is_empty() {
        steady.push(format!("{} kept as your own copies", report.kept.len()));
    }
    if !report.off.is_empty() {
        steady.push(format!("{} turned off", report.off.len()));
    }
    let steady = if steady.is_empty() { String::new() } else { format!(" ({})", steady.join(", ")) };
    if linked + skipped + pruned == 0 {
        return format!("Every agent already has every skill{steady}.");
    }
    let mut parts = vec![format!("Linked {}", plural(linked, "skill"))];
    if skipped > 0 {
        // Name the first skip, since its reason is what the user can act on.
        parts.push(format!("skipped {skipped} ({})", report.skipped[0]));
    }
    if pruned > 0 {
        parts.push(format!("removed {}", plural(pruned, "broken link")));
    }
    parts.join(", ") + &steady
}

#[cfg(test)]
mod tests {
    use super::sync_summary;
    use crate::engine::skills::SyncReport;

    #[test]
    fn sync_summary_reads_short() {
        assert_eq!(sync_summary(&SyncReport::default()), "Every agent already has every skill.");
        let report = SyncReport {
            linked: vec!["a → x".into(), "b → y".into()],
            skipped: vec!["pdf: taken".into()],
            pruned: vec!["~/.claude/skills/dead".into()],
            ..SyncReport::default()
        };
        assert_eq!(sync_summary(&report), "Linked 2 skills, skipped 1 (pdf: taken), removed 1 broken link");
    }

    #[test]
    fn a_sync_with_only_kept_copies_reads_as_nothing_to_do() {
        let report = SyncReport {
            kept: vec!["canvas-design (Claude.ai): ~/.agents/skills already has a skill with this name".into(); 4],
            off: vec!["fmt (Personal): turned off in settings".into()],
            ..SyncReport::default()
        };
        assert_eq!(sync_summary(&report), "Every agent already has every skill (4 kept as your own copies, 1 turned off).");
        let report = SyncReport { linked: vec!["a → x".into()], kept: vec!["k".into()], ..SyncReport::default() };
        assert_eq!(sync_summary(&report), "Linked 1 skill (1 kept as your own copies)");
    }
}

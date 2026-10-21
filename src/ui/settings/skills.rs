//! The Skills page: discovered skills, each switched on or off and marked with the agents that load
//! it, plus sync, import and create. Spec: settings.md §8.

use super::controls::{act_button, chips, edit, empty_box, icon, list_row, listen, more_row, search_box};
use crate::{
    engine,
    ui::{
        shell::Shell,
        widgets::{
            section::{card, section_head},
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
        // Agents turned off in settings get no chip.
        let installed: Vec<&'static str> = self.agents.iter().filter(|a| a.enabled).map(|a| a.harness.id()).collect();
        let mut names = vec!["All".to_string()];
        names.extend(installed.iter().map(|id| engine::harness(id).map_or(*id, |h| h.name()).to_string()));
        let chip = self.settings.chip.get("skills").copied().unwrap_or(0).min(installed.len());
        let only = chip.checked_sub(1).and_then(|i| installed.get(i)).copied();
        let query = self.query("skills", cx);
        let list = self.settings.skills.clone();
        let shown: Vec<_> = list
            .iter()
            .flatten()
            .filter(|k| only.is_none_or(|a| k.agents.contains(&a)))
            .filter(|k| query.is_empty() || format!("{} {}", k.name, k.description).to_lowercase().contains(&query))
            .cloned()
            .collect();
        let enabled = |id: &str| self.state.settings.skills_enabled.get(id).copied().unwrap_or(true);
        let on = shown.iter().filter(|k| enabled(&k.id)).count();
        let ids: Vec<String> = shown.iter().map(|k| k.id.clone()).collect();
        let (all_on, all_off) = (ids.clone(), ids);
        let acts = vec![
            act_button("skills-enable-all", "Enable all", None, &theme, listen(cx, move |this, cx| this.bulk_skills(&all_on, true, cx))),
            act_button("skills-disable-all", "Disable all", None, &theme, listen(cx, move |this, cx| this.bulk_skills(&all_off, false, cx))),
        ];
        let title = if list.is_some() { format!("Skills · {}", shown.len()) } else { "Skills".to_string() };
        let head = section_head(title, list.is_some().then(|| format!("{on} on").into()), acts, &theme);
        let search = self.settings.searches.get("skills").map(|(input, _)| search_box(input, &theme));
        let chips = chips("skill-chip", &names, chip, &theme, edit(&weak, |this: &mut Shell, i: usize, cx| this.pick_chip("skills", i, cx)));
        let limit = self.limit("skills", 8);
        let left = shown.len().saturating_sub(limit);
        let body = match &list {
            None => empty_box(None, "Looking for skills…", &theme),
            Some(_) if shown.is_empty() => empty_box(None, if query.is_empty() { "No skills yet" } else { "No matching skills" }, &theme),
            Some(_) => {
                let mut rows = Vec::new();
                for skill in shown.iter().take(limit) {
                    let on = enabled(&skill.id);
                    let id = skill.id.clone();
                    let toggle = Toggle::new(SharedString::from(format!("skill-{}", skill.id)), on, &theme).on_change(edit(
                        &weak,
                        move |this: &mut Shell, on: bool, cx| {
                            this.state.settings.skills_enabled.insert(id.clone(), on);
                            this.save(cx);
                        },
                    ));
                    let agents: Vec<&str> = skill.agents.iter().map(|a| engine::harness(a).map_or(*a, |h| h.name())).collect();
                    let note = if agents.is_empty() { skill.source.clone() } else { agents.join(", ") };
                    rows.push(list_row(skill.name.clone(), Some(note.into()), vec![toggle.into_any_element()], &theme).into_any_element());
                }
                if left > 0 {
                    rows.push(more_row("skills-more", 25, left, &theme, listen(cx, |this, cx| this.show_more("skills", 8, 25, cx))));
                }
                card(rows, &theme).into_any_element()
            }
        };
        let foot = div()
            .flex()
            .flex_wrap()
            .gap(px(8.))
            .pt(px(12.))
            .child(act_button("new-skill", "New skill", Some(icon::PLUS), &theme, listen(cx, |this, cx| this.new_skill(cx))))
            .child(act_button("import-skill", "Import folder", None, &theme, listen(cx, |this, cx| this.import_skill(cx))))
            .child(act_button("sync-skills", "Sync to all agents", None, &theme, listen(cx, |this, cx| this.sync_skills(cx))));
        vec![div().flex().flex_col().child(head).children(search).child(chips).child(body).child(foot).into_any_element()]
    }

    /// Turns every skill in `ids` on or off.
    fn bulk_skills(&mut self, ids: &[String], on: bool, cx: &mut Context<Self>) {
        for id in ids {
            self.state.settings.skills_enabled.insert(id.clone(), on);
        }
        self.save(cx);
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

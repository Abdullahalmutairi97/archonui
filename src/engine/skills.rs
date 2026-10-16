//! Skills: folders with a SKILL.md, found on disk and passed to harnesses that support them.
#![allow(dead_code)]

use std::{
    collections::{HashMap, HashSet},
    env, fs, io,
    path::{Path, PathBuf},
};

/// The harnesses that read skill folders, in the order `Skill::agents` lists them.
pub const AGENTS: [&str; 5] = ["claude", "codex", "gemini", "opencode", "pi"];

#[derive(Clone, Debug, PartialEq)]
pub struct Skill {
    /// Stable id: the folder path.
    pub id: String,
    pub name: String,
    pub description: String,
    /// Slash command that runs it, e.g. `/review`.
    pub command: String,
    /// Where it came from: "Personal", "Project", "Claude.ai", a plugin name, a harness.
    pub source: String,
    pub path: PathBuf,
    /// The harnesses (from `AGENTS`) that load this folder, in `AGENTS` order.
    pub agents: Vec<&'static str>,
}

/// The harnesses that read `~/.agents/skills`.
const HUB_READERS: [&str; 4] = ["codex", "gemini", "opencode", "pi"];

/// What `sync` did, one line per change.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SyncReport {
    /// `name → link` for each symlink made.
    pub linked: Vec<String>,
    /// `name: reason` for each skill left unlinked that needs the user: no free folder name for it.
    pub skipped: Vec<String>,
    /// `name: reason` for each skill left unlinked because the agent already loads another skill
    /// with that name, usually the user's own copy. This is the steady state, not a problem.
    pub kept: Vec<String>,
    /// `name: reason` for each skill left unlinked because it is turned off in settings.
    pub off: Vec<String>,
    /// Each dead symlink removed.
    pub pruned: Vec<String>,
}

/// How far under a root a harness looks for SKILL.md folders.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Depth {
    /// Only the folders directly under the root.
    One,
    /// Any folder below the root; a folder with a SKILL.md is not searched further.
    Any,
}

/// A folder that harnesses read skills from.
struct Root {
    path: PathBuf,
    source: String,
    agents: &'static [&'static str],
    depth: Depth,
    /// Roots sharing a group list each folder name once, from the first of them.
    group: Option<String>,
}

impl Root {
    fn new(path: PathBuf, source: &str, agents: &'static [&'static str], depth: Depth) -> Self {
        Self { path, source: source.into(), agents, depth, group: None }
    }
}

/// A skill folder found from the roots, with its fully resolved path.
struct Found {
    skill: Skill,
    canonical: PathBuf,
}

/// Every skill visible from `cwd`: personal, project, harness, Claude.ai and installed plugin
/// skill folders, each with the harnesses that load it. Folders are read in place; a folder
/// without a readable SKILL.md is skipped.
pub fn discover(cwd: &Path) -> Vec<Skill> {
    home().map_or_else(Vec::new, |home| discover_in(&home, cwd))
}

/// `discover` with the home directory given.
fn discover_in(home: &Path, cwd: &Path) -> Vec<Skill> {
    let mut roots = user_roots(home);
    roots.extend(project_roots(cwd));
    scan(&roots).into_iter().map(|f| f.skill).collect()
}

/// The skill roots in the home folder, in source-label order: the first root to reach a folder
/// names its source.
fn user_roots(home: &Path) -> Vec<Root> {
    let hub = home.join(".agents/skills");
    let mut roots = vec![
        Root::new(hub.clone(), "Personal", &["codex", "gemini", "opencode"], Depth::One),
        Root::new(hub, "Personal", &["pi"], Depth::Any),
        Root::new(home.join(".claude/skills"), "Claude Code", &["claude", "opencode"], Depth::One),
    ];
    roots.extend(synced_buckets(&home.join(".claude/skills/synced")).into_iter().map(|bucket| Root {
        group: Some("claude.ai".into()),
        ..Root::new(bucket, "Claude.ai", &["claude"], Depth::One)
    }));
    roots.extend([
        Root::new(home.join(".gemini/skills"), "Gemini", &["gemini"], Depth::One),
        Root::new(home.join(".pi/agent/skills"), "Pi", &["pi"], Depth::Any),
        Root::new(home.join(".codex/skills"), "Codex", &["codex"], Depth::One),
        Root::new(home.join(".config/opencode/skills"), "OpenCode", &["opencode"], Depth::One),
        Root::new(home.join(".config/opencode/skill"), "OpenCode", &["opencode"], Depth::One),
    ]);
    roots.extend(
        extension_skill_dirs(home)
            .into_iter()
            .map(|(path, name)| Root::new(path, &name, &["gemini"], Depth::One)),
    );
    // Several copies of a plugin (versions, accounts) list each skill once, from the newest.
    let mut plugins = plugin_skill_dirs(&home.join(".claude/plugins"), 0);
    plugins.sort_by_cached_key(|(path, _)| (std::cmp::Reverse(modified(path.parent().unwrap_or(path))), path.clone()));
    roots.extend(plugins.into_iter().map(|(path, name)| Root {
        group: Some(format!("plugin:{name}")),
        ..Root::new(path, &name, &["claude"], Depth::One)
    }));
    roots
}

/// The skill roots in the project at `cwd`. Pi also reads `.agents/skills` in each folder above
/// `cwd`, up to the git root, or to `/` outside a repo.
fn project_roots(cwd: &Path) -> Vec<Root> {
    let mut roots = vec![
        Root::new(cwd.join(".claude/skills"), "Project", &["claude", "opencode"], Depth::One),
        Root::new(cwd.join(".agents/skills"), "Project", &["codex", "gemini", "opencode"], Depth::One),
        Root::new(cwd.join(".gemini/skills"), "Project", &["gemini"], Depth::One),
        Root::new(cwd.join(".pi/skills"), "Project", &["pi"], Depth::Any),
        Root::new(cwd.join(".opencode/skills"), "Project", &["opencode"], Depth::One),
        Root::new(cwd.join(".opencode/skill"), "Project", &["opencode"], Depth::One),
    ];
    for dir in cwd.ancestors() {
        roots.push(Root::new(dir.join(".agents/skills"), "Project", &["pi"], Depth::Any));
        if dir.join(".git").exists() {
            break;
        }
    }
    roots
}

/// The claude.ai sync buckets under `synced`, newest first. Dot-named entries are markers.
fn synced_buckets(synced: &Path) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(synced) else { return Vec::new() };
    let mut buckets: Vec<PathBuf> = entries
        .flatten()
        .filter(|e| !e.file_name().to_string_lossy().starts_with('.'))
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .collect();
    buckets.sort_by_cached_key(|p| (std::cmp::Reverse(modified(p)), p.clone()));
    buckets
}

/// The skill folders the roots reach, once per resolved path, sorted by source, name and path.
/// A folder reached from several roots is loaded by all of their harnesses. It is listed from the
/// first root that holds the folder itself, else from the first root that links to it, so linking
/// a skill into another root (as `sync` does) keeps its id, and with it its on/off setting, and its source.
fn scan(roots: &[Root]) -> Vec<Found> {
    let mut out: Vec<Found> = Vec::new();
    let mut index: HashMap<PathBuf, usize> = HashMap::new();
    let mut agents: Vec<HashSet<&'static str>> = Vec::new();
    // Whether each entry was listed from a symlink, and so may still be listed from its real folder.
    let mut via_link: Vec<bool> = Vec::new();
    let mut grouped = HashSet::new();
    for root in roots {
        for dir in walk(&root.path, root.depth) {
            let folder = dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            if root.group.as_ref().is_some_and(|g| !grouped.insert((g.clone(), folder))) {
                continue;
            }
            let canonical = fs::canonicalize(&dir).unwrap_or_else(|_| dir.clone());
            let link = fs::symlink_metadata(&dir).is_ok_and(|m| m.file_type().is_symlink());
            let i = match index.get(&canonical) {
                Some(&i) => {
                    if via_link[i] && !link {
                        if let Some(skill) = parse_skill(&dir, &root.source) {
                            out[i].skill = skill;
                            via_link[i] = false;
                        }
                    }
                    i
                }
                None => {
                    let Some(skill) = parse_skill(&dir, &root.source) else { continue };
                    index.insert(canonical.clone(), out.len());
                    out.push(Found { skill, canonical });
                    agents.push(HashSet::new());
                    via_link.push(link);
                    out.len() - 1
                }
            };
            agents[i].extend(root.agents);
        }
    }
    for (found, agents) in out.iter_mut().zip(&agents) {
        found.skill.agents = AGENTS.into_iter().filter(|a| agents.contains(a) && loads(a, &found.skill)).collect();
    }
    out.sort_by(|a, b| (&a.skill.source, &a.skill.name, &a.skill.path).cmp(&(&b.skill.source, &b.skill.name, &b.skill.path)));
    out
}

/// Whether `agent`, reading a folder that holds `skill`, loads it. Pi skips a skill whose
/// SKILL.md has no description.
fn loads(agent: &str, skill: &Skill) -> bool {
    agent != "pi" || !skill.description.trim().is_empty()
}

/// The folders under `root` that hold a SKILL.md, in path order. Symlinked folders are followed;
/// a folder already searched (a link loop) is not searched again.
fn walk(root: &Path, depth: Depth) -> Vec<PathBuf> {
    let mut out = Vec::new();
    walk_into(root, depth, 0, &mut HashSet::new(), &mut out);
    out
}

fn walk_into(dir: &Path, depth: Depth, level: usize, seen: &mut HashSet<PathBuf>, out: &mut Vec<PathBuf>) {
    const MAX_LEVEL: usize = 8;
    let Ok(entries) = fs::read_dir(dir) else { return };
    let mut dirs: Vec<PathBuf> = entries.flatten().map(|e| e.path()).filter(|p| p.is_dir()).collect();
    dirs.sort();
    for dir in dirs {
        if dir.join("SKILL.md").is_file() {
            out.push(dir);
        } else if depth == Depth::Any
            && level < MAX_LEVEL
            && !skipped_dir(&dir)
            && fs::canonicalize(&dir).is_ok_and(|c| seen.insert(c))
        {
            walk_into(&dir, depth, level + 1, seen, out);
        }
    }
}

/// Folders a recursive search does not enter: hidden ones and dependency checkouts.
fn skipped_dir(dir: &Path) -> bool {
    dir.file_name().is_none_or(|n| n.to_string_lossy().starts_with('.') || n == "node_modules")
}

/// When `path` was last modified, if it can be read.
fn modified(path: &Path) -> Option<std::time::SystemTime> {
    fs::metadata(path).and_then(|m| m.modified()).ok()
}

fn parse_skill(dir: &Path, source: &str) -> Option<Skill> {
    let text = fs::read_to_string(dir.join("SKILL.md")).ok()?;
    let folder = dir.file_name()?.to_string_lossy().into_owned();
    let front = front_matter(&text);
    let name = front.get("name").filter(|n| !n.is_empty()).cloned().unwrap_or(folder);
    Some(Skill {
        id: dir.display().to_string(),
        command: format!("/{name}"),
        name,
        description: front.get("description").cloned().unwrap_or_default(),
        source: source.into(),
        path: dir.to_path_buf(),
        agents: Vec::new(),
    })
}

/// The `key: value` pairs in a SKILL.md's YAML front matter. A key whose value is `>` or `|`, or
/// empty, takes the indented lines after it, joined with spaces. Other YAML is not read.
fn front_matter(text: &str) -> HashMap<String, String> {
    let mut lines = text.strip_prefix('\u{feff}').unwrap_or(text).lines();
    if lines.next().map(str::trim_end) != Some("---") {
        return HashMap::new();
    }
    let mut out = HashMap::new();
    let mut block: Option<String> = None;
    for line in lines {
        if line.trim_end() == "---" {
            break;
        }
        if let (true, Some(key)) = (line.starts_with([' ', '\t']), &block) {
            let value = out.entry(key.clone()).or_insert_with(String::new);
            if !value.is_empty() {
                value.push(' ');
            }
            value.push_str(line.trim());
            continue;
        }
        block = None;
        let Some((key, value)) = line.split_once(':') else { continue };
        let (key, value) = (key.trim(), value.trim());
        // `>-`, `|+` and the like are block scalars with a chomping indicator.
        if matches!(value, ">" | "|" | ">-" | "|-" | ">+" | "|+" | "") {
            out.insert(key.to_string(), String::new());
            block = Some(key.to_string());
        } else {
            out.insert(key.to_string(), unquote(value));
        }
    }
    out
}

/// A one-line YAML scalar's text. A double-quoted value has its escapes (`\"`, `\\`, `\n`, `\t`)
/// read, and a single-quoted one its doubled quotes, so descriptions show what their author wrote.
fn unquote(s: &str) -> String {
    if let Some(inner) = s.strip_prefix('"').and_then(|x| x.strip_suffix('"')) {
        let mut out = String::with_capacity(inner.len());
        let mut chars = inner.chars();
        while let Some(c) = chars.next() {
            if c != '\\' {
                out.push(c);
                continue;
            }
            match chars.next() {
                Some('n') => out.push('\n'),
                Some('t') => out.push('\t'),
                Some(other) => out.push(other),
                None => out.push('\\'),
            }
        }
        return out;
    }
    if let Some(inner) = s.strip_prefix('\'').and_then(|x| x.strip_suffix('\'')) {
        return inner.replace("''", "'");
    }
    s.to_string()
}

/// `skills` folders inside installed Claude plugins, each with its plugin's name. Marketplace
/// checkouts under `marketplaces/` hold plugins that are not installed, so they are skipped.
fn plugin_skill_dirs(dir: &Path, depth: usize) -> Vec<(PathBuf, String)> {
    const MAX_DEPTH: usize = 6;
    let Ok(entries) = fs::read_dir(dir) else { return Vec::new() };
    let mut out = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name();
        if !path.is_dir() || matches!(name.to_str(), Some("marketplaces" | "node_modules" | ".git")) {
            continue;
        }
        if name == "skills" {
            out.push((path, plugin_name(dir)));
        } else if depth < MAX_DEPTH {
            out.extend(plugin_skill_dirs(&path, depth + 1));
        }
    }
    out
}

/// The `name` in a plugin's manifest, else its folder name. `dir` is the folder holding `skills`.
fn plugin_name(dir: &Path) -> String {
    manifest_name(dir, ".claude-plugin/plugin.json")
}

/// `skills` folders inside the Gemini extensions installed in `home` and turned on there, each
/// with its extension's name. Gemini loads no skills from an extension that is off.
fn extension_skill_dirs(home: &Path) -> Vec<(PathBuf, String)> {
    let mut out: Vec<(PathBuf, String)> = gemini_extensions(home)
        .into_iter()
        .filter(|(dir, _, on)| *on && dir.join("skills").is_dir())
        .map(|(dir, name, _)| (dir.join("skills"), name))
        .collect();
    out.sort();
    out
}

/// The Gemini extensions installed in `home`: each folder, its extension's name, and whether it
/// is on in the user scope, that is for the home folder, as `gemini extensions list` run there
/// reports it.
fn gemini_extensions(home: &Path) -> Vec<(PathBuf, String, bool)> {
    let dir = home.join(".gemini/extensions");
    let Ok(entries) = fs::read_dir(&dir) else { return Vec::new() };
    let rules: serde_json::Value = fs::read_to_string(dir.join("extension-enablement.json"))
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default();
    let mut out: Vec<(PathBuf, String, bool)> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .map(|p| {
            let name = manifest_name(&p, "gemini-extension.json");
            let on = gemini_enabled(&rules[name.as_str()]["overrides"], home);
            (p, name, on)
        })
        .collect();
    out.sort();
    out
}

/// Gemini's rule for an extension's `overrides` in `extension-enablement.json`: each is a folder,
/// `!` in front to turn the extension off there, `*` after it to cover the folders below too. The
/// last rule that matches `dir` decides; with none the extension is on.
fn gemini_enabled(overrides: &serde_json::Value, dir: &Path) -> bool {
    let mut path = dir.to_string_lossy().replace('\\', "/");
    if !path.starts_with('/') {
        path.insert(0, '/');
    }
    if !path.ends_with('/') {
        path.push('/');
    }
    let mut on = true;
    for rule in overrides.as_array().into_iter().flatten().filter_map(|r| r.as_str()) {
        let (off, rule) = rule.strip_prefix('!').map_or((false, rule), |r| (true, r));
        let matches = match rule.strip_suffix('*') {
            // `/a/b/*` covers `/a/b/` and everything below it.
            Some(base) => path.starts_with(base),
            None => path == rule,
        };
        if matches {
            on = !off;
        }
    }
    on
}

/// The `name` in the JSON manifest at `dir/manifest`, else the folder name of `dir`.
fn manifest_name(dir: &Path, manifest: &str) -> String {
    let manifest = fs::read_to_string(dir.join(manifest)).ok();
    let name = manifest
        .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
        .and_then(|v| v["name"].as_str().map(String::from));
    name.unwrap_or_else(|| dir.file_name().map_or_else(String::new, |n| n.to_string_lossy().into_owned()))
}

/// Links every user-level skill into the folders each harness reads, so all of them load the
/// same skills: `~/.agents/skills` (Codex, Gemini, OpenCode, Pi) and `~/.claude/skills` (Claude
/// Code). Only symlinks are made or removed; skill folders are never moved, copied or deleted.
/// Skills whose ids are in `disabled` (turned off in settings) are not linked anywhere new.
/// Only the folders of the harnesses in `agents` (those installed and turned on) are changed: the
/// hub when one of its readers is there, Claude Code's folder when Claude Code is.
pub fn sync(disabled: &[String], agents: &[&str]) -> io::Result<SyncReport> {
    sync_in(&home_or_err()?, disabled, agents)
}

/// `sync` with the home directory given.
fn sync_in(home: &Path, disabled: &[String], agents: &[&str]) -> io::Result<SyncReport> {
    let hub = home.join(".agents/skills");
    let claude = home.join(".claude/skills");
    let to_hub = agents.iter().any(|a| HUB_READERS.contains(a));
    let to_claude = agents.contains(&"claude");
    let targets: Vec<&Path> = [(to_claude, claude.as_path()), (to_hub, hub.as_path())]
        .into_iter()
        .filter_map(|(on, dir)| on.then_some(dir))
        .collect();
    let mut report = SyncReport::default();
    for dir in &targets {
        fs::create_dir_all(dir)?;
    }
    for dir in targets.iter().rev() {
        prune(home, dir, &mut report.pruned)?;
    }
    // Skills of a Gemini extension that is off are loaded by no one, so links to them go too.
    let off_extensions: Vec<PathBuf> =
        gemini_extensions(home).into_iter().filter(|(_, _, on)| !on).filter_map(|(dir, _, _)| fs::canonicalize(dir).ok()).collect();
    if !off_extensions.is_empty() {
        // Claude's links may lead through the hub's, so they go first, while they still resolve.
        for dir in &targets {
            unlink_into(home, dir, &off_extensions, &mut report.pruned)?;
        }
    }

    let found = scan(&user_roots(home));
    // Skills turned off in settings, by resolved folder.
    let off: HashMap<&Path, &Skill> =
        found.iter().filter(|f| disabled.contains(&f.skill.id)).map(|f| (f.canonical.as_path(), &f.skill)).collect();
    let skip_off = |skill: &Skill, skipped: &mut Vec<String>| {
        let line = format!("{} ({}): turned off in settings", skill.name, skill.source);
        if !skipped.contains(&line) {
            skipped.push(line);
        }
    };
    // Into the hub: every skill its readers do not reach yet.
    let mut in_hub: HashSet<PathBuf> = walk(&hub, Depth::One).iter().filter_map(|d| fs::canonicalize(d).ok()).collect();
    let names: HashMap<&Path, &str> = found.iter().map(|f| (f.canonical.as_path(), f.skill.name.as_str())).collect();
    // Skill names the hub's readers already load, from the hub or their own folders, with the
    // folders and readers that have each: a second skill under the same name would be a duplicate
    // to them, so it is skipped rather than linked under another folder name.
    let mut hub_names: HashMap<String, Vec<(PathBuf, Vec<&str>)>> = HashMap::new();
    for f in &found {
        let readers: Vec<&str> =
            f.skill.agents.iter().copied().filter(|a| HUB_READERS.contains(a) && agents.contains(a)).collect();
        if !readers.is_empty() {
            hub_names.entry(f.skill.name.clone()).or_default().push((f.canonical.clone(), readers));
        }
    }
    for f in found.iter().filter(|_| to_hub) {
        // A personal skill below the hub's top level is Pi's alone; linking it would list it twice.
        if f.skill.source == "Personal" || in_hub.contains(&f.canonical) {
            continue;
        }
        // Every hub reader that is on loads it already.
        if agents.iter().filter(|a| HUB_READERS.contains(a)).all(|a| f.skill.agents.contains(a)) {
            continue;
        }
        if off.contains_key(f.canonical.as_path()) {
            skip_off(&f.skill, &mut report.off);
            continue;
        }
        let others: Vec<&(PathBuf, Vec<&str>)> =
            hub_names.get(&f.skill.name).into_iter().flatten().filter(|(c, _)| *c != f.canonical).collect();
        if !others.is_empty() {
            let holder = if others.iter().any(|(c, _)| in_hub.contains(c)) {
                tilde(home, &hub)
            } else {
                let mut readers: Vec<&str> = others.iter().flat_map(|(_, r)| r.iter().copied()).collect();
                readers.sort_by_key(|a| AGENTS.iter().position(|b| b == a));
                readers.dedup();
                readers.iter().map(|a| super::agent_name(a)).collect::<Vec<_>>().join(" and ")
            };
            report.kept.push(format!("{} ({}): {holder} already has a skill with this name", f.skill.name, f.skill.source));
            continue;
        }
        link(home, &hub, &f.canonical, &f.skill.source, &[], &mut report)?;
        in_hub.insert(f.canonical.clone());
        hub_names.entry(f.skill.name.clone()).or_default().push((f.canonical.clone(), HUB_READERS.to_vec()));
    }
    if !to_claude {
        return Ok(report);
    }
    // Into Claude Code's folder: every hub skill it does not load yet, and, when the hub was left
    // alone, every other agent's skill that would have gone through it.
    let mut candidates: Vec<(String, PathBuf)> = walk(&hub, Depth::One)
        .into_iter()
        .filter_map(|entry| Some((entry.file_name()?.to_string_lossy().into_owned(), fs::canonicalize(&entry).ok()?)))
        .collect();
    if !to_hub {
        for f in found.iter().filter(|f| f.skill.source != "Personal" && !in_hub.contains(&f.canonical)) {
            let folder = f.canonical.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            candidates.push((folder, f.canonical.clone()));
        }
    }
    let sources: HashMap<&Path, &str> = found.iter().map(|f| (f.canonical.as_path(), f.skill.source.as_str())).collect();
    let mut in_claude: HashSet<PathBuf> =
        found.iter().filter(|f| f.skill.agents.contains(&"claude")).map(|f| f.canonical.clone()).collect();
    // Claude Code namespaces plugin and Claude.ai skills (`runpod:discovery`), so only the names in
    // its own folder can clash.
    let mut claude_names: HashSet<String> = walk(&claude, Depth::One)
        .iter()
        .filter_map(|d| fs::canonicalize(d).ok())
        .filter_map(|c| names.get(c.as_path()).map(|n| n.to_string()))
        .collect();
    for (name, canonical) in candidates {
        if in_claude.contains(&canonical) {
            continue;
        }
        if let Some(skill) = off.get(canonical.as_path()) {
            skip_off(skill, &mut report.off);
            continue;
        }
        if let Some(name) = names.get(canonical.as_path()).filter(|n| claude_names.contains(**n)) {
            report.kept.push(format!("{name}: Claude Code already has a skill with this name"));
            continue;
        }
        let source = sources.get(canonical.as_path()).copied().unwrap_or("Personal");
        link_named(home, &claude, &canonical, &name, source, &["synced"], &mut report)?;
        if let Some(skill) = names.get(canonical.as_path()) {
            claude_names.insert(skill.to_string());
        }
        in_claude.insert(canonical);
    }
    Ok(report)
}

/// Links `dir/<folder name>` to `target`, the resolved skill folder.
fn link(home: &Path, dir: &Path, target: &Path, source: &str, reserved: &[&str], report: &mut SyncReport) -> io::Result<()> {
    let name = target.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    link_named(home, dir, target, &name, source, reserved, report)
}

/// Links `dir/<name>` to `target`, or `dir/<source slug>-<name>` when `name` is taken by
/// anything else or reserved. Records a skip when both are taken.
fn link_named(
    home: &Path,
    dir: &Path,
    target: &Path,
    name: &str,
    source: &str,
    reserved: &[&str],
    report: &mut SyncReport,
) -> io::Result<()> {
    let mut names = vec![name.to_string()];
    names.extend(slug(source).map(|s| format!("{s}-{name}")));
    for candidate in &names {
        let path = dir.join(candidate);
        // Already linked to the same folder: nothing to do.
        let same = || fs::canonicalize(&path).is_ok_and(|c| c == target);
        if reserved.contains(&candidate.as_str()) || fs::symlink_metadata(&path).is_ok() {
            if same() {
                return Ok(());
            }
            continue;
        }
        // A link must point at a real folder outside itself, never at itself or a chain.
        if target.starts_with(&path) {
            break;
        }
        match symlink_dir(target, &path) {
            Ok(()) => {
                report.linked.push(format!("{name} → {}", tilde(home, &path)));
                return Ok(());
            }
            // Something else (another sync, `sync-skills`, an agent) took the name meanwhile.
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {
                if same() {
                    return Ok(());
                }
            }
            Err(e) => return Err(e),
        }
    }
    let taken: Vec<String> = names.iter().map(|n| format!("`{n}`")).collect();
    report.skipped.push(format!("{name}: {} already has {}", tilde(home, dir), taken.join(" and ")));
    Ok(())
}

/// The folders `sync` links skills from. A link into one of them whose target is gone is a link
/// `sync` made (or one like it) to a skill that was removed or moved.
fn link_sources(home: &Path) -> Vec<PathBuf> {
    [
        ".agents/skills",
        ".claude/skills",
        ".claude/plugins",
        ".gemini/skills",
        ".gemini/extensions",
        ".pi/agent/skills",
        ".codex/skills",
        ".config/opencode/skills",
        ".config/opencode/skill",
    ]
    .iter()
    .map(|p| resolve(&home.join(p)))
    .collect()
}

/// `path` with its longest existing leading part resolved, so a path that is gone compares with
/// the resolved paths `sync` links to.
fn resolve(path: &Path) -> PathBuf {
    for base in path.ancestors() {
        if let Ok(real) = fs::canonicalize(base) {
            return path.strip_prefix(base).map_or(real.clone(), |rest| real.join(rest));
        }
    }
    path.to_path_buf()
}

/// Removes the symlinks in `dir` whose target is gone, when that target was in one of the skill
/// folders `sync` links from. Anything else (a link the user made to another place, such as a
/// drive that is not mounted, or a link that cannot be read), and Claude.ai's `synced` folder,
/// is left alone.
fn prune(home: &Path, dir: &Path, pruned: &mut Vec<String>) -> io::Result<()> {
    let sources = link_sources(home);
    let ours = |link: &Path| {
        let Ok(target) = fs::read_link(link) else { return false };
        let target = resolve(&link.parent().map_or(target.clone(), |parent| parent.join(&target)));
        sources.iter().any(|s| target.starts_with(s))
    };
    let mut dead: Vec<PathBuf> = fs::read_dir(dir)?
        .flatten()
        .filter(|e| e.file_name() != "synced")
        .filter(|e| e.file_type().is_ok_and(|t| t.is_symlink()))
        .map(|e| e.path())
        .filter(|p| fs::metadata(p).is_err_and(|e| e.kind() == io::ErrorKind::NotFound))
        .filter(|p| ours(p))
        .collect();
    dead.sort();
    for path in dead {
        match fs::remove_file(&path) {
            Ok(()) => pruned.push(tilde(home, &path)),
            // Removed meanwhile by another sync.
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => return Err(e),
        }
    }
    Ok(())
}

/// Removes the symlinks in `dir` that lead into one of the folders `into` (resolved paths).
fn unlink_into(home: &Path, dir: &Path, into: &[PathBuf], pruned: &mut Vec<String>) -> io::Result<()> {
    let mut links: Vec<PathBuf> = fs::read_dir(dir)?
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_symlink()))
        .map(|e| e.path())
        .filter(|p| fs::canonicalize(p).is_ok_and(|c| into.iter().any(|d| c.starts_with(d))))
        .collect();
    links.sort();
    for path in links {
        match fs::remove_file(&path) {
            Ok(()) => pruned.push(tilde(home, &path)),
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => return Err(e),
        }
    }
    Ok(())
}

#[cfg(unix)]
fn symlink_dir(target: &Path, link: &Path) -> io::Result<()> {
    std::os::unix::fs::symlink(target, link)
}

#[cfg(not(unix))]
fn symlink_dir(_target: &Path, _link: &Path) -> io::Result<()> {
    Err(io::Error::new(io::ErrorKind::Unsupported, "skill links need a Unix system"))
}

/// `path` with the home folder shown as `~`.
fn tilde(home: &Path, path: &Path) -> String {
    path.strip_prefix(home).map_or_else(|_| path.display().to_string(), |rest| format!("~/{}", rest.display()))
}

/// Makes `~/.agents/skills/<slug>/SKILL.md` from a template and returns the new folder. When
/// that folder exists, the next free one is used: `<slug>-2`, `<slug>-3`, and so on.
pub fn create(name: &str) -> io::Result<PathBuf> {
    create_in(&home_or_err()?, name)
}

fn create_in(home: &Path, name: &str) -> io::Result<PathBuf> {
    let slug = slug(name).ok_or_else(invalid_name)?;
    let root = home.join(".agents/skills");
    fs::create_dir_all(&root)?;
    for n in 1.. {
        let folder = if n == 1 { slug.clone() } else { format!("{slug}-{n}") };
        let dir = root.join(&folder);
        // `create_dir` (not `_all`) fails when the folder exists, so nothing is overwritten.
        match fs::create_dir(&dir) {
            Ok(()) => {
                fs::write(dir.join("SKILL.md"), template(&folder))?;
                return Ok(dir);
            }
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e),
        }
    }
    unreachable!("an unbounded range ends only by returning")
}

/// Copies a folder that holds a SKILL.md into `~/.agents/skills`, named after the folder, and
/// returns the copy. Fails if that name is taken. Symlinks and `.git` are not copied.
pub fn import(folder: &Path) -> io::Result<PathBuf> {
    import_in(&home_or_err()?, folder)
}

fn import_in(home: &Path, folder: &Path) -> io::Result<PathBuf> {
    if !folder.join("SKILL.md").is_file() {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "the folder has no SKILL.md"));
    }
    let name = folder.file_name().and_then(|n| slug(&n.to_string_lossy())).ok_or_else(invalid_name)?;
    let root = home.join(".agents/skills");
    fs::create_dir_all(&root)?;
    // A folder that holds the skills folder (e.g. the home folder with a SKILL.md) would be copied into
    // itself without end.
    if fs::canonicalize(&root)?.starts_with(fs::canonicalize(folder)?) {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "the folder contains the skills folder"));
    }
    let dest = root.join(&name);
    fs::create_dir(&dest)?;
    if let Err(e) = copy_dir(folder, &dest) {
        let _ = fs::remove_dir_all(&dest);
        return Err(e);
    }
    Ok(dest)
}

/// Copies the files and folders in `from` into `to`, which exists. Symlinks are skipped.
fn copy_dir(from: &Path, to: &Path) -> io::Result<()> {
    for entry in fs::read_dir(from)? {
        let entry = entry?;
        let name = entry.file_name();
        if name == ".git" {
            continue;
        }
        let (src, dst) = (entry.path(), to.join(&name));
        let kind = entry.file_type()?;
        if kind.is_dir() {
            fs::create_dir(&dst)?;
            copy_dir(&src, &dst)?;
        } else if kind.is_file() {
            fs::copy(&src, &dst)?;
        }
    }
    Ok(())
}

/// A folder name from `name`: lower-case ASCII letters and digits joined by single hyphens.
fn slug(name: &str) -> Option<String> {
    let mut out = String::new();
    for c in name.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        } else if !out.is_empty() && !out.ends_with('-') {
            out.push('-');
        }
    }
    while out.ends_with('-') {
        out.pop();
    }
    (!out.is_empty()).then_some(out)
}

fn template(name: &str) -> String {
    format!(
        "---\nname: {name}\ndescription: Describe what this skill does and when to use it.\n---\n\n# {name}\n\nDescribe the steps the agent should follow.\n"
    )
}

fn invalid_name() -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, "a skill name needs at least one letter or digit")
}

fn home() -> Option<PathBuf> {
    env::var_os("HOME").map(PathBuf::from)
}

fn home_or_err() -> io::Result<PathBuf> {
    home().ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "HOME is not set"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn scratch(name: &str) -> PathBuf {
        static N: AtomicUsize = AtomicUsize::new(0);
        let dir = env::temp_dir().join(format!("archonui-skills-{name}-{}-{}", std::process::id(), N.fetch_add(1, Ordering::SeqCst)));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn skill_md(dir: &Path, text: &str) {
        fs::create_dir_all(dir).unwrap();
        fs::write(dir.join("SKILL.md"), text).unwrap();
    }

    #[test]
    fn front_matter_reads_single_line_and_block_values() {
        let text = "---\nname: \"review\"\ndescription: >\n  Reviews a diff\n  for bugs.\nmodel: x\n---\nBody: not front matter\n";
        let front = front_matter(text);
        assert_eq!(front["name"], "review");
        assert_eq!(front["description"], "Reviews a diff for bugs.");
        assert_eq!(front["model"], "x");
        assert!(!front.contains_key("Body"));
        assert!(front_matter("# no front matter\nname: x").is_empty());
    }

    #[test]
    fn front_matter_reads_chomped_blocks_and_skips_a_bom() {
        let text = "\u{feff}---\r\nname: lint\r\ndescription: >-\r\n  Lints code\r\n  before commits.\r\n---\r\n";
        let front = front_matter(text);
        assert_eq!(front["name"], "lint");
        assert_eq!(front["description"], "Lints code before commits.");
        assert_eq!(front_matter("---\ndescription: |-\n  one\n---\n")["description"], "one");
    }

    #[test]
    fn front_matter_reads_escapes_in_quoted_values() {
        let text = "---\nname: 'it''s'\ndescription: \"the \\\"browser\\\" pane, C:\\\\x\"\n---\n";
        let front = front_matter(text);
        assert_eq!(front["name"], "it's");
        assert_eq!(front["description"], "the \"browser\" pane, C:\\x");
    }

    #[test]
    fn import_refuses_a_folder_that_holds_the_skills_folder() {
        let home = scratch("into-itself");
        skill_md(&home, "---\nname: home\n---\n");
        assert_eq!(import_in(&home, &home).unwrap_err().kind(), io::ErrorKind::InvalidInput);
        assert!(fs::read_dir(home.join(".agents/skills")).unwrap().next().is_none(), "nothing was copied");
        let _ = fs::remove_dir_all(&home);
    }

    #[cfg(unix)]
    #[test]
    fn a_symlinked_skill_is_listed_once() {
        let home = scratch("listed-once");
        let cwd = home.join("repo");
        skill_md(&home.join(".agents/skills/fmt"), "---\nname: fmt\n---\n");
        fs::create_dir_all(home.join(".claude/skills")).unwrap();
        std::os::unix::fs::symlink(home.join(".agents/skills/fmt"), home.join(".claude/skills/fmt")).unwrap();
        let found = discover_in(&home, &cwd);
        assert_eq!(found.len(), 1, "{found:?}");
        assert_eq!(found[0].source, "Personal");
        let _ = fs::remove_dir_all(&home);
    }

    #[test]
    fn import_skips_symlinks_out_of_the_folder() {
        let home = scratch("symlink");
        let src = home.join("linked");
        skill_md(&src, "---\nname: linked\n---\n");
        fs::write(home.join("secret"), "secret").unwrap();
        std::os::unix::fs::symlink(home.join("secret"), src.join("leak")).unwrap();
        std::os::unix::fs::symlink(&home, src.join("loop")).unwrap();
        let imported = import_in(&home, &src).unwrap();
        assert!(imported.join("SKILL.md").is_file());
        assert!(fs::symlink_metadata(imported.join("leak")).is_err());
        assert!(fs::symlink_metadata(imported.join("loop")).is_err());
        let _ = fs::remove_dir_all(&home);
    }

    #[test]
    fn discovers_skills_from_each_location_with_sources() {
        let home = scratch("discover");
        let cwd = home.join("repo");
        skill_md(&home.join(".agents/skills/fmt"), "---\nname: fmt\ndescription: Formats code.\n---\n");
        skill_md(&home.join(".claude/skills/plain"), "# no front matter\n");
        skill_md(&home.join(".config/opencode/skill/oc"), "---\nname: oc-skill\n---\n");
        skill_md(&cwd.join(".agents/skills/local"), "---\ndescription: Project only.\n---\n");
        fs::create_dir_all(home.join(".agents/skills/empty")).unwrap();
        // An installed plugin's skill is found; a marketplace checkout is not.
        fs::create_dir_all(home.join(".claude/plugins/synced/x/design/.claude-plugin")).unwrap();
        fs::write(home.join(".claude/plugins/synced/x/design/.claude-plugin/plugin.json"), r#"{"name": "design"}"#).unwrap();
        skill_md(&home.join(".claude/plugins/synced/x/design/skills/critique"), "---\nname: critique\n---\n");
        skill_md(&home.join(".claude/plugins/marketplaces/m/plugins/other/skills/nope"), "---\nname: nope\n---\n");

        let skills = discover_in(&home, &cwd);
        let rows: Vec<(&str, &str, &str, &str)> = skills
            .iter()
            .map(|s| (s.source.as_str(), s.name.as_str(), s.command.as_str(), s.description.as_str()))
            .collect();
        assert_eq!(
            rows,
            [
                ("Claude Code", "plain", "/plain", ""),
                ("OpenCode", "oc-skill", "/oc-skill", ""),
                ("Personal", "fmt", "/fmt", "Formats code."),
                ("Project", "local", "/local", "Project only."),
                ("design", "critique", "/critique", ""),
            ]
        );
        let _ = fs::remove_dir_all(&home);
    }

    #[test]
    fn create_and_import_never_overwrite() {
        let home = scratch("write");
        let made = create_in(&home, "Code Review!").unwrap();
        assert_eq!(made, home.join(".agents/skills/code-review"));
        assert!(fs::read_to_string(made.join("SKILL.md")).unwrap().contains("name: code-review"));
        assert_eq!(create_in(&home, "code review").unwrap(), home.join(".agents/skills/code-review-2"));
        assert_eq!(create_in(&home, "!!!").unwrap_err().kind(), io::ErrorKind::InvalidInput);

        let src = home.join("src-skill");
        skill_md(&src, "---\nname: src\n---\n");
        fs::create_dir_all(src.join("refs")).unwrap();
        fs::write(src.join("refs/notes.md"), "notes").unwrap();
        fs::create_dir_all(src.join(".git")).unwrap();
        let imported = import_in(&home, &src).unwrap();
        assert_eq!(imported, home.join(".agents/skills/src-skill"));
        assert!(imported.join("refs/notes.md").is_file());
        assert!(!imported.join(".git").exists());
        assert_eq!(import_in(&home, &src).unwrap_err().kind(), io::ErrorKind::AlreadyExists);
        assert_eq!(import_in(&home, &home).unwrap_err().kind(), io::ErrorKind::InvalidData);
        let _ = fs::remove_dir_all(&home);
    }

    /// `link` → `target`, making the link's parent folder.
    fn ln(target: &Path, link: &Path) {
        fs::create_dir_all(link.parent().unwrap()).unwrap();
        std::os::unix::fs::symlink(target, link).unwrap();
    }

    /// The skill named `name`, which must be listed once.
    fn named<'a>(skills: &'a [Skill], name: &str) -> &'a Skill {
        let hits: Vec<&Skill> = skills.iter().filter(|s| s.name == name).collect();
        assert_eq!(hits.len(), 1, "{name}: {skills:?}");
        hits[0]
    }

    /// A Claude plugin manifest naming the plugin in `dir`.
    fn plugin_json(dir: &Path, name: &str) {
        fs::create_dir_all(dir.join(".claude-plugin")).unwrap();
        fs::write(dir.join(".claude-plugin/plugin.json"), format!(r#"{{"name": "{name}"}}"#)).unwrap();
    }

    /// Sets a folder's modified time, `secs` after the epoch.
    fn touch(dir: &Path, secs: u64) {
        let time = std::time::UNIX_EPOCH + std::time::Duration::from_secs(secs);
        fs::File::open(dir).unwrap().set_modified(time).unwrap();
    }

    #[test]
    fn pi_does_not_load_a_skill_without_a_description() {
        let home = scratch("pi-description");
        skill_md(&home.join(".agents/skills/bare"), "---\nname: bare\n---\n");
        skill_md(&home.join(".agents/skills/blank"), "---\nname: blank\ndescription: \"  \"\n---\n");
        skill_md(&home.join(".agents/skills/full"), "---\nname: full\ndescription: Does things.\n---\n");
        skill_md(&home.join(".pi/agent/skills/pi-bare"), "# no front matter\n");
        let skills = discover_in(&home, &home.join("repo"));
        assert_eq!(named(&skills, "bare").agents, ["codex", "gemini", "opencode"]);
        assert_eq!(named(&skills, "blank").agents, ["codex", "gemini", "opencode"]);
        assert_eq!(named(&skills, "full").agents, ["codex", "gemini", "opencode", "pi"]);
        assert!(named(&skills, "pi-bare").agents.is_empty());
        let _ = fs::remove_dir_all(&home);
    }

    #[test]
    fn gemini_enablement_rules_follow_the_last_match() {
        let home = Path::new("/home/u");
        let rules = |r: &[&str]| serde_json::json!(r);
        assert!(gemini_enabled(&serde_json::Value::Null, home));
        assert!(!gemini_enabled(&rules(&["!/home/u/*"]), home));
        assert!(gemini_enabled(&rules(&["!/home/u/*", "/home/u/"]), home));
        assert!(!gemini_enabled(&rules(&["!/*"]), home), "off everywhere");
        assert!(gemini_enabled(&rules(&["!/home/u/proj/*"]), home), "off only in a project");
        assert!(gemini_enabled(&rules(&["!/home/u/*", "/home/*"]), home));
    }

    #[test]
    fn a_gemini_extension_turned_off_has_its_skills_unlisted_and_unlinked() {
        let home = scratch("gemini-off");
        let ext = home.join(".gemini/extensions");
        skill_md(&ext.join("on-ext/skills/kept"), "---\nname: kept\ndescription: d\n---\n");
        skill_md(&ext.join("off-ext/skills/dropped"), "---\nname: dropped\ndescription: d\n---\n");
        fs::write(ext.join("off-ext/gemini-extension.json"), r#"{"name": "off"}"#).unwrap();
        // A link an earlier sync made while the extension was on.
        ln(&ext.join("off-ext/skills/dropped"), &home.join(".agents/skills/dropped"));
        ln(&home.join(".agents/skills/dropped"), &home.join(".claude/skills/dropped"));
        let rules = format!(r#"{{"off": {{"overrides": ["!{}/*"]}}}}"#, home.display());
        fs::write(ext.join("extension-enablement.json"), rules).unwrap();

        let report = sync_in(&home, &[], &AGENTS).unwrap();
        assert_eq!(report.pruned, ["~/.claude/skills/dropped", "~/.agents/skills/dropped"], "{report:?}");
        assert!(ext.join("off-ext/skills/dropped/SKILL.md").is_file(), "the extension is untouched");
        let skills = discover_in(&home, &home.join("repo"));
        assert!(skills.iter().all(|s| s.name != "dropped"), "{skills:?}");
        assert_eq!(named(&skills, "kept").agents, AGENTS);
        let _ = fs::remove_dir_all(&home);
    }

    #[test]
    fn synced_buckets_list_a_name_once_from_the_newest_bucket() {
        let home = scratch("synced");
        let synced = home.join(".claude/skills/synced");
        skill_md(&synced.join("old-bucket/pdf"), "---\nname: pdf\ndescription: old\n---\n");
        skill_md(&synced.join("old-bucket/docx"), "---\nname: docx\n---\n");
        skill_md(&synced.join("new-bucket/pdf"), "---\nname: pdf\ndescription: new\n---\n");
        skill_md(&synced.join(".bucket-hidden/secret"), "---\nname: secret\n---\n");
        fs::write(synced.join("old-bucket/manifest.json"), "{}").unwrap();
        touch(&synced.join("old-bucket"), 1_000);
        touch(&synced.join("new-bucket"), 2_000);

        let skills = discover_in(&home, &home.join("repo"));
        let pdf = named(&skills, "pdf");
        assert_eq!((pdf.source.as_str(), pdf.description.as_str()), ("Claude.ai", "new"));
        assert_eq!(pdf.agents, ["claude"]);
        assert_eq!(named(&skills, "docx").source, "Claude.ai");
        assert!(skills.iter().all(|s| s.name != "secret"), "{skills:?}");
        let _ = fs::remove_dir_all(&home);
    }

    #[test]
    fn plugin_copies_list_each_skill_once() {
        let home = scratch("plugins");
        let cache = home.join(".claude/plugins/cache/runpod/runpod");
        skill_md(&cache.join("1.0/skills/discovery"), "---\nname: discovery\ndescription: old\n---\n");
        skill_md(&cache.join("2.0/skills/discovery"), "---\nname: discovery\ndescription: new\n---\n");
        plugin_json(&cache.join("1.0"), "runpod");
        plugin_json(&cache.join("2.0"), "runpod");
        touch(&cache.join("1.0"), 1_000);
        touch(&cache.join("2.0"), 2_000);
        let skills = discover_in(&home, &home.join("repo"));
        let discovery = named(&skills, "discovery");
        assert_eq!((discovery.source.as_str(), discovery.description.as_str()), ("runpod", "new"));
        assert_eq!(discovery.agents, ["claude"]);
        let _ = fs::remove_dir_all(&home);
    }

    #[test]
    fn harness_folders_have_their_sources_and_agents() {
        let home = scratch("harnesses");
        let cwd = home.join("repo");
        fs::create_dir_all(cwd.join(".git")).unwrap();
        skill_md(&home.join(".agents/skills/hub"), "---\nname: hub\ndescription: d\n---\n");
        skill_md(&home.join(".agents/skills/group/deep"), "---\nname: deep\ndescription: d\n---\n");
        skill_md(&home.join(".claude/skills/cc"), "---\nname: cc\ndescription: d\n---\n");
        skill_md(&home.join(".gemini/skills/gem"), "---\nname: gem\ndescription: d\n---\n");
        skill_md(&home.join(".gemini/extensions/ext/skills/extskill"), "---\nname: extskill\ndescription: d\n---\n");
        fs::write(home.join(".gemini/extensions/ext/gemini-extension.json"), r#"{"name": "conductor"}"#).unwrap();
        skill_md(&home.join(".pi/agent/skills/a/b/pideep"), "---\nname: pideep\ndescription: d\n---\n");
        skill_md(&home.join(".codex/skills/cx"), "---\nname: cx\ndescription: d\n---\n");
        skill_md(&home.join(".config/opencode/skills/oc"), "---\nname: oc\ndescription: d\n---\n");
        skill_md(&cwd.join(".gemini/skills/pg"), "---\nname: pg\ndescription: d\n---\n");
        skill_md(&cwd.join(".pi/skills/pp"), "---\nname: pp\ndescription: d\n---\n");
        skill_md(&cwd.join(".opencode/skill/po"), "---\nname: po\ndescription: d\n---\n");
        skill_md(&cwd.join("sub/.agents/skills/above"), "---\nname: above\ndescription: d\n---\n");

        let skills = discover_in(&home, &cwd.join("sub"));
        let row = |name: &str| {
            let s = named(&skills, name);
            (s.source.clone(), s.agents.clone())
        };
        assert_eq!(row("hub"), ("Personal".into(), vec!["codex", "gemini", "opencode", "pi"]));
        assert_eq!(row("deep"), ("Personal".into(), vec!["pi"]));
        assert_eq!(row("cc"), ("Claude Code".into(), vec!["claude", "opencode"]));
        assert_eq!(row("gem"), ("Gemini".into(), vec!["gemini"]));
        assert_eq!(row("extskill"), ("conductor".into(), vec!["gemini"]));
        assert_eq!(row("pideep"), ("Pi".into(), vec!["pi"]));
        assert_eq!(row("cx"), ("Codex".into(), vec!["codex"]));
        assert_eq!(row("oc"), ("OpenCode".into(), vec!["opencode"]));
        // Project folders are read from the session folder, `sub`; Pi also reads up to the git root.
        assert!(skills.iter().all(|s| !["pg", "pp", "po"].contains(&s.name.as_str())), "{skills:?}");
        assert_eq!(row("above"), ("Project".into(), vec!["codex", "gemini", "opencode", "pi"]));
        let in_repo = discover_in(&home, &cwd);
        assert_eq!(named(&in_repo, "pg").agents, ["gemini"]);
        assert_eq!(named(&in_repo, "pp").agents, ["pi"]);
        assert_eq!(named(&in_repo, "po").agents, ["opencode"]);
        let _ = fs::remove_dir_all(&home);
    }

    #[test]
    fn a_skill_linked_into_several_folders_is_loaded_by_each_reader() {
        let home = scratch("agents");
        let real = home.join("elsewhere/review");
        skill_md(&real, "---\nname: review\ndescription: d\n---\n");
        ln(&real, &home.join(".agents/skills/review"));
        ln(&real, &home.join(".claude/skills/review"));
        skill_md(&home.join(".claude/skills/solo"), "---\nname: solo\ndescription: d\n---\n");
        let skills = discover_in(&home, &home.join("repo"));
        assert_eq!(named(&skills, "review").agents, AGENTS);
        assert_eq!(named(&skills, "review").source, "Personal");
        assert_eq!(named(&skills, "solo").agents, ["claude", "opencode"]);
        let _ = fs::remove_dir_all(&home);
    }

    /// The link at `path` and its target.
    fn target(path: &Path) -> PathBuf {
        assert!(fs::symlink_metadata(path).unwrap().file_type().is_symlink(), "{path:?} is a link");
        fs::read_link(path).unwrap()
    }

    #[test]
    fn sync_links_both_ways_and_then_does_nothing() {
        let home = scratch("sync");
        let hub = home.join(".agents/skills");
        let claude = home.join(".claude/skills");
        skill_md(&hub.join("fmt"), "---\nname: fmt\ndescription: d\n---\n");
        skill_md(&claude.join("cc-only"), "---\nname: cc-only\ndescription: d\n---\n");
        skill_md(&claude.join("synced/bucket/pdf"), "---\nname: pdf\ndescription: d\n---\n");
        skill_md(&home.join(".gemini/skills/gem"), "---\nname: gem\ndescription: d\n---\n");
        skill_md(&home.join(".claude/plugins/cache/p/runpod/1/skills/discovery"), "---\nname: discovery\ndescription: d\n---\n");
        skill_md(&home.join("repo/.agents/skills/local"), "---\nname: local\ndescription: d\n---\n");
        let canon = |p: PathBuf| fs::canonicalize(p).unwrap();

        let report = sync_in(&home, &[], &AGENTS).unwrap();
        assert!(report.skipped.is_empty() && report.pruned.is_empty(), "{report:?}");
        assert_eq!(target(&hub.join("cc-only")), canon(claude.join("cc-only")));
        assert_eq!(target(&hub.join("pdf")), canon(claude.join("synced/bucket/pdf")));
        assert_eq!(target(&hub.join("gem")), canon(home.join(".gemini/skills/gem")));
        assert_eq!(target(&hub.join("discovery")), canon(home.join(".claude/plugins/cache/p/runpod/1/skills/discovery")));
        assert_eq!(target(&claude.join("fmt")), canon(hub.join("fmt")));
        assert_eq!(target(&claude.join("gem")), canon(home.join(".gemini/skills/gem")));
        assert!(fs::symlink_metadata(claude.join("pdf")).is_err(), "Claude loads synced skills already");
        assert!(fs::symlink_metadata(claude.join("discovery")).is_err(), "Claude loads plugin skills already");
        assert!(fs::symlink_metadata(hub.join("local")).is_err(), "project skills stay in the project");
        assert_eq!(report.linked.len(), 6, "{report:?}");
        assert!(report.linked.contains(&"fmt → ~/.claude/skills/fmt".to_string()), "{report:?}");

        // Every agent now loads every user-level skill.
        for s in discover_in(&home, &home.join("elsewhere")) {
            assert_eq!(s.agents, AGENTS, "{s:?}");
        }
        // Real folders are untouched.
        assert!(!fs::symlink_metadata(hub.join("fmt")).unwrap().file_type().is_symlink());
        assert!(!fs::symlink_metadata(claude.join("cc-only")).unwrap().file_type().is_symlink());
        assert_eq!(sync_in(&home, &[], &AGENTS).unwrap(), SyncReport::default());
        let _ = fs::remove_dir_all(&home);
    }

    #[test]
    fn sync_keeps_each_skills_id_and_source() {
        let home = scratch("stable-id");
        skill_md(&home.join(".claude/skills/cc-only"), "---\nname: cc-only\n---\n");
        skill_md(&home.join(".claude/skills/synced/b/pdf"), "---\nname: pdf\n---\n");
        skill_md(&home.join(".gemini/skills/gem"), "---\nname: gem\n---\n");
        let rows = |skills: Vec<Skill>| -> Vec<(String, String, String)> {
            skills.into_iter().map(|s| (s.name, s.id, s.source)).collect()
        };
        let before = rows(discover_in(&home, &home.join("repo")));
        let report = sync_in(&home, &[], &AGENTS).unwrap();
        assert_eq!(report.linked.len(), 4, "{report:?}");
        // The ids key the on/off settings: a skill turned off must stay off after a sync.
        assert_eq!(rows(discover_in(&home, &home.join("repo"))), before);
        let _ = fs::remove_dir_all(&home);
    }

    #[test]
    fn sync_leaves_a_skill_turned_off_where_it_is() {
        let home = scratch("disabled");
        let hub = home.join(".agents/skills");
        let claude = home.join(".claude/skills");
        skill_md(&hub.join("fmt"), "---\nname: fmt\n---\n");
        skill_md(&hub.join("on"), "---\nname: on\n---\n");
        skill_md(&claude.join("cc-only"), "---\nname: cc-only\n---\n");
        let off = [hub.join("fmt").display().to_string(), claude.join("cc-only").display().to_string()];
        let report = sync_in(&home, &off, &AGENTS).unwrap();
        assert_eq!(report.linked, ["on → ~/.claude/skills/on"]);
        assert_eq!(report.off, ["cc-only (Claude Code): turned off in settings", "fmt (Personal): turned off in settings"]);
        assert!(fs::symlink_metadata(claude.join("fmt")).is_err() && fs::symlink_metadata(hub.join("cc-only")).is_err());
        let _ = fs::remove_dir_all(&home);
    }

    #[test]
    fn sync_prefixes_a_taken_folder_and_skips_a_taken_skill_name() {
        let home = scratch("collide");
        let hub = home.join(".agents/skills");
        // A different skill already uses the folder name `discovery`.
        skill_md(&hub.join("discovery"), "---\nname: find-things\n---\n");
        // A skill named `pdf` is already in the hub, under another folder.
        skill_md(&hub.join("my-pdf"), "---\nname: pdf\n---\n");
        // Both folder names a third skill could use are taken by other skills.
        skill_md(&hub.join("fmt"), "---\nname: format\n---\n");
        skill_md(&hub.join("claude-ai-fmt"), "---\nname: format-2\n---\n");
        let plugin = home.join(".claude/plugins/cache/p/runpod/1/skills/discovery");
        skill_md(&plugin, "---\nname: discovery\n---\n");
        plugin_json(&home.join(".claude/plugins/cache/p/runpod/1"), "runpod");
        skill_md(&home.join(".claude/skills/synced/b/pdf"), "---\nname: pdf\n---\n");
        skill_md(&home.join(".claude/skills/synced/b/fmt"), "---\nname: fmt\n---\n");

        let report = sync_in(&home, &[], &AGENTS).unwrap();
        assert_eq!(target(&hub.join("runpod-discovery")), fs::canonicalize(&plugin).unwrap());
        assert!(report.linked.contains(&"discovery → ~/.agents/skills/runpod-discovery".to_string()), "{report:?}");
        assert_eq!(report.kept, ["pdf (Claude.ai): ~/.agents/skills already has a skill with this name"], "{report:?}");
        assert_eq!(report.skipped, ["fmt: ~/.agents/skills already has `fmt` and `claude-ai-fmt`"], "{report:?}");
        // Hub skills reach Claude under their own folder names; the plugin link is already Claude's.
        assert!(fs::symlink_metadata(home.join(".claude/skills/discovery")).is_ok());
        assert!(fs::symlink_metadata(home.join(".claude/skills/runpod-discovery")).is_err());
        let _ = fs::remove_dir_all(&home);
    }

    #[test]
    fn sync_skips_a_skill_whose_name_a_hub_reader_has_in_its_own_folder() {
        let home = scratch("reader-name");
        // Gemini loads its own `review`; linking Claude Code's `review` into the hub would give it two.
        skill_md(&home.join(".claude/skills/review"), "---\nname: review\n---\n");
        skill_md(&home.join(".gemini/skills/review"), "---\nname: review\n---\n");
        let report = sync_in(&home, &[], &AGENTS).unwrap();
        assert!(fs::symlink_metadata(home.join(".agents/skills/review")).is_err(), "{report:?}");
        assert!(report.linked.is_empty(), "{report:?}");
        assert_eq!(
            report.kept,
            ["review (Claude Code): Gemini CLI already has a skill with this name", "review (Gemini): OpenCode already has a skill with this name"]
        );
        let _ = fs::remove_dir_all(&home);
    }

    #[test]
    fn a_second_sync_of_the_real_layout_changes_nothing() {
        // The real layout: skills in the hub linked into Claude's folder, a Claude-only skill, and
        // claude.ai copies of some of the same skills, which stay the user's own copies.
        let home = scratch("steady");
        let (hub, claude) = (home.join(".agents/skills"), home.join(".claude/skills"));
        for n in ["archon-triage", "canvas-design", "theme-factory"] {
            skill_md(&hub.join(n), &format!("---\nname: {n}\ndescription: d\n---\n"));
            ln(&hub.join(n), &claude.join(n));
            skill_md(&claude.join("synced/b").join(n), &format!("---\nname: {n}\ndescription: d\n---\n"));
        }
        skill_md(&claude.join("setup-writing-style"), "---\nname: setup-writing-style\ndescription: d\n---\n");
        skill_md(&claude.join("synced/b/setup-writing-style"), "---\nname: setup-writing-style\ndescription: d\n---\n");
        let first = sync_in(&home, &[], &AGENTS).unwrap();
        assert_eq!(first.linked, ["setup-writing-style → ~/.agents/skills/setup-writing-style"]);
        let second = sync_in(&home, &[], &AGENTS).unwrap();
        assert_eq!(second.kept.len(), 4, "{second:?}");
        assert_eq!(SyncReport { kept: Vec::new(), ..second }, SyncReport::default());
        let _ = fs::remove_dir_all(&home);
    }

    #[test]
    fn sync_skips_a_hub_skill_whose_name_claude_already_has_in_its_folder() {
        let home = scratch("claude-name");
        skill_md(&home.join(".agents/skills/review"), "---\nname: review\n---\n");
        skill_md(&home.join(".claude/skills/review-mine"), "---\nname: review\n---\n");
        let report = sync_in(&home, &[], &AGENTS).unwrap();
        assert!(fs::symlink_metadata(home.join(".claude/skills/review")).is_err());
        assert!(report.kept.contains(&"review: Claude Code already has a skill with this name".to_string()), "{report:?}");
        assert!(report.skipped.is_empty(), "{report:?}");
        let _ = fs::remove_dir_all(&home);
    }

    #[test]
    fn sync_does_not_take_the_synced_name_in_claudes_folder() {
        let home = scratch("reserved");
        skill_md(&home.join(".agents/skills/synced"), "---\nname: synced\n---\n");
        let report = sync_in(&home, &[], &AGENTS).unwrap();
        assert_eq!(report.linked, ["synced → ~/.claude/skills/personal-synced"]);
        let _ = fs::remove_dir_all(&home);
    }

    #[test]
    fn sync_prunes_only_dead_links() {
        let home = scratch("prune");
        let hub = home.join(".agents/skills");
        let claude = home.join(".claude/skills");
        skill_md(&hub.join("fmt"), "---\nname: fmt\n---\n");
        fs::create_dir_all(hub.join("no-skill")).unwrap();
        ln(&claude.join("synced/b/gone"), &hub.join("dead"));
        ln(&hub.join("gone"), &claude.join("dead"));
        ln(Path::new("../../.agents/skills/gone-too"), &claude.join("relative"));
        ln(&home.join(".claude/plugins/cache/p/1/skills/gone"), &hub.join("old-plugin"));
        ln(&hub.join("fmt"), &claude.join("fmt"));
        ln(&home.join("gone"), &claude.join("synced/b/dead"));

        let report = sync_in(&home, &[], &AGENTS).unwrap();
        assert_eq!(
            report.pruned,
            ["~/.agents/skills/dead", "~/.agents/skills/old-plugin", "~/.claude/skills/dead", "~/.claude/skills/relative"]
        );
        assert!(report.linked.is_empty() && report.skipped.is_empty(), "{report:?}");
        assert!(hub.join("no-skill").is_dir() && hub.join("fmt/SKILL.md").is_file());
        assert!(fs::symlink_metadata(claude.join("fmt")).is_ok());
        assert!(fs::symlink_metadata(claude.join("synced/b/dead")).is_ok(), "synced is never touched");
        let _ = fs::remove_dir_all(&home);
    }

    #[test]
    fn sync_keeps_a_dead_link_the_user_made_to_another_place() {
        let home = scratch("prune-own");
        let hub = home.join(".agents/skills");
        let claude = home.join(".claude/skills");
        // Links to a drive that is not mounted, or to a project that moved, are the user's own.
        ln(Path::new("/nonexistent-archonui-drive/skills/notes"), &hub.join("notes"));
        ln(&home.join("projects/gone/skill"), &claude.join("project-skill"));
        ln(&claude.join("loop"), &claude.join("loop"));
        let report = sync_in(&home, &[], &AGENTS).unwrap();
        assert_eq!(report, SyncReport::default());
        for link in [hub.join("notes"), claude.join("project-skill"), claude.join("loop")] {
            assert!(fs::symlink_metadata(&link).is_ok(), "{link:?} was removed");
        }
        let _ = fs::remove_dir_all(&home);
    }

    #[test]
    fn two_syncs_at_once_both_succeed() {
        for round in 0..20 {
            let home = scratch(&format!("race-{round}"));
            for n in 0..20 {
                skill_md(&home.join(format!(".agents/skills/hub-{n}")), &format!("---\nname: hub-{n}\n---\n"));
                skill_md(&home.join(format!(".gemini/skills/gem-{n}")), &format!("---\nname: gem-{n}\n---\n"));
            }
            ln(&home.join(".agents/skills/gone"), &home.join(".claude/skills/dead"));
            let runs: Vec<_> = (0..2)
                .map(|_| {
                    let home = home.clone();
                    std::thread::spawn(move || sync_in(&home, &[], &AGENTS))
                })
                .collect();
            let reports: Vec<SyncReport> = runs.into_iter().map(|t| t.join().unwrap().unwrap()).collect();
            // Each link is made, and each dead link removed, by exactly one of them.
            assert_eq!(reports.iter().map(|r| r.linked.len()).sum::<usize>(), 60, "{reports:?}");
            assert_eq!(reports.iter().map(|r| r.pruned.len()).sum::<usize>(), 1, "{reports:?}");
            assert_eq!(sync_in(&home, &[], &AGENTS).unwrap(), SyncReport::default());
            let _ = fs::remove_dir_all(&home);
        }
    }

    #[test]
    fn sync_leaves_the_folders_of_agents_that_are_off_alone() {
        let home = scratch("agents-off");
        let hub = home.join(".agents/skills");
        let claude = home.join(".claude/skills");
        skill_md(&hub.join("fmt"), "---\nname: fmt\ndescription: d\n---\n");
        skill_md(&claude.join("cc-only"), "---\nname: cc-only\ndescription: d\n---\n");
        skill_md(&home.join(".gemini/skills/gem"), "---\nname: gem\ndescription: d\n---\n");
        ln(&hub.join("gone"), &claude.join("dead"));
        ln(&hub.join("gone"), &hub.join("dead"));

        // Only Claude Code: nothing new in the hub; the Gemini skill goes straight to Claude.
        let report = sync_in(&home, &[], &["claude"]).unwrap();
        assert_eq!(report.linked, ["fmt → ~/.claude/skills/fmt", "gem → ~/.claude/skills/gem"], "{report:?}");
        assert_eq!(report.pruned, ["~/.claude/skills/dead"]);
        assert!(fs::symlink_metadata(hub.join("dead")).is_ok() && fs::symlink_metadata(hub.join("cc-only")).is_err());

        // Only Gemini: the hub gets Claude Code's skill; Claude's folder is not touched.
        ln(&hub.join("gone"), &claude.join("dead"));
        let report = sync_in(&home, &[], &["gemini"]).unwrap();
        assert_eq!(report.linked, ["cc-only → ~/.agents/skills/cc-only"], "{report:?}");
        assert_eq!(report.pruned, ["~/.agents/skills/dead"]);
        assert!(fs::symlink_metadata(claude.join("dead")).is_ok());

        // None: nothing at all.
        let empty = scratch("agents-none");
        assert_eq!(sync_in(&empty, &[], &[]).unwrap(), SyncReport::default());
        assert!(!empty.join(".agents").exists() && !empty.join(".claude").exists());
        let _ = fs::remove_dir_all(&home);
        let _ = fs::remove_dir_all(&empty);
    }

    #[test]
    fn sync_makes_missing_folders() {
        let home = scratch("empty-home");
        assert_eq!(sync_in(&home, &[], &AGENTS).unwrap(), SyncReport::default());
        assert!(home.join(".agents/skills").is_dir() && home.join(".claude/skills").is_dir());
        let _ = fs::remove_dir_all(&home);
    }

    #[test]
    fn create_picks_the_next_free_folder() {
        let home = scratch("create-next");
        let root = home.join(".agents/skills");
        assert_eq!(create_in(&home, "new-skill").unwrap(), root.join("new-skill"));
        assert_eq!(create_in(&home, "new-skill").unwrap(), root.join("new-skill-2"));
        let third = create_in(&home, "New Skill").unwrap();
        assert_eq!(third, root.join("new-skill-3"));
        assert!(fs::read_to_string(third.join("SKILL.md")).unwrap().contains("name: new-skill-3"));
        let _ = fs::remove_dir_all(&home);
    }
}

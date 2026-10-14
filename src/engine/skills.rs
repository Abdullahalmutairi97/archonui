//! Skills: folders with a SKILL.md, found on disk and passed to harnesses that support them.
#![allow(dead_code)]

use std::{
    collections::{HashMap, HashSet},
    env, fs, io,
    path::{Path, PathBuf},
};

#[derive(Clone, Debug, PartialEq)]
pub struct Skill {
    /// Stable id: the folder path.
    pub id: String,
    pub name: String,
    pub description: String,
    /// Slash command that runs it, e.g. `/review`.
    pub command: String,
    /// Where it came from: "Personal", "Project", a plugin name, a harness.
    pub source: String,
    pub path: PathBuf,
}

/// Every skill visible from `cwd`: personal, project, harness and installed plugin skill folders.
/// Folders are read in place; a folder without a readable SKILL.md is skipped.
pub fn discover(cwd: &Path) -> Vec<Skill> {
    home().map_or_else(Vec::new, |home| discover_in(&home, cwd))
}

/// `discover` with the home directory given.
fn discover_in(home: &Path, cwd: &Path) -> Vec<Skill> {
    let mut roots: Vec<(PathBuf, String)> = [
        (home.join(".agents/skills"), "Personal"),
        (home.join(".claude/skills"), "Claude Code"),
        (home.join(".pi/agent/skills"), "Pi"),
        (home.join(".config/opencode/skills"), "OpenCode"),
        (home.join(".config/opencode/skill"), "OpenCode"),
        (cwd.join(".claude/skills"), "Project"),
        (cwd.join(".agents/skills"), "Project"),
    ]
    .into_iter()
    .map(|(path, source)| (path, source.to_string()))
    .collect();
    roots.extend(plugin_skill_dirs(&home.join(".claude/plugins"), 0));
    let mut seen = HashSet::new();
    let mut out: Vec<Skill> = roots
        .iter()
        .flat_map(|(root, source)| skills_in(root, source))
        .filter(|s| seen.insert(s.path.clone()))
        .collect();
    out.sort_by(|a, b| (&a.source, &a.name).cmp(&(&b.source, &b.name)));
    out
}

/// The skill folders directly under `root`, in name order.
fn skills_in(root: &Path, source: &str) -> Vec<Skill> {
    let Ok(entries) = fs::read_dir(root) else { return Vec::new() };
    let mut dirs: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.join("SKILL.md").is_file())
        .collect();
    dirs.sort();
    dirs.iter().filter_map(|dir| parse_skill(dir, source)).collect()
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
    })
}

/// The `key: value` pairs in a SKILL.md's YAML front matter. A key whose value is `>` or `|`, or
/// empty, takes the indented lines after it, joined with spaces. Other YAML is not read.
fn front_matter(text: &str) -> HashMap<String, String> {
    let mut lines = text.lines();
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
        if matches!(value, ">" | "|" | "") {
            out.insert(key.to_string(), String::new());
            block = Some(key.to_string());
        } else {
            out.insert(key.to_string(), unquote(value).to_string());
        }
    }
    out
}

fn unquote(s: &str) -> &str {
    s.strip_prefix('"')
        .and_then(|x| x.strip_suffix('"'))
        .or_else(|| s.strip_prefix('\'').and_then(|x| x.strip_suffix('\'')))
        .unwrap_or(s)
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
    let manifest = fs::read_to_string(dir.join(".claude-plugin/plugin.json")).ok();
    let name = manifest
        .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
        .and_then(|v| v["name"].as_str().map(String::from));
    name.unwrap_or_else(|| dir.file_name().map_or_else(String::new, |n| n.to_string_lossy().into_owned()))
}

/// Makes `~/.agents/skills/<slug>/SKILL.md` from a template and returns the new folder. Fails if
/// the folder already exists.
pub fn create(name: &str) -> io::Result<PathBuf> {
    create_in(&home_or_err()?, name)
}

fn create_in(home: &Path, name: &str) -> io::Result<PathBuf> {
    let slug = slug(name).ok_or_else(invalid_name)?;
    let root = home.join(".agents/skills");
    fs::create_dir_all(&root)?;
    let dir = root.join(&slug);
    // `create_dir` (not `_all`) fails when the folder exists, so nothing is overwritten.
    fs::create_dir(&dir)?;
    fs::write(dir.join("SKILL.md"), template(&slug))?;
    Ok(dir)
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
    fn create_and_import_refuse_to_overwrite() {
        let home = scratch("write");
        let made = create_in(&home, "Code Review!").unwrap();
        assert_eq!(made, home.join(".agents/skills/code-review"));
        assert!(fs::read_to_string(made.join("SKILL.md")).unwrap().contains("name: code-review"));
        assert_eq!(create_in(&home, "code review").unwrap_err().kind(), io::ErrorKind::AlreadyExists);
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
}

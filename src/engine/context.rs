//! The context window of a model, for the meter before the agent reports one. Claude Code reports its
//! window after every turn; the others are looked up here.

use std::{
    collections::HashMap,
    path::Path,
    sync::{Mutex, OnceLock},
};

/// Tokens of context for `model` on `agent`. A Pi model listed in Pi's `models.json` uses that
/// file's figure; other models go by their family, and an unknown one by a common size. Answers are
/// kept, since the meter asks on every frame.
pub fn context_window(agent: &str, model: Option<&str>) -> u64 {
    static SEEN: OnceLock<Mutex<HashMap<(String, String), u64>>> = OnceLock::new();
    let model = model.unwrap_or_default().to_ascii_lowercase();
    let key = (agent.to_string(), model.clone());
    let seen = SEEN.get_or_init(Default::default);
    if let Some(&window) = seen.lock().ok().as_ref().and_then(|m| m.get(&key)) {
        return window;
    }
    let window = lookup(agent, &model);
    if let Ok(mut m) = seen.lock() {
        m.insert(key, window);
    }
    window
}

fn lookup(agent: &str, model: &str) -> u64 {
    if agent == "pi" {
        if let Some(window) = home().and_then(|h| pi_window(&h.join(".pi/agent/models.json"), model)) {
            return window;
        }
    }
    by_family(agent, model)
}

/// Context windows by model family. The agent's own default model is the empty string.
fn by_family(agent: &str, model: &str) -> u64 {
    match () {
        _ if model.contains("[1m]") || model.ends_with("-1m") => 1_000_000,
        _ if model.contains("gemini") || (agent == "gemini" && model.is_empty()) => 1_048_576,
        _ if model.contains("gpt-5") || model.contains("codex") || (agent == "codex" && model.is_empty()) => 400_000,
        _ if model.contains("gpt-4.1") => 1_047_576,
        _ if model.contains("claude") || ["opus", "sonnet", "haiku", "fable"].iter().any(|f| model.contains(f)) => 200_000,
        _ if agent == "claude" => 200_000,
        _ => 128_000,
    }
}

/// The `contextWindow` of `model` in a Pi `models.json`: by id, or by `provider/id`.
fn pi_window(path: &Path, model: &str) -> Option<u64> {
    let text = std::fs::read_to_string(path).ok()?;
    let json: serde_json::Value = serde_json::from_str(&text).ok()?;
    let providers = json["providers"].as_object()?;
    providers.iter().find_map(|(provider, p)| {
        p["models"].as_array()?.iter().find_map(|m| {
            let id = m["id"].as_str()?.to_ascii_lowercase();
            (id == model || format!("{}/{id}", provider.to_ascii_lowercase()) == model).then(|| m["contextWindow"].as_u64())?
        })
    })
}

fn home() -> Option<std::path::PathBuf> {
    std::env::var_os("HOME").map(Into::into)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn families_have_their_windows() {
        assert_eq!(by_family("claude", ""), 200_000);
        assert_eq!(by_family("claude", "opus[1m]"), 1_000_000);
        assert_eq!(by_family("codex", ""), 400_000);
        assert_eq!(by_family("gemini", "gemini-2.5-pro"), 1_048_576);
        assert_eq!(by_family("opencode", "anthropic/claude-sonnet-4"), 200_000);
        assert_eq!(by_family("opencode", "some/local-model"), 128_000);
    }

    #[test]
    fn pi_models_json_wins_by_id_or_provider_id() {
        let dir = std::env::temp_dir().join(format!("archonui-ctx-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("models.json");
        std::fs::write(&path, r#"{"providers":{"cursor":{"models":[{"id":"Big","contextWindow":500000}]}}}"#).unwrap();
        assert_eq!(pi_window(&path, "big"), Some(500_000));
        assert_eq!(pi_window(&path, "cursor/big"), Some(500_000));
        assert_eq!(pi_window(&path, "other"), None);
        let _ = std::fs::remove_dir_all(dir);
    }
}

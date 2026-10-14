//! Fast titles: a short, read-only one-shot that names a chat from its first exchange, on the
//! cheapest model its harness offers.

use super::{
    detect::DetectedAgent,
    discover::ModelInfo,
    harness::{Mode, ToolSettings},
    parse::Out,
    session::{start_turn, Event, TurnOptions},
};
use std::{
    path::Path,
    time::{Duration, Instant},
};

/// Longest the title one-shot may run before it is abandoned.
pub const TIMEOUT: Duration = Duration::from_secs(30);
/// Characters of each side of the exchange the title model sees.
const EXCERPT_CHARS: usize = 600;
/// Longest title kept.
const MAX_CHARS: usize = 48;
/// Quotes, stops and spaces dropped from the ends of a title.
const TRIM: &[char] = &['"', '\'', '“', '”', '‘', '’', '`', '*', '.', '!', '?', ':', ';', ',', ' '];

/// The first prompt of a chat and the agent's reply to it.
#[derive(Clone, Debug, PartialEq)]
pub struct Exchange {
    pub prompt: String,
    pub reply: String,
}

/// Which model writes a chat's title.
#[derive(Clone, Debug, PartialEq)]
pub enum TitleModel {
    /// A cheap model, by id.
    Cheap(String),
    /// No cheap model is listed, so the agent's own default writes the title.
    Default,
    /// The harness has no cheap model we know of, so no fast title is made.
    Skip,
}

/// The model that writes titles for the harness `harness`, given the models it lists.
pub fn title_model(harness: &str, models: &[ModelInfo]) -> TitleModel {
    let first = |pick: fn(&ModelInfo) -> bool| models.iter().find(|m| pick(m)).map(|m| TitleModel::Cheap(m.id.clone()));
    match harness {
        "claude" => TitleModel::Cheap("haiku".into()),
        "codex" => first(|m| contains_any(&m.label, &["mini", "luna"])).unwrap_or(TitleModel::Default),
        "opencode" | "pi" => {
            first(|m| contains_any(&m.id, &["flash", "mini", "haiku"])).unwrap_or(TitleModel::Skip)
        }
        _ => TitleModel::Skip,
    }
}

fn contains_any(text: &str, words: &[&str]) -> bool {
    let text = text.to_lowercase();
    words.iter().any(|w| text.contains(w))
}

/// The message the title model gets: the instruction, then the exchange, each side cut to 600 characters.
pub fn title_prompt(exchange: &Exchange) -> String {
    format!(
        "Write a 3–6 word title for this conversation. Reply with the title only.\n\nUser: {}\nAssistant: {}",
        cut(&exchange.prompt, EXCERPT_CHARS),
        cut(&exchange.reply, EXCERPT_CHARS),
    )
}

fn cut(text: &str, chars: usize) -> String {
    text.chars().take(chars).collect()
}

/// The title the model wrote: its first non-empty line, without surrounding quotes or stops, at
/// most 48 characters. `None` when nothing is left.
pub fn clean_title(raw: &str) -> Option<String> {
    let line = raw.lines().map(str::trim).find(|l| !l.is_empty())?;
    let title = cut(line.trim_matches(TRIM), MAX_CHARS);
    let title = title.trim_matches(TRIM);
    (!title.is_empty()).then(|| title.to_string())
}

/// Asks `agent` for a title for `exchange`: a read-only turn on `model` (`None` for the agent's
/// default), with no resumed session. Gives up after `TIMEOUT`.
pub fn suggest(agent: &DetectedAgent, model: Option<&str>, cwd: &Path, exchange: &Exchange) -> Result<String, String> {
    let prompt = title_prompt(exchange);
    let opts = TurnOptions { model, mode: Mode::Ask, tools: ToolSettings::default(), ..Default::default() };
    let turn = start_turn(agent, cwd, &prompt, None, opts).map_err(|e| e.to_string())?;
    let deadline = Instant::now() + TIMEOUT;
    let mut reply = String::new();
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        // `None` means the timer won: the agent did not finish in time.
        let next = smol::block_on(smol::future::or(
            async { Some(turn.events.recv().await.ok()) },
            async {
                smol::Timer::after(left).await;
                None
            },
        ));
        match next {
            None => return Err(format!("no title within {} s", TIMEOUT.as_secs())),
            Some(None) => return Err("the agent stopped without a reply".into()),
            Some(Some(Event::Out(Out::Text(t)))) => reply.push_str(&t),
            Some(Some(Event::Out(Out::Final(t)))) if reply.is_empty() => reply = t,
            Some(Some(Event::Out(Out::Error(e)))) => return Err(e),
            Some(Some(Event::Out(_))) => {}
            Some(Some(Event::Finished(Ok(())))) => break,
            Some(Some(Event::Finished(Err(e)))) => return Err(e),
        }
    }
    clean_title(&reply).ok_or_else(|| "the agent wrote no title".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn model(id: &str, label: &str) -> ModelInfo {
        ModelInfo { id: id.into(), label: label.into(), description: String::new(), efforts: Vec::new() }
    }

    #[test]
    fn claude_titles_use_haiku() {
        assert_eq!(title_model("claude", &[]), TitleModel::Cheap("haiku".into()));
    }

    #[test]
    fn codex_titles_use_a_mini_model_or_the_default() {
        let models = [model("gpt-5", "GPT-5"), model("gpt-5-mini", "GPT-5 Mini")];
        assert_eq!(title_model("codex", &models), TitleModel::Cheap("gpt-5-mini".into()));
        assert_eq!(title_model("codex", &models[..1]), TitleModel::Default);
    }

    #[test]
    fn opencode_and_pi_titles_only_use_a_known_cheap_model() {
        let models = [model("provider/big", "Big"), model("provider/flash", "Flash")];
        assert_eq!(title_model("opencode", &models), TitleModel::Cheap("provider/flash".into()));
        assert_eq!(title_model("pi", &models[..1]), TitleModel::Skip);
        assert_eq!(title_model("gemini", &models), TitleModel::Skip, "harnesses without a rule skip");
    }

    #[test]
    fn titles_are_cleaned() {
        assert_eq!(clean_title("\"Fix the login bug.\"").as_deref(), Some("Fix the login bug"));
        assert_eq!(clean_title("\n\n  Cache invalidation  \nsecond line").as_deref(), Some("Cache invalidation"));
        assert_eq!(clean_title(" ' ' ").as_deref(), None);
        let long = clean_title(&"word ".repeat(30)).unwrap();
        assert!(long.chars().count() <= 48, "capped at 48 characters");
        assert!(!long.ends_with(' '));
    }

    #[test]
    fn the_prompt_cuts_each_side_to_600_characters() {
        let exchange = Exchange { prompt: "p".repeat(900), reply: "r".repeat(10) };
        let prompt = title_prompt(&exchange);
        assert!(prompt.starts_with("Write a 3–6 word title for this conversation. Reply with the title only.\n\nUser: "));
        assert!(prompt.contains(&format!("User: {}\n", "p".repeat(600))));
        assert!(prompt.ends_with("\nAssistant: rrrrrrrrrr"));
    }
}

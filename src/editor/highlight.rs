//! Syntax highlighting with syntect, parsed lazily one line at a time.

use std::{ops::Range, path::Path, sync::OnceLock, time::Instant};

use syntect::{
    highlighting::{
        FontStyle, HighlightIterator, HighlightState, Highlighter as SyntectHighlighter, Theme,
        ThemeSet,
    },
    parsing::{ParseState, ScopeStack, SyntaxReference, SyntaxSet},
};

/// Theme used for every file.
const THEME: &str = "base16-ocean.dark";
/// Colour of unstyled text: the theme's foreground.
const PLAIN_FG: u32 = 0xc0c5ce;
/// Lines longer than this (in chars) get one unstyled span, since parsing them is slow.
const MAX_LINE_CHARS: usize = 3000;
/// `fill` checks its deadline once per this many parsed lines.
const DEADLINE_CHECK_LINES: usize = 16;

#[cfg(test)]
thread_local! {
    /// Lines parsed on this thread, so tests can check how much a call re-parsed.
    static PARSED_LINES: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// A styled byte range within a line.
pub type Span = (Range<usize>, Style);

/// How a span looks: its colour as 0xRRGGBB, and font flags.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Style {
    pub fg: u32,
    pub bold: bool,
    pub italic: bool,
}

struct Assets {
    syntaxes: SyntaxSet,
    theme: Theme,
}

/// The syntax set and theme, loaded once.
fn assets() -> &'static Assets {
    static ASSETS: OnceLock<Assets> = OnceLock::new();
    ASSETS.get_or_init(|| Assets {
        syntaxes: SyntaxSet::load_defaults_newlines(),
        theme: ThemeSet::load_defaults()
            .themes
            .remove(THEME)
            .expect("bundled theme"),
    })
}

/// Parser state at the start of a line.
#[derive(Clone, PartialEq, Eq)]
struct State {
    parse: ParseState,
    highlight: HighlightState,
}

/// Highlights one file. Parser state is cached at the start of each line, so an edit only re-parses
/// the lines it reaches, and a line is only parsed when it is asked for.
pub struct Highlighter {
    syntax: Option<&'static SyntaxReference>,
    /// Line count the caches were built for; a different count resets them.
    line_count: usize,
    /// `states[i]` is the state at the start of line `i`.
    states: Vec<Option<State>>,
    lines: Vec<Option<Vec<Span>>>,
    /// First line whose cached state or spans may be stale; `usize::MAX` when all are fresh.
    stale: usize,
    /// Last line whose text changed. Cached lines past it are reused once a re-parse reaches a state
    /// that matches the cache.
    changed_last: usize,
}

impl Highlighter {
    /// Picks the syntax by `path`'s extension, else by `first_line` (for a shebang), else plain text.
    pub fn new(path: Option<&Path>, first_line: &str) -> Self {
        let set = &assets().syntaxes;
        let by_extension = path
            .and_then(|p| p.extension())
            .and_then(|ext| ext.to_str())
            .and_then(|ext| set.find_syntax_by_extension(ext));
        Self {
            syntax: by_extension.or_else(|| set.find_syntax_by_first_line(first_line)),
            line_count: 0,
            states: Vec::new(),
            lines: Vec::new(),
            stale: usize::MAX,
            changed_last: 0,
        }
    }

    /// The name of the language the file was matched to, if any.
    pub fn language(&self) -> Option<&'static str> {
        self.syntax.map(|syntax| syntax.name.as_str())
    }

    /// Marks lines `first..=last` as changed in place. Same as `edited(first, n, n)` for `n` lines.
    pub fn invalidate(&mut self, first: usize, last: usize) {
        let n = last.saturating_sub(first) + 1;
        self.edited(first, n, n);
    }

    /// Records that lines `first..first + removed` of the old text were replaced by `inserted` new
    /// lines; a plain in-line edit is `edited(line, 1, 1)`. Cached states before `first` stay valid,
    /// and cached lines after the edit are reused once a re-parse reaches a matching state.
    pub fn edited(&mut self, first: usize, removed: usize, inserted: usize) {
        if self.states.is_empty() {
            // Nothing is cached yet; the next `spans` call builds the cache.
            return;
        }
        if first + removed > self.line_count {
            // Doesn't match the cached text: drop the cache and let the next call rebuild it.
            self.states.clear();
            self.lines.clear();
            self.line_count = 0;
            return;
        }
        self.line_count = self.line_count + inserted - removed;
        self.lines.splice(first..first + removed, vec![None; inserted]);
        // The state after the replaced lines is kept for the first line after them, where a re-parse
        // can match it. With no new lines, the state before `first` is already that state.
        let mut after = self.states.split_off(first + removed);
        self.states.truncate(first + 1);
        if inserted == 0 {
            after.remove(0);
        } else {
            self.states.resize(first + inserted, None);
        }
        self.states.append(&mut after);
        self.stale = self.stale.min(first);
        let shifted = if self.changed_last >= first + removed {
            self.changed_last + inserted - removed
        } else {
            self.changed_last
        };
        self.changed_last = shifted.max((first + inserted).saturating_sub(1));
    }

    /// Styled spans of `line`, as byte ranges within the line. `line_text` gives a line's text without
    /// its line break, and `line_count` is the buffer's line count.
    pub fn spans(
        &mut self,
        line_text: impl Fn(usize) -> String,
        line_count: usize,
        line: usize,
    ) -> Vec<Span> {
        self.lookup(line_text, line_count, line, None).unwrap_or_default()
    }

    /// Like `spans`, but stops parsing once `deadline` passes and returns `None` if `line` is not ready
    /// yet. The lines parsed so far stay cached, so calling again continues from there.
    pub fn spans_by(
        &mut self,
        line_text: impl Fn(usize) -> String,
        line_count: usize,
        line: usize,
        deadline: Instant,
    ) -> Option<Vec<Span>> {
        self.lookup(line_text, line_count, line, Some(deadline))
    }

    /// The last line of the run of fresh lines from line 0. Returns 0 when no line is fresh, the same
    /// as when only line 0 is.
    pub fn ready_through(&self) -> usize {
        self.lines
            .iter()
            .take(self.stale)
            .take_while(|spans| spans.is_some())
            .count()
            .saturating_sub(1)
    }

    /// The spans of `line` if they are fresh after parsing up to it, which stops at `deadline`.
    fn lookup(
        &mut self,
        line_text: impl Fn(usize) -> String,
        line_count: usize,
        line: usize,
        deadline: Option<Instant>,
    ) -> Option<Vec<Span>> {
        if line >= line_count {
            return Some(Vec::new());
        }
        let Some(syntax) = self.syntax else {
            return Some(vec![(0..line_text(line).len(), plain_style())]);
        };
        if self.line_count != line_count {
            self.reset(syntax, line_count);
        }
        self.fill(&line_text, line, deadline);
        if line < self.stale {
            self.lines[line].clone()
        } else {
            None
        }
    }

    /// Drops every cached state and span, for a buffer whose line count changed.
    fn reset(&mut self, syntax: &'static SyntaxReference, line_count: usize) {
        self.line_count = line_count;
        self.states = vec![None; line_count + 1];
        self.states[0] = Some(initial_state(syntax));
        self.lines = vec![None; line_count];
        self.stale = usize::MAX;
        self.changed_last = 0;
    }

    /// Computes lines up to `target` from the nearest valid cached state. Stale lines are re-parsed
    /// until one ends in the state already cached for the next line. Stops early once `deadline`
    /// passes, keeping the lines computed so far.
    fn fill(
        &mut self,
        line_text: &impl Fn(usize) -> String,
        target: usize,
        deadline: Option<Instant>,
    ) {
        let mut i = self.stale.min(target);
        while self.states[i].is_none() {
            i -= 1;
        }
        let mut parsed = 0;
        while i <= target {
            if i < self.stale && self.lines[i].is_some() {
                i += 1;
                continue;
            }
            if parsed % DEADLINE_CHECK_LINES == 0 && deadline.is_some_and(|d| Instant::now() >= d) {
                return;
            }
            parsed += 1;
            let Some(state) = self.states[i].clone() else {
                return;
            };
            let (spans, end) = highlight_line(&line_text(i), state);
            let reached_cache = i >= self.stale
                && i >= self.changed_last
                && self.states[i + 1].as_ref() == Some(&end);
            self.lines[i] = Some(spans);
            if reached_cache {
                self.stale = usize::MAX;
                self.changed_last = 0;
            } else {
                self.states[i + 1] = Some(end);
                if i >= self.stale {
                    self.stale = i + 1;
                }
            }
            i += 1;
        }
    }
}

/// Parses and styles one line from `state`. Returns its spans and the state at the next line's start.
fn highlight_line(text: &str, mut state: State) -> (Vec<Span>, State) {
    #[cfg(test)]
    PARSED_LINES.with(|count| count.set(count.get() + 1));
    if text.chars().count() > MAX_LINE_CHARS {
        return (vec![(0..text.len(), plain_style())], state);
    }
    let assets = assets();
    let line = format!("{text}\n");
    let Ok(ops) = state.parse.parse_line(&line, &assets.syntaxes) else {
        return (vec![(0..text.len(), plain_style())], state);
    };
    let highlighter = SyntectHighlighter::new(&assets.theme);
    let mut spans = Vec::new();
    let mut offset = 0;
    for (style, piece) in HighlightIterator::new(&mut state.highlight, &ops, &line, &highlighter) {
        let end = (offset + piece.len()).min(text.len());
        if offset < end {
            spans.push((offset..end, to_style(style)));
        }
        offset += piece.len();
    }
    (spans, state)
}

fn initial_state(syntax: &SyntaxReference) -> State {
    let highlighter = SyntectHighlighter::new(&assets().theme);
    State {
        parse: ParseState::new(syntax),
        highlight: HighlightState::new(&highlighter, ScopeStack::new()),
    }
}

fn to_style(style: syntect::highlighting::Style) -> Style {
    let color = style.foreground;
    Style {
        fg: (u32::from(color.r) << 16) | (u32::from(color.g) << 8) | u32::from(color.b),
        bold: style.font_style.contains(FontStyle::BOLD),
        italic: style.font_style.contains(FontStyle::ITALIC),
    }
}

fn plain_style() -> Style {
    Style {
        fg: PLAIN_FG,
        bold: false,
        italic: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn lines_of(text: &[String]) -> impl Fn(usize) -> String + '_ {
        move |i| text[i].clone()
    }

    #[test]
    fn out_of_range_edits_do_not_panic() {
        let text: Vec<String> = (0..20).map(|i| format!("let x{i} = {i};")).collect();
        let mut h = Highlighter::new(Some(Path::new("a.rs")), &text[0]);
        h.spans(|i| text[i].clone(), text.len(), 19);
        h.invalidate(5, 500);
        h.edited(40, 3, 1);
        h.invalidate(9, 3);
        assert!(!h.spans(|i| text[i].clone(), text.len(), 19).is_empty());
    }

    #[test]
    fn keyword_and_identifier_get_different_colours() {
        let text: Vec<String> = ["fn main() {", "    let x = 1;", "}"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let mut h = Highlighter::new(Some(Path::new("main.rs")), &text[0]);
        let spans = h.spans(lines_of(&text), text.len(), 0);
        let color_at = |byte: usize| {
            spans
                .iter()
                .find(|(range, _)| range.contains(&byte))
                .map(|(_, style)| style.fg)
        };
        assert!(color_at(0).is_some());
        assert_ne!(color_at(0), color_at(3));
    }

    #[test]
    fn unknown_extension_gives_one_plain_span_per_line() {
        let text = vec!["hello world".to_string(), String::new()];
        let mut h = Highlighter::new(Some(Path::new("notes.zzqx")), &text[0]);
        assert_eq!(
            h.spans(lines_of(&text), text.len(), 0),
            vec![(0..11, plain_style())]
        );
        assert_eq!(h.spans(lines_of(&text), text.len(), 1), vec![(0..0, plain_style())]);
    }

    #[test]
    fn edit_at_line_zero_then_line_999_matches_fresh_parse() {
        let mut text: Vec<String> = (0..1000).map(|i| format!("let v{i} = {i};")).collect();
        let mut h = Highlighter::new(Some(Path::new("a.rs")), &text[0]);
        assert!(!h.spans(lines_of(&text), text.len(), 999).is_empty());

        // Changes the state that every later line starts from.
        text[0] = "/* open comment".to_string();
        h.invalidate(0, 0);
        let mut fresh = Highlighter::new(Some(Path::new("a.rs")), &text[0]);
        assert_eq!(
            h.spans(lines_of(&text), text.len(), 999),
            fresh.spans(lines_of(&text), text.len(), 999)
        );

        // Changes a line without changing the state after it, so the cache is reused.
        text[0] = "let v0 = 10;".to_string();
        h.invalidate(0, 0);
        let mut fresh = Highlighter::new(Some(Path::new("a.rs")), &text[0]);
        assert_eq!(
            h.spans(lines_of(&text), text.len(), 999),
            fresh.spans(lines_of(&text), text.len(), 999)
        );
        assert_eq!(
            h.spans(lines_of(&text), text.len(), 500),
            fresh.spans(lines_of(&text), text.len(), 500)
        );
    }

    #[test]
    fn very_long_line_is_one_unstyled_span() {
        let text = vec!["x".repeat(MAX_LINE_CHARS + 1)];
        let mut h = Highlighter::new(Some(Path::new("a.rs")), "");
        assert_eq!(
            h.spans(lines_of(&text), text.len(), 0),
            vec![(0..text[0].len(), plain_style())]
        );
    }

    #[test]
    fn budgeted_spans_stop_at_deadline_then_finish() {
        let text: Vec<String> = (0..50_000).map(|i| format!("let v{i} = {i};")).collect();
        let mut h = Highlighter::new(Some(Path::new("a.rs")), &text[0]);

        let start = Instant::now();
        let deadline = start + Duration::from_millis(20);
        let early = h.spans_by(lines_of(&text), text.len(), 25_000, deadline);
        assert!(early.is_none());
        assert!(start.elapsed() < Duration::from_millis(200));

        let mut ready = h.ready_through();
        let mut calls = 1;
        let budgeted = loop {
            let deadline = Instant::now() + Duration::from_millis(20);
            if let Some(spans) = h.spans_by(lines_of(&text), text.len(), 25_000, deadline) {
                break spans;
            }
            calls += 1;
            assert!(calls < 100_000, "highlighting never finished");
            assert!(h.ready_through() >= ready);
            ready = h.ready_through();
        };
        assert!(calls > 1);
        assert!(h.ready_through() >= 25_000);

        let mut fresh = Highlighter::new(Some(Path::new("a.rs")), &text[0]);
        assert_eq!(budgeted, fresh.spans(lines_of(&text), text.len(), 25_000));
    }

    #[test]
    fn inserted_line_reuses_cached_lines_after_it() {
        let mut text: Vec<String> = (0..2_000).map(|i| format!("let v{i} = {i};")).collect();
        let mut h = Highlighter::new(Some(Path::new("a.rs")), &text[0]);
        h.spans(lines_of(&text), text.len(), 1_999);

        // Line 1000 becomes two lines; the lines after it move down by one.
        text.splice(
            1000..1001,
            ["let v1000 = 1000;".to_string(), "let split = 0;".to_string()],
        );
        h.edited(1000, 1, 2);

        PARSED_LINES.with(|count| count.set(0));
        let shifted = h.spans(lines_of(&text), text.len(), 2_000);
        let parsed = PARSED_LINES.with(|count| count.get());
        assert!(parsed < 50, "re-parsed {parsed} lines");

        let mut fresh = Highlighter::new(Some(Path::new("a.rs")), &text[0]);
        assert_eq!(shifted, fresh.spans(lines_of(&text), text.len(), 2_000));
        for line in 1_000..text.len() {
            assert_eq!(
                h.spans(lines_of(&text), text.len(), line),
                fresh.spans(lines_of(&text), text.len(), line),
                "line {line}"
            );
        }
    }

    #[test]
    fn opening_comment_recolours_later_lines_after_edit() {
        let mut text: Vec<String> = (0..100).map(|i| format!("let v{i} = {i};")).collect();
        let mut h = Highlighter::new(Some(Path::new("a.rs")), &text[0]);
        let before = h.spans(lines_of(&text), text.len(), 20);
        h.spans(lines_of(&text), text.len(), 99);

        text[10] = format!("/* {}", text[10]);
        h.edited(10, 1, 1);

        let mut fresh = Highlighter::new(Some(Path::new("a.rs")), &text[0]);
        for line in 10..50 {
            assert_eq!(
                h.spans(lines_of(&text), text.len(), line),
                fresh.spans(lines_of(&text), text.len(), line),
                "line {line}"
            );
        }
        assert_ne!(h.spans(lines_of(&text), text.len(), 20), before);
    }

    #[test]
    fn deleting_opening_comment_matches_fresh_parse() {
        let mut text: Vec<String> = (0..100).map(|i| format!("let v{i} = {i};")).collect();
        text[10] = "/* open".to_string();
        let mut h = Highlighter::new(Some(Path::new("a.rs")), &text[0]);
        h.spans(lines_of(&text), text.len(), 99);

        text.remove(10);
        h.edited(10, 1, 0);

        let mut fresh = Highlighter::new(Some(Path::new("a.rs")), &text[0]);
        for line in 0..text.len() {
            assert_eq!(
                h.spans(lines_of(&text), text.len(), line),
                fresh.spans(lines_of(&text), text.len(), line),
                "line {line}"
            );
        }
    }
}

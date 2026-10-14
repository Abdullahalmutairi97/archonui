//! Chart rows and code tokens for the rich reply blocks (chat.md §1.6 and §1.7). Pure, so the rules are
//! tested without GPUI.

use std::ops::Range;

/// Words coloured as keywords (chat.md §1.6, `KW`).
const KEYWORDS: &[&str] = &[
    "const", "let", "var", "function", "return", "export", "import", "from", "async", "await", "if", "else", "for",
    "while", "new", "type", "interface", "class", "extends", "of", "in", "try", "catch", "throw", "default",
    "private", "constructor", "describe", "it", "expect",
];

/// Words coloured as literals (chat.md §1.6, `LIT`).
const LITERALS: &[&str] = &["null", "undefined", "true", "false", "this"];

/// One bar of a chart: `label` is every word but the last, `value` the last word as a number.
#[derive(Debug, Clone, PartialEq)]
pub struct ChartRow {
    pub label: String,
    pub value: f64,
}

/// The rows of a chart body. Lines with a single word are skipped; a non-numeric value counts as 0.
pub fn chart_rows(body: &str) -> Vec<ChartRow> {
    body.lines()
        .filter_map(|line| {
            let words: Vec<&str> = line.split_whitespace().collect();
            let (value, label) = words.split_last().filter(|_| words.len() > 1)?;
            Some(ChartRow { label: label.join(" "), value: value.parse().unwrap_or(0.) })
        })
        .collect()
}

/// The colour class of a code span (chat.md §1.6's table).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tok {
    Plain,
    Comment,
    Str,
    Number,
    Keyword,
    Literal,
    /// An identifier starting with an uppercase letter (a type or class).
    Upper,
    Punct,
}

/// Splits one line of code into coloured spans. Adjacent spans of one class are merged. The spans cover
/// the whole line, so an empty line yields no spans.
pub fn tokenize(line: &str) -> Vec<(Range<usize>, Tok)> {
    let mut spans: Vec<(Range<usize>, Tok)> = Vec::new();
    let mut push = |range: Range<usize>, tok: Tok| match spans.last_mut() {
        Some((last, last_tok)) if *last_tok == tok && last.end == range.start => last.end = range.end,
        _ => spans.push((range, tok)),
    };
    let bytes = line.as_bytes();
    let mut i = 0;
    while i < line.len() {
        let rest = &line[i..];
        let c = bytes[i];
        let end = if rest.starts_with("//") || (i == 0 && c == b'#') {
            Some((line.len(), Tok::Comment))
        } else if matches!(c, b'"' | b'\'' | b'`') {
            rest[1..].find(c as char).map(|close| (i + close + 2, Tok::Str))
        } else if c.is_ascii_alphanumeric() || c == b'_' || c == b'$' {
            let run = rest.find(|ch: char| !(ch.is_ascii_alphanumeric() || ch == '_' || ch == '$')).unwrap_or(rest.len());
            let word = &rest[..run];
            let tok = if word.bytes().all(|b| b.is_ascii_digit()) {
                Tok::Number
            } else if KEYWORDS.contains(&word) {
                Tok::Keyword
            } else if LITERALS.contains(&word) {
                Tok::Literal
            } else if word.starts_with(|ch: char| ch.is_ascii_uppercase()) {
                Tok::Upper
            } else {
                Tok::Plain
            };
            Some((i + run, tok))
        } else if c.is_ascii_whitespace() {
            let run = rest.find(|ch: char| !ch.is_whitespace()).unwrap_or(rest.len());
            Some((i + run, Tok::Plain))
        } else if b"{}()[];,.:=<>".contains(&c) {
            Some((i + 1, Tok::Punct))
        } else {
            None
        };
        let (end, tok) = end.unwrap_or_else(|| (i + char_len(rest), Tok::Plain));
        push(i..end, tok);
        i = end;
    }
    spans
}

/// Byte length of the first character of `s`.
fn char_len(s: &str) -> usize {
    s.chars().next().map_or(1, char::len_utf8)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chart_rows_split_label_and_value() {
        let rows = chart_rows("09:00 38\n\nlone\n14:00 91\nRun A 2");
        let pairs: Vec<(&str, f64)> = rows.iter().map(|r| (r.label.as_str(), r.value)).collect();
        assert_eq!(pairs, [("09:00", 38.), ("14:00", 91.), ("Run A", 2.)]);
    }

    #[test]
    fn non_numeric_chart_values_count_as_zero() {
        assert_eq!(chart_rows("Mon n/a")[0].value, 0.);
    }

    fn classes(line: &str) -> Vec<(&str, Tok)> {
        tokenize(line).into_iter().map(|(r, t)| (&line[r], t)).collect()
    }

    #[test]
    fn tokens_follow_the_prototype_classes() {
        assert_eq!(
            classes("const hit = Map.get(\"k\", 300);"),
            [
                ("const", Tok::Keyword),
                (" hit ", Tok::Plain),
                ("=", Tok::Punct),
                (" ", Tok::Plain),
                ("Map", Tok::Upper),
                (".", Tok::Punct),
                ("get", Tok::Plain),
                ("(", Tok::Punct),
                ("\"k\"", Tok::Str),
                (",", Tok::Punct),
                (" ", Tok::Plain),
                ("300", Tok::Number),
                (");", Tok::Punct),
            ]
        );
    }

    #[test]
    fn comments_run_to_the_end_of_the_line() {
        assert_eq!(classes("x // note this"), [("x ", Tok::Plain), ("// note this", Tok::Comment)]);
        assert_eq!(classes("# shell"), [("# shell", Tok::Comment)]);
    }

    #[test]
    fn literals_and_unclosed_quotes() {
        assert_eq!(classes("this"), [("this", Tok::Literal)]);
        assert_eq!(classes("it's"), [("it", Tok::Keyword), ("'s", Tok::Plain)]);
    }

    #[test]
    fn non_ascii_text_is_kept_whole() {
        let line = "é = 1";
        let spans = tokenize(line);
        assert_eq!(spans.first().map(|(r, _)| r.start), Some(0));
        assert_eq!(spans.last().map(|(r, _)| r.end), Some(line.len()));
    }
}

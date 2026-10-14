//! Agent text as blocks: pulldown-cmark events folded into paragraphs, headings, lists, quotes,
//! tables and rules, plus the fenced blocks of chat.md §1.6–§1.8 (code, chart, image). Pure data, so
//! the rules are tested without GPUI.

use super::code::{chart_rows, tokenize, ChartRow, Tok};
use pulldown_cmark::{CodeBlockKind, Event, Options, Parser, Tag, TagEnd};
use std::{
    iter::Peekable,
    ops::Range,
    path::{Path, PathBuf},
};

/// Image extensions shown as image blocks (chat.md §3.1).
const IMAGE_EXTENSIONS: [&str; 5] = ["png", "jpg", "jpeg", "gif", "webp"];

/// One block of an agent reply.
#[derive(Debug, Clone, PartialEq)]
pub enum Block {
    Text(Text),
    List { ordered: bool, start: u64, items: Vec<Vec<Block>> },
    Quote(Vec<Block>),
    Table { head: Vec<Text>, rows: Vec<Vec<Text>> },
    Rule,
    Code(Code),
    Chart { title: String, rows: Vec<ChartRow> },
    Image(Image),
}

/// Inline text and the styling runs over it. `id` is unique within one message, so a selection can name
/// its block. Runs may overlap (a link inside strong text).
#[derive(Debug, Clone, PartialEq)]
pub struct Text {
    pub id: usize,
    /// The heading level (1–6), or `None` for body text.
    pub heading: Option<u8>,
    pub text: String,
    pub marks: Vec<(Range<usize>, Mark)>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Mark {
    Strong,
    Emphasis,
    Code,
    Link(String),
}

/// A single tokenized line in a fenced code block.
#[derive(Debug, Clone, PartialEq)]
pub struct CodeLine {
    pub text: String,
    pub spans: Vec<(Range<usize>, Tok)>,
}

/// A fenced code block: ```` ```<lang> <path> ````.
#[derive(Debug, Clone, PartialEq)]
pub struct Code {
    pub lang: String,
    pub path: Option<String>,
    pub body: String,
    pub lines: Vec<CodeLine>,
}

impl Code {
    pub fn new(lang: String, path: Option<String>, body: String) -> Self {
        let lines = body
            .lines()
            .map(|line| CodeLine {
                text: if line.is_empty() { " ".to_string() } else { line.to_string() },
                spans: tokenize(line),
            })
            .collect();
        Self { lang, path, body, lines }
    }
}

/// An image block from a ```` ```image <path> <name> ```` fence, a markdown image, or a bare image path.
#[derive(Debug, Clone, PartialEq)]
pub struct Image {
    pub source: Source,
    pub name: String,
    pub caption: String,
}

/// Where an image comes from. Remote images are only linked; local files are drawn.
#[derive(Debug, Clone, PartialEq)]
pub enum Source {
    Local(PathBuf),
    Missing(PathBuf),
    Remote(String),
}

type Events<'a> = Peekable<Parser<'a>>;

/// Parses an agent reply. Relative image paths resolve against `cwd`, the session's folder.
pub fn parse(markdown: &str, cwd: &Path) -> Vec<Block> {
    let options = Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TASKLISTS;
    let mut events = Parser::new_ext(markdown, options).peekable();
    let mut ids = 0;
    blocks(&mut events, &mut ids, cwd)
}

/// The blocks up to the end of the enclosing container, which is consumed.
fn blocks(events: &mut Events, ids: &mut usize, cwd: &Path) -> Vec<Block> {
    let mut out = Vec::new();
    while let Some(event) = events.peek().cloned() {
        match event {
            Event::End(_) => {
                events.next();
                break;
            }
            Event::Start(Tag::Paragraph) => {
                events.next();
                push_inline(&mut out, inline(events, ids, cwd, Some(TagEnd::Paragraph)), cwd);
            }
            Event::Start(Tag::Heading { level, .. }) => {
                events.next();
                let (mut text, images) = inline(events, ids, cwd, Some(TagEnd::Heading(level)));
                text.heading = Some(level as u8);
                push_inline(&mut out, (text, images), cwd);
            }
            Event::Start(Tag::BlockQuote(_)) => {
                events.next();
                out.push(Block::Quote(blocks(events, ids, cwd)));
            }
            Event::Start(Tag::List(start)) => {
                events.next();
                let mut items = Vec::new();
                loop {
                    match events.peek() {
                        Some(Event::Start(Tag::Item)) => {
                            events.next();
                            items.push(blocks(events, ids, cwd));
                        }
                        Some(Event::End(_)) => {
                            events.next();
                            break;
                        }
                        None => break,
                        Some(_) => {
                            events.next();
                        }
                    }
                }
                out.push(Block::List { ordered: start.is_some(), start: start.unwrap_or(1), items });
            }
            Event::Start(Tag::CodeBlock(kind)) => {
                events.next();
                let info = match kind {
                    CodeBlockKind::Fenced(info) => info.to_string(),
                    CodeBlockKind::Indented => String::new(),
                };
                let mut body = String::new();
                while let Some(event) = events.next() {
                    match event {
                        Event::Text(text) => body.push_str(&text),
                        Event::End(_) => break,
                        _ => {}
                    }
                }
                out.push(fence(&info, body.strip_suffix('\n').unwrap_or(&body), cwd));
            }
            Event::Start(Tag::Table(_)) => {
                events.next();
                out.push(table(events, ids, cwd));
            }
            Event::Start(Tag::HtmlBlock) => {
                events.next();
                let mut html = String::new();
                while let Some(event) = events.next() {
                    match event {
                        Event::Html(text) | Event::Text(text) => html.push_str(&text),
                        Event::End(_) => break,
                        _ => {}
                    }
                }
                let id = take_id(ids);
                out.push(Block::Text(Text { id, heading: None, text: html.trim_end().to_string(), marks: Vec::new() }));
            }
            Event::Rule => {
                events.next();
                out.push(Block::Rule);
            }
            // Tight list items carry their text without a paragraph around it.
            Event::Text(_)
            | Event::Code(_)
            | Event::SoftBreak
            | Event::HardBreak
            | Event::InlineHtml(_)
            | Event::Start(Tag::Emphasis | Tag::Strong | Tag::Strikethrough | Tag::Link { .. } | Tag::Image { .. }) => {
                push_inline(&mut out, inline(events, ids, cwd, None), cwd);
            }
            _ => {
                events.next();
            }
        }
    }
    out
}

/// Adds a folded inline run as a text block, then the images that came with it and any bare image
/// paths in its text.
fn push_inline(out: &mut Vec<Block>, (text, images): (Text, Vec<Block>), cwd: &Path) {
    if text.text.trim().is_empty() {
        out.extend(images);
        return;
    }
    let found = bare_images(&text.text, cwd);
    out.push(Block::Text(text));
    out.extend(images);
    out.extend(found);
}

/// The inline text up to `until` (consumed), or up to the first block-level event when `until` is `None`.
/// Returns the text and the images it contained.
fn inline(events: &mut Events, ids: &mut usize, cwd: &Path, until: Option<TagEnd>) -> (Text, Vec<Block>) {
    let mut text = String::new();
    let mut marks = Vec::new();
    // Open inline containers: where their text starts, and the mark to record at their end.
    let mut open: Vec<(usize, Option<Mark>)> = Vec::new();
    let mut images = Vec::new();
    while let Some(event) = events.peek().cloned() {
        match event {
            Event::Text(t) => {
                events.next();
                text.push_str(&t);
            }
            Event::InlineHtml(t) => {
                events.next();
                text.push_str(&t);
            }
            Event::Code(code) => {
                events.next();
                let start = text.len();
                text.push_str(&code);
                marks.push((start..text.len(), Mark::Code));
            }
            Event::SoftBreak | Event::HardBreak => {
                events.next();
                text.push('\n');
            }
            Event::Start(Tag::Emphasis) => {
                events.next();
                open.push((text.len(), Some(Mark::Emphasis)));
            }
            Event::Start(Tag::Strong) => {
                events.next();
                open.push((text.len(), Some(Mark::Strong)));
            }
            Event::Start(Tag::Link { dest_url, .. }) => {
                events.next();
                open.push((text.len(), Some(Mark::Link(dest_url.to_string()))));
            }
            Event::Start(Tag::Strikethrough) => {
                events.next();
                open.push((text.len(), None));
            }
            Event::End(TagEnd::Emphasis | TagEnd::Strong | TagEnd::Link | TagEnd::Strikethrough) => {
                events.next();
                if let Some((start, Some(mark))) = open.pop() {
                    marks.push((start..text.len(), mark));
                }
            }
            Event::End(end) => {
                if until == Some(end) {
                    events.next();
                }
                break;
            }
            Event::Start(Tag::Image { dest_url, .. }) => {
                events.next();
                let mut alt = String::new();
                while let Some(event) = events.next() {
                    match event {
                        Event::Text(t) => alt.push_str(&t),
                        Event::End(_) => break,
                        _ => {}
                    }
                }
                images.push(Block::Image(markdown_image(&dest_url, &alt, cwd)));
            }
            _ if until.is_none() => break,
            _ => {
                events.next();
            }
        }
    }
    let id = take_id(ids);
    (Text { id, heading: None, text, marks }, images)
}

/// A table: the head row, then body rows, each cell as inline text.
fn table(events: &mut Events, ids: &mut usize, cwd: &Path) -> Block {
    let mut head = Vec::new();
    let mut rows = Vec::new();
    let mut row = Vec::new();
    while let Some(event) = events.next() {
        match event {
            Event::Start(Tag::TableCell) => row.push(inline(events, ids, cwd, Some(TagEnd::TableCell)).0),
            Event::Start(Tag::TableRow) => row.clear(),
            Event::End(TagEnd::TableHead) => head = std::mem::take(&mut row),
            Event::End(TagEnd::TableRow) => rows.push(std::mem::take(&mut row)),
            Event::End(TagEnd::Table) => break,
            _ => {}
        }
    }
    Block::Table { head, rows }
}

/// The block for a fenced code block's info string and body (chat.md §1.6–§1.8).
fn fence(info: &str, body: &str, cwd: &Path) -> Block {
    let info = info.trim();
    let (lang, rest) = info.split_once(char::is_whitespace).map_or((info, ""), |(l, r)| (l, r.trim()));
    match lang {
        "chart" => Block::Chart {
            title: if rest.is_empty() { "Chart".to_string() } else { rest.to_string() },
            rows: chart_rows(body),
        },
        "image" => {
            let mut words = rest.split_whitespace();
            let path = words.next().unwrap_or_default();
            let name = words.next().map_or_else(|| basename(path), str::to_string);
            Block::Image(Image { source: source_of(path, cwd), name, caption: body.trim().to_string() })
        }
        _ => Block::Code(Code::new(
            lang.to_string(),
            (!rest.is_empty()).then(|| rest.to_string()),
            body.to_string(),
        )),
    }
}

fn markdown_image(dest: &str, alt: &str, cwd: &Path) -> Image {
    let name = if alt.trim().is_empty() { basename(dest) } else { alt.trim().to_string() };
    Image { source: source_of(dest, cwd), name, caption: String::new() }
}

/// Image paths in running text that exist on disk, in order. Only whole words are considered.
fn bare_images(text: &str, cwd: &Path) -> Vec<Block> {
    text.split_whitespace()
        .map(|word| word.trim_matches(|c: char| "`\"'()[]<>,;:".contains(c)).trim_end_matches('.'))
        .filter(|word| is_image_path(word))
        .map(|word| Image { source: source_of(word, cwd), name: basename(word), caption: String::new() })
        .filter(|image| matches!(image.source, Source::Local(_)))
        .map(Block::Image)
        .collect()
}

fn is_image_path(word: &str) -> bool {
    let ext = word.rsplit('.').next().unwrap_or_default().to_ascii_lowercase();
    word.contains('.') && IMAGE_EXTENSIONS.contains(&ext.as_str())
}

/// Where the image at `path` comes from: a URL, a file under `cwd` (or absolute), or a missing file.
fn source_of(path: &str, cwd: &Path) -> Source {
    if path.starts_with("http://") || path.starts_with("https://") {
        return Source::Remote(path.to_string());
    }
    let full = if Path::new(path).is_absolute() { PathBuf::from(path) } else { cwd.join(path) };
    if full.is_file() {
        Source::Local(full)
    } else {
        Source::Missing(full)
    }
}

fn basename(path: &str) -> String {
    path.rsplit('/').next().unwrap_or(path).to_string()
}

fn take_id(ids: &mut usize) -> usize {
    *ids += 1;
    *ids - 1
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cwd() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
    }

    fn text(block: &Block) -> &Text {
        match block {
            Block::Text(t) => t,
            other => panic!("not text: {other:?}"),
        }
    }

    #[test]
    fn paragraphs_and_marks_keep_their_ranges() {
        let blocks = parse("Use **bold** and `code` and [docs](https://x.y).", &cwd());
        let t = text(&blocks[0]);
        assert_eq!(t.text, "Use bold and code and docs.");
        assert!(t.marks.contains(&(4..8, Mark::Strong)));
        assert!(t.marks.contains(&(13..17, Mark::Code)));
        assert!(t.marks.contains(&(22..26, Mark::Link("https://x.y".into()))));
    }

    #[test]
    fn headings_carry_their_level() {
        let blocks = parse("## Plan\n\nbody", &cwd());
        assert_eq!(text(&blocks[0]).heading, Some(2));
        assert_eq!(text(&blocks[1]).heading, None);
    }

    #[test]
    fn nested_lists_keep_their_items() {
        let blocks = parse("- one\n  - inner\n- two", &cwd());
        let Block::List { ordered, items, .. } = &blocks[0] else { panic!("{blocks:?}") };
        assert!(!ordered);
        assert_eq!(items.len(), 2);
        assert!(matches!(&items[0][1], Block::List { items, .. } if items.len() == 1));
        assert_eq!(text(&items[1][0]).text, "two");
    }

    #[test]
    fn ordered_lists_keep_their_start() {
        let blocks = parse("3. c\n4. d", &cwd());
        assert!(matches!(&blocks[0], Block::List { ordered: true, start: 3, .. }));
    }

    #[test]
    fn tables_split_head_and_rows() {
        let blocks = parse("| a | b |\n|---|---|\n| 1 | 2 |", &cwd());
        let Block::Table { head, rows } = &blocks[0] else { panic!("{blocks:?}") };
        assert_eq!(head.iter().map(|c| c.text.as_str()).collect::<Vec<_>>(), ["a", "b"]);
        assert_eq!(rows[0][1].text, "2");
    }

    #[test]
    fn fenced_code_takes_language_and_path() {
        let blocks = parse("```ts src/cache.ts\nget(key)\n```", &cwd());
        assert_eq!(
            blocks[0],
            Block::Code(Code::new("ts".into(), Some("src/cache.ts".into()), "get(key)".into()))
        );
    }

    #[test]
    fn chart_fence_parses_rows() {
        let blocks = parse("```chart Hit rate\n09:00 38\n10:00 61\n```", &cwd());
        let Block::Chart { title, rows } = &blocks[0] else { panic!("{blocks:?}") };
        assert_eq!(title, "Hit rate");
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[1].label, "10:00");
        assert_eq!(rows[1].value, 61.0);
    }

    #[test]
    fn image_fence_resolves_local_files() {
        let blocks = parse("```image assets/themes/wallpapers/bonfire.jpg fire.png\nfire.wgsl · 1920×1080\n```", &cwd());
        let Block::Image(image) = &blocks[0] else { panic!("{blocks:?}") };
        assert_eq!(image.name, "fire.png");
        assert_eq!(image.caption, "fire.wgsl · 1920×1080");
        assert!(matches!(&image.source, Source::Local(p) if p.ends_with("bonfire.jpg")));
    }

    #[test]
    fn markdown_images_and_remote_urls() {
        let blocks = parse("![the sky](assets/themes/wallpapers/bonfire.jpg)\n\n![x](https://example.com/a.png)", &cwd());
        assert!(matches!(&blocks[0], Block::Image(i) if i.name == "the sky"));
        assert!(matches!(&blocks[1], Block::Image(i) if matches!(i.source, Source::Remote(_))));
    }

    #[test]
    fn bare_existing_image_paths_become_images() {
        let blocks = parse("Frame saved to assets/themes/wallpapers/bonfire.jpg.", &cwd());
        assert!(matches!(&blocks[0], Block::Text(_)));
        assert!(matches!(&blocks[1], Block::Image(i) if i.name == "bonfire.jpg"));
        let none = parse("See missing/shot.png", &cwd());
        assert_eq!(none.len(), 1);
    }

    #[test]
    fn rules_and_quotes() {
        let blocks = parse("> quoted\n\n---", &cwd());
        assert!(matches!(&blocks[0], Block::Quote(inner) if text(&inner[0]).text == "quoted"));
        assert_eq!(blocks[1], Block::Rule);
    }
}

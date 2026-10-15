//! Rows of a message stream: user, agent and error messages, tool-call rows, and the text, list, table,
//! code, chart and image blocks of chat.md §1.3–§1.8 with their Copy / Open / Attach controls
//! (tools-and-behaviour.md §C 20–26).

use super::{
    code::{tokenize, Tok},
    markdown::{Block, Code, Image as Figure, Mark, Source, Text},
    Item, Lightbox, RowCtx, Selection, StreamHost,
};
use crate::{
    engine::{attach::{human_size, Attachment, Kind}, Mode},
    store::{Role, ToolCall, ToolKind},
    ui::{
        glass,
        theme::{Rgba, Theme},
        widgets::{AttachmentCard, CardVariant},
    },
};
use gpui::{
    div, img, linear_color_stop, linear_gradient, prelude::*, relative, AnyElement, App, ClipboardItem, Div,
    FontWeight, HighlightStyle, Hsla, Image as GpuiImage, ImageFormat, MouseButton, MouseDownEvent,
    MouseMoveEvent, MouseUpEvent, ObjectFit, Point, Pixels, SharedString, StyledText, TextLayout, UnderlineStyle,
    Window,
};
use crate::ui::scale::px;
use std::{ops::Range, sync::Arc};

/// The app logo, or nothing when the theme has none. `size` is the square edge in pixels.
pub(super) fn logo(theme: &Theme, size: f32) -> Option<AnyElement> {
    let bytes = glass::logo_bytes(theme.logo)?;
    let image = Arc::new(GpuiImage::from_bytes(ImageFormat::Png, bytes.to_vec()));
    Some(img(image).size(px(size)).into_any_element())
}

/// One message of the stream, centred, with the 16px gap below it (chat.md §1.1).
pub(super) fn message_row<H: StreamHost>(ctx: &RowCtx<H>, ix: usize, item: &Item) -> AnyElement {
    let row = match item.role {
        Role::User => user_row(ctx, ix, item),
        Role::Agent => agent_row(ctx, ix, item),
        Role::Error => error_row(ctx, ix, item),
    };
    // The conversation is a centred column, so on a wide window it reads down the middle. Auto margins
    // in block layout, not a flex row: the list measures each row's height, and a flex row made it
    // measure the text at a different width than it drew it, so messages overlapped.
    div().pb(px(16.)).child(div().max_w(px(COLUMN_WIDTH)).mx_auto().child(row)).into_any_element()
}

/// Width of the centred conversation column: the reply text width plus room for the header,
/// tool rows and the user's bubbles.
const COLUMN_WIDTH: f32 = 880.;

/// A user message: right-aligned, with its attachment cards above the bubble (chat.md §1.3, §1.9).
fn user_row<H: StreamHost>(ctx: &RowCtx<H>, ix: usize, item: &Item) -> AnyElement {
    let theme = &ctx.theme;
    let cards = item.attachments.iter().enumerate().map(|(n, file)| attachment_card(ctx, ix, n, file));
    let bubble = div()
        .px(px(14.))
        .py(px(11.))
        .rounded_tl(px(10.))
        .rounded_tr(px(10.))
        .rounded_br(px(3.))
        .rounded_bl(px(10.))
        .bg(theme.soft.to_hsla())
        .line_height(relative(1.55))
        .children(item.blocks.iter().map(|block| block_view(ctx, ix, "0", block)));
    div()
        .w_full()
        .flex()
        .justify_end()
        .child(
            div()
                .max_w(relative(0.82))
                .flex()
                .flex_col()
                .items_end()
                .gap(px(6.))
                .when(!item.attachments.is_empty(), |column| {
                    column.child(div().flex().flex_wrap().justify_end().gap(px(6.)).children(cards))
                })
                .child(bubble),
        )
        .into_any_element()
}

/// An error message: a bordered box in the accent colour (no spec for error styling; see the stream notes).
fn error_row<H: StreamHost>(ctx: &RowCtx<H>, ix: usize, item: &Item) -> AnyElement {
    let accent = ctx.theme.acc.to_hsla();
    div()
        .max_w(relative(0.94))
        .px(px(12.))
        .py(px(9.))
        .rounded(px(8.))
        .border_1()
        .border_color(accent)
        .text_color(accent)
        .children(item.blocks.iter().map(|block| block_view(ctx, ix, "0", block)))
        .into_any_element()
}

/// An agent reply: header, tool-call group, the streaming label, then its blocks (chat.md §1.4).
fn agent_row<H: StreamHost>(ctx: &RowCtx<H>, ix: usize, item: &Item) -> AnyElement {
    let streaming = ctx.streaming == Some(ix);
    let theme = &ctx.theme;
    div()
        .flex()
        .flex_col()
        .gap(px(8.))
        .max_w(relative(0.94))
        .child(agent_header(ctx, item, streaming))
        .when(ctx.show_tools && !item.tools.is_empty(), |column| column.child(tool_group(&item.tools, theme)))
        .when(streaming, |column| {
            column.child(
                div()
                    .text_size(px(13.))
                    .text_color(theme.muted.to_hsla())
                    .italic()
                    .child(format!("Working · {} tool calls", item.tools.len())),
            )
        })
        .when(!item.blocks.is_empty(), |column| {
            column.child(
                block_column(ctx, ix, "", &item.blocks)
                    .max_w(px(READING_WIDTH))
                    .font_family(crate::ui::fonts::reading_font(&theme.ui_font)),
            )
        })
        .into_any_element()
}

/// Widest a reply's text runs, about 90 characters, so lines stay easy to follow on a wide window.
const READING_WIDTH: f32 = 820.;
/// Space between the paragraphs, lists, tables and code of a reply, so each stands apart.
const BLOCK_GAP: f32 = 14.;
/// Extra space above a heading that follows other blocks, so it opens a new section.
const HEADING_GAP: f32 = 10.;

/// `blocks` stacked with `BLOCK_GAP` between them. `path` prefixes each block's path.
fn block_column<H: StreamHost>(ctx: &RowCtx<H>, ix: usize, path: &str, blocks: &[Block]) -> Div {
    div().flex().flex_col().gap(px(BLOCK_GAP)).children(blocks.iter().enumerate().map(|(i, block)| {
        let path = if path.is_empty() { i.to_string() } else { format!("{path}.{i}") };
        let heading = matches!(block, Block::Text(Text { heading: Some(_), .. }));
        div().when(heading && i > 0, |d| d.pt(px(HEADING_GAP))).child(block_view(ctx, ix, &path, block))
    }))
}

/// `ARCHONUI · thought for 14s`, with the mode appended for non-Auto sessions (chat.md §1.4 item 1).
/// In a chat that switched agents, the reply's agent comes first: `ARCHONUI · CODEX CLI · …`.
fn agent_header<H: StreamHost>(ctx: &RowCtx<H>, item: &Item, streaming: bool) -> Div {
    let mut meta = Vec::new();
    if let Some(agent) = item.agent.as_deref().filter(|_| ctx.mixed_agents) {
        meta.push(crate::engine::agent_name(agent).to_uppercase());
    }
    if streaming {
        meta.push("working".to_string());
    } else if let Some(secs) = item.thought_secs {
        meta.push(format!("thought for {secs}s"));
    } else if let Some(secs) = item.took_secs {
        meta.push(format!("worked for {secs}s"));
    }
    if !matches!(ctx.mode, Mode::Auto) {
        meta.push(format!("{} mode", ctx.mode.name().to_lowercase()));
    }
    div()
        .flex()
        .items_center()
        .gap(px(8.))
        .text_size(px(11.))
        .text_color(ctx.theme.muted.to_hsla())
        .children(logo(&ctx.theme, 16.))
        .child(ctx.app_name.to_uppercase())
        .when(!meta.is_empty(), |header| header.child(format!("· {}", meta.join(" · "))))
}

/// The tool-call rows as one bordered group (chat.md §1.5).
fn tool_group(tools: &[ToolCall], theme: &Theme) -> AnyElement {
    div()
        .w_full()
        .min_w_0()
        .flex()
        .flex_col()
        .gap(px(1.))
        .rounded(px(8.))
        .overflow_hidden()
        .border_1()
        .border_color(theme.line2.to_hsla())
        .children(tools.iter().map(|tool| tool_row(tool, theme)))
        .into_any_element()
}

/// One tool-call row: icon, kind, target, and result. A failed call shows its result in the accent colour.
fn tool_row(tool: &ToolCall, theme: &Theme) -> AnyElement {
    let (glyph, kind) = tool_glyph(tool.kind);
    let result = if tool.ok == Some(false) { theme.acc } else { theme.alt };
    div()
        .w_full()
        .min_w_0()
        .flex()
        .items_center()
        .gap(px(10.))
        .h(px(30.))
        .px(px(11.))
        .text_size(px(12.))
        .bg(theme.line.to_hsla())
        .child(div().w(px(14.)).flex_none().text_center().text_color(theme.acc2.to_hsla()).child(glyph))
        .child(div().w(px(38.)).flex_none().text_color(theme.muted.to_hsla()).child(kind))
        .child(div().flex_1().min_w_0().truncate().text_color(theme.fg.to_hsla()).child(tool.target.clone()))
        .child(div().flex_none().text_color(result.to_hsla()).child(tool.detail.clone().unwrap_or_default()))
        .into_any_element()
}

/// The glyph and label of a tool kind. Read, grep, edit and run are the prototype's `TOOL_IC` glyphs; the rest
/// are chosen here because the prototype has none for them.
fn tool_glyph(kind: ToolKind) -> (&'static str, &'static str) {
    match kind {
        ToolKind::Read => ("◇", "read"),
        ToolKind::Grep => ("⌕", "grep"),
        ToolKind::Glob => ("⌕", "glob"),
        ToolKind::Edit => ("✎", "edit"),
        ToolKind::Write => ("✎", "write"),
        ToolKind::Run => ("▸", "run"),
        ToolKind::Web => ("◎", "web"),
        ToolKind::Fetch => ("◎", "fetch"),
        ToolKind::Task => ("◆", "task"),
        ToolKind::Other => ("·", "other"),
    }
}

/// One block of a reply. `path` names the block within its message (`"2"`, `"1.0.3"`), so element ids stay unique.
fn block_view<H: StreamHost>(ctx: &RowCtx<H>, ix: usize, path: &str, block: &Block) -> AnyElement {
    match block {
        Block::Text(text) => text_block(ctx, ix, text),
        Block::List { ordered, start, items } => list_block(ctx, ix, path, *ordered, *start, items),
        Block::Quote(inner) => block_column(ctx, ix, path, inner)
            .pl(px(12.))
            .border_l_2()
            .border_color(ctx.theme.line2.to_hsla())
            .text_color(ctx.theme.muted.to_hsla())
            .into_any_element(),
        Block::Table { head, rows } => table_block(ctx, ix, head, rows),
        Block::Rule => div().w_full().h(px(1.)).bg(ctx.theme.line.to_hsla()).into_any_element(),
        Block::Code(code) => code_block(ctx, ix, path, code),
        Block::Chart { title, rows } => chart_block(ctx, ix, path, title, rows),
        Block::Image(image) => image_block(ctx, ix, path, image),
    }
}

/// Inline text with its marks and any selection. Clicking a link opens it; dragging selects within the block.
fn text_block<H: StreamHost>(ctx: &RowCtx<H>, ix: usize, text: &Text) -> AnyElement {
    if text.text.is_empty() {
        return div().into_any_element();
    }
    let styled = StyledText::new(text.text.clone()).with_highlights(runs(text, &ctx.theme, selection_range(ctx, ix, text.id)));
    let layout = styled.layout().clone();
    let (session, block) = (ctx.session.clone(), text.id);
    let host = ctx.host.clone();
    let marks = text.marks.clone();

    let on_down = {
        let (layout, host, session) = (layout.clone(), host.clone(), session.clone());
        move |event: &MouseDownEvent, window: &mut Window, cx: &mut App| {
            let index = text_index(&layout, event.position);
            host.update(cx, |h, cx| {
                let ui = h.stream_ui();
                ui.selection = Some(Selection { session: session.clone(), item: ix, block, anchor: index, head: index });
                if let Some(focus) = ui.focus.clone() {
                    focus.focus(window);
                }
                cx.notify();
            });
        }
    };
    let on_drag = {
        let (layout, host, session) = (layout.clone(), host.clone(), session.clone());
        move |event: &MouseMoveEvent, _window: &mut Window, cx: &mut App| {
            if !event.dragging() {
                return;
            }
            let index = text_index(&layout, event.position);
            host.update(cx, |h, cx| {
                if let Some(selection) = h.stream_ui().selection.as_mut().filter(|s| {
                    s.session == session && s.item == ix && s.block == block
                }) {
                    selection.head = index;
                    cx.notify();
                }
            });
        }
    };
    let on_up = move |event: &MouseUpEvent, _window: &mut Window, cx: &mut App| {
        // A release past the end of the text is not a click on the link that ends the line.
        let Ok(index) = layout.index_for_position(event.position) else { return };
        let Some(url) = marks.iter().find_map(|(range, mark)| match mark {
            Mark::Link(url) if range.contains(&index) => Some(url.clone()),
            _ => None,
        }) else {
            return;
        };
        host.update(cx, |h, cx| {
            // A press that never moved, in this block, is a click; a drag ends with anchor and head apart, or
            // began in another block.
            let clicked = h
                .stream_ui()
                .selection
                .as_ref()
                .is_some_and(|s| s.anchor == s.head && s.session == session && s.item == ix && s.block == block);
            if clicked {
                h.stream_open_link(&url, cx);
            }
        });
    };

    div()
        .line_height(relative(1.65))
        // A paragraph that starts in a right-to-left script reads from the right edge.
        .when(starts_rtl(&text.text), |body| body.w_full().text_right())
        .cursor_text()
        .on_mouse_down(MouseButton::Left, on_down)
        .on_mouse_move(on_drag)
        .on_mouse_up(MouseButton::Left, on_up)
        .when_some(text.heading, |body, level| {
            body.text_size(px(ctx.text_size * heading_scale(level)))
                .line_height(relative(1.3))
                .font_weight(FontWeight::SEMIBOLD)
        })
        .child(styled)
        .into_any_element()
}

/// A heading's size relative to body text: `#` largest, `###` and below just above body size.
fn heading_scale(level: u8) -> f32 {
    match level {
        1 => 1.4,
        2 => 1.22,
        _ => 1.08,
    }
}

/// Whether the first letter of `text` is in a right-to-left script (Arabic, Hebrew, Syriac, Thaana and
/// their presentation forms), which is how Unicode bidi picks a paragraph's direction.
pub(super) fn starts_rtl(text: &str) -> bool {
    text.chars().find(|c| c.is_alphabetic()).is_some_and(|c| {
        matches!(c as u32, 0x0590..=0x08FF | 0xFB1D..=0xFDFF | 0xFE70..=0xFEFF | 0x10E60..=0x10EFF | 0x1EE00..=0x1EEFF)
    })
}

fn text_index(layout: &TextLayout, position: Point<Pixels>) -> usize {
    match layout.index_for_position(position) {
        Ok(index) | Err(index) => index,
    }
}

fn selection_range<H: StreamHost>(ctx: &RowCtx<H>, ix: usize, block: usize) -> Range<usize> {
    ctx.selection
        .as_ref()
        .filter(|s| s.session == ctx.session && s.item == ix && s.block == block)
        .map(Selection::range)
        .unwrap_or(0..0)
}

/// The marks and the selection as non-overlapping styled runs, which `StyledText` requires.
fn runs(text: &Text, theme: &Theme, selected: Range<usize>) -> Vec<(Range<usize>, HighlightStyle)> {
    let mut edges = vec![0, text.text.len(), selected.start, selected.end];
    for (range, _) in &text.marks {
        edges.push(range.start);
        edges.push(range.end);
    }
    edges.retain(|&edge| edge <= text.text.len());
    edges.sort_unstable();
    edges.dedup();
    edges
        .windows(2)
        .filter_map(|pair| {
            let run = pair[0]..pair[1];
            let mut style = text
                .marks
                .iter()
                .filter(|(range, _)| range.start <= run.start && run.end <= range.end)
                .fold(HighlightStyle::default(), |style, (_, mark)| style.highlight(mark_style(mark, theme)));
            if !selected.is_empty() && selected.start <= run.start && run.end <= selected.end {
                style = style.highlight(HighlightStyle {
                    background_color: Some(theme.soft.to_hsla()),
                    ..Default::default()
                });
            }
            (style != HighlightStyle::default()).then_some((run, style))
        })
        .collect()
}

fn mark_style(mark: &Mark, theme: &Theme) -> HighlightStyle {
    match mark {
        Mark::Strong => HighlightStyle { font_weight: Some(FontWeight::BOLD), ..Default::default() },
        Mark::Emphasis => HighlightStyle { font_style: Some(gpui::FontStyle::Italic), ..Default::default() },
        Mark::Code => HighlightStyle { background_color: Some(theme.line2.to_hsla()), ..Default::default() },
        Mark::Link(_) => HighlightStyle {
            color: Some(theme.acc2.to_hsla()),
            underline: Some(UnderlineStyle { thickness: px(1.), color: None, wavy: false }),
            ..Default::default()
        },
    }
}

fn list_block<H: StreamHost>(
    ctx: &RowCtx<H>,
    ix: usize,
    path: &str,
    ordered: bool,
    start: u64,
    items: &[Vec<Block>],
) -> AnyElement {
    div()
        .flex()
        .flex_col()
        .gap(px(6.))
        .children(items.iter().enumerate().map(|(i, item)| {
            let marker = if ordered { format!("{}.", start + i as u64) } else { "•".to_string() };
            div()
                .flex()
                .gap(px(8.))
                .child(div().w(px(18.)).flex_none().text_right().text_color(ctx.theme.muted.to_hsla()).child(marker))
                .child(block_column(ctx, ix, &format!("{path}.{i}"), item).flex_1().min_w_0().gap(px(6.)))
        }))
        .into_any_element()
}

fn table_block<H: StreamHost>(ctx: &RowCtx<H>, ix: usize, head: &[Text], rows: &[Vec<Text>]) -> AnyElement {
    let line = ctx.theme.line.to_hsla();
    let row = |cells: &[Text], bold: bool| {
        div().flex().border_b_1().border_color(line).children(cells.iter().map(|cell| {
            div()
                .flex_1()
                .min_w_0()
                .px(px(10.))
                .py(px(6.))
                .when(bold, |cell_div| cell_div.font_weight(FontWeight::BOLD))
                .child(text_block(ctx, ix, cell))
        }))
    };
    div()
        .flex()
        .flex_col()
        .rounded(px(6.))
        .border_1()
        .border_color(ctx.theme.line2.to_hsla())
        .overflow_hidden()
        .child(row(head, true))
        .children(rows.iter().map(|cells| row(cells, false)))
        .into_any_element()
}

/// The code block frame: language, path, and Copy in the header; numbered, coloured lines below (chat.md §1.6).
fn code_block<H: StreamHost>(ctx: &RowCtx<H>, ix: usize, path: &str, code: &Code) -> AnyElement {
    let theme = &ctx.theme;
    let toast = match &code.path {
        Some(file) => format!("Copied {}", file_name(file)),
        None => "Copied code".to_string(),
    };
    let lines = code.body.lines().enumerate().map(|(n, line)| code_line(theme, n + 1, line));
    div()
        .flex()
        .flex_col()
        .rounded(px(8.))
        .border_1()
        .border_color(theme.line2.to_hsla())
        .bg(code_bg(theme))
        .overflow_hidden()
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(10.))
                .pl(px(12.))
                .pr(px(6.))
                .py(px(5.))
                .border_b_1()
                .border_color(theme.line.to_hsla())
                .text_size(px(11.))
                .text_color(theme.muted.to_hsla())
                .when(!code.lang.is_empty(), |header| {
                    header.child(div().text_color(theme.acc2.to_hsla()).child(code.lang.to_uppercase()))
                })
                .child(div().flex_1().min_w_0().truncate().child(code.path.clone().unwrap_or_default()))
                .child(small_button(
                    ctx,
                    format!("stream-copy-{ix}-{path}"),
                    "Copy",
                    ButtonAction::Text { payload: code.body.clone(), toast },
                )),
        )
        .child(
            div()
                .py(px(10.))
                .font_family(theme.code_font.clone())
                .text_size(px(12.5))
                .line_height(relative(1.65))
                .children(lines),
        )
        .into_any_element()
}

/// One numbered line of code, with its tokens coloured by class (chat.md §1.6).
fn code_line(theme: &Theme, number: usize, line: &str) -> AnyElement {
    let text = if line.is_empty() { " ".to_string() } else { line.to_string() };
    let spans = tokenize(line).into_iter().map(|(range, tok)| {
        (range, HighlightStyle { color: Some(token_colour(tok, theme)), ..Default::default() })
    });
    div()
        .flex()
        .gap(px(12.))
        .pr(px(12.))
        .whitespace_nowrap()
        .child(
            div()
                .w(px(26.))
                .flex_none()
                .text_right()
                .text_color(theme.faint.to_hsla())
                .child(number.to_string()),
        )
        .child(StyledText::new(text).with_highlights(spans))
        .into_any_element()
}

pub(in crate::ui) fn token_colour(tok: Tok, theme: &Theme) -> Hsla {
    match tok {
        Tok::Plain => theme.fg,
        Tok::Comment | Tok::Punct => theme.muted,
        Tok::Str | Tok::Upper => theme.acc2,
        Tok::Number | Tok::Literal => theme.alt,
        Tok::Keyword => theme.acc,
    }
    .to_hsla()
}

/// The bar chart: a 150px plot with grid lines at 25 / 50 / 75%, value labels, and x labels (chat.md §1.7).
fn chart_block<H: StreamHost>(
    ctx: &RowCtx<H>,
    ix: usize,
    path: &str,
    title: &str,
    rows: &[super::code::ChartRow],
) -> AnyElement {
    let theme = &ctx.theme;
    let muted = theme.muted.to_hsla();
    let peak = rows.iter().map(|row| row.value).fold(1., f64::max);
    let csv = rows.iter().map(|row| format!("{},{}", row.label, number(row.value))).collect::<Vec<_>>().join("\n");

    let grid = [25., 50., 75.].map(|percent| {
        div()
            .absolute()
            .left_0()
            .right_0()
            .bottom(relative(percent / 100.))
            .h(px(1.))
            .bg(with_alpha(theme.line, 0.09))
    });
    let bars = rows.iter().map(|row| {
        let (value_colour, fill) = if row.value == peak {
            (
                theme.acc2.to_hsla(),
                linear_gradient(
                    180.,
                    linear_color_stop(theme.acc2.to_hsla(), 0.),
                    linear_color_stop(theme.acc.to_hsla(), 1.),
                ),
            )
        } else {
            (
                muted,
                linear_gradient(
                    180.,
                    linear_color_stop(with_alpha(theme.acc, 0.85), 0.),
                    linear_color_stop(with_alpha(theme.acc, 0.25), 1.),
                ),
            )
        };
        let height = (row.value / peak * 118.).round().max(2.) as f32;
        div()
            .flex_1()
            .min_w_0()
            .flex()
            .flex_col()
            .items_center()
            .justify_end()
            .gap(px(5.))
            .child(div().text_size(px(11.)).text_color(value_colour).child(number(row.value)))
            .child(div().w_full().max_w(px(46.)).h(px(height)).rounded_t(px(4.)).bg(fill))
    });
    let plot = div()
        .relative()
        .h(px(150.))
        .flex()
        .items_end()
        .gap(px(10.))
        .border_b_1()
        .border_color(theme.line2.to_hsla())
        .children(grid)
        .children(bars);
    let labels = div().flex().gap(px(10.)).pt(px(7.)).children(rows.iter().map(|row| {
        div()
            .flex_1()
            .min_w_0()
            .text_center()
            .truncate()
            .text_size(px(11.))
            .text_color(muted)
            .child(row.label.clone())
    }));
    let header = div()
        .flex()
        .items_center()
        .gap(px(10.))
        .pl(px(14.))
        .pr(px(6.))
        .py(px(6.))
        .border_b_1()
        .border_color(theme.line.to_hsla())
        .text_size(px(11.))
        .text_color(muted)
        .child(bar_icon(theme))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .text_size(px(12.5))
                .text_color(theme.fg.to_hsla())
                .child(title.to_string()),
        )
        .child(format!("{} points · peak {}", rows.len(), number(peak)))
        .child(small_button(ctx, format!("stream-csv-{ix}-{path}"), "Copy CSV", ButtonAction::Text { payload: csv, toast: "Copied CSV".into() }));

    div()
        .flex()
        .flex_col()
        .rounded(px(10.))
        .border_1()
        .border_color(theme.line2.to_hsla())
        .bg(code_bg(theme))
        .overflow_hidden()
        .child(header)
        .child(div().px(px(16.)).pt(px(16.)).pb(px(10.)).child(plot).child(labels))
        .into_any_element()
}

/// The three-bar chart glyph, drawn from the prototype's path `M3 13V9M8 13V4M13 13V7`.
fn bar_icon(theme: &Theme) -> AnyElement {
    let bar = |height: f32| div().w(px(2.)).h(px(height)).bg(theme.acc2.to_hsla());
    div()
        .flex()
        .items_end()
        .justify_between()
        .w(px(13.))
        .h(px(13.))
        .child(bar(4.))
        .child(bar(9.))
        .child(bar(6.))
        .into_any_element()
}

/// The image figure: a thumbnail (opens the lightbox), then the name, caption and Open / Attach controls (chat.md §1.8).
/// Remote images cannot be drawn offline, so they show a link instead; missing files show their path.
fn image_block<H: StreamHost>(ctx: &RowCtx<H>, ix: usize, path: &str, image: &Figure) -> AnyElement {
    let theme = &ctx.theme;
    let host = ctx.host.clone();
    let muted = theme.muted.to_hsla();
    let placeholder = |text: String| {
        div()
            .w_full()
            .h(px(268.))
            .flex()
            .items_center()
            .justify_center()
            .text_size(px(12.))
            .text_color(muted)
            .child(text)
    };
    let (thumb, actions) = match &image.source {
        Source::Local(full) => {
            let open = {
                let (host, path, name, caption) = (host.clone(), full.clone(), image.name.clone(), image.caption.clone());
                move |window: &mut Window, cx: &mut App| {
                    host.update(cx, |h, cx| {
                        // The lightbox takes focus so Escape reaches it.
                        let focus = cx.focus_handle();
                        focus.focus(window);
                        h.stream_ui().lightbox = Some(Lightbox {
                            path: path.clone(),
                            name: name.clone(),
                            caption: caption.clone(),
                            focus,
                        });
                        cx.notify();
                    })
                }
            };
            let thumb = {
                let open = open.clone();
                div()
                    .w_full()
                    .h(px(268.))
                    .overflow_hidden()
                    .cursor_pointer()
                    .on_mouse_down(MouseButton::Left, move |_: &MouseDownEvent, window: &mut Window, cx: &mut App| {
                        open(window, cx)
                    })
                    .child(img(full.clone()).size_full().object_fit(ObjectFit::Cover))
                    .into_any_element()
            };
            let attach = {
                let (host, session, path) = (host.clone(), ctx.session.clone(), full.clone());
                move |_: &mut Window, cx: &mut App| match Attachment::inspect(&path) {
                    Ok(file) => host.update(cx, |h, cx| {
                        let name = file.name.clone();
                        h.stream_attach(&session, vec![file], cx);
                        h.stream_toast(format!("Attached {name}"), cx);
                    }),
                    Err(error) => host.update(cx, |h, cx| h.stream_toast(error, cx)),
                }
            };
            let actions = div()
                .flex()
                .gap(px(4.))
                .child(small_button(ctx, format!("stream-open-{ix}-{path}"), "Open", ButtonAction::Action(Box::new(open))))
                .child(small_button(ctx, format!("stream-attach-{ix}-{path}"), "Attach", ButtonAction::Action(Box::new(attach))))
                .into_any_element();
            (thumb, actions)
        }
        Source::Remote(url) => {
            let url = url.clone();
            let open = {
                let (host, url) = (host.clone(), url.clone());
                move |_: &mut Window, cx: &mut App| host.update(cx, |h, cx| h.stream_open_link(&url, cx))
            };
            let thumb = placeholder(format!("Remote image · {url}")).into_any_element();
            let actions = small_button(ctx, format!("stream-open-{ix}-{path}"), "Open in browser", ButtonAction::Action(Box::new(open)));
            (thumb, actions)
        }
        Source::Missing(full) => {
            let thumb = placeholder(format!("Missing file · {}", full.display())).into_any_element();
            (thumb, div().into_any_element())
        }
    };

    div()
        .max_w(px(560.))
        .flex()
        .flex_col()
        .rounded(px(10.))
        .border_1()
        .border_color(theme.line2.to_hsla())
        .bg(code_bg(theme))
        .overflow_hidden()
        .child(thumb)
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(6.))
                .pl(px(14.))
                .pr(px(6.))
                .py(px(7.))
                .border_t_1()
                .border_color(theme.line.to_hsla())
                .child(
                    div()
                        .w(px(13.))
                        .h(px(13.))
                        .rounded(px(2.))
                        .border_1()
                        .border_color(theme.acc2.to_hsla()),
                )
                .child(div().text_size(px(12.5)).text_color(theme.fg.to_hsla()).truncate().child(image.name.clone()))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .text_size(px(11.))
                        .text_color(muted)
                        .truncate()
                        .child(image.caption.clone()),
                )
                .child(actions),
        )
        .into_any_element()
}

/// What a small button does when clicked.
enum ButtonAction {
    /// Writes `payload` to the clipboard, then toasts.
    Text { payload: String, toast: String },
    /// Runs an action with the app.
    Action(Box<dyn Fn(&mut Window, &mut App)>),
}

/// The Copy / Open / Attach control style (chat.md §2, §1.6, §1.8).
fn small_button<H: StreamHost>(ctx: &RowCtx<H>, id: String, label: &'static str, action: ButtonAction) -> AnyElement {
    let host = ctx.host.clone();
    let on_click = move |_: &MouseDownEvent, window: &mut Window, cx: &mut App| match &action {
        ButtonAction::Text { payload, toast } => {
            cx.write_to_clipboard(ClipboardItem::new_string(payload.clone()));
            host.update(cx, |h, cx| h.stream_toast(toast.clone(), cx));
        }
        ButtonAction::Action(run) => run(window, cx),
    };
    div()
        .id(SharedString::from(id))
        .px(px(9.))
        .py(px(4.))
        .rounded(px(5.))
        .text_size(px(11.))
        .text_color(ctx.theme.muted.to_hsla())
        .cursor_pointer()
        .hover(|button| button.bg(ctx.theme.line2.to_hsla()).text_color(ctx.theme.fg.to_hsla()))
        .child(label)
        .on_mouse_down(MouseButton::Left, on_click)
        .into_any_element()
}

fn attachment_card<H: StreamHost>(ctx: &RowCtx<H>, ix: usize, n: usize, file: &Attachment) -> AnyElement {
    let card = AttachmentCard::new(
        SharedString::from(format!("stream-att-{ix}-{n}")),
        file.path.display().to_string(),
        human_size(file.size),
        CardVariant::Sent,
        &ctx.theme,
    );
    let card = if file.kind == Kind::Image { card.thumbnail(file.path.clone()) } else { card };
    card.into_any_element()
}

/// The code surface: the theme's `bg` at the prototype's 0.72 alpha (chat.md §1.6, themes.md `codeBg`).
fn code_bg(theme: &Theme) -> Hsla {
    with_alpha(theme.bg, 0.72)
}

fn with_alpha(colour: Rgba, alpha: f32) -> Hsla {
    Hsla { a: alpha, ..colour.to_hsla() }
}

/// A number as the prototype prints it: whole values without a decimal point.
fn number(value: f64) -> String {
    if value.fract() == 0. {
        format!("{value:.0}")
    } else {
        value.to_string()
    }
}

fn file_name(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

#[cfg(test)]
mod tests {

    #[test]
    fn rtl_follows_the_first_letter() {
        assert!(starts_rtl("مرحبا، كيف حالك؟ Hello"));
        assert!(starts_rtl("123 — شالوم"));
        assert!(!starts_rtl("Hello مرحبا"));
        assert!(!starts_rtl("`src/` 42"));
        assert!(!starts_rtl(""));
    }

    use super::{code_bg, starts_rtl};
    use crate::ui::{
        fonts::FontChoice,
        theme::{resolve, Rgba, Theme, ThemeId},
    };
    use gpui::Hsla;
    use std::collections::HashMap;

    fn theme(id: ThemeId) -> Theme {
        resolve(&id.preset(), &HashMap::new(), FontChoice::default())
    }

    /// The frame must follow the theme, so the light Ink theme no longer draws a dark slab (chat.md §1.6).
    #[test]
    fn code_surface_follows_the_theme_background() {
        let ink = code_bg(&theme(ThemeId::Ink));
        assert_eq!(ink, Hsla { a: 0.72, ..Rgba::hex(0xf2f0ea).to_hsla() });
        let blood = code_bg(&theme(ThemeId::BloodMoon));
        assert_eq!(blood, Hsla { a: 0.72, ..Rgba::hex(0x0d0a08).to_hsla() });
        assert!(ink.l > blood.l, "Ink's code surface is light, Blood Moon's dark");
    }
}

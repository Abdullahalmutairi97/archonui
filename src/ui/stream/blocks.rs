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
    store::{Role, Todo, TodoStatus, ToolCall, ToolKind, Usage},
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
    let bytes = glass::logo_bytes(&theme.logo)?;
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
        .when(!item.todos.is_empty(), |column| column.child(checklist(&item.todos, theme)))
        .when(streaming, |column| {
            column.child(
                div()
                    .text_size(px(13.))
                    .text_color(theme.muted.to_hsla())
                    .italic()
                    .child(format!("Working · {} tool calls · Esc to stop", item.tools.len())),
            )
        })
        // Notes the agent wrote between tool calls: dimmed, then a divider before the final answer.
        .when(!item.progress.is_empty(), |column| {
            column
                .child(
                    block_column(ctx, ix, "p", &item.progress)
                        .max_w(px(READING_WIDTH))
                        .text_size(px(ctx.text_size * PROGRESS_SCALE))
                        .text_color(theme.muted.to_hsla()),
                )
                .child(answer_divider(theme))
        })
        .when(!item.blocks.is_empty(), |column| {
            column.child(
                block_column(ctx, ix, "", &item.blocks).max_w(px(READING_WIDTH)),
            )
        })
        .when(!streaming, |column| column.children(reply_footer(item, theme)))
        .into_any_element()
}

/// Progress notes' text size relative to the final answer's.
const PROGRESS_SCALE: f32 = 0.92;
/// Lines added and removed, the colours diff views use, readable on every dark theme.
const ADDED: Rgba = Rgba::hex(0x3fb950);
const REMOVED: Rgba = Rgba::hex(0xf85149);

/// The agent's task list as a compact plan: a header with the counts, then up to six rows, each with
/// its state glyph and number. The row in progress is marked `active`; finished rows are muted.
fn checklist(todos: &[Todo], theme: &Theme) -> AnyElement {
    let done = todos.iter().filter(|t| t.status == TodoStatus::Completed).count();
    let active = todos.iter().filter(|t| t.status == TodoStatus::InProgress).count();
    let mut counts = format!("{done}/{} complete", todos.len());
    if active > 0 {
        counts.push_str(&format!(" · {active} in progress"));
    }
    let (shown, hidden) = plan_window(todos, PLAN_ROWS);
    let first = shown.start;
    let rows = todos[shown].iter().enumerate().map(|(i, t)| {
        let number = first + i + 1;
        let (glyph, glyph_colour, text_colour) = match t.status {
            TodoStatus::Completed => ("✓", ADDED, theme.muted),
            TodoStatus::InProgress => ("●", theme.acc, theme.fg),
            TodoStatus::Pending => ("○", theme.muted, theme.fg.alpha(0.75)),
        };
        let in_progress = t.status == TodoStatus::InProgress;
        div()
            .flex()
            .items_center()
            .gap(px(8.))
            .child(div().w(px(12.)).flex_none().text_color(glyph_colour.to_hsla()).child(glyph))
            .child(
                div()
                    .w(px(18.))
                    .flex_none()
                    .text_right()
                    .text_color(theme.muted.to_hsla())
                    .child(number.to_string()),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .text_color(text_colour.to_hsla())
                    .when(in_progress, |d| d.font_weight(FontWeight::SEMIBOLD))
                    .child(t.text.clone()),
            )
            .when(in_progress, |d| {
                d.child(div().flex_none().pl(px(8.)).text_size(px(11.)).text_color(theme.acc.to_hsla()).child("active"))
            })
    });
    let more = (hidden > 0).then(|| {
        div()
            .pl(px(38.))
            .text_color(theme.muted.to_hsla())
            .child(format!("… {hidden} more {}", if hidden == 1 { "step" } else { "steps" }))
    });
    div()
        .flex()
        .flex_col()
        .gap(px(3.))
        .max_w(px(READING_WIDTH))
        .px(px(12.))
        .py(px(8.))
        .rounded(px(8.))
        .bg(theme.fg.alpha(0.04).to_hsla())
        .font_family(theme.code_font.clone())
        .text_size(px(12.5))
        .line_height(relative(1.5))
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(8.))
                .child(div().text_color(theme.acc2.to_hsla()).child("▸ plan"))
                .child(div().text_color(theme.muted.to_hsla()).child(counts)),
        )
        .children(rows)
        .children(more)
        .into_any_element()
}

/// Rows a checklist shows at most, the last of them the "… N more steps" line when items are hidden.
const PLAN_ROWS: usize = 6;

/// Which items of a plan are drawn and how many are hidden: all of them when they fit in `max` rows,
/// otherwise a window of `max - 1` items around the first one in progress (or else the first pending
/// one), with up to two finished items before it. Once everything is done, the last items are shown.
fn plan_window(todos: &[Todo], max: usize) -> (Range<usize>, usize) {
    let n = todos.len();
    if n <= max {
        return (0..n, 0);
    }
    let size = max.saturating_sub(1).max(1);
    let focus = todos
        .iter()
        .position(|t| t.status == TodoStatus::InProgress)
        .or_else(|| todos.iter().position(|t| t.status == TodoStatus::Pending))
        .unwrap_or(n - 1);
    let start = focus.saturating_sub(2).min(n - size);
    (start..start + size, n - size)
}

/// The line between a reply's progress notes and its final answer.
fn answer_divider(theme: &Theme) -> AnyElement {
    div()
        .flex()
        .items_center()
        .gap(px(10.))
        .pt(px(6.))
        .max_w(px(READING_WIDTH))
        .child(
            div()
                .flex_none()
                .text_size(px(11.))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(theme.acc2.to_hsla())
                .child("FINAL ANSWER"),
        )
        .child(div().flex_1().h(px(1.)).bg(theme.line2.to_hsla()))
        .into_any_element()
}

/// The line under a finished reply: how long it took, the tokens and cache hits, cost, and the lines
/// its edits added and removed. `None` when none of these are known.
fn reply_footer(item: &Item, theme: &Theme) -> Option<AnyElement> {
    let mut parts = Vec::new();
    if let Some(secs) = item.took_secs {
        parts.push(format!("worked {}", duration(secs)));
    }
    if let Some(u) = item.usage {
        parts.extend(usage_parts(&u));
    }
    let (added, removed) = item
        .tools
        .iter()
        .filter(|t| matches!(t.kind, ToolKind::Edit | ToolKind::Write) && t.ok != Some(false))
        .filter_map(|t| t.detail.as_deref().and_then(diff_counts))
        .fold((0, 0), |(a, r), (x, y)| (a + x, r + y));
    if parts.is_empty() && added + removed == 0 {
        return None;
    }
    Some(
        div()
            .flex()
            .flex_wrap()
            .items_center()
            .gap(px(8.))
            .pt(px(8.))
            .max_w(px(READING_WIDTH))
            .border_t_1()
            .border_color(theme.line.to_hsla())
            .text_size(px(11.))
            .text_color(theme.muted.to_hsla())
            .child(div().text_color(theme.acc2.to_hsla()).child("✓ Done"))
            .children(parts.into_iter().map(|p| div().child(format!("· {p}"))))
            .when(added + removed > 0, |row| row.child("·").children(diff_spans(added, removed)))
            .into_any_element(),
    )
}

/// `tokens in`, `out`, cache hit and cost, for the parts the agent reported.
fn usage_parts(u: &Usage) -> Vec<String> {
    let mut parts = vec![format!("{} in", tokens(u.total_input())), format!("{} out", tokens(u.output))];
    if let Some(hit) = u.cache_hit().filter(|_| u.cache_read > 0) {
        parts.push(format!("{hit}% cache hit"));
    }
    if let Some(cost) = u.cost_usd.filter(|c| *c > 0.) {
        parts.push(if cost < 0.01 { "<$0.01".to_string() } else { format!("${cost:.2}") });
    }
    parts
}

/// `+N` in green and `−M` in red, leaving out a zero side.
fn diff_spans(added: u32, removed: u32) -> Vec<AnyElement> {
    let mut out = Vec::new();
    if added > 0 {
        out.push(div().text_color(ADDED.to_hsla()).child(format!("+{added}")).into_any_element());
    }
    if removed > 0 {
        out.push(div().text_color(REMOVED.to_hsla()).child(format!("−{removed}")).into_any_element());
    }
    out
}

/// The lines added and removed in an edit's detail, such as `+24 −8`, `+41` or `-3`. `None` for any
/// other detail.
pub(super) fn diff_counts(detail: &str) -> Option<(u32, u32)> {
    let (mut added, mut removed, mut any) = (0, 0, false);
    for word in detail.split_whitespace() {
        if let Some(n) = word.strip_prefix('+') {
            added += n.parse::<u32>().ok()?;
        } else if let Some(n) = word.strip_prefix('−').or_else(|| word.strip_prefix('-')) {
            removed += n.parse::<u32>().ok()?;
        } else {
            return None;
        }
        any = true;
    }
    any.then_some((added, removed))
}

/// `45s`, `1m 12s` or `1h 3m`.
fn duration(secs: u32) -> String {
    match secs {
        0..=59 => format!("{secs}s"),
        60..=3599 => format!("{}m {}s", secs / 60, secs % 60),
        _ => format!("{}h {}m", secs / 3600, secs % 3600 / 60),
    }
}

/// `950`, `1.2k`, `45k` or `1.2M` tokens.
fn tokens(n: u64) -> String {
    match n {
        0..=999 => n.to_string(),
        1_000..=9_999 => format!("{:.1}k", n as f64 / 1e3),
        10_000..=999_999 => format!("{}k", (n as f64 / 1e3).round()),
        _ => format!("{:.1}M", n as f64 / 1e6),
    }
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
        .child(match tool.detail.as_deref().and_then(diff_counts).filter(|_| tool.ok != Some(false)) {
            Some((added, removed)) => div().flex_none().flex().gap(px(6.)).children(diff_spans(added, removed)),
            None => div().flex_none().text_color(result.to_hsla()).child(tool.detail.clone().unwrap_or_default()),
        })
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
        // Lighter than `line2`, so a reply full of paths doesn't turn into a row of grey boxes.
        Mark::Code => HighlightStyle { background_color: Some(theme.fg.alpha(0.12).to_hsla()), ..Default::default() },
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
    fn diff_details_parse_into_counts() {
        assert_eq!(diff_counts("+24 −8"), Some((24, 8)));
        assert_eq!(diff_counts("+41"), Some((41, 0)));
        assert_eq!(diff_counts("-3"), Some((0, 3)));
        assert_eq!(diff_counts("128 passed"), None);
        assert_eq!(diff_counts(""), None);
    }

    #[test]
    fn durations_and_token_counts_read_short() {
        assert_eq!((duration(45), duration(72), duration(3780)), ("45s".into(), "1m 12s".into(), "1h 3m".into()));
        assert_eq!((tokens(950), tokens(1234), tokens(45_231), tokens(1_200_000)), ("950".into(), "1.2k".into(), "45k".into(), "1.2M".into()));
        let u = Usage { input: 100, output: 50, cache_read: 800, cache_write: 100, cost_usd: Some(0.004) };
        assert_eq!(usage_parts(&u), ["1.0k in", "50 out", "80% cache hit", "<$0.01"]);
    }

    #[test]
    fn plan_window_keeps_the_active_step_in_view() {
        fn todos(statuses: &str) -> Vec<Todo> {
            statuses
                .chars()
                .map(|c| Todo {
                    text: c.to_string(),
                    status: match c {
                        'x' => TodoStatus::Completed,
                        '>' => TodoStatus::InProgress,
                        _ => TodoStatus::Pending,
                    },
                    active: None,
                })
                .collect()
        }
        // Everything fits in six rows.
        assert_eq!(plan_window(&todos("xx>..."), 6), (0..6, 0));
        // Seven steps: five rows and one hidden, the active step with two finished before it.
        assert_eq!(plan_window(&todos("x>......"), 6), (0..5, 3));
        assert_eq!(plan_window(&todos("xxx>...."), 6), (1..6, 3));
        // Near the end the window stays full.
        assert_eq!(plan_window(&todos("xxxxxxxxx>"), 6), (5..10, 5));
        // No active step: start at the first pending one.
        assert_eq!(plan_window(&todos("xxxx.....xx"), 6), (2..7, 6));
        // All done: the last items.
        assert_eq!(plan_window(&todos("xxxxxxxxxxxx"), 6), (7..12, 7));
    }

    #[test]
    fn rtl_follows_the_first_letter() {
        assert!(starts_rtl("مرحبا، كيف حالك؟ Hello"));
        assert!(starts_rtl("123 — شالوم"));
        assert!(!starts_rtl("Hello مرحبا"));
        assert!(!starts_rtl("`src/` 42"));
        assert!(!starts_rtl(""));
    }

    use super::{code_bg, diff_counts, duration, plan_window, starts_rtl, tokens, usage_parts};
    use crate::store::{Todo, TodoStatus};
    use crate::store::Usage;
    use crate::ui::{
        fonts::FontChoice,
        theme::{resolve, Rgba, Theme},
    };
    use gpui::Hsla;
    use std::collections::HashMap;

    fn theme(bg: u32) -> Theme {
        let mut preset = crate::ui::theme::preset("nord").clone();
        preset.bg = Rgba::hex(bg);
        resolve(&preset, &HashMap::new(), FontChoice::default())
    }

    /// The frame must follow the theme, so a light theme doesn't draw a dark slab (chat.md §1.6).
    #[test]
    fn code_surface_follows_the_theme_background() {
        let light = code_bg(&theme(0xf2f0ea));
        assert_eq!(light, Hsla { a: 0.72, ..Rgba::hex(0xf2f0ea).to_hsla() });
        let dark = code_bg(&theme(0x0d0a08));
        assert_eq!(dark, Hsla { a: 0.72, ..Rgba::hex(0x0d0a08).to_hsla() });
        assert!(light.l > dark.l, "a light theme's code surface is light, a dark one's dark");
    }
}

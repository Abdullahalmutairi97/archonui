//! Text buffer with cursor, selection and undo, stored in a rope.

use std::{
    fs,
    io::{self, Write},
    ops::{Range, RangeInclusive},
    path::{Path, PathBuf},
};

use ropey::Rope;

/// Largest file the editor opens.
const MAX_FILE_BYTES: u64 = 5 * 1024 * 1024;
/// Leading bytes checked for NUL when telling binary files from text.
const BINARY_PROBE_BYTES: usize = 8 * 1024;
/// Most undo steps kept, and most redo steps.
const MAX_HISTORY: usize = 1000;

/// A cursor position. `line` is 0-based and `col` counts chars in the line.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct Pos {
    pub line: usize,
    pub col: usize,
}

/// The state that undo and redo restore.
#[derive(Clone)]
struct Snapshot {
    rope: Rope,
    cursor: Pos,
    anchor: Option<Pos>,
}

/// An editable text file: content, cursor, selection and undo history.
pub struct Buffer {
    rope: Rope,
    cursor: Pos,
    /// The other end of the selection, which runs from here to `cursor`.
    anchor: Option<Pos>,
    /// Column that vertical moves try to keep.
    goal_col: Option<usize>,
    dirty: bool,
    path: Option<PathBuf>,
    /// Whether the file used CRLF line endings, which save writes back.
    crlf: bool,
    indent: String,
    /// Whether the last edit was a typed char, so the next typed char may join its undo step.
    typing: bool,
    undo_stack: Vec<Snapshot>,
    redo_stack: Vec<Snapshot>,
    /// Lines changed since `take_edit` last ran, as `(first, removed, inserted)`.
    last_edit: Option<(usize, usize, usize)>,
}

impl Buffer {
    /// A buffer holding `text`, with no file path.
    pub fn from_str(text: &str) -> Self {
        let (text, crlf) = normalize_eol(text);
        Self {
            rope: Rope::from_str(&text),
            cursor: Pos::default(),
            anchor: None,
            goal_col: None,
            dirty: false,
            path: None,
            crlf,
            indent: detect_indent(&text),
            typing: false,
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            last_edit: None,
        }
    }

    /// Opens the text file at `path`. Files over 5 MB, or with a NUL byte in the first 8 KB, are rejected.
    pub fn open(path: &Path) -> io::Result<Self> {
        if fs::metadata(path)?.len() > MAX_FILE_BYTES {
            return Err(binary_or_too_large());
        }
        let bytes = fs::read(path)?;
        if bytes[..bytes.len().min(BINARY_PROBE_BYTES)].contains(&0) {
            return Err(binary_or_too_large());
        }
        let text = String::from_utf8(bytes)
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "not valid UTF-8"))?;
        let mut buffer = Self::from_str(&text);
        buffer.path = Some(path.to_path_buf());
        Ok(buffer)
    }

    /// Writes the buffer to its path through a temp file, keeping the file's permissions.
    pub fn save(&mut self) -> io::Result<()> {
        let path = self
            .path
            .as_deref()
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "buffer has no file path"))?;
        let text = self.text();
        let content = if self.crlf { text.replace('\n', "\r\n") } else { text };
        write_atomic(path, &content)?;
        self.dirty = false;
        Ok(())
    }

    /// Replaces the content with `text` read from disk. The cursor stays in range, the change is not undoable, and the buffer is clean.
    pub fn reload_from(&mut self, text: &str) {
        let (text, crlf) = normalize_eol(text);
        let old_count = self.line_count();
        self.rope = Rope::from_str(&text);
        self.record_edit(0, old_count, self.line_count());
        self.crlf = crlf;
        self.cursor = self.pos_of(self.char_index(self.cursor));
        self.anchor = self.anchor.map(|anchor| self.pos_of(self.char_index(anchor)));
        self.goal_col = None;
        self.typing = false;
        self.dirty = false;
    }

    /// Returns the lines changed since the last call, as `(first line, lines removed, lines inserted)`
    /// counted in the text before the change, and clears them. Edits in between are merged into one range.
    pub fn take_edit(&mut self) -> Option<(usize, usize, usize)> {
        self.last_edit.take()
    }

    /// The whole text, with `\n` line endings.
    pub fn text(&self) -> String {
        self.rope.to_string()
    }

    pub fn line_count(&self) -> usize {
        self.rope.len_lines()
    }

    /// Line `line` without its line break.
    pub fn line(&self, line: usize) -> String {
        let line = self.clamp_line(line);
        self.rope.line(line).slice(..self.line_chars(line)).to_string()
    }

    pub fn cursor(&self) -> Pos {
        self.cursor
    }

    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    /// The text one indent level adds: a tab, or spaces.
    pub fn indent_unit(&self) -> &str {
        &self.indent
    }

    /// Char index of `pos`, clamped to the buffer.
    pub fn char_index(&self, pos: Pos) -> usize {
        let line = self.clamp_line(pos.line);
        self.rope.line_to_char(line) + pos.col.min(self.line_chars(line))
    }

    /// The position of char index `index`, clamped to the buffer.
    pub fn pos_of(&self, index: usize) -> Pos {
        let index = index.min(self.rope.len_chars());
        let line = self.rope.char_to_line(index);
        Pos {
            line,
            col: index - self.rope.line_to_char(line),
        }
    }

    /// The selected range as `(start, end)`, or `None` when nothing is selected.
    pub fn selection_range(&self) -> Option<(Pos, Pos)> {
        let anchor = self.anchor?;
        (anchor != self.cursor).then(|| (anchor.min(self.cursor), anchor.max(self.cursor)))
    }

    /// The selected text, or an empty string.
    pub fn selected_text(&self) -> String {
        self.selection_chars()
            .map(|range| self.rope.slice(range).to_string())
            .unwrap_or_default()
    }

    /// Moves the cursor one char left. With a selection and no `select`, collapses to its start.
    pub fn move_left(&mut self, select: bool) {
        match self.selection_range() {
            Some((start, _)) if !select => self.move_cursor(start, false),
            _ => {
                let idx = self.char_index(self.cursor);
                self.move_cursor(self.pos_of(idx.saturating_sub(1)), select);
            }
        }
    }

    /// Moves the cursor one char right. With a selection and no `select`, collapses to its end.
    pub fn move_right(&mut self, select: bool) {
        match self.selection_range() {
            Some((_, end)) if !select => self.move_cursor(end, false),
            _ => {
                let idx = self.char_index(self.cursor);
                self.move_cursor(self.pos_of(idx + 1), select);
            }
        }
    }

    pub fn move_up(&mut self, select: bool) {
        if self.cursor.line == 0 {
            self.move_cursor(Pos::default(), select);
        } else {
            self.move_to_line(self.cursor.line - 1, select);
        }
    }

    pub fn move_down(&mut self, select: bool) {
        if self.cursor.line + 1 == self.line_count() {
            self.move_cursor(self.end_pos(), select);
        } else {
            self.move_to_line(self.cursor.line + 1, select);
        }
    }

    pub fn page_up(&mut self, lines: usize, select: bool) {
        self.move_to_line(self.cursor.line.saturating_sub(lines), select);
    }

    pub fn page_down(&mut self, lines: usize, select: bool) {
        self.move_to_line((self.cursor.line + lines).min(self.line_count() - 1), select);
    }

    pub fn move_word_left(&mut self, select: bool) {
        let idx = self.word_start_before(self.char_index(self.cursor));
        self.move_cursor(self.pos_of(idx), select);
    }

    pub fn move_word_right(&mut self, select: bool) {
        let idx = self.word_end_after(self.char_index(self.cursor));
        self.move_cursor(self.pos_of(idx), select);
    }

    /// Moves to the first non-blank char of the line, or to column 0 when already there.
    pub fn move_home(&mut self, select: bool) {
        let line = self.cursor.line;
        let first = self.first_non_blank(line);
        let col = if self.cursor.col == first { 0 } else { first };
        self.move_cursor(Pos { line, col }, select);
    }

    pub fn move_end(&mut self, select: bool) {
        let line = self.cursor.line;
        self.move_cursor(Pos { line, col: self.line_chars(line) }, select);
    }

    pub fn move_doc_start(&mut self, select: bool) {
        self.move_cursor(Pos::default(), select);
    }

    pub fn move_doc_end(&mut self, select: bool) {
        self.move_cursor(self.end_pos(), select);
    }

    /// Moves the cursor to `pos`, clamped. With `select`, the selection extends from where the cursor was.
    pub fn set_cursor(&mut self, pos: Pos, select: bool) {
        let pos = self.pos_of(self.char_index(pos));
        self.move_cursor(pos, select);
    }

    pub fn select_all(&mut self) {
        self.set_selection(Pos::default(), self.end_pos());
    }

    /// Selects the word at `pos`. A position just past a word selects that word.
    pub fn select_word_at(&mut self, pos: Pos) {
        let len = self.rope.len_chars();
        let idx = self.char_index(pos);
        let on_word = |i: usize| i < len && is_word(self.rope.char(i));
        let word = if on_word(idx) {
            Some(idx)
        } else {
            (idx > 0 && on_word(idx - 1)).then(|| idx - 1)
        };
        let Some(word) = word else {
            self.set_cursor(pos, false);
            return;
        };
        let mut start = word;
        while start > 0 && is_word(self.rope.char(start - 1)) {
            start -= 1;
        }
        let mut end = word;
        while end < len && is_word(self.rope.char(end)) {
            end += 1;
        }
        self.set_selection(self.pos_of(start), self.pos_of(end));
    }

    /// Selects `line` including its line break.
    pub fn select_line(&mut self, line: usize) {
        let line = self.clamp_line(line);
        let end = if line + 1 < self.line_count() {
            Pos { line: line + 1, col: 0 }
        } else {
            Pos { line, col: self.line_chars(line) }
        };
        self.set_selection(Pos { line, col: 0 }, end);
    }

    /// Inserts `text` at the cursor, replacing the selection.
    pub fn insert(&mut self, text: &str) {
        let mut chars = text.chars();
        let typed = match (chars.next(), chars.next()) {
            (Some(c), None) if c != '\n' && self.selection_range().is_none() => Some(c),
            _ => None,
        };
        // A space after a word starts a new undo step.
        let new_group = typed.is_some_and(|c| {
            c.is_whitespace() && self.char_before_cursor().is_some_and(is_word)
        });
        self.replace_selection(text, typed.is_some(), new_group);
    }

    /// Inserts a line break. The new line gets the indentation of the text before the cursor, plus one indent unit after `{`, `[`, `(` or a `:` ending the text before the cursor.
    pub fn newline(&mut self) {
        let start = self.pos_of(self.edit_range().start);
        let before = self.rope.line(start.line).slice(..start.col).to_string();
        let indent: String = before
            .chars()
            .take_while(|c| matches!(c, ' ' | '\t'))
            .collect();
        let opens = before.ends_with(['{', '[', '(']) || before.trim_end().ends_with(':');
        let extra = if opens { self.indent.as_str() } else { "" };
        self.replace_selection(&format!("\n{indent}{extra}"), false, false);
    }

    /// Deletes the selection, or else the char before the cursor. Inside leading spaces that are a multiple of the indent unit, it deletes a whole indent unit.
    pub fn backspace(&mut self) {
        let range = self.selection_chars().unwrap_or_else(|| {
            let idx = self.char_index(self.cursor);
            let unit = self.indent.len();
            let before = self
                .rope
                .line(self.cursor.line)
                .slice(..self.cursor.col)
                .to_string();
            let in_indent = self.cursor.col >= unit
                && self.cursor.col % unit == 0
                && before.chars().all(|c| c == ' ');
            idx.saturating_sub(if in_indent { unit } else { 1 })..idx
        });
        self.replace_chars(range, "", false, false);
    }

    /// Deletes the selection, or else the char after the cursor.
    pub fn delete(&mut self) {
        let range = self.selection_chars().unwrap_or_else(|| {
            let idx = self.char_index(self.cursor);
            idx..(idx + 1).min(self.rope.len_chars())
        });
        self.replace_chars(range, "", false, false);
    }

    /// Deletes the selection, or else back to the start of the word before the cursor.
    pub fn delete_word_left(&mut self) {
        let idx = self.char_index(self.cursor);
        let range = self
            .selection_chars()
            .unwrap_or_else(|| self.word_start_before(idx)..idx);
        self.replace_chars(range, "", false, false);
    }

    /// Indents the lines of a multi-line selection. Otherwise inserts spaces up to the next indent stop, replacing any selection.
    pub fn indent(&mut self) {
        let Some(lines) = self.block_lines() else {
            let col = self.pos_of(self.edit_range().start).col;
            let spaces = self.indent_to_stop(col);
            self.replace_selection(&spaces, false, false);
            return;
        };
        self.checkpoint(false, false);
        let count = lines.end() - lines.start() + 1;
        self.record_edit(*lines.start(), count, count);
        let width = self.indent.chars().count();
        for line in lines {
            let at = self.rope.line_to_char(line);
            self.rope.insert(at, &self.indent);
            self.shift_cols(line, width as isize);
        }
        self.goal_col = None;
        self.dirty = true;
    }

    /// Removes one indent unit from the start of each selected line, or of the cursor's line.
    pub fn outdent(&mut self) {
        let lines = self
            .block_lines()
            .unwrap_or(self.cursor.line..=self.cursor.line);
        let widths: Vec<(usize, usize)> = lines
            .map(|line| (line, self.outdent_width(line)))
            .filter(|&(_, width)| width > 0)
            .collect();
        if widths.is_empty() {
            return;
        }
        self.checkpoint(false, false);
        let (first, last) = (widths[0].0, widths[widths.len() - 1].0);
        self.record_edit(first, last - first + 1, last - first + 1);
        for (line, width) in widths {
            let at = self.rope.line_to_char(line);
            self.rope.remove(at..at + width);
            self.shift_cols(line, -(width as isize));
        }
        self.goal_col = None;
        self.dirty = true;
    }

    /// Removes and returns the selection, or the whole current line when nothing is selected.
    pub fn cut(&mut self) -> Option<String> {
        let text = self.copy()?;
        let range = self
            .selection_chars()
            .unwrap_or_else(|| self.line_span(self.cursor.line));
        self.replace_chars(range, "", false, false);
        Some(text)
    }

    /// The selected text, or the current line with its line break when nothing is selected. `None` when that is empty.
    pub fn copy(&self) -> Option<String> {
        let text = match self.selection_chars() {
            Some(range) => self.rope.slice(range).to_string(),
            None => self.rope.slice(self.line_span(self.cursor.line)).to_string(),
        };
        (!text.is_empty()).then_some(text)
    }

    /// Inserts pasted `text` at the cursor, replacing the selection. CRLF becomes LF.
    pub fn paste(&mut self, text: &str) {
        let (text, _) = normalize_eol(text);
        self.replace_selection(&text, false, false);
    }

    /// Reverts the last undo step, restoring text, cursor and selection. Returns false when there is nothing to undo.
    pub fn undo(&mut self) -> bool {
        let Some(snapshot) = self.undo_stack.pop() else {
            return false;
        };
        let current = self.snapshot();
        push_capped(&mut self.redo_stack, current);
        self.restore(snapshot);
        true
    }

    /// Re-applies the last undone step. Returns false when there is nothing to redo.
    pub fn redo(&mut self) -> bool {
        let Some(snapshot) = self.redo_stack.pop() else {
            return false;
        };
        let current = self.snapshot();
        push_capped(&mut self.undo_stack, current);
        self.restore(snapshot);
        true
    }

    /// Finds the next match of `query` at or after `from`, or the previous one before it when `forward` is false. Matching ignores case and wraps around the buffer. Returns the match's start and end.
    pub fn find(&self, query: &str, from: Pos, forward: bool) -> Option<(Pos, Pos)> {
        let needle: Vec<char> = query.chars().map(fold_case).collect();
        if needle.is_empty() {
            return None;
        }
        let hay: Vec<char> = self.rope.chars().map(fold_case).collect();
        let last = hay.len().checked_sub(needle.len())?;
        let from = self.char_index(from).min(last + 1);
        let matches_at = |s: usize| hay[s..s + needle.len()] == needle[..];
        let start = if forward {
            (from..=last).chain(0..from).find(|&s| matches_at(s))
        } else {
            (0..from).rev().chain((from..=last).rev()).find(|&s| matches_at(s))
        }?;
        Some((self.pos_of(start), self.pos_of(start + needle.len())))
    }

    fn move_cursor(&mut self, pos: Pos, select: bool) {
        self.anchor = if select { self.anchor.or(Some(self.cursor)) } else { None };
        self.cursor = pos;
        self.goal_col = None;
        self.typing = false;
    }

    /// Moves to `line`, keeping the goal column.
    fn move_to_line(&mut self, line: usize, select: bool) {
        let goal = self.goal_col.unwrap_or(self.cursor.col);
        let col = goal.min(self.line_chars(line));
        self.move_cursor(Pos { line, col }, select);
        self.goal_col = Some(goal);
    }

    fn set_selection(&mut self, anchor: Pos, cursor: Pos) {
        self.anchor = Some(anchor);
        self.cursor = cursor;
        self.goal_col = None;
        self.typing = false;
    }

    fn clamp_line(&self, line: usize) -> usize {
        line.min(self.line_count() - 1)
    }

    /// Chars in `line`, not counting its line break.
    fn line_chars(&self, line: usize) -> usize {
        let text = self.rope.line(line);
        let len = text.len_chars();
        if len > 0 && text.char(len - 1) == '\n' {
            len - 1
        } else {
            len
        }
    }

    /// Chars of `line` including its line break. The last line has none to include.
    fn line_span(&self, line: usize) -> Range<usize> {
        let start = self.rope.line_to_char(line);
        let end = if line + 1 < self.line_count() {
            self.rope.line_to_char(line + 1)
        } else {
            self.rope.len_chars()
        };
        start..end
    }

    fn end_pos(&self) -> Pos {
        let line = self.line_count() - 1;
        Pos {
            line,
            col: self.line_chars(line),
        }
    }

    fn first_non_blank(&self, line: usize) -> usize {
        self.rope
            .line(line)
            .chars()
            .take_while(|&c| c != '\n' && c.is_whitespace())
            .count()
    }

    fn char_before_cursor(&self) -> Option<char> {
        let idx = self.char_index(self.cursor);
        (idx > 0).then(|| self.rope.char(idx - 1))
    }

    /// Char index of the start of the word before `idx`, skipping whitespace and punctuation first.
    fn word_start_before(&self, idx: usize) -> usize {
        let mut i = idx;
        while i > 0 && !is_word(self.rope.char(i - 1)) {
            i -= 1;
        }
        while i > 0 && is_word(self.rope.char(i - 1)) {
            i -= 1;
        }
        i
    }

    /// Char index of the end of the word after `idx`, skipping whitespace and punctuation first.
    fn word_end_after(&self, idx: usize) -> usize {
        let len = self.rope.len_chars();
        let mut i = idx;
        while i < len && !is_word(self.rope.char(i)) {
            i += 1;
        }
        while i < len && is_word(self.rope.char(i)) {
            i += 1;
        }
        i
    }

    /// The selection as a char range.
    fn selection_chars(&self) -> Option<Range<usize>> {
        let (start, end) = self.selection_range()?;
        Some(self.char_index(start)..self.char_index(end))
    }

    /// The char range an edit replaces: the selection, or an empty range at the cursor.
    fn edit_range(&self) -> Range<usize> {
        self.selection_chars().unwrap_or_else(|| {
            let idx = self.char_index(self.cursor);
            idx..idx
        })
    }

    /// The lines of a multi-line selection. A last line that the selection only touches at column 0 is left out.
    fn block_lines(&self) -> Option<RangeInclusive<usize>> {
        let (start, end) = self.selection_range()?;
        let last = if end.col == 0 { end.line - 1 } else { end.line };
        (start.line < last).then_some(start.line..=last)
    }

    /// Spaces, or a tab, that move `col` to the next indent stop.
    fn indent_to_stop(&self, col: usize) -> String {
        if self.indent.starts_with('\t') {
            "\t".to_string()
        } else {
            " ".repeat(self.indent.len() - col % self.indent.len())
        }
    }

    /// Chars that outdenting `line` removes: a leading tab, or up to one unit of leading spaces.
    fn outdent_width(&self, line: usize) -> usize {
        let text = self.rope.line(line);
        if self.indent.starts_with('\t') {
            usize::from(text.chars().next() == Some('\t'))
        } else {
            text.chars()
                .take(self.indent.len())
                .take_while(|&c| c == ' ')
                .count()
        }
    }

    /// Moves cursor and anchor columns on `line` by `delta` chars, not below zero.
    fn shift_cols(&mut self, line: usize, delta: isize) {
        let shift = |pos: &mut Pos| {
            if pos.line == line {
                pos.col = pos.col.saturating_add_signed(delta);
            }
        };
        shift(&mut self.cursor);
        if let Some(anchor) = self.anchor.as_mut() {
            shift(anchor);
        }
    }

    fn snapshot(&self) -> Snapshot {
        Snapshot {
            rope: self.rope.clone(),
            cursor: self.cursor,
            anchor: self.anchor,
        }
    }

    fn restore(&mut self, snapshot: Snapshot) {
        let old = std::mem::replace(&mut self.rope, snapshot.rope);
        self.record_diff(&old);
        self.cursor = snapshot.cursor;
        self.anchor = snapshot.anchor;
        self.goal_col = None;
        self.typing = false;
        self.dirty = true;
    }

    /// Records an undo step before an edit. A typed char joins the open step unless `new_group` is set; any other edit starts a new step.
    fn checkpoint(&mut self, typing: bool, new_group: bool) {
        if !typing || new_group || !self.typing {
            let snapshot = self.snapshot();
            push_capped(&mut self.undo_stack, snapshot);
            self.redo_stack.clear();
        }
        self.typing = typing;
    }

    /// Notes an edit for `take_edit`, merged with any edit not yet taken.
    fn record_edit(&mut self, first: usize, removed: usize, inserted: usize) {
        let edit = (first, removed, inserted);
        self.last_edit = Some(self.last_edit.map_or(edit, |pending| merge_edits(pending, edit)));
    }

    /// Notes the lines that differ between `old` and the current text as one edit.
    fn record_diff(&mut self, old: &Rope) {
        let (old_len, new_len) = (old.len_lines(), self.rope.len_lines());
        let shared = old_len.min(new_len);
        let prefix = (0..shared).take_while(|&i| old.line(i) == self.rope.line(i)).count();
        let suffix = (1..=shared - prefix)
            .take_while(|&i| old.line(old_len - i) == self.rope.line(new_len - i))
            .count();
        let (removed, inserted) = (old_len - prefix - suffix, new_len - prefix - suffix);
        if removed > 0 || inserted > 0 {
            self.record_edit(prefix, removed, inserted);
        }
    }

    fn replace_selection(&mut self, text: &str, typing: bool, new_group: bool) {
        let range = self.edit_range();
        self.replace_chars(range, text, typing, new_group);
    }

    /// Replaces the chars in `range` with `text` as one undo step, and puts the cursor after `text`.
    fn replace_chars(&mut self, range: Range<usize>, text: &str, typing: bool, new_group: bool) {
        if range.is_empty() && text.is_empty() {
            return;
        }
        self.checkpoint(typing, new_group);
        let first = self.rope.char_to_line(range.start);
        let removed = self.rope.char_to_line(range.end) - first + 1;
        self.record_edit(first, removed, text.matches('\n').count() + 1);
        let start = range.start;
        self.rope.remove(range);
        self.rope.insert(start, text);
        self.cursor = self.pos_of(start + text.chars().count());
        self.anchor = None;
        self.goal_col = None;
        self.dirty = true;
    }
}

/// The one range covering edits `a` and then `b`, where `b` is counted in the text after `a`.
fn merge_edits(a: (usize, usize, usize), b: (usize, usize, usize)) -> (usize, usize, usize) {
    let (first_a, removed_a, inserted_a) = a;
    let (first_b, removed_b, inserted_b) = b;
    let start = first_a.min(first_b);
    // The end of the covered lines, in the text after `a`.
    let end = (first_a + inserted_a).max(first_b + removed_b);
    let old_end = end - inserted_a + removed_a;
    let new_end = end - removed_b + inserted_b;
    (start, old_end - start, new_end - start)
}

fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// Lowercase form of `c` as one char, so char indices stay aligned with the text.
fn fold_case(c: char) -> char {
    c.to_lowercase().next().unwrap_or(c)
}

fn push_capped(history: &mut Vec<Snapshot>, snapshot: Snapshot) {
    if history.len() == MAX_HISTORY {
        history.remove(0);
    }
    history.push(snapshot);
}

fn binary_or_too_large() -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, "binary or too large")
}

/// Replaces CRLF with LF. The bool says whether the text used CRLF.
fn normalize_eol(text: &str) -> (String, bool) {
    if text.contains("\r\n") {
        (text.replace("\r\n", "\n"), true)
    } else {
        (text.to_string(), false)
    }
}

/// The indent unit of `text`: a tab when tab-indented lines outnumber space-indented ones, else the
/// space width (4, 2 or 8) that the most indented lines are a multiple of. Four spaces when unindented.
fn detect_indent(text: &str) -> String {
    let mut tabs = 0;
    let mut widths = Vec::new();
    for line in text.lines().filter(|line| !line.trim().is_empty()) {
        if line.starts_with('\t') {
            tabs += 1;
        } else {
            let width = line.len() - line.trim_start_matches(' ').len();
            if width > 0 {
                widths.push(width);
            }
        }
    }
    if tabs > widths.len() {
        return "\t".to_string();
    }
    let mut best = (0, 4);
    for unit in [4, 2, 8] {
        let score = widths.iter().filter(|width| *width % unit == 0).count();
        if score > best.0 {
            best = (score, unit);
        }
    }
    " ".repeat(best.1)
}

/// Writes `content` to a temp file beside `path`, then renames it over `path`.
fn write_atomic(path: &Path, content: &str) -> io::Result<()> {
    let name = path.file_name().unwrap_or_default().to_string_lossy();
    let tmp = path.with_file_name(format!(".{name}.archonui-tmp"));
    let result = write_and_rename(path, &tmp, content);
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result
}

fn write_and_rename(path: &Path, tmp: &Path, content: &str) -> io::Result<()> {
    let mut file = fs::File::create(tmp)?;
    file.write_all(content.as_bytes())?;
    file.sync_all()?;
    if let Ok(meta) = fs::metadata(path) {
        fs::set_permissions(tmp, meta.permissions())?;
    }
    fs::rename(tmp, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pos(line: usize, col: usize) -> Pos {
        Pos { line, col }
    }

    fn type_text(buffer: &mut Buffer, text: &str) {
        for c in text.chars() {
            buffer.insert(&c.to_string());
        }
    }

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("archonui-buffer-{name}-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn typing_and_undo_redo_restore_text_and_cursor() {
        let mut b = Buffer::from_str("");
        type_text(&mut b, "abc");
        b.move_left(false);
        b.insert("x");
        assert_eq!((b.text().as_str(), b.cursor()), ("abxc", pos(0, 3)));
        assert!(b.undo());
        assert_eq!((b.text().as_str(), b.cursor()), ("abc", pos(0, 2)));
        assert!(b.undo());
        assert_eq!((b.text().as_str(), b.cursor()), ("", pos(0, 0)));
        assert!(b.redo());
        assert_eq!((b.text().as_str(), b.cursor()), ("abc", pos(0, 2)));
        assert!(b.redo());
        assert_eq!((b.text().as_str(), b.cursor()), ("abxc", pos(0, 3)));
        assert!(!b.redo());
    }

    #[test]
    fn typed_word_breaks_at_space() {
        let mut b = Buffer::from_str("");
        type_text(&mut b, "ab cd");
        assert!(b.undo());
        assert_eq!(b.text(), "ab");
        assert!(b.undo());
        assert_eq!(b.text(), "");
    }

    #[test]
    fn insert_replaces_selection() {
        let mut b = Buffer::from_str("hello world");
        b.set_cursor(pos(0, 0), false);
        b.set_cursor(pos(0, 5), true);
        assert_eq!(b.selected_text(), "hello");
        b.insert("bye");
        assert_eq!(b.text(), "bye world");
        assert_eq!(b.cursor(), pos(0, 3));
    }

    #[test]
    fn backspace_joins_lines() {
        let mut b = Buffer::from_str("ab\ncd");
        b.set_cursor(pos(1, 0), false);
        b.backspace();
        assert_eq!(b.text(), "abcd");
        assert_eq!(b.cursor(), pos(0, 2));
    }

    #[test]
    fn smart_backspace_deletes_an_indent_unit() {
        let mut b = Buffer::from_str("if x:\n        y");
        assert_eq!(b.indent_unit(), "    ");
        b.set_cursor(pos(1, 8), false);
        b.backspace();
        assert_eq!(b.text(), "if x:\n    y");
        assert_eq!(b.cursor(), pos(1, 4));
    }

    #[test]
    fn newline_auto_indents_after_brace_and_colon() {
        let mut b = Buffer::from_str("fn main() {");
        b.set_cursor(pos(0, 11), false);
        b.newline();
        assert_eq!(b.text(), "fn main() {\n    ");
        assert_eq!(b.cursor(), pos(1, 4));

        let mut b = Buffer::from_str("    key:");
        b.set_cursor(pos(0, 8), false);
        b.newline();
        assert_eq!(b.text(), "    key:\n        ");
        assert_eq!(b.cursor(), pos(1, 8));
    }

    #[test]
    fn word_movement_skips_punctuation() {
        let mut b = Buffer::from_str("foo_bar.baz(qux)");
        for col in [7, 11, 15] {
            b.move_word_right(false);
            assert_eq!(b.cursor(), pos(0, col));
        }
        for col in [12, 8] {
            b.move_word_left(false);
            assert_eq!(b.cursor(), pos(0, col));
        }
    }

    #[test]
    fn vertical_moves_keep_goal_column() {
        let mut b = Buffer::from_str("abcdef\nab\nabcdef");
        b.set_cursor(pos(0, 5), false);
        b.move_down(false);
        assert_eq!(b.cursor(), pos(1, 2));
        b.move_down(false);
        assert_eq!(b.cursor(), pos(2, 5));
        b.move_up(false);
        assert_eq!(b.cursor(), pos(1, 2));
        b.move_up(false);
        assert_eq!(b.cursor(), pos(0, 5));
    }

    #[test]
    fn indent_and_outdent_selected_lines() {
        let mut b = Buffer::from_str("a\nb\nc");
        b.set_cursor(pos(0, 0), false);
        b.set_cursor(pos(2, 1), true);
        b.indent();
        assert_eq!(b.text(), "    a\n    b\n    c");
        b.outdent();
        assert_eq!(b.text(), "a\nb\nc");
    }

    #[test]
    fn tab_without_selection_inserts_spaces_to_next_stop() {
        let mut b = Buffer::from_str("ab");
        b.set_cursor(pos(0, 2), false);
        b.indent();
        assert_eq!(b.text(), "ab  ");
    }

    #[test]
    fn copy_without_selection_copies_the_line() {
        let mut b = Buffer::from_str("one\ntwo");
        b.set_cursor(pos(1, 1), false);
        assert_eq!(b.copy().as_deref(), Some("two"));
        b.set_cursor(pos(0, 1), false);
        assert_eq!(b.copy().as_deref(), Some("one\n"));
        assert_eq!(b.cut().as_deref(), Some("one\n"));
        assert_eq!(b.text(), "two");
    }

    #[test]
    fn crlf_file_keeps_line_endings_on_save() {
        let dir = temp_dir("crlf");
        let path = dir.join("notes.txt");
        fs::write(&path, "a\r\nb\r\n").unwrap();
        let mut b = Buffer::open(&path).unwrap();
        assert_eq!(b.line_count(), 3);
        assert_eq!(b.line(0), "a");
        b.set_cursor(pos(0, 1), false);
        b.insert("X");
        assert!(b.is_dirty());
        b.save().unwrap();
        assert!(!b.is_dirty());
        assert_eq!(fs::read_to_string(&path).unwrap(), "aX\r\nb\r\n");
        fs::remove_dir_all(&dir).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn save_keeps_file_permissions() {
        use std::os::unix::fs::PermissionsExt;

        let dir = temp_dir("perms");
        let path = dir.join("run.sh");
        fs::write(&path, "echo\n").unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o750)).unwrap();
        let mut b = Buffer::open(&path).unwrap();
        b.insert("#");
        b.save().unwrap();
        let mode = fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o750);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn binary_file_is_rejected() {
        let dir = temp_dir("binary");
        let path = dir.join("image.bin");
        fs::write(&path, [0x89, b'P', 0, 1]).unwrap();
        let err = Buffer::open(&path).err().unwrap();
        assert_eq!(err.to_string(), "binary or too large");
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn char_index_and_pos_of_count_chars() {
        let b = Buffer::from_str("héllo wörld\nañb");
        assert_eq!(b.line(0), "héllo wörld");
        assert_eq!(b.char_index(pos(0, 7)), 7);
        assert_eq!(b.pos_of(7), pos(0, 7));
        assert_eq!(b.char_index(pos(1, 2)), 14);
        assert_eq!(b.pos_of(14), pos(1, 2));
        assert_eq!(b.char_index(pos(0, 99)), 11);
        assert_eq!(b.pos_of(999), pos(1, 3));
    }

    #[test]
    fn find_ignores_case_and_wraps() {
        let b = Buffer::from_str("Foo bar foo");
        assert_eq!(b.find("FOO", pos(0, 1), true), Some((pos(0, 8), pos(0, 11))));
        assert_eq!(b.find("foo", pos(0, 9), true), Some((pos(0, 0), pos(0, 3))));
        assert_eq!(b.find("foo", pos(0, 8), false), Some((pos(0, 0), pos(0, 3))));
        assert_eq!(b.find("zzz", pos(0, 0), true), None);
    }

    #[test]
    fn take_edit_reports_lines_changed_by_each_edit() {
        let mut b = Buffer::from_str("abc\ndef\nghi");
        b.set_cursor(pos(1, 1), false);
        b.insert("x");
        assert_eq!(b.take_edit(), Some((1, 1, 1)));
        assert_eq!(b.take_edit(), None);
        b.insert("\n");
        assert_eq!(b.take_edit(), Some((1, 1, 2)));
        b.set_cursor(pos(0, 1), false);
        b.set_cursor(pos(2, 1), true);
        b.delete();
        assert_eq!(b.take_edit(), Some((0, 3, 1)));
    }

    #[test]
    fn take_edit_merges_edits_made_before_it_is_taken() {
        let mut b = Buffer::from_str("a\nb\nc\nd");
        b.set_cursor(pos(1, 0), false);
        b.newline();
        b.set_cursor(pos(2, 0), false);
        b.newline();
        assert_eq!(b.text(), "a\n\n\nb\nc\nd");
        assert_eq!(b.take_edit(), Some((1, 1, 3)));
    }

    #[test]
    fn take_edit_covers_undo_and_reload() {
        let mut b = Buffer::from_str("a\nb\nc");
        b.set_cursor(pos(2, 1), false);
        b.insert("\nd");
        b.take_edit();
        assert!(b.undo());
        assert_eq!(b.take_edit(), Some((2, 2, 1)));
        b.reload_from("x\ny");
        assert_eq!(b.take_edit(), Some((0, 3, 2)));
    }

    #[test]
    fn take_edit_reports_indent_and_outdent_lines() {
        let mut b = Buffer::from_str("a\nb\nc");
        b.set_cursor(pos(0, 0), false);
        b.set_cursor(pos(1, 1), true);
        b.indent();
        assert_eq!(b.take_edit(), Some((0, 2, 2)));
        b.outdent();
        assert_eq!(b.take_edit(), Some((0, 2, 2)));
    }
}

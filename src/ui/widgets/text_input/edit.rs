//! Editing rules for the text input, free of GPUI so they can be unit-tested. Text lives in
//! [`Buffer`]; offsets here are char indices, and IME ranges are converted with the helpers below.

use std::ops::Range;

use crate::editor::buffer::Buffer;

/// What Enter does in a text input.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnterAction {
    Submit,
    Newline,
    Ignored,
}

/// The Enter policy. `secondary` is Ctrl on Linux and Windows, Cmd on macOS.
/// With `enter_submits` Enter sends and Shift+Enter breaks the line; without it the keys swap.
/// Ctrl/Cmd+Enter always sends.
pub fn enter_action(multiline: bool, enter_submits: bool, shift: bool, secondary: bool) -> EnterAction {
    match (shift, secondary) {
        (_, true) => EnterAction::Submit,
        (true, false) if multiline => EnterAction::Newline,
        (true, false) => EnterAction::Ignored,
        (false, false) if !multiline || enter_submits => EnterAction::Submit,
        (false, false) => EnterAction::Newline,
    }
}

/// A text model with a length limit and a single-line or multi-line mode.
pub struct Edit {
    buffer: Buffer,
    multiline: bool,
    max_len: Option<usize>,
}

impl Edit {
    /// Text `text`, in single-line or multi-line mode, holding at most `max_len` chars.
    pub fn new(text: &str, multiline: bool, max_len: Option<usize>) -> Self {
        Self { buffer: Buffer::from_str(text), multiline, max_len }
    }

    /// The text model, for layout and for movement and selection. Edits go through the methods here.
    pub fn buffer(&self) -> &Buffer {
        &self.buffer
    }

    /// The buffer for cursor movement and selection. Don't insert or delete through it: that skips the length limit.
    pub fn buffer_mut(&mut self) -> &mut Buffer {
        &mut self.buffer
    }

    pub fn text(&self) -> String {
        self.buffer.text()
    }

    pub fn char_len(&self) -> usize {
        self.text().chars().count()
    }

    /// The selected char range, or the cursor as an empty range.
    pub fn char_selection(&self) -> Range<usize> {
        match self.buffer.selection_range() {
            Some((start, end)) => self.buffer.char_index(start)..self.buffer.char_index(end),
            None => {
                let at = self.buffer.char_index(self.buffer.cursor());
                at..at
            }
        }
    }

    /// Replaces `chars` with `text` (typed, pasted, or IME text). Returns the char range the new text
    /// occupies. Newlines are dropped to spaces in single-line mode, and the text is cut to the length limit.
    pub fn insert_text(&mut self, text: &str) -> Range<usize> {
        let fitted = self.fit(text);
        let start = self.char_selection().start;
        let inserted = fitted.chars().count();
        if !fitted.is_empty() || self.buffer.selection_range().is_some() {
            self.buffer.insert(&fitted);
        }
        start..start + inserted
    }

    /// Replaces the char range `chars` with `text`, for IME. Returns the new range of the text.
    pub fn replace_range(&mut self, chars: Range<usize>, text: &str) -> Range<usize> {
        self.select_chars(chars);
        self.insert_text(text)
    }

    /// Makes `chars` the selection, with the cursor at its end.
    pub fn select_chars(&mut self, chars: Range<usize>) {
        let start = self.buffer.pos_of(chars.start);
        let end = self.buffer.pos_of(chars.end);
        self.buffer.set_cursor(start, false);
        self.buffer.set_cursor(end, true);
    }

    /// Inserts a line break. Does nothing in single-line mode. Returns whether the text changed.
    pub fn newline(&mut self) -> bool {
        if !self.multiline || self.room() == 0 {
            return false;
        }
        self.insert_text("\n");
        true
    }

    pub fn backspace(&mut self) {
        self.buffer.backspace();
    }

    pub fn delete(&mut self) {
        self.buffer.delete();
    }

    pub fn delete_word_left(&mut self) {
        self.buffer.delete_word_left();
    }

    /// Deletes the selection, or else up to the end of the word after the cursor.
    pub fn delete_word_right(&mut self) {
        if self.buffer.selection_range().is_none() {
            self.buffer.move_word_right(true);
        }
        self.buffer.delete();
    }

    pub fn select_all(&mut self) {
        self.buffer.select_all();
    }

    pub fn copy(&self) -> Option<String> {
        self.buffer.copy()
    }

    pub fn cut(&mut self) -> Option<String> {
        self.buffer.cut()
    }

    pub fn undo(&mut self) -> bool {
        self.buffer.undo()
    }

    pub fn redo(&mut self) -> bool {
        self.buffer.redo()
    }

    /// Chars that can still be typed, given the current selection.
    fn room(&self) -> usize {
        match self.max_len {
            Some(max) => max.saturating_sub(self.char_len() - self.selected_len()),
            None => usize::MAX,
        }
    }

    fn selected_len(&self) -> usize {
        let chars = self.char_selection();
        chars.end - chars.start
    }

    /// `text` with line endings normalised, newlines flattened in single-line mode, cut to the room left.
    fn fit(&self, text: &str) -> String {
        let text = text.replace("\r\n", "\n").replace('\r', "");
        let text = if self.multiline { text } else { text.replace('\n', " ") };
        text.chars().take(self.room()).collect()
    }
}

/// UTF-16 offset of char index `chars` in `text`, for the platform's IME ranges.
pub fn char_to_utf16(text: &str, chars: usize) -> usize {
    text.chars().take(chars).map(char::len_utf16).sum()
}

/// Char index of the UTF-16 offset `units` in `text`. Offsets past the end clamp to the end.
pub fn utf16_to_char(text: &str, units: usize) -> usize {
    let mut seen = 0;
    for (index, c) in text.chars().enumerate() {
        if seen >= units {
            return index;
        }
        seen += c.len_utf16();
    }
    text.chars().count()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn single(text: &str, max: Option<usize>) -> Edit {
        Edit::new(text, false, max)
    }

    #[test]
    fn enter_submits_by_default_and_shift_enter_breaks_the_line() {
        assert_eq!(enter_action(true, true, false, false), EnterAction::Submit);
        assert_eq!(enter_action(true, true, true, false), EnterAction::Newline);
    }

    #[test]
    fn enter_newline_policy_swaps_the_keys_and_ctrl_enter_always_submits() {
        assert_eq!(enter_action(true, false, false, false), EnterAction::Newline);
        assert_eq!(enter_action(true, false, false, true), EnterAction::Submit);
        assert_eq!(enter_action(true, true, true, true), EnterAction::Submit);
    }

    #[test]
    fn single_line_enter_submits_and_shift_enter_is_ignored() {
        assert_eq!(enter_action(false, true, false, false), EnterAction::Submit);
        assert_eq!(enter_action(false, false, false, false), EnterAction::Submit);
        assert_eq!(enter_action(false, true, true, false), EnterAction::Ignored);
    }

    #[test]
    fn max_length_cuts_typed_text_to_the_room_left() {
        let mut edit = single("abcdef", Some(8));
        edit.select_chars(6..6);
        let range = edit.insert_text("ghijk");
        assert_eq!(edit.text(), "abcdefgh");
        assert_eq!(range, 6..8);
    }

    #[test]
    fn selected_text_frees_room_for_the_replacement() {
        let mut edit = single("abcdef", Some(6));
        edit.select_chars(1..4);
        edit.insert_text("XYZW");
        assert_eq!(edit.text(), "aXYZef");
    }

    #[test]
    fn full_field_refuses_new_text_and_newlines() {
        let mut edit = Edit::new("abcd", true, Some(4));
        edit.buffer_mut().move_doc_end(false);
        assert!(!edit.newline());
        edit.insert_text("z");
        assert_eq!(edit.text(), "abcd");
    }

    #[test]
    fn single_line_flattens_pasted_newlines() {
        let mut edit = single("", None);
        edit.insert_text("one\r\ntwo\nthree");
        assert_eq!(edit.text(), "one two three");
    }

    #[test]
    fn multi_line_keeps_newlines_and_normalises_crlf() {
        let mut edit = Edit::new("", true, None);
        edit.insert_text("a\r\nb");
        assert_eq!(edit.text(), "a\nb");
        assert!(edit.newline());
        assert_eq!(edit.text(), "a\nb\n");
    }

    #[test]
    fn word_moves_and_word_delete_follow_buffer_rules() {
        let mut edit = Edit::new("alpha beta gamma", false, None);
        edit.buffer_mut().move_doc_end(false);
        edit.buffer_mut().move_word_left(false);
        assert_eq!(edit.buffer().cursor().col, 11);
        edit.delete_word_left();
        assert_eq!(edit.text(), "alpha gamma");
        edit.buffer_mut().move_doc_start(false);
        edit.delete_word_right();
        assert_eq!(edit.text(), " gamma");
    }

    #[test]
    fn undo_and_redo_restore_typed_text() {
        let mut edit = Edit::new("", true, None);
        edit.insert_text("hello");
        assert!(edit.undo());
        assert_eq!(edit.text(), "");
        assert!(edit.redo());
        assert_eq!(edit.text(), "hello");
    }

    #[test]
    fn replace_range_swaps_an_ime_span() {
        let mut edit = single("ab", None);
        let marked = edit.replace_range(2..2, "ni");
        assert_eq!(marked, 2..4);
        let committed = edit.replace_range(marked, "你");
        assert_eq!(edit.text(), "ab你");
        assert_eq!(committed, 2..3);
    }

    #[test]
    fn utf16_conversions_handle_astral_chars() {
        let text = "a😀b";
        assert_eq!(char_to_utf16(text, 2), 3);
        assert_eq!(utf16_to_char(text, 3), 2);
        assert_eq!(utf16_to_char(text, 1), 1);
        assert_eq!(utf16_to_char(text, 99), 3);
    }

    #[test]
    fn max_length_counts_chars_not_bytes_or_utf16_units() {
        let mut edit = single("", Some(3));
        edit.insert_text("😀é你x");
        assert_eq!(edit.text(), "😀é你");
        assert_eq!(edit.char_len(), 3);
    }

    #[test]
    fn backspace_and_selection_treat_an_emoji_as_one_char() {
        let mut edit = single("a😀b", None);
        edit.buffer_mut().move_doc_end(false);
        edit.buffer_mut().move_left(false);
        edit.backspace();
        assert_eq!(edit.text(), "ab");
        edit.select_chars(0..1);
        assert_eq!(edit.copy().as_deref(), Some("a"));
    }

    #[test]
    fn select_chars_clamps_past_the_end() {
        let mut edit = single("héllo", None);
        edit.select_chars(2..99);
        assert_eq!(edit.char_selection(), 2..5);
        edit.insert_text("!");
        assert_eq!(edit.text(), "hé!");
    }

    #[test]
    fn empty_paste_deletes_the_selection_and_keeps_the_caret() {
        let mut edit = single("abc", None);
        edit.select_chars(1..2);
        assert_eq!(edit.insert_text(""), 1..1);
        assert_eq!(edit.text(), "ac");
        assert_eq!(edit.char_selection(), 1..1);
    }

    #[test]
    fn pasted_newlines_in_a_limited_multiline_field_are_cut_to_the_room() {
        let mut edit = Edit::new("", true, Some(4));
        let range = edit.insert_text("a\r\nb\nc\nd");
        assert_eq!(edit.text(), "a\nb\n");
        assert_eq!(range, 0..4);
    }

    #[test]
    fn text_longer_than_the_limit_can_still_be_replaced_or_shrunk() {
        let mut edit = single("abcdef", Some(3));
        edit.select_chars(0..6);
        edit.insert_text("xyzw");
        assert_eq!(edit.text(), "xyz");
        edit.buffer_mut().move_doc_end(false);
        edit.backspace();
        assert_eq!(edit.text(), "xy");
    }

    #[test]
    fn undo_after_replacing_a_selection_restores_it() {
        let mut edit = single("hello", None);
        edit.select_all();
        edit.insert_text("bye");
        assert_eq!(edit.text(), "bye");
        assert!(edit.undo());
        assert_eq!(edit.text(), "hello");
    }

    #[test]
    fn word_moves_cross_multibyte_words() {
        let mut edit = Edit::new("héllo wörld 😀 end", false, None);
        edit.buffer_mut().move_doc_end(false);
        edit.delete_word_left();
        assert_eq!(edit.text(), "héllo wörld 😀 ");
        edit.buffer_mut().move_doc_start(false);
        edit.delete_word_right();
        assert!(!edit.text().starts_with('h'));
    }

    #[test]
    fn utf16_offsets_inside_a_surrogate_pair_round_up() {
        let text = "a😀b";
        assert_eq!(utf16_to_char(text, 2), 2);
        assert_eq!(char_to_utf16(text, 99), 4);
        assert_eq!(utf16_to_char("", 5), 0);
    }
}

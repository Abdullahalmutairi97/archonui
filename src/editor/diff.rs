//! Change markers: which lines of a file differ from its saved text.

use std::collections::BTreeMap;

use similar::{DiffOp, TextDiff};

/// How a line of the new text differs from the old text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mark {
    Added,
    Modified,
    /// Lines were deleted just above this line.
    Removed,
}

/// The marked lines of `new` (by line index) that differ from `old`, sorted by line.
pub fn line_marks(old: &str, new: &str) -> Vec<(usize, Mark)> {
    let diff = TextDiff::from_lines(old, new);
    let new_count = diff.new_slices().len();
    let mut marks = BTreeMap::new();
    for op in diff.ops() {
        match *op {
            DiffOp::Insert {
                new_index, new_len, ..
            } => {
                for line in new_index..new_index + new_len {
                    marks.insert(line, Mark::Added);
                }
            }
            DiffOp::Replace {
                old_len,
                new_index,
                new_len,
                ..
            } => {
                let paired = old_len.min(new_len);
                for line in new_index..new_index + paired {
                    marks.insert(line, Mark::Modified);
                }
                for line in new_index + paired..new_index + new_len {
                    marks.insert(line, Mark::Added);
                }
                if old_len > new_len {
                    mark_removed(&mut marks, new_index + new_len, new_count);
                }
            }
            DiffOp::Delete { new_index, .. } => mark_removed(&mut marks, new_index, new_count),
            DiffOp::Equal { .. } => {}
        }
    }
    marks.into_iter().collect()
}

/// The first marked line, if any.
pub fn first_change(marks: &[(usize, Mark)]) -> Option<usize> {
    marks.first().map(|&(line, _)| line)
}

/// Marks the line at `at` as `Removed`, or the last line when the deletion is at the end. Added and
/// modified marks on the same line win. Empty text still shows one line, which gets the mark.
fn mark_removed(marks: &mut BTreeMap<usize, Mark>, at: usize, new_count: usize) {
    marks.entry(at.min(new_count.saturating_sub(1))).or_insert(Mark::Removed);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inserted_lines_are_added() {
        assert_eq!(
            line_marks("a\nb\n", "a\nx\nb\n"),
            vec![(1, Mark::Added)]
        );
    }

    #[test]
    fn replaced_line_is_modified_and_extra_lines_are_added() {
        assert_eq!(
            line_marks("a\nb\nc\n", "a\nB\nc\n"),
            vec![(1, Mark::Modified)]
        );
        assert_eq!(
            line_marks("a\nb\n", "a\nx\ny\n"),
            vec![(1, Mark::Modified), (2, Mark::Added)]
        );
    }

    #[test]
    fn deleted_lines_mark_the_following_line() {
        let marks = line_marks("a\nb\nc\n", "a\nc\n");
        assert_eq!(marks, vec![(1, Mark::Removed)]);
        assert_eq!(first_change(&marks), Some(1));
    }

    #[test]
    fn deleted_lines_at_the_end_mark_the_last_line() {
        assert_eq!(line_marks("a\nb\n", "a\n"), vec![(0, Mark::Removed)]);
    }

    #[test]
    fn deleting_everything_marks_the_one_line_left() {
        assert_eq!(line_marks("a\nb\n", ""), vec![(0, Mark::Removed)]);
        assert_eq!(line_marks("a", ""), vec![(0, Mark::Removed)]);
        assert_eq!(line_marks("", ""), vec![]);
    }

    #[test]
    fn identical_text_has_no_marks() {
        assert_eq!(line_marks("a\nb\n", "a\nb\n"), vec![]);
        assert_eq!(first_change(&[]), None);
    }
}

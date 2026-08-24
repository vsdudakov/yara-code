//! Folding for the window editor.
//!
//! The editor widget works on one flat string, so a folded document is shown as
//! a *display* string with the hidden lines removed. Edits come back against
//! that string, and [`Mapping::splice`] puts them where they belong in the real
//! text — including the case where a selection spanned a folded block, which
//! then disappears with it, exactly as it does elsewhere.

use std::collections::BTreeSet;

/// One run of consecutive visible lines: where it sits in the display string
/// and where the same bytes live in the real text.
struct Segment {
    display_start: usize,
    real_start: usize,
    len: usize,
}

pub struct Mapping {
    pub display: String,
    /// Real line number for each displayed line, in order.
    pub lines: Vec<usize>,
    segments: Vec<Segment>,
    real_len: usize,
}

impl Mapping {
    /// Builds the display string for `text` with `hidden` lines removed.
    pub fn new(text: &str, hidden: &BTreeSet<usize>) -> Self {
        let mut display = String::with_capacity(text.len());
        let mut lines = Vec::new();
        let mut segments: Vec<Segment> = Vec::new();
        let mut real_offset = 0usize;

        for (number, line) in text.split('\n').enumerate() {
            // Every line but the last carries a newline in the real text.
            let real_len = line.len() + 1;
            if hidden.contains(&number) {
                real_offset += real_len;
                continue;
            }
            let display_start = display.len();
            display.push_str(line);
            display.push('\n');
            lines.push(number);
            match segments.last_mut() {
                Some(last)
                    if last.real_start + last.len == real_offset
                        && last.display_start + last.len == display_start =>
                {
                    last.len += real_len;
                }
                _ => segments.push(Segment {
                    display_start,
                    real_start: real_offset,
                    len: real_len,
                }),
            }
            real_offset += real_len;
        }
        // `split` produced a trailing empty piece for a text ending in a
        // newline; drop the extra newline the loop appended.
        display.pop();
        if let Some(last) = segments.last_mut() {
            last.len = last.len.saturating_sub(1);
        }

        Self {
            display,
            lines,
            segments,
            real_len: text.len(),
        }
    }

    /// Real byte offset for a display byte offset.
    fn to_real(&self, offset: usize) -> usize {
        for segment in &self.segments {
            if offset < segment.display_start + segment.len {
                let within = offset.saturating_sub(segment.display_start);
                return segment.real_start + within;
            }
        }
        self.real_len
    }

    /// Applies an edit made against the display string to the real text.
    pub fn splice(&self, real: &mut String, edited: &str) {
        if edited == self.display {
            return;
        }
        let old = self.display.as_bytes();
        let new = edited.as_bytes();

        // The changed span is what is left after the shared head and tail.
        let mut head = 0;
        while head < old.len() && head < new.len() && old[head] == new[head] {
            head += 1;
        }
        let mut tail = 0;
        while tail < old.len() - head
            && tail < new.len() - head
            && old[old.len() - 1 - tail] == new[new.len() - 1 - tail]
        {
            tail += 1;
        }
        // Back off to character boundaries so the splice stays valid UTF-8.
        while head > 0 && (!self.display.is_char_boundary(head) || !edited.is_char_boundary(head)) {
            head -= 1;
        }
        let mut old_end = old.len() - tail;
        let mut new_end = new.len() - tail;
        while old_end < old.len()
            && (!self.display.is_char_boundary(old_end) || !edited.is_char_boundary(new_end))
        {
            old_end += 1;
            new_end += 1;
        }

        let real_start = self.to_real(head);
        let real_end = self.to_real(old_end).max(real_start);
        if real_start <= real.len() && real_end <= real.len() {
            real.replace_range(real_start..real_end, &edited[head..new_end]);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEXT: &str = "a\nb\nc\nd\ne";

    fn hidden(lines: &[usize]) -> BTreeSet<usize> {
        lines.iter().copied().collect()
    }

    #[test]
    fn display_drops_hidden_lines() {
        let m = Mapping::new(TEXT, &hidden(&[1, 2]));
        assert_eq!(m.display, "a\nd\ne");
        assert_eq!(m.lines, vec![0, 3, 4]);
    }

    #[test]
    fn nothing_hidden_is_the_text_itself() {
        let m = Mapping::new(TEXT, &hidden(&[]));
        assert_eq!(m.display, TEXT);
        assert_eq!(m.lines, vec![0, 1, 2, 3, 4]);
    }

    #[test]
    fn typing_after_a_fold_lands_in_the_right_place() {
        let m = Mapping::new(TEXT, &hidden(&[1, 2]));
        let mut real = TEXT.to_string();
        m.splice(&mut real, "a\ndX\ne");
        assert_eq!(real, "a\nb\nc\ndX\ne");
    }

    #[test]
    fn typing_before_a_fold_lands_in_the_right_place() {
        let m = Mapping::new(TEXT, &hidden(&[1, 2]));
        let mut real = TEXT.to_string();
        m.splice(&mut real, "aX\nd\ne");
        assert_eq!(real, "aX\nb\nc\nd\ne");
    }

    #[test]
    fn deleting_across_a_fold_takes_the_hidden_lines_with_it() {
        let m = Mapping::new(TEXT, &hidden(&[1, 2]));
        let mut real = TEXT.to_string();
        // The user selected "a\nd" in the display and typed "Z".
        m.splice(&mut real, "Z\ne");
        assert_eq!(real, "Z\ne");
    }

    #[test]
    fn multibyte_text_survives_a_splice() {
        let text = "фыв\nбюё\nzzz";
        let m = Mapping::new(text, &hidden(&[1]));
        assert_eq!(m.display, "фыв\nzzz");
        let mut real = text.to_string();
        m.splice(&mut real, "фыва\nzzz");
        assert_eq!(real, "фыва\nбюё\nzzz");
    }

    #[test]
    fn an_unchanged_display_leaves_the_text_alone() {
        let m = Mapping::new(TEXT, &hidden(&[1]));
        let mut real = TEXT.to_string();
        m.splice(&mut real, &m.display.clone());
        assert_eq!(real, TEXT);
    }
}

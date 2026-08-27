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
        // The loop put a newline after every visible line, and the real text
        // has one after every line but the last. When the last line is on
        // show, the display has one newline too many; when a fold hides it,
        // every newline in the display is a real one and all of them stay.
        let last_line = text.split('\n').count() - 1;
        if !hidden.contains(&last_line) {
            display.pop();
            if let Some(last) = segments.last_mut() {
                last.len = last.len.saturating_sub(1);
            }
        }

        Self {
            display,
            lines,
            segments,
            real_len: text.len(),
        }
    }

    /// Real byte offset for a display byte offset. The end of the display is
    /// the end of the last visible line, not the end of the file: a fold may
    /// hide everything after it, and an edit at the end of what is shown
    /// belongs before that, not after.
    fn to_real(&self, offset: usize) -> usize {
        for segment in &self.segments {
            if offset < segment.display_start + segment.len {
                let within = offset.saturating_sub(segment.display_start);
                return segment.real_start + within;
            }
        }
        match self.segments.last() {
            Some(last) if offset <= last.display_start + last.len => last.real_start + last.len,
            Some(_) => self.real_len,
            None => 0,
        }
    }

    /// Whether a display offset sits on the seam between a visible line and
    /// a fold after it — where "just after" and "just before" the fold are
    /// different places in the real text.
    fn at_fold(&self, offset: usize) -> bool {
        self.segments.iter().any(|segment| {
            offset == segment.display_start + segment.len
                && segment.real_start + segment.len < self.real_len
        })
    }

    /// Applies an edit made against the display string to the real text.
    pub fn splice(&self, real: &mut String, edited: &str) {
        self.splice_at(real, edited, None);
    }

    /// The same, told where the caret ended up in `edited`. An edit that
    /// starts and ends with the same character as the text around a fold —
    /// Enter at the end of a folded header, say — reads the same whether it
    /// went before the fold or after it, and only the caret can tell which.
    pub fn splice_at(&self, real: &mut String, edited: &str, caret: Option<usize>) {
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
        let mut old_end = old.len() - tail;
        let mut new_end = new.len() - tail;
        // The shared head ran as far as it could, which can carry the edit
        // across a fold's seam: "fn f() {" + "\n    " + "\n" and
        // "fn f() {\n" + "    \n" are the same string. When the caret says
        // the edit ended earlier, the span is slid back over the seam.
        if let Some(caret) = caret {
            while head > 0
                && new_end > caret
                && old_end > 0
                && old[old_end - 1] == new[new_end - 1]
                && self.at_fold(head)
            {
                head -= 1;
                old_end -= 1;
                new_end -= 1;
            }
        }
        // Back off to character boundaries so the splice stays valid UTF-8.
        while head > 0 && (!self.display.is_char_boundary(head) || !edited.is_char_boundary(head)) {
            head -= 1;
        }
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
    fn a_fold_to_the_end_of_a_file_without_a_final_newline_keeps_its_body() {
        // The last line is hidden and carries no newline, so nothing was
        // appended for it and nothing is popped: the display keeps its
        // newline, and its end is the end of the header, not of the file.
        let text = "fn main() {\n    x\n}";
        let m = Mapping::new(text, &hidden(&[1, 2]));
        assert_eq!(m.display, "fn main() {\n");

        let mut real = text.to_string();
        m.splice(&mut real, "fn main() {X\n");
        assert_eq!(real, "fn main() {X\n    x\n}", "typing lands on the header");

        let mut real = text.to_string();
        m.splice(&mut real, "fn main() \n");
        assert_eq!(
            real, "fn main() \n    x\n}",
            "backspace takes one character"
        );
    }

    #[test]
    fn enter_at_the_end_of_a_folded_header_opens_a_line_under_it() {
        let text = "fn main() {\n    a();\n    b();\n}\n";
        let m = Mapping::new(text, &hidden(&[1, 2, 3]));
        assert_eq!(m.display, "fn main() {\n");
        // Smart indent inserted "\n    " after the brace; the caret sits at
        // the end of that.
        let edited = "fn main() {\n    \n";
        let mut real = text.to_string();
        m.splice_at(&mut real, edited, Some("fn main() {\n    ".len()));
        assert_eq!(real, "fn main() {\n    \n    a();\n    b();\n}\n");
    }

    #[test]
    fn enter_before_the_line_after_a_fold_stays_after_it() {
        // Caret before "d", Enter: the new line goes before d, after the
        // hidden block — the caret says so.
        let m = Mapping::new(TEXT, &hidden(&[1, 2]));
        let mut real = TEXT.to_string();
        m.splice_at(&mut real, "a\n\nd\ne", Some(3));
        assert_eq!(real, "a\nb\nc\n\nd\ne");
    }

    #[test]
    fn an_unchanged_display_leaves_the_text_alone() {
        let m = Mapping::new(TEXT, &hidden(&[1]));
        let mut real = TEXT.to_string();
        m.splice(&mut real, &m.display.clone());
        assert_eq!(real, TEXT);
    }
}

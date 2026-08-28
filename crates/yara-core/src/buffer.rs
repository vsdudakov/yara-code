//! An open file: its text, where the caret is, and the edits made to it.
//! The caret is a char offset; lines and columns are worked out from it.

use std::path::{Path, PathBuf};

use crate::history::{EditKind, History, Snapshot};

pub struct Buffer {
    pub path: PathBuf,
    pub text: String,
    saved_text: String,
    /// The caret, as a char offset into `text`.
    pub cursor: usize,
    /// The column the caret wants when moving up and down through shorter
    /// lines; forgotten by any other move.
    goal: Option<usize>,
    history: History,
    /// The file ends its lines with `\r\n`; the buffer holds `\n` and gives
    /// the endings back on save.
    crlf: bool,
}

impl Buffer {
    pub fn open(path: &Path) -> std::io::Result<Self> {
        let raw = std::fs::read_to_string(path)?;
        let crlf = raw.contains("\r\n");
        let text = if crlf { raw.replace("\r\n", "\n") } else { raw };
        Ok(Self {
            path: path.to_path_buf(),
            saved_text: text.clone(),
            text,
            cursor: 0,
            goal: None,
            history: History::default(),
            crlf,
        })
    }

    pub fn save(&mut self) -> std::io::Result<()> {
        let out = if self.crlf {
            self.text.replace('\n', "\r\n")
        } else {
            self.text.clone()
        };
        std::fs::write(&self.path, out)?;
        self.saved_text = self.text.clone();
        Ok(())
    }

    pub fn modified(&self) -> bool {
        self.text != self.saved_text
    }

    pub fn extension(&self) -> &str {
        self.path.extension().and_then(|e| e.to_str()).unwrap_or("")
    }

    /// The caret as (line, column), both from zero.
    pub fn line_col(&self) -> (usize, usize) {
        let before: String = self.text.chars().take(self.cursor).collect();
        let line = before.matches('\n').count();
        let col = before.rsplit('\n').next().map_or(0, |l| l.chars().count());
        (line, col)
    }

    fn offset_of(&self, line: usize, col: usize) -> usize {
        let mut offset = 0;
        for (i, text) in self.text.split('\n').enumerate() {
            let len = text.chars().count();
            if i == line {
                return offset + col.min(len);
            }
            offset += len + 1;
        }
        self.text.chars().count()
    }

    fn byte_at(&self, char_offset: usize) -> usize {
        self.text
            .char_indices()
            .nth(char_offset)
            .map_or(self.text.len(), |(i, _)| i)
    }

    pub fn insert(&mut self, s: &str) {
        self.history
            .begin(EditKind::Insert, &self.text, self.cursor);
        let at = self.byte_at(self.cursor);
        self.text.insert_str(at, s);
        self.cursor += s.chars().count();
        self.goal = None;
    }

    /// Deletes the char before the caret.
    pub fn backspace(&mut self) {
        if self.cursor == 0 {
            return;
        }
        self.history
            .begin(EditKind::Delete, &self.text, self.cursor);
        let (from, to) = (self.byte_at(self.cursor - 1), self.byte_at(self.cursor));
        self.text.replace_range(from..to, "");
        self.cursor -= 1;
        self.goal = None;
    }

    /// Deletes the char under the caret.
    pub fn delete(&mut self) {
        if self.cursor >= self.text.chars().count() {
            return;
        }
        self.history
            .begin(EditKind::Delete, &self.text, self.cursor);
        let (from, to) = (self.byte_at(self.cursor), self.byte_at(self.cursor + 1));
        self.text.replace_range(from..to, "");
        self.goal = None;
    }

    pub fn left(&mut self) {
        self.cursor = self.cursor.saturating_sub(1);
        self.moved();
    }

    pub fn right(&mut self) {
        self.cursor = (self.cursor + 1).min(self.text.chars().count());
        self.moved();
    }

    pub fn up(&mut self) {
        self.vertical(-1);
    }

    pub fn down(&mut self) {
        self.vertical(1);
    }

    fn vertical(&mut self, delta: isize) {
        let (line, col) = self.line_col();
        let goal = self.goal.unwrap_or(col);
        let target = (line as isize + delta).max(0) as usize;
        self.cursor = self.offset_of(target, goal);
        self.history.end_run();
        self.goal = Some(goal);
    }

    pub fn home(&mut self) {
        let (line, _) = self.line_col();
        self.cursor = self.offset_of(line, 0);
        self.moved();
    }

    pub fn end(&mut self) {
        let (line, _) = self.line_col();
        self.cursor = self.offset_of(line, usize::MAX);
        self.moved();
    }

    fn moved(&mut self) {
        self.history.end_run();
        self.goal = None;
    }

    pub fn undo(&mut self) {
        let now = self.snapshot();
        if let Some(back) = self.history.undo(now) {
            self.text = back.text;
            self.cursor = back.cursor;
        }
    }

    pub fn redo(&mut self) {
        let now = self.snapshot();
        if let Some(next) = self.history.redo(now) {
            self.text = next.text;
            self.cursor = next.cursor;
        }
    }

    fn snapshot(&self) -> Snapshot {
        Snapshot {
            text: self.text.clone(),
            cursor: self.cursor,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::Dir;

    fn buffer(dir: &Dir, text: &str) -> Buffer {
        Buffer::open(&dir.file("f.rs", text)).unwrap()
    }

    #[test]
    fn typing_moves_the_caret_and_marks_the_buffer_dirty_until_saved() {
        let dir = Dir::new("yara-buffer-type");
        let mut b = buffer(&dir, "ab\n");
        assert!(!b.modified());
        b.right();
        b.insert("é");
        assert_eq!(b.text, "aéb\n");
        assert_eq!(b.cursor, 2);
        assert!(b.modified());
        b.save().unwrap();
        assert!(!b.modified());
        assert_eq!(std::fs::read_to_string(&b.path).unwrap(), "aéb\n");
        assert_eq!(b.extension(), "rs");
    }

    #[test]
    fn crlf_files_keep_their_endings() {
        let dir = Dir::new("yara-buffer-crlf");
        let mut b = buffer(&dir, "a\r\nb\r\n");
        assert_eq!(b.text, "a\nb\n");
        b.insert("x");
        b.save().unwrap();
        assert_eq!(std::fs::read_to_string(&b.path).unwrap(), "xa\r\nb\r\n");
    }

    #[test]
    fn backspace_and_delete_work_at_the_edges() {
        let dir = Dir::new("yara-buffer-del");
        let mut b = buffer(&dir, "ab");
        b.backspace();
        b.delete();
        assert_eq!(b.text, "b");
        b.right();
        b.delete();
        b.backspace();
        assert_eq!(b.text, "");
        assert_eq!(b.cursor, 0);
    }

    #[test]
    fn the_caret_keeps_its_column_across_short_lines() {
        let dir = Dir::new("yara-buffer-move");
        let mut b = buffer(&dir, "long line\n\nanother\n");
        b.end();
        assert_eq!(b.line_col(), (0, 9));
        b.down();
        assert_eq!(b.line_col(), (1, 0));
        b.down();
        assert_eq!(b.line_col(), (2, 7), "back to the goal column");
        b.home();
        assert_eq!(b.line_col(), (2, 0));
        b.up();
        b.up();
        b.up();
        assert_eq!(b.line_col(), (0, 0), "never above the first line");
        b.down();
        b.down();
        b.down();
        b.down();
        assert_eq!(b.cursor, b.text.chars().count(), "never past the end");
    }

    #[test]
    fn a_run_of_typing_undoes_at_once_and_redoes_again() {
        let dir = Dir::new("yara-buffer-undo");
        let mut b = buffer(&dir, "");
        for c in ["a", "b", "c"] {
            b.insert(c);
        }
        b.left();
        b.insert("X");
        assert_eq!(b.text, "abXc");
        b.undo();
        assert_eq!((b.text.as_str(), b.cursor), ("abc", 2));
        b.undo();
        assert_eq!(b.text, "");
        b.redo();
        assert_eq!(b.text, "abc");
        b.redo();
        assert_eq!(b.text, "abXc");
        b.redo();
        assert_eq!(b.text, "abXc", "nothing more to redo");
    }
}

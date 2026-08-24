//! Open files and the set of them — frontend-independent.

use std::path::{Path, PathBuf};

use crate::core::history::{EditKind, History, Snapshot};

pub struct Buffer {
    pub path: PathBuf,
    pub text: String,
    pub saved_text: String,
    pub extension: String,
    /// Undo/redo for this buffer, fed by whichever frontend is editing it.
    pub history: History,
}

impl Buffer {
    /// Records the state a change is about to replace. Call before mutating
    /// `text`, with the kind of change and where the cursor is.
    pub fn record(&mut self, kind: EditKind, cursor: usize) {
        self.history.begin(kind, &self.text, cursor);
    }

    /// Steps back, returning where the cursor should go.
    pub fn undo(&mut self, cursor: usize) -> Option<usize> {
        let now = Snapshot {
            text: self.text.clone(),
            cursor,
        };
        let back = self.history.undo(now)?;
        self.text = back.text;
        Some(back.cursor)
    }

    pub fn redo(&mut self, cursor: usize) -> Option<usize> {
        let now = Snapshot {
            text: self.text.clone(),
            cursor,
        };
        let forward = self.history.redo(now)?;
        self.text = forward.text;
        Some(forward.cursor)
    }

    pub fn modified(&self) -> bool {
        self.text != self.saved_text
    }

    pub fn name(&self) -> String {
        self.path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| self.path.display().to_string())
    }
}

#[derive(Default)]
pub struct Buffers {
    pub list: Vec<Buffer>,
    pub active: usize,
}

impl Buffers {
    pub fn is_empty(&self) -> bool {
        self.list.is_empty()
    }

    pub fn active(&self) -> Option<&Buffer> {
        self.list.get(self.active)
    }

    pub fn active_mut(&mut self) -> Option<&mut Buffer> {
        self.list.get_mut(self.active)
    }

    /// Opens `path`, focusing an existing buffer if it is already open.
    /// Returns false if the file could not be read as text.
    pub fn open(&mut self, path: PathBuf) -> bool {
        if let Some(i) = self.list.iter().position(|b| b.path == path) {
            self.active = i;
            return true;
        }
        let Ok(text) = std::fs::read_to_string(&path) else {
            return false;
        };
        let extension = path
            .extension()
            .map(|e| e.to_string_lossy().into_owned())
            .unwrap_or_default();
        self.list.push(Buffer {
            path,
            saved_text: text.clone(),
            text,
            extension,
            history: History::default(),
        });
        self.active = self.list.len() - 1;
        true
    }

    pub fn save_active(&mut self) -> bool {
        self.save(self.active)
    }

    /// Writes buffer `index` back to its file.
    pub fn save(&mut self, index: usize) -> bool {
        let Some(buf) = self.list.get_mut(index) else {
            return false;
        };
        if std::fs::write(&buf.path, &buf.text).is_ok() {
            buf.saved_text = buf.text.clone();
            true
        } else {
            false
        }
    }

    /// Moves a tab to another position, keeping the active buffer active.
    pub fn reorder(&mut self, from: usize, to: usize) {
        if from >= self.list.len() || to >= self.list.len() || from == to {
            return;
        }
        let buf = self.list.remove(from);
        self.list.insert(to, buf);
        self.active = shift_index(self.active, from, to);
    }

    pub fn close(&mut self, index: usize) {
        if index < self.list.len() {
            self.list.remove(index);
            if self.active >= index && self.active > 0 {
                self.active -= 1;
            }
        }
    }

    /// Closes every buffer at or under `path` (used after deletion).
    pub fn close_path(&mut self, path: &Path) {
        let active_path = self.list.get(self.active).map(|b| b.path.clone());
        self.list.retain(|b| !b.path.starts_with(path));
        self.active = active_path
            .and_then(|p| self.list.iter().position(|b| b.path == p))
            .unwrap_or(0);
    }

    /// Rewrites buffer paths after `from` was moved to `to`.
    pub fn retarget(&mut self, from: &Path, to: &Path) {
        for buf in &mut self.list {
            if let Ok(rest) = buf.path.strip_prefix(from) {
                buf.path = if rest.as_os_str().is_empty() {
                    to.to_path_buf()
                } else {
                    to.join(rest)
                };
            }
        }
    }
}

/// Where an index ends up after the item at `from` moves to `to`.
pub fn shift_index(index: usize, from: usize, to: usize) -> usize {
    if index == from {
        to
    } else if from < index && index <= to {
        index - 1
    } else if to <= index && index < from {
        index + 1
    } else {
        index
    }
}

pub fn relative_path(path: &Path, root: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .display()
        .to_string()
}

/// The identifier surrounding char index `idx`, as (word, start, end) in char
/// offsets. Returns `None` if the position isn't inside a navigable identifier.
pub fn word_at(text: &str, idx: usize) -> Option<(String, usize, usize)> {
    let chars: Vec<char> = text.chars().collect();
    let is_ident = |c: char| c.is_alphanumeric() || c == '_';
    // A cursor can sit past the end — of a shorter line, or of text an edit has
    // just trimmed — and that is the end of the text, not a panic.
    let idx = idx.min(chars.len());
    if idx == chars.len() && !(idx > 0 && is_ident(chars[idx - 1])) {
        return None;
    }
    let mut start = idx;
    let mut end = start;
    while start > 0 && is_ident(chars[start - 1]) {
        start -= 1;
    }
    while end < chars.len() && is_ident(chars[end]) {
        end += 1;
    }
    if start == end {
        return None;
    }
    let word: String = chars[start..end].iter().collect();
    if word.chars().next().is_some_and(|c| c.is_ascii_digit()) || word.chars().count() < 2 {
        return None;
    }
    Some((word, start, end))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_moved_tab_takes_the_selection_with_it() {
        // Dragging the active tab right: it is still the active one.
        assert_eq!(shift_index(0, 0, 2), 2);
        // Tabs it stepped over shift back by one.
        assert_eq!(shift_index(1, 0, 2), 0);
        assert_eq!(shift_index(2, 0, 2), 1);
        // Dragging left pushes the ones it passes to the right.
        assert_eq!(shift_index(1, 3, 1), 2);
        assert_eq!(shift_index(3, 3, 1), 1);
        // Anything outside the moved range stays put.
        assert_eq!(shift_index(4, 0, 2), 4);
    }
}

#[cfg(test)]
mod buffer_tests {
    use super::*;
    use crate::core::test_support::Dir;

    fn open(dir: &Dir, name: &str, body: &str) -> PathBuf {
        dir.file(name, body)
    }

    #[test]
    fn opening_the_same_file_twice_focuses_the_tab_it_already_has() {
        let dir = Dir::new("yara-buf-open");
        let path = open(&dir, "a.rs", "fn main() {}\n");
        let other = open(&dir, "b.rs", "");
        let mut buffers = Buffers::default();
        assert!(buffers.open(path.clone()));
        assert!(buffers.open(other));
        assert_eq!(buffers.active, 1);
        assert!(buffers.open(path));
        assert_eq!(buffers.active, 0, "no second tab for a file already open");
        assert_eq!(buffers.list.len(), 2);
    }

    #[test]
    fn a_file_that_is_not_text_does_not_open() {
        let dir = Dir::new("yara-buf-binary");
        let path = dir.path().join("blob.bin");
        std::fs::write(&path, [0xff, 0xfe, 0x00, 0x9f]).unwrap();
        let mut buffers = Buffers::default();
        assert!(!buffers.open(path));
        assert!(buffers.is_empty());
        assert!(buffers.active().is_none());
    }

    #[test]
    fn saving_writes_the_text_and_clears_the_modified_mark() {
        let dir = Dir::new("yara-buf-save");
        let path = open(&dir, "a.txt", "one\n");
        let mut buffers = Buffers::default();
        buffers.open(path.clone());
        buffers.active_mut().unwrap().text.push_str("two\n");
        assert!(buffers.active().unwrap().modified());
        assert!(buffers.save_active());
        assert!(!buffers.active().unwrap().modified());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "one\ntwo\n");
        assert!(!buffers.save(9), "there is no ninth buffer");
    }

    #[test]
    fn closing_keeps_the_selection_on_a_neighbour() {
        let dir = Dir::new("yara-buf-close");
        let mut buffers = Buffers::default();
        for name in ["a.txt", "b.txt", "c.txt"] {
            buffers.open(open(&dir, name, ""));
        }
        assert_eq!(buffers.active, 2);
        buffers.close(2);
        assert_eq!(buffers.active, 1);
        buffers.close(0);
        assert_eq!(buffers.list.len(), 1);
        buffers.close(7); // out of range: nothing happens
        assert_eq!(buffers.list.len(), 1);
    }

    #[test]
    fn deleting_a_folder_closes_what_was_open_under_it() {
        let dir = Dir::new("yara-buf-tree");
        let mut buffers = Buffers::default();
        buffers.open(open(&dir, "keep.txt", ""));
        buffers.open(open(&dir, "src/one.rs", ""));
        buffers.open(open(&dir, "src/two.rs", ""));
        buffers.active = 0;
        buffers.close_path(&dir.path().join("src"));
        assert_eq!(buffers.list.len(), 1);
        assert_eq!(buffers.active().unwrap().name(), "keep.txt");
    }

    #[test]
    fn a_moved_folder_takes_its_open_files_with_it() {
        let dir = Dir::new("yara-buf-move");
        let mut buffers = Buffers::default();
        buffers.open(open(&dir, "src/one.rs", ""));
        let from = dir.path().join("src");
        let to = dir.path().join("lib");
        buffers.retarget(&from, &to);
        assert_eq!(buffers.list[0].path, to.join("one.rs"));
        // The folder itself, renamed, lands on the new name rather than under it.
        buffers.list[0].path = to.clone();
        buffers.retarget(&to, &dir.path().join("core"));
        assert_eq!(buffers.list[0].path, dir.path().join("core"));
    }

    #[test]
    fn reordering_moves_the_tab_and_keeps_the_active_one_active() {
        let dir = Dir::new("yara-buf-reorder");
        let mut buffers = Buffers::default();
        for name in ["a.txt", "b.txt", "c.txt"] {
            buffers.open(open(&dir, name, ""));
        }
        buffers.active = 0;
        buffers.reorder(0, 2);
        assert_eq!(buffers.list[2].name(), "a.txt");
        assert_eq!(buffers.active, 2, "the dragged tab stays in front");
        buffers.reorder(0, 0); // no-op
        buffers.reorder(0, 9); // out of range
        assert_eq!(buffers.list[2].name(), "a.txt");
    }

    #[test]
    fn a_path_is_shown_relative_to_the_project() {
        let root = Path::new("/work/project");
        assert_eq!(
            relative_path(&root.join("src/main.rs"), root),
            "src/main.rs"
        );
        // Outside the project, the whole path is the only honest answer.
        let outside = Path::new("/elsewhere/file.rs");
        assert_eq!(relative_path(outside, root), "/elsewhere/file.rs");
    }

    #[test]
    fn a_buffer_names_itself_after_its_file() {
        let buffer = Buffer {
            path: PathBuf::from("/a/b/main.rs"),
            text: String::new(),
            saved_text: String::new(),
            extension: "rs".into(),
            history: History::default(),
        };
        assert_eq!(buffer.name(), "main.rs");
        assert!(!buffer.modified());
    }

    #[test]
    fn the_word_under_the_cursor_is_an_identifier_or_nothing() {
        let text = "let total = other_value + 1;";
        let (word, start, end) = word_at(text, 6).unwrap();
        assert_eq!((word.as_str(), start, end), ("total", 4, 9));
        // The end of a word counts as being in it.
        assert_eq!(word_at(text, 9).unwrap().0, "total");
        // Punctuation, digits and single letters are not worth navigating to.
        assert!(word_at(text, 10).is_none());
        assert!(word_at("x = 1", 0).is_none(), "one letter");
        assert!(word_at("42 things", 0).is_none(), "starts with a digit");
        assert!(word_at("", 0).is_none());
        // A cursor past the end reads as the end of the text: the word there,
        // if any, rather than a panic.
        assert_eq!(word_at("value", 99).unwrap().0, "value");
        assert!(word_at("value ", 99).is_none());
    }
}

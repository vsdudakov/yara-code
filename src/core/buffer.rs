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
    if idx >= chars.len() && !(idx > 0 && is_ident(chars[idx - 1])) {
        return None;
    }
    let mut start = idx.min(chars.len());
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

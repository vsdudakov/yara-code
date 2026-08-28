//! Undo/redo for one buffer, shared by both frontends.
//!
//! A step is a whole text snapshot rather than a diff: files here are small
//! enough that the simplicity is worth more than the bytes, and it makes a bulk
//! rewrite (Replace All) exactly as undoable as a single keystroke.

/// What produced a change. Consecutive changes of the same kind fold into one
/// step, so a run of typing undoes as a word, not a letter at a time; anything
/// `Bulk` always stands alone.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum EditKind {
    Insert,
    Delete,
    Bulk,
}

#[derive(Clone, Debug)]
pub struct Snapshot {
    pub text: String,
    /// Cursor position as a char offset, restored along with the text.
    pub cursor: usize,
}

/// How many steps a buffer keeps. Past this the oldest are dropped.
const MAX_STEPS: usize = 200;

#[derive(Default)]
pub struct History {
    steps: Vec<Snapshot>,
    undone: Vec<Snapshot>,
    /// The run currently being folded into the newest step.
    open: Option<EditKind>,
}

impl History {
    /// Called *before* a change, with the state it is about to replace. Only
    /// the first change of a run stores anything.
    pub fn begin(&mut self, kind: EditKind, text: &str, cursor: usize) {
        if self.open == Some(kind) && kind != EditKind::Bulk {
            return;
        }
        self.undone.clear();
        self.steps.push(Snapshot {
            text: text.to_string(),
            cursor,
        });
        if self.steps.len() > MAX_STEPS {
            self.steps.remove(0);
        }
        self.open = if kind == EditKind::Bulk {
            None
        } else {
            Some(kind)
        };
    }

    /// Ends the current run: the next change starts a step of its own. Called
    /// when the cursor moves, so undo follows what was typed where.
    pub fn end_run(&mut self) {
        self.open = None;
    }

    /// Steps back, handing `now` to redo. `None` when there is nothing to undo.
    pub fn undo(&mut self, now: Snapshot) -> Option<Snapshot> {
        let previous = self.steps.pop()?;
        self.undone.push(now);
        self.open = None;
        Some(previous)
    }

    /// Steps forward again, handing `now` back to undo.
    pub fn redo(&mut self, now: Snapshot) -> Option<Snapshot> {
        let next = self.undone.pop()?;
        self.steps.push(now);
        self.open = None;
        Some(next)
    }

    pub fn can_undo(&self) -> bool {
        !self.steps.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.undone.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snap(text: &str) -> Snapshot {
        Snapshot {
            text: text.to_string(),
            cursor: text.chars().count(),
        }
    }

    #[test]
    fn a_run_of_typing_undoes_in_one_step() {
        let mut history = History::default();
        history.begin(EditKind::Insert, "", 0);
        history.begin(EditKind::Insert, "a", 1);
        history.begin(EditKind::Insert, "ab", 2);
        let back = history.undo(snap("abc")).unwrap();
        assert_eq!(back.text, "", "the whole run steps back at once");
        assert!(!history.can_undo());
    }

    #[test]
    fn a_different_kind_starts_its_own_step() {
        let mut history = History::default();
        history.begin(EditKind::Insert, "", 0);
        history.begin(EditKind::Delete, "ab", 2);
        assert_eq!(history.undo(snap("a")).unwrap().text, "ab");
        assert_eq!(history.undo(snap("ab")).unwrap().text, "");
    }

    #[test]
    fn a_bulk_change_stands_alone() {
        let mut history = History::default();
        history.begin(EditKind::Bulk, "one", 0);
        history.begin(EditKind::Bulk, "two", 0);
        assert_eq!(history.undo(snap("three")).unwrap().text, "two");
        assert_eq!(history.undo(snap("two")).unwrap().text, "one");
    }

    #[test]
    fn redo_replays_what_undo_took_back() {
        let mut history = History::default();
        history.begin(EditKind::Bulk, "before", 0);
        let back = history.undo(snap("after")).unwrap();
        assert_eq!(back.text, "before");
        assert!(history.can_redo());
        assert_eq!(history.redo(snap("before")).unwrap().text, "after");
        assert!(history.can_undo());
    }

    #[test]
    fn editing_after_an_undo_drops_the_redo_trail() {
        let mut history = History::default();
        history.begin(EditKind::Bulk, "before", 0);
        history.undo(snap("after")).unwrap();
        history.begin(EditKind::Insert, "before", 0);
        assert!(!history.can_redo());
    }

    #[test]
    fn a_new_run_starts_after_the_cursor_moves() {
        let mut history = History::default();
        history.begin(EditKind::Insert, "", 0);
        history.end_run();
        history.begin(EditKind::Insert, "ab", 2);
        assert_eq!(history.undo(snap("abc")).unwrap().text, "ab");
        assert_eq!(history.undo(snap("ab")).unwrap().text, "");
    }
}

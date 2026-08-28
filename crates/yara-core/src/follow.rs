//! The follow loop: the edits an agent has made, in the order it made them,
//! and where the FOLLOW pane stands among them.
//!
//! The pane is either *live* — pinned to the newest edit, snapping forward as
//! each new one lands — or *paused* on an edit the user scrubbed back to.
//! Reviewing is a separate bit per edit: marking one reviewed moves to the
//! oldest still unreviewed, and once none remain the pane goes live again.

use std::path::PathBuf;

/// What a line of a hunk did to the file.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LineKind {
    Context,
    Added,
    Removed,
}

/// One line of a hunk, without its leading sign.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HunkLine {
    pub kind: LineKind,
    pub text: String,
}

/// A run of changed lines, numbered from where it starts in each version.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Hunk {
    pub old_start: usize,
    pub new_start: usize,
    pub lines: Vec<HunkLine>,
}

impl Hunk {
    pub fn added(&self) -> usize {
        self.count(LineKind::Added)
    }

    pub fn removed(&self) -> usize {
        self.count(LineKind::Removed)
    }

    fn count(&self, kind: LineKind) -> usize {
        self.lines.iter().filter(|line| line.kind == kind).count()
    }
}

/// One edit the agent made: a file and what changed in it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EditEvent {
    pub path: PathBuf,
    pub hunks: Vec<Hunk>,
}

impl EditEvent {
    /// Reads the hunks of a unified diff. The file header, if any, is skipped;
    /// a diff without hunk headers is read as one hunk starting at line 1, so
    /// a tool that reports only `+`/`-` lines still produces an edit.
    pub fn from_unified(path: impl Into<PathBuf>, diff: &str) -> Self {
        let mut hunks: Vec<Hunk> = Vec::new();
        for line in diff.lines() {
            if let Some(header) = line.strip_prefix("@@") {
                let (old_start, new_start) = hunk_start(header).unwrap_or((1, 1));
                hunks.push(Hunk {
                    old_start,
                    new_start,
                    lines: Vec::new(),
                });
                continue;
            }
            let (kind, text) = match line.chars().next() {
                Some('+') if !line.starts_with("+++") => (LineKind::Added, &line[1..]),
                Some('-') if !line.starts_with("---") => (LineKind::Removed, &line[1..]),
                Some(' ') => (LineKind::Context, &line[1..]),
                // `\ No newline at end of file`, `diff --git`, `index …`.
                _ => continue,
            };
            if hunks.is_empty() {
                hunks.push(Hunk {
                    old_start: 1,
                    new_start: 1,
                    lines: Vec::new(),
                });
            }
            let hunk = hunks.last_mut().expect("a hunk was just pushed");
            hunk.lines.push(HunkLine {
                kind,
                text: text.to_string(),
            });
        }
        Self {
            path: path.into(),
            hunks,
        }
    }

    pub fn added(&self) -> usize {
        self.hunks.iter().map(Hunk::added).sum()
    }

    pub fn removed(&self) -> usize {
        self.hunks.iter().map(Hunk::removed).sum()
    }
}

/// The start lines of `-a,b +c,d`; the counts are not needed, the lines
/// themselves say how long the hunk is.
fn hunk_start(header: &str) -> Option<(usize, usize)> {
    let mut parts = header.split_whitespace();
    let old = parts.next()?.strip_prefix('-')?;
    let new = parts.next()?.strip_prefix('+')?;
    let start = |range: &str| range.split(',').next()?.parse::<usize>().ok();
    Some((start(old)?, start(new)?))
}

/// Where the FOLLOW pane stands. `cursor` is meaningful only while there are
/// edits; `live` means it tracks the newest one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FollowState {
    pub live: bool,
    pub cursor: usize,
}

impl Default for FollowState {
    fn default() -> Self {
        Self {
            live: true,
            cursor: 0,
        }
    }
}

/// How one edit is drawn on the timeline strip.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tick {
    /// The edit under the cursor, whether or not it is reviewed.
    Current,
    Unreviewed,
    Reviewed,
}

/// The slice of the timeline that fits a strip of limited width, and how many
/// edits fall off either end.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Window {
    pub start: usize,
    pub end: usize,
    pub hidden_before: usize,
    pub hidden_after: usize,
}

/// The timeline and the pane's place on it.
#[derive(Clone, Debug, Default)]
pub struct Follow {
    state: FollowState,
    edits: Vec<EditEvent>,
    /// One flag per edit.
    reviewed: Vec<bool>,
}

impl Follow {
    pub fn state(&self) -> FollowState {
        self.state
    }

    pub fn edits(&self) -> &[EditEvent] {
        &self.edits
    }

    pub fn len(&self) -> usize {
        self.edits.len()
    }

    pub fn is_empty(&self) -> bool {
        self.edits.is_empty()
    }

    pub fn is_live(&self) -> bool {
        self.state.live
    }

    pub fn cursor(&self) -> usize {
        self.state.cursor
    }

    /// The edit under the cursor; none until the agent has made one.
    pub fn current(&self) -> Option<&EditEvent> {
        self.edits.get(self.state.cursor)
    }

    /// Records an edit. A live pane snaps to it; a paused one stays put.
    pub fn push(&mut self, edit: EditEvent) -> usize {
        self.edits.push(edit);
        self.reviewed.push(false);
        let index = self.edits.len() - 1;
        if self.state.live {
            self.state.cursor = index;
        }
        index
    }

    /// One edit older. Scrubbing at all pauses the pane, even at the oldest
    /// edit — the user asked to look, not to follow.
    pub fn scrub_back(&mut self) {
        if self.edits.is_empty() {
            return;
        }
        self.state.live = false;
        self.state.cursor = self.state.cursor.saturating_sub(1);
    }

    /// One edit newer; stepping onto the newest is the same as going live.
    pub fn scrub_forward(&mut self) {
        if self.edits.is_empty() {
            return;
        }
        self.jump_to((self.state.cursor + 1).min(self.edits.len() - 1));
    }

    /// Pins the pane to the newest edit again.
    pub fn go_live(&mut self) {
        self.state.live = true;
        self.state.cursor = self.edits.len().saturating_sub(1);
    }

    /// Moves to an edit outright — a click on its tick. Landing on the newest
    /// one is going live; anywhere else is a pause.
    pub fn jump_to(&mut self, index: usize) {
        if index >= self.edits.len() {
            return;
        }
        if index + 1 == self.edits.len() {
            self.go_live();
        } else {
            self.state.live = false;
            self.state.cursor = index;
        }
    }

    pub fn is_reviewed(&self, index: usize) -> bool {
        self.reviewed.get(index).copied().unwrap_or(false)
    }

    pub fn unreviewed_count(&self) -> usize {
        self.reviewed.iter().filter(|done| !**done).count()
    }

    /// Marks the current edit reviewed and moves on to the oldest one that is
    /// not; with nothing left to review the pane goes live.
    pub fn mark_reviewed(&mut self) {
        if self.edits.is_empty() {
            return;
        }
        self.reviewed[self.state.cursor] = true;
        match self.oldest_unreviewed() {
            Some(index) => self.jump_to(index),
            None => self.go_live(),
        }
    }

    /// Jumps to the next unreviewed edit after the cursor, wrapping around —
    /// the status bar's counter. Nothing to do when everything is reviewed.
    pub fn jump_to_next_unreviewed(&mut self) {
        let len = self.edits.len();
        let found = (1..=len)
            .map(|step| (self.state.cursor + step) % len.max(1))
            .find(|&index| !self.reviewed[index]);
        if let Some(index) = found {
            self.jump_to(index);
        }
    }

    fn oldest_unreviewed(&self) -> Option<usize> {
        self.reviewed.iter().position(|done| !done)
    }

    /// One tick per edit, oldest first.
    pub fn ticks(&self) -> Vec<Tick> {
        (0..self.edits.len())
            .map(|index| {
                if index == self.state.cursor {
                    Tick::Current
                } else if self.reviewed[index] {
                    Tick::Reviewed
                } else {
                    Tick::Unreviewed
                }
            })
            .collect()
    }

    /// Which ticks to draw when only `width` fit: all of them if they do,
    /// otherwise a window kept around the cursor.
    pub fn window(&self, width: usize) -> Window {
        let len = self.edits.len();
        if len <= width {
            return Window {
                start: 0,
                end: len,
                hidden_before: 0,
                hidden_after: 0,
            };
        }
        let end = (self.state.cursor + width / 2 + 1).clamp(width, len);
        let start = end - width;
        Window {
            start,
            end,
            hidden_before: start,
            hidden_after: len - end,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn edit(name: &str) -> EditEvent {
        EditEvent::from_unified(name, "@@ -1,2 +1,3 @@\n a\n-b\n+B\n+c\n")
    }

    fn follow_with(n: usize) -> Follow {
        let mut follow = Follow::default();
        for i in 0..n {
            follow.push(edit(&format!("f{i}.rs")));
        }
        follow
    }

    #[test]
    fn a_unified_diff_reads_into_numbered_hunks() {
        let edit = EditEvent::from_unified(
            "src/main.rs",
            "diff --git a/src/main.rs b/src/main.rs\n--- a/src/main.rs\n+++ b/src/main.rs\n@@ -10,3 +10,4 @@ fn main()\n x\n-y\n+Y\n+z\n@@ -40 +41 @@\n-q\n\\ No newline at end of file\n",
        );
        assert_eq!(edit.path, PathBuf::from("src/main.rs"));
        assert_eq!(edit.hunks.len(), 2);
        assert_eq!((edit.hunks[0].old_start, edit.hunks[0].new_start), (10, 10));
        assert_eq!(edit.hunks[0].lines[1].kind, LineKind::Removed);
        assert_eq!(edit.hunks[0].lines[1].text, "y");
        assert_eq!((edit.hunks[1].old_start, edit.hunks[1].new_start), (40, 41));
        assert_eq!((edit.added(), edit.removed()), (2, 2));
    }

    #[test]
    fn a_diff_without_hunk_headers_still_counts_as_an_edit() {
        let edit = EditEvent::from_unified("a.txt", "+one\n+two\n");
        assert_eq!(edit.hunks.len(), 1);
        assert_eq!(edit.hunks[0].new_start, 1);
        assert_eq!(edit.added(), 2);
    }

    #[test]
    fn an_empty_timeline_has_nothing_current_and_ignores_every_key() {
        let mut follow = Follow::default();
        assert!(follow.current().is_none());
        follow.scrub_back();
        follow.scrub_forward();
        follow.mark_reviewed();
        follow.jump_to_next_unreviewed();
        assert_eq!(follow.state(), FollowState::default());
        assert_eq!(follow.unreviewed_count(), 0);
    }

    #[test]
    fn a_live_pane_snaps_to_each_new_edit() {
        let mut follow = Follow::default();
        follow.push(edit("a"));
        follow.push(edit("b"));
        assert!(follow.is_live());
        assert_eq!(follow.current().unwrap().path, PathBuf::from("b"));
    }

    #[test]
    fn scrubbing_back_pauses_and_a_new_edit_leaves_a_paused_pane_alone() {
        let mut follow = follow_with(3);
        follow.scrub_back();
        assert_eq!((follow.is_live(), follow.cursor()), (false, 1));
        follow.push(edit("d"));
        assert_eq!(follow.cursor(), 1);
        assert_eq!(follow.len(), 4);
    }

    #[test]
    fn scrubbing_back_at_the_oldest_edit_stays_there_but_still_pauses() {
        let mut follow = follow_with(1);
        follow.scrub_back();
        assert_eq!((follow.is_live(), follow.cursor()), (false, 0));
    }

    #[test]
    fn scrubbing_forward_onto_the_newest_edit_goes_live() {
        let mut follow = follow_with(3);
        follow.scrub_back();
        follow.scrub_back();
        follow.scrub_forward();
        assert_eq!((follow.is_live(), follow.cursor()), (false, 1));
        follow.scrub_forward();
        assert_eq!((follow.is_live(), follow.cursor()), (true, 2));
    }

    #[test]
    fn f_goes_back_to_the_newest_edit() {
        let mut follow = follow_with(4);
        follow.jump_to(0);
        follow.go_live();
        assert_eq!((follow.is_live(), follow.cursor()), (true, 3));
    }

    #[test]
    fn reviewing_moves_to_the_oldest_unreviewed_edit_then_goes_live() {
        let mut follow = follow_with(3);
        assert_eq!(follow.unreviewed_count(), 3);
        follow.mark_reviewed();
        assert_eq!((follow.is_live(), follow.cursor()), (false, 0));
        follow.mark_reviewed();
        assert_eq!((follow.is_live(), follow.cursor()), (false, 1));
        follow.mark_reviewed();
        assert_eq!((follow.is_live(), follow.cursor()), (true, 2));
        assert_eq!(follow.unreviewed_count(), 0);
        assert_eq!(
            follow.ticks(),
            vec![Tick::Reviewed, Tick::Reviewed, Tick::Current]
        );
    }

    #[test]
    fn the_counter_jumps_to_the_next_unreviewed_edit_and_wraps() {
        let mut follow = follow_with(4);
        follow.jump_to(1);
        follow.mark_reviewed(); // reviews 1, lands on 0
        follow.mark_reviewed(); // reviews 0, lands on 2
        follow.jump_to(3);
        follow.jump_to_next_unreviewed();
        assert_eq!(follow.cursor(), 2);
        follow.jump_to_next_unreviewed();
        assert_eq!(follow.cursor(), 3);
        assert!(follow.is_live());
    }

    #[test]
    fn ticks_show_the_cursor_over_the_review_state() {
        let mut follow = follow_with(3);
        follow.mark_reviewed();
        follow.mark_reviewed();
        follow.jump_to(2);
        assert_eq!(
            follow.ticks(),
            vec![Tick::Reviewed, Tick::Unreviewed, Tick::Current]
        );
    }

    #[test]
    fn a_short_timeline_fits_the_strip_whole() {
        let follow = follow_with(5);
        assert_eq!(
            follow.window(12),
            Window {
                start: 0,
                end: 5,
                hidden_before: 0,
                hidden_after: 0
            }
        );
    }

    #[test]
    fn a_long_timeline_is_windowed_around_the_cursor() {
        let mut follow = follow_with(30);
        assert_eq!(follow.window(12).end, 30);
        assert_eq!(follow.window(12).hidden_before, 18);
        follow.jump_to(15);
        let window = follow.window(12);
        assert_eq!(window.end - window.start, 12);
        assert!(window.start <= 15 && 15 < window.end);
        assert_eq!(window.hidden_before + window.hidden_after, 18);
        follow.jump_to(0);
        assert_eq!(follow.window(12).start, 0);
    }
}

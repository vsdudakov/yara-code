//! A real shell on a pseudo-terminal, shared by both frontends.
//!
//! The frontends differ only in how they paint the resulting screen: the window
//! draws the grid with egui, the terminal frontend with ratatui.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use portable_pty::{native_pty_system, Child, CommandBuilder, MasterPty, PtySize};

const SCROLLBACK: usize = 5000;

pub struct Pty {
    parser: Arc<Mutex<vt100::Parser>>,
    writer: Box<dyn Write + Send>,
    master: Box<dyn MasterPty + Send>,
    child: Box<dyn Child + Send + Sync>,
    size: (u16, u16), // (rows, cols)
    /// Where the shell was started; the first half of the tab title.
    cwd: PathBuf,
    /// The last title worked out, and when — asking the system what is in
    /// front costs a process, so it is done once a second, not once a frame.
    title: Mutex<Option<(Instant, String)>>,
    /// The run of cells the mouse has dragged over, if any.
    selection: Option<Selection>,
}

/// One cell of the grid, counted from the top of the live screen: row 0 is the
/// shell's first line and the scrollback above it is negative. Counting this
/// way keeps a selection on the text it was made over while the panel scrolls.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct GridPoint {
    pub row: isize,
    pub col: u16,
}

/// What the mouse dragged over: the cell the button went down on and the one
/// the pointer is on now, either of which may be the earlier.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Selection {
    pub anchor: GridPoint,
    pub cursor: GridPoint,
}

impl Selection {
    /// The two ends in reading order.
    pub fn ordered(&self) -> (GridPoint, GridPoint) {
        if (self.anchor.row, self.anchor.col) <= (self.cursor.row, self.cursor.col) {
            (self.anchor, self.cursor)
        } else {
            (self.cursor, self.anchor)
        }
    }

    /// A press that never left its cell selects nothing, so an ordinary click
    /// in the terminal stays an ordinary click.
    pub fn is_empty(&self) -> bool {
        self.anchor == self.cursor
    }

    /// The columns covered on one row, as a half-open range. Rows in the
    /// middle of the run are covered to the end, which is what a terminal
    /// shows for a line that wrapped.
    pub fn span_on(&self, row: isize, width: u16) -> Option<(u16, u16)> {
        if self.is_empty() {
            return None;
        }
        let (start, end) = self.ordered();
        if row < start.row || row > end.row {
            return None;
        }
        let from = if row == start.row { start.col } else { 0 };
        // The cell under the pointer is part of the selection, so the end is
        // one past it.
        let to = if row == end.row {
            end.col.saturating_add(1)
        } else {
            width
        };
        let to = to.min(width);
        (from < to).then_some((from, to))
    }
}

/// How long a worked-out title is trusted before asking again.
const TITLE_REFRESH: Duration = Duration::from_secs(1);

/// The user's shell, and the arguments that make it a login shell. Windows has
/// neither `$SHELL` nor `/bin/sh`, so it gets what it does have.
fn login_shell() -> (String, Vec<String>) {
    if cfg!(windows) {
        let shell = std::env::var("COMSPEC").unwrap_or_else(|_| "cmd.exe".to_string());
        return (shell, Vec::new());
    }
    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string());
    (shell, vec!["-l".to_string()])
}

impl Pty {
    /// Spawns the user's login shell in `cwd`. `notify` is called from the
    /// reader thread whenever new output arrives, so the frontend can redraw.
    pub fn new<F>(cwd: &Path, notify: F) -> Result<Self, String>
    where
        F: Fn() + Send + 'static,
    {
        let (rows, cols) = (24u16, 80u16);
        let pair = native_pty_system()
            .openpty(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|e| e.to_string())?;

        let (shell, args) = login_shell();
        let mut cmd = CommandBuilder::new(&shell);
        cmd.args(&args);
        cmd.env("TERM", "xterm-256color");
        cmd.cwd(cwd);
        let child = pair.slave.spawn_command(cmd).map_err(|e| e.to_string())?;
        drop(pair.slave);

        let mut reader = pair.master.try_clone_reader().map_err(|e| e.to_string())?;
        let writer = pair.master.take_writer().map_err(|e| e.to_string())?;

        let parser = Arc::new(Mutex::new(vt100::Parser::new(rows, cols, SCROLLBACK)));
        let sink = Arc::clone(&parser);
        std::thread::spawn(move || {
            let mut buf = [0u8; 8192];
            loop {
                match reader.read(&mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        sink.lock().unwrap().process(&buf[..n]);
                        notify();
                    }
                }
            }
            notify();
        });

        Ok(Self {
            parser,
            writer,
            master: pair.master,
            child,
            size: (rows, cols),
            cwd: cwd.to_path_buf(),
            title: Mutex::new(None),
            selection: None,
        })
    }

    pub fn size(&self) -> (u16, u16) {
        self.size
    }

    /// What the session is doing, as a tab title: the folder it runs in and
    /// the program in front — `yara-code — zsh` at the prompt, `yara-code —
    /// claude` once an agent is running. Refreshed at most once a second.
    pub fn title(&self) -> String {
        let mut cached = self.title.lock().unwrap();
        if let Some((at, title)) = cached.as_ref() {
            if at.elapsed() < TITLE_REFRESH {
                return title.clone();
            }
        }
        let folder = self
            .cwd
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| self.cwd.display().to_string());
        let title = match self.foreground() {
            Some(program) => format!("{folder} — {program}"),
            None => folder,
        };
        *cached = Some((Instant::now(), title.clone()));
        title
    }

    /// The program in the foreground of the terminal, by name. On Unix that
    /// is the leader of the pty's foreground process group, asked of `ps`;
    /// elsewhere the title the program set for itself, or nothing.
    fn foreground(&self) -> Option<String> {
        #[cfg(unix)]
        {
            let pid = self.master.process_group_leader()?;
            let out = std::process::Command::new("ps")
                .args(["-o", "comm=", "-p", &pid.to_string()])
                .output()
                .ok()?;
            let name = program_name(&String::from_utf8_lossy(&out.stdout));
            if !name.is_empty() {
                return Some(name);
            }
        }
        let title = self.with_screen(|screen| screen.title().to_string());
        (!title.trim().is_empty()).then(|| title.trim().to_string())
    }

    /// Matches the shell's grid to the visible area. A no-op when unchanged.
    pub fn resize(&mut self, rows: u16, cols: u16) {
        if (rows, cols) == self.size || rows == 0 || cols == 0 {
            return;
        }
        self.size = (rows, cols);
        self.parser.lock().unwrap().set_size(rows, cols);
        let _ = self.master.resize(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        });
    }

    pub fn write(&mut self, bytes: &[u8]) {
        if bytes.is_empty() {
            return;
        }
        let _ = self.writer.write_all(bytes);
        let _ = self.writer.flush();
    }

    /// Runs `f` over the current screen. Scrollback is applied first, so what
    /// the callback sees is exactly what should be drawn.
    pub fn with_screen<R>(&self, f: impl FnOnce(&vt100::Screen) -> R) -> R {
        let parser = self.parser.lock().unwrap();
        f(parser.screen())
    }

    /// Scrolls the view back through history; 0 is the live screen.
    pub fn set_scrollback(&mut self, lines: usize) {
        self.parser.lock().unwrap().set_scrollback(lines);
    }

    pub fn scrollback(&self) -> usize {
        self.parser.lock().unwrap().screen().scrollback()
    }

    // ----- selection -----------------------------------------------------

    /// A point on the visible grid, as a point in the shell's own text — see
    /// [`GridPoint`].
    fn point_at(&self, view_row: u16, col: u16) -> GridPoint {
        GridPoint {
            row: view_row as isize - self.scrollback() as isize,
            col,
        }
    }

    /// Starts a selection where the mouse went down.
    pub fn begin_selection(&mut self, view_row: u16, col: u16) {
        let at = self.point_at(view_row, col);
        self.selection = Some(Selection {
            anchor: at,
            cursor: at,
        });
    }

    /// Drags the open end of the selection to where the pointer is now.
    pub fn extend_selection(&mut self, view_row: u16, col: u16) {
        let at = self.point_at(view_row, col);
        if let Some(selection) = &mut self.selection {
            selection.cursor = at;
        }
    }

    pub fn clear_selection(&mut self) {
        self.selection = None;
    }

    /// The selection, once it covers more than the cell it started on.
    pub fn selection(&self) -> Option<Selection> {
        self.selection.filter(|s| !s.is_empty())
    }

    /// The selected text, read off the grid a screenful at a time. A selection
    /// taller than the panel — dragged, then scrolled — is reached by moving
    /// the view to it and back, which the caller never sees.
    pub fn selected_text(&mut self) -> Option<String> {
        let (start, end) = self.selection()?.ordered();
        let (rows, cols) = self.size;
        let restore = self.scrollback();
        let mut text = String::new();
        let mut row = start.row;
        while row <= end.row {
            // Scrolled back far enough to put `row` on the top line, or not at
            // all when it is on the live screen already.
            let back = (-row).max(0) as usize;
            self.set_scrollback(back);
            if self.scrollback() != back {
                // History that far back has already been dropped.
                break;
            }
            let top = (row + back as isize) as u16;
            let last = (end.row + back as isize).min(rows as isize - 1) as u16;
            let from = if row == start.row { start.col } else { 0 };
            let to = if last as isize - back as isize == end.row {
                end.col.saturating_add(1).min(cols)
            } else {
                cols
            };
            text.push_str(&self.with_screen(|screen| screen.contents_between(top, from, last, to)));
            row = last as isize - back as isize + 1;
            if row <= end.row {
                text.push('\n');
            }
        }
        self.set_scrollback(restore);
        (!text.is_empty()).then_some(text)
    }

    /// Sends pasted text the way a terminal emulator does — see
    /// [`paste_bytes`]. Any selection goes, as it does when a key is pressed.
    pub fn paste(&mut self, text: &str) {
        let bracketed = self.with_screen(|screen| screen.bracketed_paste());
        let bytes = paste_bytes(text, bracketed);
        self.clear_selection();
        self.set_scrollback(0);
        self.write(&bytes);
    }
}

/// The bytes a paste sends. Line endings become carriage returns, the way a
/// terminal turns a pasted line ending into the Return key, and the text is
/// wrapped in the paste brackets when the program in front asked for them —
/// without those, a shell or an agent runs every pasted line the moment it
/// arrives, which is the whole reason bracketed paste exists. An escape inside
/// the text would end the bracket early or be read as a key sequence, so it is
/// dropped.
pub fn paste_bytes(text: &str, bracketed: bool) -> Vec<u8> {
    let text = text.replace("\r\n", "\r").replace('\n', "\r");
    let mut bytes = Vec::with_capacity(text.len() + 12);
    if bracketed {
        bytes.extend_from_slice(b"\x1b[200~");
    }
    bytes.extend(text.bytes().filter(|b| *b != 0x1b));
    if bracketed {
        bytes.extend_from_slice(b"\x1b[201~");
    }
    bytes
}

/// `ps` reports a login shell as `-zsh` and some programs by their full
/// path; the tab wants only the name.
fn program_name(comm: &str) -> String {
    let comm = comm.trim().trim_start_matches('-');
    comm.rsplit(['/', '\\']).next().unwrap_or(comm).to_string()
}

impl Drop for Pty {
    fn drop(&mut self) {
        let _ = self.child.kill();
    }
}

/// Several shells behind one panel, with a tab strip on top — the frontends
/// draw the strip and render whichever session is active.
#[derive(Default)]
pub struct Terminals {
    list: Vec<Pty>,
    /// Per-session tab name; empty means "unnamed", drawn as its position.
    names: Vec<String>,
    active: usize,
    /// Why the last attempt to open a shell failed.
    pub error: Option<String>,
}

impl Terminals {
    /// What the tab strip shows: the name the user gave the session, or what
    /// the session is doing when it has none — see [`Pty::title`].
    pub fn name(&self, index: usize) -> String {
        match self.names.get(index) {
            Some(name) if !name.is_empty() => name.clone(),
            _ => match self.list.get(index) {
                Some(pty) => pty.title(),
                None => (index + 1).to_string(),
            },
        }
    }

    /// True when this session carries a name of its own.
    pub fn is_named(&self, index: usize) -> bool {
        self.names.get(index).is_some_and(|name| !name.is_empty())
    }

    /// Renames a session; an empty name puts it back to its position.
    pub fn rename(&mut self, index: usize, name: &str) {
        if let Some(slot) = self.names.get_mut(index) {
            *slot = name.trim().to_string();
        }
    }

    /// Moves a session to another position in the strip, keeping the active
    /// session active wherever it lands.
    pub fn reorder(&mut self, from: usize, to: usize) {
        if from >= self.list.len() || to >= self.list.len() || from == to {
            return;
        }
        let pty = self.list.remove(from);
        let name = self.names.remove(from);
        self.list.insert(to, pty);
        self.names.insert(to, name);
        self.active = crate::core::buffer::shift_index(self.active, from, to);
    }

    pub fn len(&self) -> usize {
        self.list.len()
    }

    pub fn is_empty(&self) -> bool {
        self.list.is_empty()
    }

    pub fn active_index(&self) -> usize {
        self.active
    }

    pub fn set_active(&mut self, index: usize) {
        if index < self.list.len() {
            self.active = index;
        }
    }

    pub fn active(&self) -> Option<&Pty> {
        self.list.get(self.active)
    }

    pub fn active_mut(&mut self) -> Option<&mut Pty> {
        self.list.get_mut(self.active)
    }

    /// Starts another shell and switches to it.
    pub fn open<F>(&mut self, cwd: &Path, notify: F)
    where
        F: Fn() + Send + 'static,
    {
        match Pty::new(cwd, notify) {
            Ok(pty) => {
                self.list.push(pty);
                self.names.push(String::new());
                self.active = self.list.len() - 1;
                self.error = None;
            }
            Err(message) => self.error = Some(message),
        }
    }

    /// Opens the first shell the moment the panel is actually shown, so an
    /// unused terminal costs nothing.
    pub fn ensure<F>(&mut self, cwd: &Path, notify: F) -> Option<&mut Pty>
    where
        F: Fn() + Send + 'static,
    {
        if self.list.is_empty() && self.error.is_none() {
            self.open(cwd, notify);
        }
        self.list.get_mut(self.active)
    }

    /// Closes one session; the panel keeps working with whatever is left.
    pub fn close(&mut self, index: usize) {
        if index >= self.list.len() {
            return;
        }
        self.list.remove(index);
        self.names.remove(index);
        // Keep pointing at the same session when an earlier tab goes away.
        if index < self.active {
            self.active -= 1;
        }
        if self.active >= self.list.len() {
            self.active = self.list.len().saturating_sub(1);
        }
    }

    pub fn close_active(&mut self) {
        self.close(self.active);
    }

    /// Drops every session, e.g. when the project changes.
    pub fn clear(&mut self) {
        self.list.clear();
        self.names.clear();
        self.active = 0;
        self.error = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Two shells in a scratch directory. Spawning a real PTY is the point:
    /// the tab bookkeeping only means anything over live sessions.
    fn two_sessions() -> (crate::core::test_support::Dir, Terminals) {
        let dir = crate::core::test_support::Dir::new("yara-pty");
        let mut terminals = Terminals::default();
        terminals.open(dir.path(), || {});
        terminals.open(dir.path(), || {});
        (dir, terminals)
    }

    #[test]
    fn a_session_opens_and_is_the_one_in_front() {
        let dir = crate::core::test_support::Dir::new("yara-pty-open");
        let mut terminals = Terminals::default();
        assert!(terminals.is_empty());
        assert!(terminals.active().is_none());

        terminals.ensure(dir.path(), || {});
        assert_eq!(terminals.len(), 1);
        assert_eq!(terminals.active_index(), 0);
        assert!(terminals.error.is_none());
        // Ensuring again does not open a second one.
        terminals.ensure(dir.path(), || {});
        assert_eq!(terminals.len(), 1);
    }

    #[test]
    fn sessions_say_what_they_run_until_they_are_named() {
        let (dir, mut terminals) = two_sessions();
        let folder = dir
            .path()
            .file_name()
            .unwrap()
            .to_string_lossy()
            .into_owned();
        let title = terminals.name(0);
        assert!(title.starts_with(&folder), "{title}");
        assert_eq!(
            terminals.name(1),
            title,
            "two shells in one folder read alike"
        );
        assert!(!terminals.is_named(0));

        terminals.rename(0, "  build  ");
        assert_eq!(terminals.name(0), "build", "the name is trimmed");
        assert!(terminals.is_named(0));
        // An empty name puts it back to what the session is doing.
        terminals.rename(0, "   ");
        assert_eq!(terminals.name(0), title);
        assert!(!terminals.is_named(0));
        // A session that is not there is not renamed, and does not panic.
        terminals.rename(9, "ghost");
        assert_eq!(terminals.name(9), "10");
    }

    #[test]
    fn a_title_names_the_folder_and_the_program_in_front() {
        let dir = crate::core::test_support::Dir::new("yara-pty-title");
        let mut terminals = Terminals::default();
        terminals.open(dir.path(), || {});
        let pty = terminals.active().unwrap();
        let title = pty.title();
        assert!(title.starts_with("yara-pty-title"), "{title}");
        if cfg!(unix) {
            // Fresh from spawn the shell itself is in front, by its bare name.
            let (_, program) = title.split_once(" — ").expect(&title);
            assert!(
                !program.contains('/') && !program.starts_with('-'),
                "{title}"
            );
        }
        // Asked again at once, the cached answer comes back unchanged.
        assert_eq!(pty.title(), title);
    }

    fn point(row: isize, col: u16) -> GridPoint {
        GridPoint { row, col }
    }

    #[test]
    fn a_selection_covers_the_cells_the_mouse_dragged_over() {
        // Dragged from the middle of one row to the middle of a lower one.
        let selection = Selection {
            anchor: point(1, 4),
            cursor: point(3, 2),
        };
        assert!(!selection.is_empty());
        // The first row runs from the press to the end of the line, the rows
        // between are covered whole, and the last stops one past the pointer.
        assert_eq!(selection.span_on(1, 20), Some((4, 20)));
        assert_eq!(selection.span_on(2, 20), Some((0, 20)));
        assert_eq!(selection.span_on(3, 20), Some((0, 3)));
        // Rows outside the run carry nothing.
        assert_eq!(selection.span_on(0, 20), None);
        assert_eq!(selection.span_on(4, 20), None);

        // Dragged the other way, it reads the same.
        let backwards = Selection {
            anchor: point(3, 2),
            cursor: point(1, 4),
        };
        assert_eq!(backwards.ordered(), selection.ordered());
        assert_eq!(backwards.span_on(1, 20), Some((4, 20)));

        // A press that never moved selects nothing at all.
        let click = Selection {
            anchor: point(1, 4),
            cursor: point(1, 4),
        };
        assert!(click.is_empty());
        assert_eq!(click.span_on(1, 20), None);

        // The end never runs past the grid, however narrow it is.
        let wide = Selection {
            anchor: point(0, 0),
            cursor: point(0, 40),
        };
        assert_eq!(wide.span_on(0, 8), Some((0, 8)));

        // Scrollback counts as rows above the live screen.
        let scrolled = Selection {
            anchor: point(-2, 3),
            cursor: point(-1, 5),
        };
        assert_eq!(scrolled.span_on(-2, 10), Some((3, 10)));
        assert_eq!(scrolled.span_on(-1, 10), Some((0, 6)));
    }

    #[test]
    fn a_paste_arrives_as_returns_and_in_brackets_when_asked_for() {
        // Every line ending becomes the Return key, whichever kind it was.
        assert_eq!(paste_bytes("a\r\nb\nc", false), b"a\rb\rc".to_vec());
        // Bracketed, the text is wrapped so the program in front can tell a
        // paste from typing and not run each line as it lands.
        assert_eq!(
            paste_bytes("ls\n", true),
            b"\x1b[200~ls\r\x1b[201~".to_vec()
        );
        // An escape inside the text would close the bracket early or read as
        // a key sequence, so it never reaches the shell.
        assert_eq!(
            paste_bytes("a\x1b[201~b", true),
            b"\x1b[200~a[201~b\x1b[201~".to_vec()
        );
        assert_eq!(paste_bytes("", true), b"\x1b[200~\x1b[201~".to_vec());
        assert_eq!(paste_bytes("", false), Vec::<u8>::new());
    }

    #[test]
    fn dragging_over_the_shell_copies_what_it_covers() {
        let dir = crate::core::test_support::Dir::new("yara-pty-select");
        let mut terminals = Terminals::default();
        terminals.open(dir.path(), || {});
        let pty = terminals.active_mut().expect("a shell started");
        pty.resize(24, 80);
        // Carriage return, not newline: that is what Enter sends, on ConPTY
        // as much as on a Unix pty, where the line discipline turns it into
        // the newline the shell reads. A bare newline is not Enter to cmd.exe.
        pty.write(b"echo yara-selection-marker\r");
        let mut row = None;
        // A shell on a CI runner can take seconds to start; wait up to ten.
        for _ in 0..200 {
            std::thread::sleep(std::time::Duration::from_millis(50));
            // The echoed line, not the one that was typed at the prompt.
            row = pty.with_screen(|screen| {
                (0..24u16).rev().find(|r| {
                    let line = screen.contents_between(*r, 0, *r, 80);
                    line.trim() == "yara-selection-marker"
                })
            });
            if row.is_some() {
                break;
            }
        }
        let row = row.expect("the shell never echoed the line");

        assert!(pty.selection().is_none(), "nothing is selected yet");
        assert!(pty.selected_text().is_none());
        pty.begin_selection(row, 0);
        // A press that has not moved is still not a selection.
        assert!(pty.selection().is_none());
        pty.extend_selection(row, 20);
        assert_eq!(
            pty.selected_text().as_deref(),
            Some("yara-selection-marker")
        );
        // The highlight is on that row, from the press to the pointer.
        let selection = pty.selection().expect("a selection stands");
        assert_eq!(selection.span_on(row as isize, 80), Some((0, 21)));

        // Typing drops it, the way it does anywhere else.
        pty.clear_selection();
        assert!(pty.selection().is_none());
    }

    #[test]
    fn a_program_name_is_the_bare_command() {
        assert_eq!(program_name("-zsh\n"), "zsh");
        assert_eq!(program_name("/usr/local/bin/claude"), "claude");
        assert_eq!(program_name("C:\\Windows\\cmd.exe"), "cmd.exe");
        assert_eq!(program_name("  "), "");
    }

    #[test]
    fn dragging_a_tab_moves_the_session_and_its_name() {
        let (_dir, mut terminals) = two_sessions();
        terminals.rename(0, "first");
        terminals.set_active(0);
        terminals.reorder(0, 1);
        assert_eq!(terminals.name(1), "first");
        assert_eq!(terminals.active_index(), 1, "the dragged tab stays active");
        // Out of range, and onto itself: nothing happens either way.
        terminals.reorder(0, 9);
        terminals.reorder(1, 1);
        assert_eq!(terminals.name(1), "first");
    }

    #[test]
    fn closing_keeps_the_selection_on_a_live_session() {
        let (_dir, mut terminals) = two_sessions();
        terminals.set_active(1);
        terminals.close(0);
        assert_eq!(terminals.len(), 1);
        assert_eq!(terminals.active_index(), 0, "it followed the session left");
        terminals.close(9); // out of range
        assert_eq!(terminals.len(), 1);
        terminals.close_active();
        assert!(terminals.is_empty());
    }

    #[test]
    fn clearing_drops_every_session() {
        let (_dir, mut terminals) = two_sessions();
        terminals.clear();
        assert!(terminals.is_empty());
        assert_eq!(terminals.active_index(), 0);
        assert!(terminals.active_mut().is_none());
    }

    #[test]
    fn a_shell_echoes_what_is_written_to_it() {
        let dir = crate::core::test_support::Dir::new("yara-pty-echo");
        let mut terminals = Terminals::default();
        terminals.open(dir.path(), || {});
        let pty = terminals.active_mut().expect("a shell started");
        assert_eq!(pty.size(), (24, 80));
        pty.resize(30, 100);
        assert_eq!(pty.size(), (30, 100));
        // Resizing to the same size, or to nothing, is a no-op.
        pty.resize(30, 100);
        pty.resize(0, 0);
        assert_eq!(pty.size(), (30, 100));

        pty.write(b"echo yara-pty-marker\r");
        pty.write(b"");
        // The shell answers on its own schedule; give it a moment.
        let mut seen = false;
        for _ in 0..200 {
            std::thread::sleep(std::time::Duration::from_millis(50));
            seen = pty.with_screen(|screen| screen.contents().contains("yara-pty-marker"));
            if seen {
                break;
            }
        }
        assert!(seen, "the shell never echoed the line");
        assert_eq!(pty.scrollback(), 0, "the live screen is not scrolled back");
        pty.set_scrollback(5);
    }
}

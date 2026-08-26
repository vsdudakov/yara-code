//! The terminal panel: a real shell on a pseudo-terminal, so completion,
//! Ctrl-C, and full-screen programs behave exactly as they do outside Yara.

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::core::pty::{Pty, Selection, Terminals};

pub struct Shell {
    pub sessions: Terminals,
    /// Set by the reader thread when new output arrives.
    dirty: Arc<AtomicBool>,
}

impl Shell {
    pub fn new(dirty: Arc<AtomicBool>) -> Self {
        Self {
            sessions: Terminals::default(),
            dirty,
        }
    }

    fn notifier(&self) -> impl Fn() + Send + 'static {
        let dirty = Arc::clone(&self.dirty);
        move || dirty.store(true, Ordering::Relaxed)
    }

    /// Spawns the first shell on first use, so an unused panel costs nothing.
    pub fn ensure(&mut self, cwd: &Path) -> Option<&mut Pty> {
        let notify = self.notifier();
        self.sessions.ensure(cwd, notify)
    }

    /// Opens another session and switches to it.
    pub fn open(&mut self, cwd: &Path) {
        let notify = self.notifier();
        self.sessions.open(cwd, notify);
    }

    pub fn close_active(&mut self) {
        self.sessions.close_active();
    }

    pub fn error(&self) -> Option<&String> {
        self.sessions.error.as_ref()
    }

    pub fn pty(&self) -> Option<&Pty> {
        self.sessions.active()
    }

    /// Drops every session, so the next use starts one in the new directory.
    pub fn restart(&mut self) {
        self.sessions.clear();
    }

    // ----- selection -----------------------------------------------------

    /// Starts a selection at a cell of the grid, counted from its top-left.
    pub fn begin_selection(&mut self, row: u16, col: u16) {
        if let Some(pty) = self.sessions.active_mut() {
            pty.begin_selection(row, col);
        }
    }

    /// Drags the open end of the selection to another cell.
    pub fn extend_selection(&mut self, row: u16, col: u16) {
        if let Some(pty) = self.sessions.active_mut() {
            pty.extend_selection(row, col);
        }
    }

    pub fn clear_selection(&mut self) {
        if let Some(pty) = self.sessions.active_mut() {
            pty.clear_selection();
        }
    }

    pub fn selection(&self) -> Option<Selection> {
        self.sessions.active()?.selection()
    }

    pub fn has_selection(&self) -> bool {
        self.selection().is_some()
    }

    /// What the selection covers, for the clipboard.
    pub fn selected_text(&mut self) -> Option<String> {
        self.sessions.active_mut()?.selected_text()
    }

    pub fn scroll(&mut self, delta: isize) {
        let Some(pty) = self.sessions.active_mut() else {
            return;
        };
        let current = pty.scrollback() as isize;
        pty.set_scrollback((current + delta).max(0) as usize);
    }

    /// One notch of the wheel over a cell of the grid, `rows` being how far
    /// the panels themselves move for it. A program that asked for the mouse
    /// scrolls its own view and is handed the notch; anything else leaves the
    /// wheel to the panel, which walks the history instead.
    pub fn wheel(&mut self, row: u16, col: u16, up: bool, rows: usize) {
        let Some(pty) = self.sessions.active_mut() else {
            return;
        };
        if pty.wants_mouse() {
            // A pager's own notch is three lines, so it takes as many notches
            // as it needs to keep up with the panels.
            let notches = ((rows + 1) / 3).max(1);
            let bytes = pty.wheel_bytes(up, row, col).repeat(notches);
            pty.write(&bytes);
            return;
        }
        self.scroll(if up { rows as isize } else { -(rows as isize) });
    }

    /// Forwards a key press to the shell as the bytes a terminal would send.
    pub fn send_key(&mut self, key: KeyEvent) {
        let Some(pty) = self.sessions.active_mut() else {
            return;
        };
        let mut bytes: Vec<u8> = Vec::new();
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let alt = key.modifiers.contains(KeyModifiers::ALT);
        match key.code {
            KeyCode::Char(c) => {
                if ctrl {
                    // Control characters: Ctrl+A is 0x01, and so on.
                    let lower = c.to_ascii_lowercase();
                    if lower.is_ascii_lowercase() {
                        bytes.push(lower as u8 - b'a' + 1);
                    } else {
                        match c {
                            ' ' => bytes.push(0),
                            '[' => bytes.push(0x1b),
                            '\\' => bytes.push(0x1c),
                            ']' => bytes.push(0x1d),
                            _ => {}
                        }
                    }
                } else {
                    if alt {
                        bytes.push(0x1b);
                    }
                    let mut buf = [0u8; 4];
                    bytes.extend_from_slice(c.encode_utf8(&mut buf).as_bytes());
                }
            }
            // Shift+Enter is the newline an agent's prompt asks for, and
            // ESC then Return is what a terminal set up for one sends. Telling
            // it from a plain Return needs the kitty protocol, which
            // [`crate::tui::run`] asks the host terminal for.
            KeyCode::Enter if key.modifiers.contains(KeyModifiers::SHIFT) => {
                bytes.extend_from_slice(b"\x1b\r")
            }
            KeyCode::Enter => bytes.push(b'\r'),
            KeyCode::Tab => bytes.push(b'\t'),
            KeyCode::BackTab => bytes.extend_from_slice(b"\x1b[Z"),
            KeyCode::Backspace => bytes.push(0x7f),
            KeyCode::Esc => bytes.push(0x1b),
            KeyCode::Up => bytes.extend_from_slice(b"\x1b[A"),
            KeyCode::Down => bytes.extend_from_slice(b"\x1b[B"),
            KeyCode::Right => bytes.extend_from_slice(b"\x1b[C"),
            KeyCode::Left => bytes.extend_from_slice(b"\x1b[D"),
            KeyCode::Home => bytes.extend_from_slice(b"\x1b[H"),
            KeyCode::End => bytes.extend_from_slice(b"\x1b[F"),
            KeyCode::PageUp => bytes.extend_from_slice(b"\x1b[5~"),
            KeyCode::PageDown => bytes.extend_from_slice(b"\x1b[6~"),
            KeyCode::Delete => bytes.extend_from_slice(b"\x1b[3~"),
            KeyCode::Insert => bytes.extend_from_slice(b"\x1b[2~"),
            KeyCode::F(n) if (1..=4).contains(&n) => {
                bytes.extend_from_slice(&[0x1b, b'O', b'P' + (n - 1)]);
            }
            KeyCode::F(n) => {
                let code = match n {
                    5 => 15,
                    6 => 17,
                    7 => 18,
                    8 => 19,
                    9 => 20,
                    10 => 21,
                    11 => 23,
                    12 => 24,
                    _ => return,
                };
                bytes.extend_from_slice(format!("\x1b[{code}~").as_bytes());
            }
            _ => return,
        }
        // Any keypress returns the view to the live screen, as terminals do,
        // and drops a selection the way typing does in the editor.
        pty.set_scrollback(0);
        pty.clear_selection();
        pty.write(&bytes);
    }

    /// Pasted text, bracketed and with its line endings turned into returns —
    /// see [`crate::core::pty::paste_bytes`].
    pub fn paste(&mut self, text: &str) {
        if let Some(pty) = self.sessions.active_mut() {
            pty.paste(text);
        }
    }

    /// Hands the raw Ctrl+V byte to the program in front. That is the last
    /// resort when the editor found nothing to paste: an agent that reads the
    /// clipboard itself still gets its turn.
    pub fn send_paste_key(&mut self) {
        if let Some(pty) = self.sessions.active_mut() {
            pty.set_scrollback(0);
            pty.write(&[0x16]);
        }
    }
}

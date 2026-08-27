//! The terminal panel: a real shell on a pseudo-terminal, so completion,
//! Ctrl-C, and full-screen programs behave exactly as they do outside Yara.

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::core::command::{Key, Mods};
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

    /// Forwards a key press to the shell as the bytes a terminal would send,
    /// which is [`crate::core::keyboard`]'s job: what the program in front has
    /// asked to be told about a press decides how it is spelled.
    pub fn send_key(&mut self, key: KeyEvent) {
        let Some(pty) = self.sessions.active_mut() else {
            return;
        };
        let mods = Mods {
            cmd: key.modifiers.contains(KeyModifiers::SUPER),
            ctrl: key.modifiers.contains(KeyModifiers::CONTROL),
            alt: key.modifiers.contains(KeyModifiers::ALT),
            shift: key.modifiers.contains(KeyModifiers::SHIFT),
        };
        let named = |name: &str| Key::Named(name.to_string());
        let key = match key.code {
            KeyCode::Char(c) => Key::Char(c),
            KeyCode::Enter => named("enter"),
            KeyCode::Tab => named("tab"),
            KeyCode::BackTab => named("backtab"),
            KeyCode::Backspace => named("backspace"),
            KeyCode::Esc => named("esc"),
            KeyCode::Up => named("up"),
            KeyCode::Down => named("down"),
            KeyCode::Right => named("right"),
            KeyCode::Left => named("left"),
            KeyCode::Home => named("home"),
            KeyCode::End => named("end"),
            KeyCode::PageUp => named("pageup"),
            KeyCode::PageDown => named("pagedown"),
            KeyCode::Delete => named("delete"),
            KeyCode::Insert => named("insert"),
            KeyCode::F(n) => named(&format!("f{n}")),
            _ => return,
        };
        pty.send_key(&key, mods);
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

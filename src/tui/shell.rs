//! The terminal panel: a real shell on a pseudo-terminal, so completion,
//! Ctrl-C, and full-screen programs behave exactly as they do outside Yara.

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::core::pty::{Pty, Terminals};

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

    pub fn scroll(&mut self, delta: isize) {
        let Some(pty) = self.sessions.active_mut() else {
            return;
        };
        let current = pty.scrollback() as isize;
        pty.set_scrollback((current + delta).max(0) as usize);
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
        // Any keypress returns the view to the live screen, as terminals do.
        pty.set_scrollback(0);
        pty.write(&bytes);
    }

    pub fn paste(&mut self, text: &str) {
        if let Some(pty) = self.sessions.active_mut() {
            pty.write(text.as_bytes());
        }
    }
}

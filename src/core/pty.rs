//! A real shell on a pseudo-terminal, shared by both frontends.
//!
//! The frontends differ only in how they paint the resulting screen: the window
//! draws the grid with egui, the terminal frontend with ratatui.

use std::io::{Read, Write};
use std::path::Path;
use std::sync::{Arc, Mutex};

use portable_pty::{native_pty_system, Child, CommandBuilder, MasterPty, PtySize};

const SCROLLBACK: usize = 5000;

pub struct Pty {
    parser: Arc<Mutex<vt100::Parser>>,
    writer: Box<dyn Write + Send>,
    master: Box<dyn MasterPty + Send>,
    child: Box<dyn Child + Send + Sync>,
    size: (u16, u16), // (rows, cols)
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

        let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string());
        let mut cmd = CommandBuilder::new(&shell);
        cmd.args(["-l"]);
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
        })
    }

    pub fn size(&self) -> (u16, u16) {
        self.size
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
    /// What the tab strip shows: the name the user gave the session, or its
    /// position when it has none.
    pub fn name(&self, index: usize) -> String {
        match self.names.get(index) {
            Some(name) if !name.is_empty() => name.clone(),
            _ => (index + 1).to_string(),
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

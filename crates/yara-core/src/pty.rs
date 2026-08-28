//! The agent on a pseudo-terminal: the program `settings.agent` names, run
//! in the project folder, its output parsed into a screen the frontend
//! paints and its keys spelled the way it asked for them.

use std::io::{Read, Write};
use std::path::Path;
use std::sync::{Arc, Mutex};

use portable_pty::{native_pty_system, Child, CommandBuilder, MasterPty, PtySize};

use crate::command::{Key, Mods};
use crate::keyboard;

const SCROLLBACK: usize = 5000;

pub struct Pty {
    parser: Arc<Mutex<vt100::Parser>>,
    /// Shared with the reader thread, which answers the keyboard protocol's
    /// questions as they arrive: the program asking is waiting on the reply
    /// before it decides what its keys are.
    writer: Arc<Mutex<Box<dyn Write + Send>>>,
    keyboard: Arc<Mutex<keyboard::Protocol>>,
    master: Box<dyn MasterPty + Send>,
    child: Box<dyn Child + Send + Sync>,
    size: (u16, u16),
}

impl Pty {
    /// Runs `command` — a program and its arguments, split on spaces — in
    /// `cwd`. `notify` is called from the reader thread whenever output
    /// arrives, and once more when the program ends.
    pub fn spawn<F>(command: &str, cwd: &Path, notify: F) -> Result<Self, String>
    where
        F: Fn() + Send + 'static,
    {
        let mut words = command.split_whitespace();
        let program = words.next().ok_or("no agent command in settings.json")?;
        let (rows, cols) = (24u16, 80u16);
        let pair = native_pty_system()
            .openpty(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|e| e.to_string())?;
        let mut cmd = CommandBuilder::new(program);
        cmd.args(words);
        cmd.env("TERM", "xterm-256color");
        cmd.cwd(cwd);
        let child = pair
            .slave
            .spawn_command(cmd)
            .map_err(|e| format!("{program}: {e}"))?;
        drop(pair.slave);

        let mut reader = pair.master.try_clone_reader().map_err(|e| e.to_string())?;
        let writer = Arc::new(Mutex::new(
            pair.master.take_writer().map_err(|e| e.to_string())?,
        ));
        let parser = Arc::new(Mutex::new(vt100::Parser::new(rows, cols, SCROLLBACK)));
        let keyboard = Arc::new(Mutex::new(keyboard::Protocol::default()));
        let (sink, asked, back) = (parser.clone(), keyboard.clone(), writer.clone());
        std::thread::spawn(move || {
            let mut buf = [0u8; 8192];
            while let Ok(n @ 1..) = reader.read(&mut buf) {
                let reply = asked.lock().unwrap().feed(&buf[..n]);
                if !reply.is_empty() {
                    let mut back = back.lock().unwrap();
                    let _ = back.write_all(&reply).and_then(|_| back.flush());
                }
                sink.lock().unwrap().process(&buf[..n]);
                notify();
            }
            notify();
        });

        Ok(Self {
            parser,
            writer,
            keyboard,
            master: pair.master,
            child,
            size: (rows, cols),
        })
    }

    /// Whether the program is still there to type at.
    pub fn is_running(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(None))
    }

    /// Matches the grid to the visible area. A no-op when unchanged.
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
        let mut writer = self.writer.lock().unwrap();
        let _ = writer.write_all(bytes).and_then(|_| writer.flush());
    }

    /// Sends a key press as the program has asked to be told about it — see
    /// [`crate::keyboard`]. Any key returns the view to the live screen.
    pub fn send_key(&mut self, key: &Key, mods: Mods) {
        let flags = self.keyboard.lock().unwrap().flags();
        let bytes = keyboard::bytes(key, mods, flags);
        if !bytes.is_empty() {
            self.set_scrollback(0);
            self.write(&bytes);
        }
    }

    /// Runs `f` over the screen as it should be drawn, scrollback applied.
    pub fn with_screen<R>(&self, f: impl FnOnce(&vt100::Screen) -> R) -> R {
        f(self.parser.lock().unwrap().screen())
    }

    /// Scrolls the view back through history; 0 is the live screen.
    pub fn set_scrollback(&mut self, lines: usize) {
        self.parser.lock().unwrap().set_scrollback(lines);
    }

    pub fn scrollback(&self) -> usize {
        self.with_screen(|screen| screen.scrollback())
    }
}

impl Drop for Pty {
    fn drop(&mut self) {
        let _ = self.child.kill();
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::test_support::Dir;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::{Duration, Instant};

    fn wait_for(pty: &Pty, text: &str) -> String {
        let start = Instant::now();
        loop {
            let screen = pty.with_screen(|s| s.contents());
            if screen.contains(text) || start.elapsed() > Duration::from_secs(5) {
                return screen;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    #[test]
    fn the_agent_command_runs_in_the_project_and_its_output_reaches_the_screen() {
        let dir = Dir::new("yara-pty");
        let notified = Arc::new(AtomicUsize::new(0));
        let count = notified.clone();
        let mut pty = Pty::spawn("sh -c pwd", dir.path(), move || {
            count.fetch_add(1, Ordering::Relaxed);
        })
        .unwrap();
        let folder = dir
            .path()
            .file_name()
            .unwrap()
            .to_string_lossy()
            .into_owned();
        assert!(wait_for(&pty, &folder).contains(&folder));
        wait_for_exit(&mut pty);
        assert!(
            notified.load(Ordering::Relaxed) >= 2,
            "output, then the end"
        );
    }

    fn wait_for_exit(pty: &mut Pty) {
        let start = Instant::now();
        while pty.is_running() && start.elapsed() < Duration::from_secs(5) {
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(!pty.is_running());
    }

    #[test]
    fn keys_are_typed_at_the_program_and_the_grid_follows_the_pane() {
        let dir = Dir::new("yara-pty-keys");
        let mut pty = Pty::spawn("cat", dir.path(), || {}).unwrap();
        assert!(pty.is_running());
        pty.resize(10, 40);
        pty.resize(10, 40);
        assert_eq!(pty.with_screen(|s| s.size()), (10, 40));
        for c in "hi".chars() {
            pty.send_key(&Key::Char(c), Mods::default());
        }
        pty.send_key(&Key::Named("enter".into()), Mods::default());
        assert!(wait_for(&pty, "hi\nhi").contains("hi\nhi"), "echoed by cat");
        assert_eq!(pty.scrollback(), 0);
        // Ctrl+D ends cat.
        pty.send_key(
            &Key::Char('d'),
            Mods {
                ctrl: true,
                ..Mods::default()
            },
        );
        wait_for_exit(&mut pty);
    }

    #[test]
    fn a_missing_program_and_an_empty_command_are_errors_not_panics() {
        let dir = Dir::new("yara-pty-missing");
        assert!(Pty::spawn("", dir.path(), || {}).is_err());
        let mut nothing = Pty::spawn("/no/such/program", dir.path(), || {});
        // Some systems report the failure at spawn, others at first read.
        if let Ok(pty) = nothing.as_mut() {
            wait_for_exit(pty);
        }
    }
}

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

        pty.write(b"echo yara-pty-marker\n");
        pty.write(b"");
        // The shell answers on its own schedule; give it a moment.
        let mut seen = false;
        for _ in 0..40 {
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

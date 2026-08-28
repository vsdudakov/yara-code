//! Everything the frame shows, and what a key does to it. A tab is a
//! session — one agent in one worktree, with its own timeline — and the
//! app is the tabs plus what is shared: settings, theme, the keyboard.

use std::ops::{Deref, DerefMut};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use crossterm::event::{KeyCode, KeyEvent};
use yara_core::buffer::Buffer;
use yara_core::command::{Chord, Command, Key};
use yara_core::follow::{EditEvent, Follow};
use yara_core::fuzzy;
use yara_core::git::{self, Change, Repo, Watcher};
use yara_core::pty::Pty;
use yara_core::settings::Settings;
use yara_core::syntax::Syntax;
use yara_core::theme::{self, Theme};
use yara_core::tree::{self, Tree};

use crate::keys::chord_of;

/// Where the keyboard is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Focus {
    Agent,
    Follow,
    Files,
    /// A file open in place of the follow pane; only Save, Close, Undo and
    /// Redo are the editor's from here, every other key is typing.
    Editor,
}

/// What the FOLLOW pane's body shows of the current edit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum View {
    Diff,
    File,
}

/// A box over the panes, closed with Esc.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Overlay {
    /// The CHANGES list, with the row the cursor is on.
    Changes(usize),
    /// A name being typed for a new tab's worktree.
    NewTab(String),
    /// A name being typed for the current tab.
    RenameTab(String),
    /// Go to file: what was typed, and the row the cursor is on.
    QuickOpen(String, usize),
}

/// One agent in one worktree.
pub struct Session {
    pub project: Option<PathBuf>,
    pub repo: Option<Repo>,
    pub agent: Option<Pty>,
    watcher: Watcher,
    /// What differs from the base branch, refreshed with the watcher.
    pub changes: Vec<Change>,
    pub follow: Follow,
    /// A file's whole diff opened from CHANGES, shown in place of the
    /// timeline's current edit until the follow keys are used again.
    pub pinned: Option<EditEvent>,
    pub view: View,
    /// The project's files, for the sidebar.
    pub tree: Option<Tree>,
    /// A file open for editing, in the follow pane's place.
    pub editor: Option<Buffer>,
    /// A name the user gave the tab; otherwise it is named by its work.
    pub name: Option<String>,
    /// The pull request the branch is on, asked of `gh` in the background.
    pr: Arc<Mutex<Option<String>>>,
}

impl Session {
    fn new(project: Option<PathBuf>, settings: &Settings) -> Self {
        let repo = project
            .as_deref()
            .and_then(|dir| git::open(dir, &settings.base_branch));
        let pr = Arc::new(Mutex::new(None));
        if let Some(root) = repo.as_ref().map(|r| r.root.clone()) {
            let slot = pr.clone();
            std::thread::spawn(move || {
                if let Some(title) = git::pull_request(&root) {
                    *slot.lock().unwrap() = Some(title);
                }
            });
        }
        Self {
            tree: project.clone().map(Tree::new),
            project,
            repo,
            agent: None,
            watcher: Watcher::default(),
            changes: Vec::new(),
            follow: Follow::default(),
            pinned: None,
            view: View::Diff,
            editor: None,
            name: None,
            pr,
        }
    }

    /// What the tab is called: the name it was given, else its pull
    /// request, else its branch, else its folder.
    pub fn title(&self) -> String {
        if let Some(name) = &self.name {
            return name.clone();
        }
        if let Some(pr) = self.pr.lock().unwrap().as_ref() {
            return pr.clone();
        }
        if let Some(repo) = &self.repo {
            return repo.branch.clone();
        }
        self.project
            .as_ref()
            .and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
            .unwrap_or_else(|| "no project".into())
    }

    /// The edit the FOLLOW pane shows: a pinned file, else the timeline's.
    pub fn shown(&self) -> Option<&EditEvent> {
        self.pinned.as_ref().or_else(|| self.follow.current())
    }

    /// Whether the agent is still there to type at.
    pub fn agent_running(&mut self) -> bool {
        self.agent.as_mut().is_some_and(Pty::is_running)
    }

    pub fn record_edit(&mut self, edit: EditEvent) {
        self.follow.push(edit);
    }
}

pub struct App {
    pub settings: Settings,
    pub theme: Theme,
    pub syntax: Syntax,
    pub sessions: Vec<Session>,
    pub active: usize,
    /// Set by an agent's reader thread when there is something new to draw.
    pub dirty: Arc<AtomicBool>,
    pub focus: Focus,
    pub show_sidebar: bool,
    pub overlay: Option<Overlay>,
    /// What the status bar says about the last thing that happened.
    pub note: Option<String>,
    pub should_quit: bool,
}

/// The active session is what nearly everything is about; the app reads as
/// it, so `app.follow` is the timeline in front.
impl Deref for App {
    type Target = Session;
    fn deref(&self) -> &Session {
        &self.sessions[self.active]
    }
}

impl DerefMut for App {
    fn deref_mut(&mut self) -> &mut Session {
        &mut self.sessions[self.active]
    }
}

impl App {
    /// An app on the built-in defaults; `load` reads the user's files.
    pub fn new(project: Option<PathBuf>) -> Self {
        Self::with_settings(project, Settings::default(), Theme::default())
    }

    /// The user's settings and themes, with a complaint about the settings
    /// file — if there is one — shown in the status bar.
    pub fn load(project: Option<PathBuf>) -> Self {
        let (settings, complaint) = Settings::load();
        let theme = theme::by_name(&theme::load_all(), &settings.theme)
            .cloned()
            .unwrap_or_default();
        let mut app = Self::with_settings(project, settings, theme);
        app.note = complaint;
        app
    }

    pub fn with_settings(project: Option<PathBuf>, settings: Settings, theme: Theme) -> Self {
        Self {
            sessions: vec![Session::new(project, &settings)],
            active: 0,
            show_sidebar: settings.show_sidebar,
            settings,
            syntax: Syntax::new(&theme),
            theme,
            dirty: Arc::new(AtomicBool::new(true)),
            focus: Focus::Agent,
            overlay: None,
            note: None,
            should_quit: false,
        }
    }

    /// Runs the agent from the settings in the session's folder — or where
    /// the editor was started, with no project. A failure is a note.
    pub fn start_agent(&mut self) {
        let cwd = self
            .project
            .clone()
            .or_else(|| std::env::current_dir().ok())
            .unwrap_or_else(|| PathBuf::from("."));
        let dirty = self.dirty.clone();
        match Pty::spawn(&self.settings.agent, &cwd, move || {
            dirty.store(true, Ordering::Relaxed)
        }) {
            Ok(pty) => self.agent = Some(pty),
            Err(e) => self.note = Some(e),
        }
    }

    /// Takes the dirty flag, so a frame is drawn once per change.
    pub fn take_dirty(&self) -> bool {
        self.dirty.swap(false, Ordering::Relaxed)
    }

    /// Looks at every session's working tree: new edits join its timeline,
    /// its CHANGES list is brought up to date. Called every `refresh_ms`.
    pub fn refresh(&mut self) {
        for session in &mut self.sessions {
            let Some(repo) = &session.repo else { continue };
            for edit in session.watcher.poll(repo) {
                session.follow.push(edit);
            }
            session.changes = git::changes(repo).unwrap_or_default();
        }
        self.dirty.store(true, Ordering::Relaxed);
    }

    /// The agent as the chrome names it: the program, without its arguments.
    pub fn agent_name(&self) -> &str {
        self.settings
            .agent
            .split_whitespace()
            .next()
            .unwrap_or("agent")
    }

    /// The chord a hint shows for a command, as glyphs; nothing if unbound.
    pub fn hint(&self, command: Command) -> String {
        self.settings
            .chord(command)
            .map(|c| c.glyphs())
            .unwrap_or_default()
    }

    /// Opens the diff of the CHANGES row under the cursor in the FOLLOW pane.
    fn open_change(&mut self, row: usize) {
        let Some((repo, change)) = self.repo.as_ref().zip(self.changes.get(row)) else {
            return;
        };
        match git::file_diff(repo, &change.path) {
            Ok(diff) => self.pinned = Some(EditEvent::from_unified(&change.path, &diff)),
            Err(e) => self.note = Some(e),
        }
        self.overlay = None;
    }

    /// Opens a file in the follow pane's place, the keyboard on it.
    pub fn open_file(&mut self, path: &std::path::Path) {
        match Buffer::open(path) {
            Ok(buffer) => {
                self.editor = Some(buffer);
                self.focus = Focus::Editor;
                if let Some(tree) = self.tree.as_mut() {
                    tree.reveal(path);
                }
            }
            Err(e) => self.note = Some(format!("{}: {e}", path.display())),
        }
    }

    fn save(&mut self) {
        let Some(buffer) = self.editor.as_mut() else {
            return;
        };
        self.note = Some(match buffer.save() {
            Ok(()) => format!("✓ saved {}", buffer.path.display()),
            Err(e) => format!("{}: {e}", buffer.path.display()),
        });
    }

    /// The files the finder offers for what was typed, best first.
    pub fn quick_open_hits(&self, query: &str) -> Vec<String> {
        let Some(root) = &self.project else {
            return Vec::new();
        };
        let files = tree::all_files(root);
        fuzzy::rank(query, files.iter().map(String::as_str))
            .into_iter()
            .take(50)
            .map(|(_, s)| s.to_string())
            .collect()
    }

    /// A new tab: a worktree of the current repository on a branch of that
    /// name, with an agent started in it.
    fn new_tab(&mut self, name: &str) {
        let Some(repo) = self.repo.clone() else {
            self.note = Some("a new tab needs a repository to branch from".into());
            return;
        };
        let dir = match self.settings.worktrees_dir.as_str() {
            "" => {
                let folder = repo
                    .root
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned());
                repo.root
                    .parent()
                    .unwrap_or(&repo.root)
                    .join(format!("{}-worktrees", folder.unwrap_or_default()))
            }
            dir => PathBuf::from(dir),
        };
        match git::worktree_add(&repo, &dir, name) {
            Ok(path) => {
                self.sessions.push(Session::new(Some(path), &self.settings));
                self.active = self.sessions.len() - 1;
                self.start_agent();
                self.refresh();
            }
            Err(e) => self.note = Some(e),
        }
    }

    pub fn handle_key(&mut self, key: KeyEvent) {
        self.note = None;
        let Some(chord) = chord_of(key) else { return };
        let closes = self.settings.command(&chord) == Some(Command::Close);
        match self.overlay.clone() {
            Some(Overlay::Changes(row)) => {
                match key.code {
                    KeyCode::Up => self.overlay = Some(Overlay::Changes(row.saturating_sub(1))),
                    KeyCode::Down => {
                        let last = self.changes.len().saturating_sub(1);
                        self.overlay = Some(Overlay::Changes((row + 1).min(last)));
                    }
                    KeyCode::Enter => self.open_change(row),
                    _ if closes => self.overlay = None,
                    _ => {}
                }
                return;
            }
            Some(Overlay::QuickOpen(mut query, row)) => {
                match key.code {
                    KeyCode::Up => {
                        self.overlay = Some(Overlay::QuickOpen(query, row.saturating_sub(1)))
                    }
                    KeyCode::Down => self.overlay = Some(Overlay::QuickOpen(query, row + 1)),
                    KeyCode::Enter => {
                        self.overlay = None;
                        if let Some((root, hit)) = self
                            .project
                            .clone()
                            .zip(self.quick_open_hits(&query).get(row).cloned())
                        {
                            self.open_file(&root.join(hit));
                        }
                    }
                    _ if closes => self.overlay = None,
                    KeyCode::Backspace => {
                        query.pop();
                        self.overlay = Some(Overlay::QuickOpen(query, 0));
                    }
                    KeyCode::Char(c) if !chord.mods.ctrl && !chord.mods.alt => {
                        query.push(c);
                        self.overlay = Some(Overlay::QuickOpen(query, 0));
                    }
                    _ => {}
                }
                return;
            }
            Some(Overlay::NewTab(mut text)) | Some(Overlay::RenameTab(mut text)) => {
                let renaming = matches!(self.overlay, Some(Overlay::RenameTab(_)));
                match key.code {
                    KeyCode::Enter => {
                        self.overlay = None;
                        if renaming {
                            let text = text.trim();
                            self.name = (!text.is_empty()).then(|| text.to_string());
                        } else {
                            self.new_tab(&text);
                        }
                    }
                    _ if closes => self.overlay = None,
                    KeyCode::Backspace => {
                        text.pop();
                        self.overlay = Some(if renaming {
                            Overlay::RenameTab(text)
                        } else {
                            Overlay::NewTab(text)
                        });
                    }
                    KeyCode::Char(c) if !chord.mods.ctrl && !chord.mods.alt => {
                        text.push(c);
                        self.overlay = Some(if renaming {
                            Overlay::RenameTab(text)
                        } else {
                            Overlay::NewTab(text)
                        });
                    }
                    _ => {}
                }
                return;
            }
            None => {}
        }
        let command = self.settings.command(&chord);
        // The agent keeps every plain key, Enter, Escape, Tab and the arrows,
        // whatever the editor binds them to — they are how it is talked to —
        // and every chord nobody bound. A bound Ctrl or Alt chord, or a
        // function key, is the editor's, unless the settings say the agent
        // uses it itself.
        if self.focus == Focus::Agent {
            let editors = command.is_some()
                && (chord.mods.ctrl || chord.mods.alt || shell_has_no_use_for(&chord))
                && !self.settings.agent_keys.contains(&chord);
            if !editors {
                if let Some(pty) = self.agent.as_mut() {
                    pty.send_key(&chord.key, chord.mods);
                }
                return;
            }
        }
        if self.focus == Focus::Editor {
            let editors_own = matches!(
                command,
                Some(Command::Save | Command::Close | Command::Undo | Command::Redo)
            ) || command.is_some_and(|_| shell_has_no_use_for(&chord));
            if !editors_own {
                self.edit_key(key, &chord);
                return;
            }
        }
        if self.focus == Focus::Files && command.is_none() {
            let Some(tree) = self.tree.as_mut() else {
                return;
            };
            match key.code {
                KeyCode::Up => tree.move_selection(-1),
                KeyCode::Down => tree.move_selection(1),
                _ => {}
            }
            return;
        }
        if let Some(command) = command {
            self.execute(command);
        }
    }

    /// A key typed into the open file.
    fn edit_key(&mut self, key: KeyEvent, chord: &Chord) {
        let Some(buffer) = self.editor.as_mut() else {
            return;
        };
        match key.code {
            KeyCode::Char(c) if !chord.mods.ctrl && !chord.mods.alt => {
                buffer.insert(&c.to_string())
            }
            KeyCode::Enter => buffer.insert("\n"),
            KeyCode::Tab => buffer.insert("    "),
            KeyCode::Backspace => buffer.backspace(),
            KeyCode::Delete => buffer.delete(),
            KeyCode::Left => buffer.left(),
            KeyCode::Right => buffer.right(),
            KeyCode::Up => buffer.up(),
            KeyCode::Down => buffer.down(),
            KeyCode::Home => buffer.home(),
            KeyCode::End => buffer.end(),
            _ => {}
        }
    }

    pub fn execute(&mut self, command: Command) {
        match command {
            Command::Quit => self.should_quit = true,
            Command::ToggleSidebar => {
                self.show_sidebar = !self.show_sidebar;
                self.focus = if self.show_sidebar {
                    Focus::Files
                } else if self.editor.is_some() {
                    Focus::Editor
                } else {
                    Focus::Agent
                };
            }
            Command::NextPane => {
                let has_editor = self.editor.is_some();
                self.focus = match self.focus {
                    Focus::Agent if self.show_sidebar => Focus::Files,
                    Focus::Agent | Focus::Files if has_editor => Focus::Editor,
                    Focus::Agent | Focus::Files => Focus::Follow,
                    Focus::Follow | Focus::Editor => Focus::Agent,
                }
            }
            Command::Save => self.save(),
            Command::Undo => {
                if let Some(b) = self.editor.as_mut() {
                    b.undo()
                }
            }
            Command::Redo => {
                if let Some(b) = self.editor.as_mut() {
                    b.redo()
                }
            }
            Command::QuickOpen => self.overlay = Some(Overlay::QuickOpen(String::new(), 0)),
            // Enter in the files pane opens what is under the cursor.
            Command::MarkReviewed if self.focus == Focus::Files => {
                let Some(tree) = self.tree.as_mut() else {
                    return;
                };
                match tree.selected_row() {
                    Some(row) if row.is_dir => tree.toggle_selected(),
                    Some(row) => {
                        let path = row.path.clone();
                        self.open_file(&path);
                    }
                    None => {}
                }
            }
            Command::ScrubBack if self.focus == Focus::Files => {}
            Command::ScrubForward if self.focus == Focus::Files => {}
            Command::FollowLive | Command::ToggleView if self.focus == Focus::Files => {}
            Command::Changes => self.overlay = Some(Overlay::Changes(0)),
            Command::NewTab => self.overlay = Some(Overlay::NewTab(String::new())),
            Command::RenameTab => self.overlay = Some(Overlay::RenameTab(String::new())),
            Command::CloseTab => {
                if self.sessions.len() > 1 {
                    self.sessions.remove(self.active);
                    self.active = self.active.min(self.sessions.len() - 1);
                } else {
                    self.should_quit = true;
                }
            }
            Command::NextTab => self.active = (self.active + 1) % self.sessions.len(),
            Command::PrevTab => {
                self.active = (self.active + self.sessions.len() - 1) % self.sessions.len()
            }
            Command::Close if self.editor.is_some() => {
                self.editor = None;
                self.focus = Focus::Follow;
            }
            Command::Close => self.pinned = None,
            Command::ToggleView => {
                self.view = match self.view {
                    View::Diff => View::File,
                    View::File => View::Diff,
                }
            }
            Command::FollowLive => {
                self.pinned = None;
                self.follow.go_live()
            }
            Command::ScrubBack => {
                self.pinned = None;
                self.follow.scrub_back()
            }
            Command::ScrubForward => {
                self.pinned = None;
                self.follow.scrub_forward()
            }
            Command::MarkReviewed => {
                self.pinned = None;
                self.follow.mark_reviewed()
            }
            // Not built yet: the other overlays, the menus, the editor.
            other => self.note = Some(format!("{} is not here yet", other.label())),
        }
    }
}

/// Whether a chord is one no program in a terminal would be listening for:
/// a function key, or Ctrl with Shift or Alt on top.
fn shell_has_no_use_for(chord: &Chord) -> bool {
    match &chord.key {
        Key::Named(name) if name.starts_with('f') && name[1..].parse::<u8>().is_ok() => true,
        _ => chord.mods.ctrl && (chord.mods.shift || chord.mods.alt),
    }
}

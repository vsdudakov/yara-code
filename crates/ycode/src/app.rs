//! Everything the frame shows, and what a key does to it. A tab is a
//! session — one agent in one worktree, with its own timeline — and the
//! app is the tabs plus what is shared: settings, theme, the keyboard.

use std::ops::{Deref, DerefMut};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use crossterm::event::{KeyCode, KeyEvent, MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::Rect;
use yara_core::buffer::Buffer;
use yara_core::command::{self, Chord, Command, Key, FILE_MENU, HELP_MENU};
use yara_core::follow::{EditEvent, Follow};
use yara_core::fuzzy;
use yara_core::git::{self, Change, Repo, Watcher};
use yara_core::pty::Pty;
use yara_core::search::{self, Hit};
use yara_core::settings::Settings;
use yara_core::syntax::Syntax;
use yara_core::theme::{self, Theme};
use yara_core::tree::{self, Tree};
use yara_core::update::{self, Checker, Installer, Progress, Release};
use yara_core::usage::{Poller, Usage};

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
    /// A name being typed for a new workspace.
    NewTab(String),
    /// A name being typed for the current tab.
    RenameTab(String),
    /// A name being typed for a new file, made in the folder under the
    /// FILES cursor.
    NewFile(String),
    /// A folder path being typed to open.
    OpenFolder(String),
    /// Go to file: what was typed, and the row the cursor is on.
    QuickOpen(String, usize),
    /// The command palette: what was typed, and the row the cursor is on.
    Palette(String, usize),
    /// Project search: the query, the row, and what the query found.
    Search(String, usize, Vec<Hit>, usize),
    /// Every command and its chord, scrolled to a row.
    Keys(usize),
    /// A menu — File is 0, Help is 1 — with the row the cursor is on.
    Menu(usize, usize),
    /// Recent projects, with the row the cursor is on.
    Recent(usize),
    /// What each agent has used of its plan.
    Usage,
    /// The themes, with the row the cursor is on.
    Themes(usize),
}

/// Where things were drawn, so a click can be told what it landed on.
#[derive(Default)]
pub struct Hits {
    pub menus: Vec<(Rect, usize)>,
    pub tabs: Vec<(Rect, usize)>,
    pub plus: Rect,
    pub usage: Rect,
    pub files: Rect,
    pub file_rows: Vec<(Rect, usize)>,
    pub agent: Rect,
    pub follow: Rect,
    pub live: Rect,
    pub ticks: Vec<(Rect, usize)>,
    pub counter: Rect,
    pub overlay: Rect,
    pub rows: Vec<(Rect, usize)>,
}

fn hit(rect: Rect, x: u16, y: u16) -> bool {
    x >= rect.x && x < rect.right() && y >= rect.y && y < rect.bottom()
}

/// Where the editor stands with its own releases.
#[derive(Default)]
pub struct Updates {
    checker: Checker,
    installer: Installer,
    /// A newer release, once the check found one.
    pub available: Option<Release>,
    /// The tag installed this session, waiting for a restart.
    pub installed: Option<String>,
}

/// The menus in the order the header shows them.
pub const MENUS: [(&str, command::Menu); 2] = [("File", FILE_MENU), ("Help", HELP_MENU)];

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
    /// The RECENT row the start page's cursor is on.
    pub start_row: usize,
    pub hits: Hits,
    pub updates: Updates,
    usage_poller: Poller,
    /// The agents' figures and how many seconds old they are.
    pub usage: Option<(Vec<Usage>, u64)>,
    pub themes: Vec<Theme>,
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
        app.themes = theme::load_all();
        app.note = complaint;
        app
    }

    /// Asks the agents what they have used, off the drawing thread.
    pub fn poll_usage(&mut self) {
        if self.settings.usage_commands.is_empty() {
            return;
        }
        let dirty = self.dirty.clone();
        self.usage_poller
            .start(self.settings.usage_commands.clone(), move || {
                dirty.store(true, Ordering::Relaxed)
            });
    }

    /// The header's chip: the first agent's figure, when there is one.
    pub fn usage_chip(&self) -> Option<String> {
        let (usage, _) = self.usage.as_ref()?;
        let first = usage.first()?;
        Some(format!("◐ {} {}%", first.agent, first.percent))
    }

    /// Takes in what the background threads have finished: the usage poll,
    /// the update check, the install.
    pub fn collect(&mut self) {
        if let Some(latest) = self.usage_poller.latest() {
            self.usage = Some(latest);
        }
        match self.updates.checker.take() {
            Some(Ok(release)) if release.is_newer() => {
                self.updates.available = Some(release.clone());
                if update::can_install() {
                    let dirty = self.dirty.clone();
                    self.updates
                        .installer
                        .start(&release, move || dirty.store(true, Ordering::Relaxed));
                } else {
                    self.note = Some(format!(
                        "{} is out — {}",
                        release.tag,
                        update::how_to_update()
                    ));
                }
            }
            Some(Ok(_)) => self.note = Some(format!("v{} is the latest", update::CURRENT)),
            Some(Err(e)) => self.note = Some(format!("update check failed: {e}")),
            None => {}
        }
        if let Some(progress) = self.updates.installer.poll() {
            let tag = self.updates.installer.tag().to_string();
            if let Progress::Done(_) = &progress {
                self.updates.installed = Some(tag.clone());
                self.note = Some(format!("↓ {tag} downloaded — restart to apply"));
            } else {
                self.note = Some(progress.line(&tag));
            }
        }
    }

    /// The version chip in the status bar: what runs, with an arrow when
    /// something newer exists and a tick once it is installed.
    pub fn version_chip(&self) -> String {
        match (&self.updates.installed, &self.updates.available) {
            (Some(tag), _) => format!("{tag} ✓"),
            (None, Some(_)) => format!("v{} ↑", update::CURRENT),
            (None, None) => format!("v{}", update::CURRENT),
        }
    }

    fn set_theme(&mut self, index: usize) {
        let Some(theme) = self.themes.get(index).cloned() else {
            return;
        };
        self.settings.theme = theme.name.clone();
        self.syntax.set_theme(&theme);
        self.theme = theme;
        let _ = self.settings.save();
    }

    /// A click. Overlays take the click if it lands in their box and close
    /// on the backdrop; otherwise the chrome answers to it.
    pub fn handle_mouse(&mut self, mouse: MouseEvent) {
        if mouse.kind != MouseEventKind::Down(MouseButton::Left) {
            return;
        }
        let (x, y) = (mouse.column, mouse.row);
        if self.overlay.is_some() {
            if !hit(self.hits.overlay, x, y) {
                self.overlay = None;
                return;
            }
            let row = self
                .hits
                .rows
                .iter()
                .find(|(r, _)| hit(*r, x, y))
                .map(|(_, i)| *i);
            if let Some(row) = row {
                self.overlay = match self.overlay.take() {
                    Some(Overlay::Changes(_)) => {
                        self.open_change(row);
                        return;
                    }
                    Some(Overlay::Menu(menu, _)) => {
                        self.overlay = None;
                        if let Some(Some(command)) = MENUS[menu].1.get(row) {
                            self.execute(*command);
                        }
                        return;
                    }
                    Some(Overlay::Recent(_)) => {
                        if let Some(path) = self.settings.recent_projects.get(row).cloned() {
                            self.open_project(path);
                        }
                        None
                    }
                    Some(Overlay::Themes(_)) => {
                        self.set_theme(row);
                        None
                    }
                    Some(Overlay::QuickOpen(q, _)) => Some(Overlay::QuickOpen(q, row)),
                    Some(Overlay::Palette(q, _)) => Some(Overlay::Palette(q, row)),
                    Some(Overlay::Search(q, _, h, f)) => Some(Overlay::Search(q, row, h, f)),
                    other => other,
                };
            }
            return;
        }
        let find =
            |rects: &[(Rect, usize)]| rects.iter().find(|(r, _)| hit(*r, x, y)).map(|(_, i)| *i);
        if let Some(menu) = find(&self.hits.menus) {
            self.overlay = Some(Overlay::Menu(menu, 0));
        } else if let Some(tab) = find(&self.hits.tabs) {
            self.active = tab;
        } else if hit(self.hits.plus, x, y) {
            self.execute(Command::NewTab);
        } else if hit(self.hits.usage, x, y) {
            self.execute(Command::AgentUsage);
        } else if hit(self.hits.live, x, y) {
            self.execute(Command::FollowLive);
        } else if let Some(i) = self
            .hits
            .ticks
            .iter()
            .find(|(r, _)| hit(*r, x, y))
            .map(|(_, i)| *i)
        {
            self.pinned = None;
            self.follow.jump_to(i);
            self.focus = Focus::Follow;
        } else if hit(self.hits.counter, x, y) {
            self.pinned = None;
            self.follow.jump_to_next_unreviewed();
        } else if let Some(i) = self
            .hits
            .file_rows
            .iter()
            .find(|(r, _)| hit(*r, x, y))
            .map(|(_, i)| *i)
        {
            self.focus = Focus::Files;
            if let Some(tree) = self.tree.as_mut() {
                tree.selected = i;
            }
            self.execute(Command::MarkReviewed);
        } else if hit(self.hits.files, x, y) {
            self.focus = Focus::Files;
        } else if hit(self.hits.agent, x, y) {
            self.focus = Focus::Agent;
        } else if hit(self.hits.follow, x, y) {
            self.focus = if self.editor.is_some() {
                Focus::Editor
            } else {
                Focus::Follow
            };
        }
    }

    pub fn with_settings(project: Option<PathBuf>, settings: Settings, theme: Theme) -> Self {
        // The path as the user typed it — `.`, most often — is not the name
        // the header should show.
        let project = project.map(|p| p.canonicalize().unwrap_or(p));
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
            start_row: 0,
            hits: Hits::default(),
            updates: Updates::default(),
            usage_poller: Poller::default(),
            usage: None,
            themes: theme::builtin(),
            note: None,
            should_quit: false,
        }
    }

    /// Opens a project: in this tab when it has none, else in a new one. The
    /// folder joins the recent list, which is saved so the start page
    /// remembers it.
    pub fn open_project(&mut self, path: PathBuf) {
        let path = path.canonicalize().unwrap_or(path);
        let session = Session::new(Some(path.clone()), &self.settings);
        if self.project.is_none() {
            self.sessions[self.active] = session;
        } else {
            self.sessions.push(session);
            self.active = self.sessions.len() - 1;
        }
        self.settings.push_recent(&path);
        let _ = self.settings.save();
        self.start_agent();
        self.refresh();
        self.focus = Focus::Agent;
    }

    /// The commands the palette offers for what was typed, best first.
    pub fn palette_hits(&self, query: &str) -> Vec<Command> {
        let commands: Vec<Command> = command::palette().collect();
        let labels: Vec<&str> = commands.iter().map(|c| c.label()).collect();
        fuzzy::rank(query, labels.iter().copied())
            .into_iter()
            .map(|(i, _)| commands[i])
            .collect()
    }

    fn search(&self, query: &str) -> (Vec<Hit>, usize) {
        match &self.project {
            Some(root) => search::search(root, query, &self.settings.search_exclude, 500),
            None => (Vec::new(), 0),
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
        let program = self
            .settings
            .agent
            .split_whitespace()
            .next()
            .unwrap_or("agent");
        std::path::Path::new(program)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or(program)
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

    /// A new, empty file in the folder under the FILES cursor — or the
    /// project root — opened for editing. Nothing is overwritten.
    fn new_file(&mut self, name: &str) {
        if name.is_empty() {
            return;
        }
        // Under the FILES cursor when the sidebar is open, else at the root.
        let cursor = self
            .tree
            .as_ref()
            .filter(|_| self.show_sidebar)
            .and_then(|t| t.selected_row());
        let dir = match cursor {
            Some(row) if row.is_dir => row.path.clone(),
            Some(row) => row
                .path
                .parent()
                .map(|p| p.to_path_buf())
                .unwrap_or_default(),
            None => match &self.project {
                Some(root) => root.clone(),
                None => {
                    self.note = Some("open a folder first".into());
                    return;
                }
            },
        };
        let path = dir.join(name);
        if path.exists() {
            self.note = Some(format!("{} already exists", path.display()));
            return;
        }
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        match std::fs::write(&path, "") {
            Ok(()) => {
                if let Some(tree) = self.tree.as_mut() {
                    tree.rebuild();
                }
                self.open_file(&path);
            }
            Err(e) => self.note = Some(format!("{}: {e}", path.display())),
        }
    }

    /// A new workspace: a worktree of the current repository on a branch of
    /// the name the user gave it, an agent started in it, and a tab called
    /// by that name.
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
                let mut session = Session::new(Some(path), &self.settings);
                session.name = Some(name.trim().to_string());
                self.sessions.push(session);
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
            Some(Overlay::Palette(mut query, row)) => {
                match key.code {
                    KeyCode::Up => {
                        self.overlay = Some(Overlay::Palette(query, row.saturating_sub(1)))
                    }
                    KeyCode::Down => self.overlay = Some(Overlay::Palette(query, row + 1)),
                    KeyCode::Enter => {
                        self.overlay = None;
                        if let Some(command) = self.palette_hits(&query).get(row).copied() {
                            self.execute(command);
                        }
                    }
                    _ if closes => self.overlay = None,
                    KeyCode::Backspace => {
                        query.pop();
                        self.overlay = Some(Overlay::Palette(query, 0));
                    }
                    KeyCode::Char(c) if !chord.mods.ctrl && !chord.mods.alt => {
                        query.push(c);
                        self.overlay = Some(Overlay::Palette(query, 0));
                    }
                    _ => {}
                }
                return;
            }
            Some(Overlay::Search(mut query, row, hits, files)) => {
                match key.code {
                    KeyCode::Up => {
                        self.overlay =
                            Some(Overlay::Search(query, row.saturating_sub(1), hits, files))
                    }
                    KeyCode::Down => {
                        let last = hits.len().saturating_sub(1);
                        self.overlay =
                            Some(Overlay::Search(query, (row + 1).min(last), hits, files))
                    }
                    KeyCode::Enter => {
                        self.overlay = None;
                        if let Some((root, hit)) = self.project.clone().zip(hits.get(row)) {
                            self.open_file(&root.join(&hit.path));
                            if let Some(buffer) = self.editor.as_mut() {
                                for _ in 1..hit.line {
                                    buffer.down();
                                }
                            }
                        }
                    }
                    _ if closes => self.overlay = None,
                    KeyCode::Backspace | KeyCode::Char(_) => {
                        match key.code {
                            KeyCode::Backspace => {
                                query.pop();
                            }
                            KeyCode::Char(c) if !chord.mods.ctrl && !chord.mods.alt => {
                                query.push(c)
                            }
                            _ => return,
                        }
                        let (hits, files) = self.search(&query);
                        self.overlay = Some(Overlay::Search(query, 0, hits, files));
                    }
                    _ => {}
                }
                return;
            }
            Some(Overlay::Keys(scroll)) => {
                match key.code {
                    KeyCode::Up => self.overlay = Some(Overlay::Keys(scroll.saturating_sub(1))),
                    KeyCode::Down => {
                        self.overlay = Some(Overlay::Keys((scroll + 1).min(command::ALL.len() - 1)))
                    }
                    _ if closes => self.overlay = None,
                    _ => {}
                }
                return;
            }
            Some(Overlay::Menu(menu, row)) => {
                let entries = MENUS[menu].1;
                let step = |from: usize, delta: isize| -> usize {
                    // Separators are skipped over, and the ends stop.
                    let mut at = from as isize;
                    loop {
                        at += delta;
                        match entries.get(at as usize) {
                            None => return from,
                            Some(Some(_)) => return at as usize,
                            Some(None) => {}
                        }
                    }
                };
                match key.code {
                    KeyCode::Up => self.overlay = Some(Overlay::Menu(menu, step(row, -1))),
                    KeyCode::Down => self.overlay = Some(Overlay::Menu(menu, step(row, 1))),
                    KeyCode::Left => {
                        self.overlay =
                            Some(Overlay::Menu((menu + MENUS.len() - 1) % MENUS.len(), 0))
                    }
                    KeyCode::Right => {
                        self.overlay = Some(Overlay::Menu((menu + 1) % MENUS.len(), 0))
                    }
                    KeyCode::Enter => {
                        self.overlay = None;
                        if let Some(Some(command)) = entries.get(row) {
                            self.execute(*command);
                        }
                    }
                    _ if closes => self.overlay = None,
                    _ => {}
                }
                return;
            }
            Some(Overlay::Usage) => {
                if closes {
                    self.overlay = None;
                }
                return;
            }
            Some(Overlay::Themes(row)) => {
                match key.code {
                    KeyCode::Up => self.overlay = Some(Overlay::Themes(row.saturating_sub(1))),
                    KeyCode::Down => {
                        self.overlay = Some(Overlay::Themes((row + 1).min(self.themes.len() - 1)))
                    }
                    KeyCode::Enter => {
                        self.overlay = None;
                        self.set_theme(row);
                    }
                    _ if closes => self.overlay = None,
                    _ => {}
                }
                return;
            }
            Some(Overlay::Recent(row)) => {
                match key.code {
                    KeyCode::Up => self.overlay = Some(Overlay::Recent(row.saturating_sub(1))),
                    KeyCode::Down => {
                        let last = self.settings.recent_projects.len().saturating_sub(1);
                        self.overlay = Some(Overlay::Recent((row + 1).min(last)))
                    }
                    KeyCode::Enter => {
                        self.overlay = None;
                        if let Some(path) = self.settings.recent_projects.get(row).cloned() {
                            self.open_project(path);
                        }
                    }
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
            Some(
                Overlay::NewTab(mut text)
                | Overlay::RenameTab(mut text)
                | Overlay::NewFile(mut text)
                | Overlay::OpenFolder(mut text),
            ) => {
                let again = |text: String| match self.overlay.as_ref().unwrap() {
                    Overlay::NewTab(_) => Overlay::NewTab(text),
                    Overlay::RenameTab(_) => Overlay::RenameTab(text),
                    Overlay::NewFile(_) => Overlay::NewFile(text),
                    _ => Overlay::OpenFolder(text),
                };
                match key.code {
                    KeyCode::Enter => {
                        let which = self.overlay.take().unwrap();
                        let text = text.trim().to_string();
                        match which {
                            Overlay::NewTab(_) => self.new_tab(&text),
                            Overlay::RenameTab(_) => self.name = (!text.is_empty()).then_some(text),
                            Overlay::NewFile(_) => self.new_file(&text),
                            _ => {
                                if !text.is_empty() {
                                    self.open_project(PathBuf::from(expand_home(&text)));
                                }
                            }
                        }
                    }
                    _ if closes => self.overlay = None,
                    KeyCode::Backspace => {
                        text.pop();
                        self.overlay = Some(again(text));
                    }
                    KeyCode::Char(c) if !chord.mods.ctrl && !chord.mods.alt => {
                        text.push(c);
                        self.overlay = Some(again(text));
                    }
                    _ => {}
                }
                return;
            }
            None => {}
        }
        let command = self.settings.command(&chord);
        // The start page: the RECENT list is what the arrows and Enter are
        // about; everything else is a command.
        if self.project.is_none() && command.is_none_or(|c| c == Command::MarkReviewed) {
            let last = self.settings.recent_projects.len().saturating_sub(1);
            match key.code {
                KeyCode::Up => self.start_row = self.start_row.saturating_sub(1),
                KeyCode::Down => self.start_row = (self.start_row + 1).min(last),
                KeyCode::Enter => {
                    if let Some(path) = self.settings.recent_projects.get(self.start_row).cloned() {
                        self.open_project(path);
                    }
                }
                _ => {}
            }
            return;
        }
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
            Command::CommandPalette => self.overlay = Some(Overlay::Palette(String::new(), 0)),
            Command::SearchProject => {
                self.overlay = Some(Overlay::Search(String::new(), 0, Vec::new(), 0))
            }
            Command::Help => self.overlay = Some(Overlay::Keys(0)),
            Command::FileMenu => self.overlay = Some(Overlay::Menu(0, 0)),
            Command::HelpMenu => self.overlay = Some(Overlay::Menu(1, 0)),
            Command::OpenRecent => self.overlay = Some(Overlay::Recent(0)),
            Command::NewFile => self.overlay = Some(Overlay::NewFile(String::new())),
            Command::OpenFolder => self.overlay = Some(Overlay::OpenFolder(String::new())),
            Command::Settings => match self.settings.ensure_file() {
                Ok(path) => self.open_file(&path),
                Err(e) => self.note = Some(e.to_string()),
            },
            // The agents only show their limits from inside their own
            // session, so the usual answer is to ask the agent: its slash
            // command is typed at it. A configured usage command gets the
            // panel instead.
            Command::AgentUsage if self.settings.usage_commands.is_empty() => {
                let slash = self
                    .settings
                    .usage_slash
                    .get(self.agent_name())
                    .cloned()
                    .unwrap_or_else(|| "/usage".into());
                if let Some(pty) = self.agent.as_mut() {
                    pty.write(format!("{slash}\r").as_bytes());
                    self.focus = Focus::Agent;
                } else {
                    self.note = Some("no agent to ask".into());
                }
            }
            Command::AgentUsage => {
                self.poll_usage();
                self.overlay = Some(Overlay::Usage);
            }
            Command::ThemePicker => {
                let current = self.themes.iter().position(|t| t.name == self.theme.name);
                self.overlay = Some(Overlay::Themes(current.unwrap_or(0)));
            }
            Command::CheckForUpdates | Command::InstallUpdate => {
                let dirty = self.dirty.clone();
                match self.updates.available.clone() {
                    Some(release) if update::can_install() => {
                        self.updates
                            .installer
                            .start(&release, move || dirty.store(true, Ordering::Relaxed));
                    }
                    Some(release) => {
                        self.note = Some(format!(
                            "{} is out — {}",
                            release.tag,
                            update::how_to_update()
                        ))
                    }
                    None => {
                        self.note = Some("checking for updates…".into());
                        self.updates
                            .checker
                            .start(move || dirty.store(true, Ordering::Relaxed));
                    }
                }
            }
            Command::Documentation => {
                if !yara_core::open_url(yara_core::DOCUMENTATION) {
                    self.note = Some(format!("no browser to open {}", yara_core::DOCUMENTATION));
                }
            }
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
        }
    }
}

/// `~` at the start of a typed path, as the shell would read it.
fn expand_home(text: &str) -> String {
    match text.strip_prefix('~') {
        Some(rest) => yara_core::home_dir()
            .map(|home| format!("{}{rest}", home.display()))
            .unwrap_or_else(|| text.to_string()),
        None => text.to_string(),
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

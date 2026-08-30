//! Everything the frame shows, and what a key does to it. A tab is a
//! session — one agent in one worktree, with its own timeline — and the
//! app is the tabs plus what is shared: settings, theme, the keyboard.

use std::ops::{Deref, DerefMut};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
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
    /// A name being typed for a new folder, made in the same place.
    NewFolder(String),
    /// Walking the filesystem for a folder to add: where the walk stands,
    /// the row the cursor is on, and what has been typed to narrow it.
    AddFolder {
        dir: PathBuf,
        row: usize,
        filter: String,
    },
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
    /// The workspace's folders, to take one out.
    Folders(usize),
    /// The menu a right click on a tab drops: which tab, and the row.
    TabMenu(usize, usize),
    /// The menu a right click in the tree drops, with the row it is on.
    TreeMenu(usize),
    /// The file being edited has unsaved changes and is about to close:
    /// save it, drop it, or stay. `quit` when the whole editor is closing.
    CloseFile { quit: bool },
}

/// Which seam a drag has hold of.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Seam {
    Panes,
    Tree,
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
    /// The blank column between the agent and the follow pane; dragging
    /// it resizes them.
    pub seam: Rect,
    /// The blank column between the tree and the pane beside it.
    pub tree_seam: Rect,
    /// The row the panes sit in, for the drag to measure against.
    pub body: Rect,
    /// The text of the file being edited, without its gutter.
    pub editor: Rect,
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

/// What a right click on a tab offers, in order.
pub const TAB_MENU: [&str; 4] = ["Rename…", "Add Folder to Task…", "Delete Task", "Close"];

/// What a right click in the tree offers.
pub const TREE_MENU: [Command; 2] = [Command::NewFile, Command::NewFolder];

/// The menus in the order the header shows them.
pub const MENUS: [(&str, command::Menu); 2] = [("File", FILE_MENU), ("Help", HELP_MENU)];

/// One folder of a task: a worktree of a repository, most often, but any
/// folder will do — what git says about it is a bonus, not a requirement.
pub struct Folder {
    pub path: PathBuf,
    pub repo: Option<Repo>,
    watcher: Watcher,
    /// What differs from the base branch, refreshed with the watcher.
    pub changes: Vec<Change>,
    /// The pull request the branch is on, asked of `gh` in the background.
    pr: Arc<Mutex<Option<String>>>,
}

impl Folder {
    fn new(path: PathBuf, settings: &Settings) -> Self {
        let path = path.canonicalize().unwrap_or(path);
        let repo = git::open(&path, &settings.base_branch);
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
            path,
            repo,
            watcher: Watcher::default(),
            changes: Vec::new(),
            pr,
        }
    }

    /// The folder's own name, which is what the tree and the paths show.
    pub fn name(&self) -> String {
        self.path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| self.path.display().to_string())
    }

    pub fn branch(&self) -> &str {
        self.repo.as_ref().map_or("—", |r| r.branch.as_str())
    }

    pub fn totals(&self) -> (usize, usize) {
        self.changes
            .iter()
            .fold((0, 0), |(a, r), c| (a + c.added, r + c.removed))
    }
}

/// Where a task's own worktree of a repository lives: under the folder the
/// settings name, or beside the repository, and called by the task.
pub fn worktree_path(repo: &Repo, settings: &Settings, slug: &str) -> PathBuf {
    let name = repo
        .root
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let base = match settings.worktrees_dir.as_str() {
        "" => repo
            .root
            .parent()
            .unwrap_or(&repo.root)
            .join(format!("{name}-worktrees")),
        dir => PathBuf::from(dir).join(&name),
    };
    base.join(slug)
}

/// A row of the CHANGES list: a folder's heading, or a file under it.
#[derive(Clone, Debug)]
pub enum ChangeRow {
    Folder(usize),
    File(usize, Change),
}

/// One task: an agent, its own timeline, and the folders it works in —
/// the workspace's folders, or a worktree of each where the task has one of
/// its own.
pub struct Task {
    pub folders: Vec<Folder>,
    pub agent: Option<Pty>,
    pub follow: Follow,
    /// A file's whole diff opened from CHANGES, shown in place of the
    /// timeline's current edit until the follow keys are used again.
    pub pinned: Option<EditEvent>,
    pub view: View,
    /// How far the follow pane's body — or the file being edited — is
    /// scrolled, in rows.
    pub scroll: u16,
    /// The caret moved since the last frame, so the view must show it.
    pub caret_moved: bool,
    /// The task's files, for the sidebar.
    pub tree: Option<Tree>,
    /// A file open for editing, in the follow pane's place.
    pub editor: Option<Buffer>,
    /// A name the user gave the tab; otherwise it is named by its work.
    pub name: Option<String>,
}

impl Task {
    fn new(paths: &[PathBuf], settings: &Settings) -> Self {
        let folders: Vec<Folder> = paths
            .iter()
            .map(|path| Folder::new(path.clone(), settings))
            .collect();
        Self {
            tree: (!folders.is_empty())
                .then(|| Tree::with_roots(folders.iter().map(|f| f.path.clone()).collect())),
            folders,
            agent: None,
            follow: Follow::default(),
            pinned: None,
            view: View::Diff,
            scroll: 0,
            caret_moved: true,
            editor: None,
            name: None,
        }
    }

    /// What the task calls its worktrees, as a folder name: its own name,
    /// with spaces and slashes made dashes. `None` while it is unnamed —
    /// then the task works in the workspace's folders themselves.
    pub fn slug(&self) -> Option<String> {
        let name = self.name.as_ref()?;
        let parts: Vec<&str> = name
            .split(|c: char| c.is_whitespace() || c == '/')
            .filter(|p| !p.is_empty())
            .collect();
        (!parts.is_empty()).then(|| parts.join("-"))
    }

    /// Points the task at the workspace's folders again — where it has a
    /// worktree of its own for one, that worktree stands in its place.
    /// Folders it already watches are kept, watchers and all.
    fn resync(&mut self, workspace: &[PathBuf], settings: &Settings) {
        let slug = self.slug();
        let mut kept: Vec<Folder> = Vec::new();
        for path in workspace {
            let path = match (&slug, git::open(path, "")) {
                (Some(slug), Some(repo)) => {
                    let mine = worktree_path(&repo, settings, slug);
                    if mine.is_dir() {
                        mine
                    } else {
                        path.clone()
                    }
                }
                _ => path.clone(),
            };
            match self.folders.iter().position(|f| f.path == path) {
                Some(i) => kept.push(self.folders.remove(i)),
                None => kept.push(Folder::new(path, settings)),
            }
        }
        self.folders = kept;
        let roots: Vec<PathBuf> = self.folders.iter().map(|f| f.path.clone()).collect();
        self.tree = (!roots.is_empty()).then(|| Tree::with_roots(roots));
    }

    /// The main folder: where the agent runs and what the chrome names.
    pub fn main(&self) -> Option<&Folder> {
        self.folders.first()
    }

    pub fn project(&self) -> Option<&std::path::Path> {
        self.main().map(|f| f.path.as_path())
    }

    pub fn repo(&self) -> Option<&Repo> {
        self.main().and_then(|f| f.repo.as_ref())
    }

    /// The folder a path belongs to.
    pub fn folder_of(&self, path: &std::path::Path) -> Option<&Folder> {
        self.folders.iter().find(|f| path.starts_with(&f.path))
    }

    /// A path as the panes show it: relative to its folder, and named by
    /// that folder as well when the workspace holds more than one.
    pub fn display_path(&self, path: &std::path::Path) -> String {
        let Some(folder) = self.folder_of(path) else {
            return path.display().to_string();
        };
        let relative = path
            .strip_prefix(&folder.path)
            .unwrap_or(path)
            .to_string_lossy()
            .replace('\\', "/");
        if self.folders.len() > 1 {
            format!("{}/{relative}", folder.name())
        } else {
            relative
        }
    }

    /// Whether git has anything to say about a path — what tints the tree.
    pub fn is_changed(&self, path: &std::path::Path) -> bool {
        let Some(folder) = self.folder_of(path) else {
            return false;
        };
        let Ok(relative) = path.strip_prefix(&folder.path) else {
            return false;
        };
        let relative = relative.to_string_lossy().replace('\\', "/");
        folder.changes.iter().any(|c| c.path == relative)
    }

    /// The CHANGES list: a heading per folder when there are several, and
    /// every changed file under it.
    pub fn change_rows(&self) -> Vec<ChangeRow> {
        let mut rows = Vec::new();
        for (i, folder) in self.folders.iter().enumerate() {
            if self.folders.len() > 1 {
                rows.push(ChangeRow::Folder(i));
            }
            rows.extend(
                folder
                    .changes
                    .iter()
                    .map(|change| ChangeRow::File(i, change.clone())),
            );
        }
        rows
    }

    /// Lines added and removed against the base, over every folder.
    pub fn totals(&self) -> (usize, usize) {
        self.folders.iter().fold((0, 0), |(a, r), f| {
            let (fa, fr) = f.totals();
            (a + fa, r + fr)
        })
    }

    /// What the tab is called: the name it was given, else the main
    /// folder's pull request, else its branch, else its own name.
    pub fn title(&self) -> String {
        if let Some(name) = &self.name {
            return name.clone();
        }
        let Some(folder) = self.main() else {
            return "no project".into();
        };
        if let Some(pr) = folder.pr.lock().unwrap().as_ref() {
            return pr.clone();
        }
        match &folder.repo {
            Some(repo) => repo.branch.clone(),
            None => folder.name(),
        }
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
    /// The workspace: the folders the project is made of. Tasks work in
    /// these, or in worktrees of their own for them.
    pub workspace: Vec<PathBuf>,
    pub settings: Settings,
    pub theme: Theme,
    pub syntax: Syntax,
    pub tasks: Vec<Task>,
    pub active: usize,
    /// Set by an agent's reader thread when there is something new to draw.
    pub dirty: Arc<AtomicBool>,
    pub focus: Focus,
    pub show_sidebar: bool,
    pub overlay: Option<Overlay>,
    /// The RECENT row the start page's cursor is on.
    pub start_row: usize,
    pub hits: Hits,
    /// The cells the mouse dragged over, as (column, row) corners, and the
    /// text of the last frame they are read from.
    pub selection: Option<((u16, u16), (u16, u16))>,
    /// A seam is being dragged: the one between the panes, or the tree's.
    pub resizing: Option<Seam>,
    /// Where the mouse is, for what lights up under it.
    pub hover: Option<(u16, u16)>,
    /// The tab being dragged along the strip, if one is.
    pub dragging_tab: Option<usize>,
    /// Agents start on their own in workspaces switched to — what the
    /// command does, and what tests do not want.
    pub autostart: bool,
    pub last_frame: Vec<String>,
    /// An OSC 52 escape waiting to be written to the terminal — a copy made
    /// where no clipboard tool answered.
    pub osc52: Option<String>,
    /// The editor caret is drawn on alternate ticks.
    pub caret_on: bool,
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
    type Target = Task;
    fn deref(&self) -> &Task {
        &self.tasks[self.active]
    }
}

impl DerefMut for App {
    fn deref_mut(&mut self) -> &mut Task {
        &mut self.tasks[self.active]
    }
}

impl App {
    /// An app on the built-in defaults; `load` reads the user's files.
    pub fn new(project: Option<PathBuf>) -> Self {
        Self::with_settings(project, Settings::default(), Theme::default())
    }

    /// An app on one folder, with one task in it.
    pub fn with_settings(project: Option<PathBuf>, settings: Settings, theme: Theme) -> Self {
        Self::with_workspace(project.into_iter().collect(), settings, theme)
    }

    /// An app on a workspace of folders, with one task in it.
    pub fn with_workspace(folders: Vec<PathBuf>, settings: Settings, theme: Theme) -> Self {
        // A path as the user typed it — `.`, most often — is not the name
        // the chrome should show.
        let workspace: Vec<PathBuf> = folders
            .into_iter()
            .map(|p| p.canonicalize().unwrap_or(p))
            .collect();
        Self {
            tasks: vec![Task::new(&workspace, &settings)],
            workspace,
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
            selection: None,
            resizing: None,
            hover: None,
            autostart: false,
            dragging_tab: None,
            last_frame: Vec::new(),
            osc52: None,
            caret_on: true,
            updates: Updates::default(),
            usage_poller: Poller::default(),
            usage: None,
            themes: theme::builtin(),
            note: None,
            should_quit: false,
        }
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
        app.autostart = true;
        app
    }

    /// Starts the active workspace's agent if it has none yet — a workspace
    /// opened from an existing worktree waits until it is looked at.
    pub fn ensure_agent(&mut self) {
        if self.autostart && self.agent.is_none() && self.project().is_some() {
            self.start_agent();
        }
    }

    /// The folders inside `dir` that the walk offers, those whose name
    /// carries what has been typed first among them.
    pub fn browse_entries(dir: &std::path::Path, filter: &str) -> Vec<PathBuf> {
        let filter = filter.to_lowercase();
        tree::list_dir(dir)
            .into_iter()
            .filter(|(_, is_dir)| *is_dir)
            .map(|(path, _)| path)
            .filter(|path| {
                filter.is_empty()
                    || path
                        .file_name()
                        .is_some_and(|n| n.to_string_lossy().to_lowercase().contains(&filter))
            })
            .collect()
    }

    /// Where a walk for a folder starts: beside the task's main folder, or
    /// at home.
    fn browse_from(&self) -> PathBuf {
        self.project()
            .and_then(|p| p.parent())
            .map(|p| p.to_path_buf())
            .or_else(yara_core::home_dir)
            .unwrap_or_else(|| PathBuf::from("/"))
    }

    /// Opens a workspace: its folders, one task in them, the recent list
    /// updated so the start page remembers it.
    pub fn open_workspace(&mut self, folders: Vec<PathBuf>) {
        let folders: Vec<PathBuf> = folders
            .into_iter()
            .map(|p| p.canonicalize().unwrap_or(p))
            .filter(|p| p.is_dir())
            .collect();
        if folders.is_empty() {
            self.note = Some("none of those folders is there any more".into());
            return;
        }
        self.workspace = folders;
        self.tasks = vec![Task::new(&self.workspace, &self.settings)];
        self.active = 0;
        self.settings.push_recent(&self.workspace.clone());
        let _ = self.settings.save();
        self.start_agent();
        self.refresh();
        self.focus = Focus::Agent;
    }

    /// Adds a folder to the workspace: another repository, or any folder
    /// the work touches. Every task takes it up — with its own worktree of
    /// it where the task has one.
    fn add_folder(&mut self, path: &str) {
        let path = PathBuf::from(expand_home(path.trim()));
        if !path.is_dir() {
            self.note = Some(format!("{} is not a folder", path.display()));
            return;
        }
        let path = path.canonicalize().unwrap_or(path);
        if self.workspace.contains(&path) {
            self.note = Some("that folder is already in this workspace".into());
            return;
        }
        let first = self.workspace.is_empty();
        self.workspace.push(path);
        self.resync_tasks();
        self.settings.push_recent(&self.workspace.clone());
        let _ = self.settings.save();
        if first {
            self.start_agent();
            self.focus = Focus::Agent;
        }
        self.refresh();
    }

    /// Takes a folder out of the workspace, and out of every task with it.
    fn remove_folder(&mut self, index: usize) {
        if index >= self.workspace.len() {
            return;
        }
        let gone = self.workspace.remove(index);
        self.resync_tasks();
        self.settings.push_recent(&self.workspace.clone());
        let _ = self.settings.save();
        self.note = Some(format!("{} left the workspace", gone.display()));
    }

    /// Points every task at the workspace's folders again.
    fn resync_tasks(&mut self) {
        let workspace = self.workspace.clone();
        let settings = self.settings.clone();
        for task in &mut self.tasks {
            task.resync(&workspace, &settings);
        }
    }

    /// A new task, named by the user: a worktree of that name for every
    /// folder of the workspace that is a repository, the folder itself for
    /// every folder that is not, and an agent of its own over them.
    fn new_tab(&mut self, name: &str) {
        if self.workspace.is_empty() {
            self.note = Some("a task needs a folder to work in".into());
            return;
        }
        let mut task = Task::new(&[], &self.settings);
        task.name = Some(name.trim().to_string());
        let Some(slug) = task.slug() else {
            self.note = Some("a task needs a name".into());
            return;
        };
        let mut paths = Vec::new();
        for folder in &self.workspace.clone() {
            match git::open(folder, &self.settings.base_branch) {
                Some(repo) => {
                    let dir = worktree_path(&repo, &self.settings, &slug);
                    let parent = dir.parent().unwrap_or(&dir).to_path_buf();
                    match git::worktree_add(&repo, &parent, &slug) {
                        Ok(path) => paths.push(path),
                        Err(e) => {
                            self.note = Some(e);
                            return;
                        }
                    }
                }
                // A folder outside git is shared: there is no branch of it
                // to make.
                None => paths.push(folder.clone()),
            }
        }
        task.resync(&paths, &self.settings);
        self.tasks.push(task);
        self.active = self.tasks.len() - 1;
        self.start_agent();
        self.refresh();
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

    /// A seam dragged to column `x`. Between the panes, the agent's share of
    /// the width follows it, kept between a fifth and four fifths; the
    /// tree's seam sets the tree's width, from a dozen columns to half.
    fn resize_to(&mut self, x: u16) {
        use yara_core::settings::Side;
        let body = self.hits.body;
        if body.width == 0 {
            return;
        }
        let from_left = x.saturating_sub(body.x);
        match self.resizing {
            Some(Seam::Panes) => {
                let percent = from_left as u32 * 100 / body.width as u32;
                let agent = match self.settings.agent_side {
                    Side::Left => percent,
                    Side::Right => 100 - percent.min(100),
                };
                self.settings.agent_width = agent.clamp(20, 80) as u16;
            }
            Some(Seam::Tree) => {
                let width = match self.settings.agent_side {
                    Side::Left => body.width.saturating_sub(from_left + 1),
                    Side::Right => from_left,
                };
                self.settings.sidebar_width = width.clamp(12, body.width / 2);
            }
            None => {}
        }
        self.dirty.store(true, Ordering::Relaxed);
    }

    /// The text the mouse dragged over, read off the last frame row by row,
    /// trailing blanks dropped. In the file being edited the gutter is left
    /// out, so what is copied is the code.
    pub fn selected_text(&self) -> Option<String> {
        let ((x0, y0), (x1, y1)) = self.selection?;
        if (x0, y0) == (x1, y1) {
            return None;
        }
        let pane = self.selection_bounds()?;
        let (top, bottom) = (y0.min(y1), y0.max(y1).min(pane.bottom().saturating_sub(1)));
        let (left, right) = if y0 == y1 {
            (x0.min(x1), x0.max(x1))
        } else {
            (pane.x, pane.right().saturating_sub(1))
        };
        let (left, right) = (left.max(pane.x), right.min(pane.right().saturating_sub(1)));
        let lines: Vec<String> = (top..=bottom)
            .filter_map(|y| {
                let row: Vec<char> = self.last_frame.get(y as usize)?.chars().collect();
                let to = (right as usize).min(row.len().saturating_sub(1));
                let text: String = row.get(left as usize..=to).unwrap_or(&[]).iter().collect();
                Some(text.trim_end().to_string())
            })
            .collect();
        Some(lines.join("\n"))
    }

    fn copy(&mut self) {
        let Some(text) = self.selected_text() else {
            self.note = Some("nothing selected".into());
            return;
        };
        if !yara_core::clipboard::copy(&text) {
            self.osc52 = Some(yara_core::clipboard::osc52(&text));
        }
        self.note = Some(format!("copied {} lines", text.lines().count().max(1)));
        self.selection = None;
    }

    fn paste(&mut self) {
        let Some(text) = yara_core::clipboard::paste() else {
            self.note = Some("nothing to paste".into());
            return;
        };
        self.caret_moved = true;
        if let Some(buffer) = self.editor.as_mut() {
            buffer.insert(&text.replace("\r\n", "\n"));
        }
    }

    /// The wheel, over whatever is under it: the agent's own scrollback, the
    /// diff, the file being edited, the tree, a list.
    fn wheel(&mut self, up: bool, x: u16, y: u16) {
        let step = |value: usize| {
            if up {
                value.saturating_sub(3)
            } else {
                value + 3
            }
        };
        match self.overlay.clone() {
            Some(Overlay::Keys(scroll)) => {
                self.overlay = Some(Overlay::Keys(step(scroll).min(command::ALL.len() - 1)))
            }
            Some(Overlay::Changes(row)) => {
                let last = self.change_rows().len().saturating_sub(1);
                self.overlay = Some(Overlay::Changes(if up {
                    row.saturating_sub(1)
                } else {
                    (row + 1).min(last)
                }))
            }
            Some(_) => {}
            None if hit(self.hits.agent, x, y) => {
                let grid = self.hits.agent;
                if let Some(pty) = self.agent.as_mut() {
                    pty.wheel(
                        up,
                        y.saturating_sub(grid.y + 1),
                        x.saturating_sub(grid.x + 1),
                    );
                }
            }
            None if hit(self.hits.follow, x, y) => {
                self.scroll = step(self.scroll as usize) as u16;
                self.caret_moved = false;
            }
            None if hit(self.hits.files, x, y) => {
                if let Some(tree) = self.tree.as_mut() {
                    tree.move_selection(if up { -3 } else { 3 });
                }
            }
            None => {}
        }
    }

    /// A click. Overlays take the click if it lands in their box and close
    /// on the backdrop; otherwise the chrome answers to it.
    pub fn handle_mouse(&mut self, mouse: MouseEvent) {
        let (x, y) = (mouse.column, mouse.row);
        if self.hover != Some((x, y)) {
            self.hover = Some((x, y));
            self.dirty.store(true, Ordering::Relaxed);
        }
        let up = match mouse.kind {
            MouseEventKind::ScrollUp => Some(true),
            MouseEventKind::ScrollDown => Some(false),
            MouseEventKind::Drag(MouseButton::Left) if self.resizing.is_some() => {
                self.resize_to(x);
                return;
            }
            MouseEventKind::Drag(MouseButton::Left) if self.dragging_tab.is_some() => {
                // The tab travels along the strip: over another tab, it
                // takes that tab's place.
                let from = self.dragging_tab.unwrap();
                let over = self
                    .hits
                    .tabs
                    .iter()
                    .find(|(r, _)| hit(*r, x, y))
                    .map(|(_, i)| *i);
                if let Some(to) = over.filter(|to| *to != from) {
                    let session = self.tasks.remove(from);
                    self.tasks.insert(to, session);
                    self.active = to;
                    self.dragging_tab = Some(to);
                    self.dirty.store(true, Ordering::Relaxed);
                }
                return;
            }
            MouseEventKind::Drag(MouseButton::Left) => {
                if let Some((_, end)) = self.selection.as_mut() {
                    *end = (x, y);
                }
                return;
            }
            MouseEventKind::Up(MouseButton::Left) if self.dragging_tab.is_some() => {
                self.dragging_tab = None;
                return;
            }
            MouseEventKind::Up(MouseButton::Left) if self.resizing.is_some() => {
                self.resizing = None;
                let _ = self.settings.save();
                return;
            }
            MouseEventKind::Down(MouseButton::Left) if hit(self.hits.seam, x, y) => {
                self.resizing = Some(Seam::Panes);
                return;
            }
            MouseEventKind::Down(MouseButton::Left) if hit(self.hits.tree_seam, x, y) => {
                self.resizing = Some(Seam::Tree);
                return;
            }
            MouseEventKind::Down(MouseButton::Left) => {
                // A press starts a selection; it becomes one when dragged.
                self.selection = Some(((x, y), (x, y)));
                None
            }
            MouseEventKind::Down(MouseButton::Right) => {
                let tab = self
                    .hits
                    .tabs
                    .iter()
                    .find(|(r, _)| hit(*r, x, y))
                    .map(|(_, i)| *i);
                if let Some(tab) = tab {
                    self.overlay = Some(Overlay::TabMenu(tab, 0));
                } else if hit(self.hits.files, x, y) {
                    // The row under the pointer is where a new file goes.
                    let row = self
                        .hits
                        .file_rows
                        .iter()
                        .find(|(r, _)| hit(*r, x, y))
                        .map(|(_, i)| *i);
                    self.focus = Focus::Files;
                    if let Some((tree, row)) = self.tree.as_mut().zip(row) {
                        tree.selected = row;
                    }
                    self.overlay = Some(Overlay::TreeMenu(0));
                }
                return;
            }
            _ => return,
        };
        if let Some(up) = up {
            self.wheel(up, x, y);
            return;
        }
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
                        if let Some(folders) = self.settings.recent_workspaces.get(row).cloned() {
                            self.open_workspace(folders);
                        }
                        None
                    }
                    Some(Overlay::Themes(_)) => {
                        self.set_theme(row);
                        None
                    }
                    Some(Overlay::Folders(_)) => {
                        self.remove_folder(row);
                        None
                    }
                    Some(Overlay::TabMenu(tab, _)) => {
                        self.tab_menu_pick(tab, row);
                        return;
                    }
                    Some(Overlay::TreeMenu(_)) => {
                        self.overlay = None;
                        self.execute(TREE_MENU[row.min(TREE_MENU.len() - 1)]);
                        return;
                    }
                    Some(Overlay::AddFolder { dir, filter, .. }) => {
                        self.overlay = Some(Overlay::AddFolder { dir, row, filter });
                        self.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
                        return;
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
            self.dragging_tab = Some(tab);
            self.ensure_agent();
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
            if self.editor.is_some() {
                self.focus = Focus::Editor;
                if hit(self.hits.editor, x, y) {
                    let line = self.scroll as usize + (y - self.hits.editor.y) as usize;
                    let col = (x - self.hits.editor.x) as usize;
                    self.caret_moved = true;
                    if let Some(buffer) = self.editor.as_mut() {
                        buffer.goto(line, col);
                    }
                }
            } else {
                self.focus = Focus::Follow;
            }
        }
    }

    /// The pane a selection lives in: it never crosses into the next one.
    pub fn selection_bounds(&self) -> Option<Rect> {
        let ((x, y), _) = self.selection?;
        [
            self.hits.editor,
            self.hits.agent,
            self.hits.follow,
            self.hits.files,
        ]
        .into_iter()
        .find(|r| hit(*r, x, y))
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
        let several = self.folders.len() > 1;
        let mut hits = Vec::new();
        let mut files = 0;
        for folder in &self.folders {
            let (found, in_files) =
                search::search(&folder.path, query, &self.settings.search_exclude, 500);
            files += in_files;
            let name = folder.name();
            hits.extend(found.into_iter().map(|mut hit| {
                if several {
                    hit.path = format!("{name}/{}", hit.path);
                }
                hit
            }));
        }
        (hits, files)
    }

    /// The folder a path from a hit or the finder came from, and the path
    /// inside it — the name the list showed carries the folder.
    fn resolve(&self, shown: &str) -> Option<PathBuf> {
        if self.folders.len() > 1 {
            let (name, rest) = shown.split_once('/')?;
            let folder = self.folders.iter().find(|f| f.name() == name)?;
            Some(folder.path.join(rest))
        } else {
            Some(self.folders.first()?.path.join(shown))
        }
    }

    /// Runs the agent from the settings in the session's folder — or where
    /// the editor was started, with no project. A failure is a note.
    pub fn start_agent(&mut self) {
        let cwd = self
            .project()
            .map(|p| p.to_path_buf())
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

    /// One step of the caret's blink; a key press lands it on again so the
    /// caret is there when it has just moved.
    pub fn blink(&mut self) {
        self.caret_on = !self.caret_on;
        self.dirty.store(true, Ordering::Relaxed);
    }

    /// Takes the dirty flag, so a frame is drawn once per change.
    pub fn take_dirty(&self) -> bool {
        self.dirty.swap(false, Ordering::Relaxed)
    }

    /// Looks at every session's working tree: new edits join its timeline,
    /// its CHANGES list is brought up to date. Called every `refresh_ms`.
    pub fn refresh(&mut self) {
        for session in &mut self.tasks {
            let mut edits = Vec::new();
            for folder in &mut session.folders {
                // Only a folder that moved is worth reading, or asking git.
                if !folder.watcher.moved(&folder.path) {
                    continue;
                }
                edits.extend(folder.watcher.poll(&folder.path, folder.repo.as_ref()));
                if let Some(repo) = &folder.repo {
                    folder.changes = git::changes(repo).unwrap_or_default();
                }
            }
            for edit in edits {
                session.follow.push(edit);
            }
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
        let rows = self.change_rows();
        let Some(ChangeRow::File(folder, change)) = rows.get(row).cloned() else {
            return;
        };
        let Some(repo) = self.folders[folder].repo.clone() else {
            return;
        };
        match git::file_diff(&repo, &change.path) {
            Ok(diff) => {
                self.pinned = Some(EditEvent::from_unified(repo.root.join(&change.path), &diff))
            }
            Err(e) => self.note = Some(e),
        }
        self.overlay = None;
    }

    /// Opens a file in the follow pane's place, the keyboard on it.
    pub fn open_file(&mut self, path: &std::path::Path) {
        // The file already open, and untouched, stays where it was scrolled.
        if self
            .editor
            .as_ref()
            .is_some_and(|b| b.path == path && !b.modified())
        {
            self.focus = Focus::Editor;
            return;
        }
        match Buffer::open(path) {
            Ok(buffer) => {
                self.editor = Some(buffer);
                self.scroll = 0;
                self.caret_moved = true;
                self.focus = Focus::Editor;
                if let Some(tree) = self.tree.as_mut() {
                    tree.reveal(path);
                }
            }
            Err(e) => self.note = Some(format!("{}: {e}", path.display())),
        }
    }

    /// One of the tab menu's rows, chosen for `tab`.
    fn tab_menu_pick(&mut self, tab: usize, row: usize) {
        self.overlay = None;
        self.active = tab.min(self.tasks.len() - 1);
        match TAB_MENU[row.min(TAB_MENU.len() - 1)] {
            "Rename…" => self.execute(Command::RenameTab),
            "Add Folder to Task…" => self.execute(Command::AddFolder),
            "Delete Task" => self.delete_task(),
            _ => self.execute(Command::CloseTab),
        }
    }

    /// Closes the task and takes its worktrees off the disk with it. A
    /// folder that is a repository's own working copy is left where it is:
    /// that is the project, not the task.
    fn delete_task(&mut self) {
        let worktrees: Vec<Repo> = self
            .folders
            .iter()
            .filter_map(|f| f.repo.clone())
            .filter(|repo| repo.worktree.is_some())
            .collect();
        if worktrees.is_empty() {
            self.note = Some("no worktree of this task's own to remove".into());
            return;
        }
        self.agent = None;
        let mut removed = 0;
        for repo in &worktrees {
            match git::worktree_remove(repo, &repo.root) {
                Ok(()) => removed += 1,
                Err(e) => self.note = Some(e),
            }
        }
        if removed > 0 {
            self.note = Some(match removed {
                1 => format!("removed {}", worktrees[0].root.display()),
                n => format!("removed {n} worktrees"),
            });
            self.execute(Command::CloseTab);
        }
    }

    /// Closes the file — or the editor, with `quit` — once it is safe to.
    fn close_file(&mut self, quit: bool) {
        self.editor = None;
        self.focus = Focus::Follow;
        if quit {
            self.should_quit = true;
        }
    }

    /// Whether the file may go: a dirty one asks first, and the answer is
    /// what closes it.
    fn may_close(&mut self, quit: bool) -> bool {
        if self.editor.as_ref().is_some_and(|b| b.modified()) {
            self.overlay = Some(Overlay::CloseFile { quit });
            return false;
        }
        true
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
        // Every folder's files, each named by its folder when there are
        // several, so a hit says which repository it is in.
        let several = self.folders.len() > 1;
        let files: Vec<String> = self
            .folders
            .iter()
            .flat_map(|folder| {
                let name = folder.name();
                tree::all_files(&folder.path).into_iter().map(move |path| {
                    if several {
                        format!("{name}/{path}")
                    } else {
                        path
                    }
                })
            })
            .collect();
        fuzzy::rank(query, files.iter().map(String::as_str))
            .into_iter()
            .take(50)
            .map(|(_, s)| s.to_string())
            .collect()
    }

    /// A new, empty file in the folder under the FILES cursor — or the
    /// project root — opened for editing. Nothing is overwritten.
    fn new_entry(&mut self, name: &str, folder: bool) {
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
            None => match self.project() {
                Some(root) => root.to_path_buf(),
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
        let made = if folder {
            std::fs::create_dir_all(&path)
        } else {
            std::fs::write(&path, "")
        };
        match made {
            Ok(()) => {
                if let Some(tree) = self.tree.as_mut() {
                    tree.rebuild();
                    tree.reveal(&path);
                }
                if !folder {
                    self.open_file(&path);
                }
            }
            Err(e) => self.note = Some(format!("{}: {e}", path.display())),
        }
    }

    pub fn handle_key(&mut self, key: KeyEvent) {
        self.note = None;
        self.caret_on = true;
        // Typing drops a selection, as it does everywhere; a copy is the one
        // key that wants it.
        let chord_now = chord_of(key);
        if chord_now.as_ref().and_then(|c| self.settings.command(c)) != Some(Command::Copy) {
            self.selection = None;
        }
        let Some(chord) = chord_of(key) else { return };
        let closes = self.settings.command(&chord) == Some(Command::Close);
        match self.overlay.clone() {
            Some(Overlay::Changes(row)) => {
                match key.code {
                    KeyCode::Up => self.overlay = Some(Overlay::Changes(row.saturating_sub(1))),
                    KeyCode::Down => {
                        let last = self.change_rows().len().saturating_sub(1);
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
                        let found = hits
                            .get(row)
                            .and_then(|h| Some((self.resolve(&h.path)?, h.line)));
                        if let Some((path, line)) = found {
                            self.open_file(&path);
                            if let Some(buffer) = self.editor.as_mut() {
                                for _ in 1..line {
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
            Some(Overlay::AddFolder {
                dir,
                row,
                mut filter,
            }) => {
                let entries = Self::browse_entries(&dir, &filter);
                let last = entries.len() + 1;
                let go = |app: &mut Self, row: usize| match row {
                    0 => {
                        app.overlay = None;
                        let path = dir.to_string_lossy().into_owned();
                        app.add_folder(&path);
                    }
                    1 => {
                        let up = dir.parent().unwrap_or(&dir).to_path_buf();
                        app.overlay = Some(Overlay::AddFolder {
                            dir: up,
                            row: 0,
                            filter: String::new(),
                        });
                    }
                    n => {
                        app.overlay = Some(Overlay::AddFolder {
                            dir: entries[n - 2].clone(),
                            row: 0,
                            filter: String::new(),
                        })
                    }
                };
                match key.code {
                    KeyCode::Up => {
                        self.overlay = Some(Overlay::AddFolder {
                            dir,
                            row: row.saturating_sub(1),
                            filter,
                        })
                    }
                    KeyCode::Down => {
                        self.overlay = Some(Overlay::AddFolder {
                            dir,
                            row: (row + 1).min(last),
                            filter,
                        })
                    }
                    KeyCode::Enter | KeyCode::Right => go(self, row.min(last)),
                    KeyCode::Left => go(self, 1),
                    _ if closes => self.overlay = None,
                    KeyCode::Backspace => {
                        if filter.pop().is_none() {
                            go(self, 1);
                        } else {
                            self.overlay = Some(Overlay::AddFolder {
                                dir,
                                row: 2,
                                filter,
                            });
                        }
                    }
                    KeyCode::Char(c) if !chord.mods.ctrl && !chord.mods.alt => {
                        filter.push(c);
                        self.overlay = Some(Overlay::AddFolder {
                            dir,
                            row: 2,
                            filter,
                        });
                    }
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
            Some(Overlay::TreeMenu(row)) => {
                match key.code {
                    KeyCode::Up => self.overlay = Some(Overlay::TreeMenu(row.saturating_sub(1))),
                    KeyCode::Down => {
                        self.overlay = Some(Overlay::TreeMenu((row + 1).min(TREE_MENU.len() - 1)))
                    }
                    KeyCode::Enter => {
                        self.overlay = None;
                        self.execute(TREE_MENU[row.min(TREE_MENU.len() - 1)]);
                    }
                    _ if closes => self.overlay = None,
                    _ => {}
                }
                return;
            }
            Some(Overlay::TabMenu(tab, row)) => {
                match key.code {
                    KeyCode::Up => {
                        self.overlay = Some(Overlay::TabMenu(tab, row.saturating_sub(1)))
                    }
                    KeyCode::Down => {
                        self.overlay =
                            Some(Overlay::TabMenu(tab, (row + 1).min(TAB_MENU.len() - 1)))
                    }
                    KeyCode::Enter => self.tab_menu_pick(tab, row),
                    _ if closes => self.overlay = None,
                    _ => {}
                }
                return;
            }
            Some(Overlay::CloseFile { quit }) => {
                match key.code {
                    KeyCode::Char('y' | 'Y') | KeyCode::Enter => {
                        self.overlay = None;
                        self.save();
                        if self.editor.as_ref().is_some_and(|b| !b.modified()) {
                            self.close_file(quit);
                        }
                    }
                    KeyCode::Char('n' | 'N') => {
                        self.overlay = None;
                        self.close_file(quit);
                    }
                    _ if closes => self.overlay = None,
                    _ => {}
                }
                return;
            }
            Some(Overlay::Folders(row)) => {
                match key.code {
                    KeyCode::Up => self.overlay = Some(Overlay::Folders(row.saturating_sub(1))),
                    KeyCode::Down => {
                        let last = self.workspace.len().saturating_sub(1);
                        self.overlay = Some(Overlay::Folders((row + 1).min(last)))
                    }
                    KeyCode::Enter => {
                        self.overlay = None;
                        self.remove_folder(row);
                    }
                    _ if closes => self.overlay = None,
                    _ => {}
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
                        let last = self.settings.recent_workspaces.len().saturating_sub(1);
                        self.overlay = Some(Overlay::Recent((row + 1).min(last)))
                    }
                    KeyCode::Enter => {
                        self.overlay = None;
                        if let Some(folders) = self.settings.recent_workspaces.get(row).cloned() {
                            self.open_workspace(folders);
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
                        if let Some(path) = self
                            .quick_open_hits(&query)
                            .get(row)
                            .and_then(|hit| self.resolve(hit))
                        {
                            self.open_file(&path);
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
                | Overlay::NewFolder(mut text),
            ) => {
                let again = |text: String| match self.overlay.as_ref().unwrap() {
                    Overlay::NewTab(_) => Overlay::NewTab(text),
                    Overlay::RenameTab(_) => Overlay::RenameTab(text),
                    Overlay::NewFolder(_) => Overlay::NewFolder(text),
                    _ => Overlay::NewFile(text),
                };
                match key.code {
                    KeyCode::Enter => {
                        let which = self.overlay.take().unwrap();
                        let text = text.trim().to_string();
                        match which {
                            Overlay::NewTab(_) => self.new_tab(&text),
                            Overlay::RenameTab(_) => self.name = (!text.is_empty()).then_some(text),
                            Overlay::NewFolder(_) => self.new_entry(&text, true),
                            _ => self.new_entry(&text, false),
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
        // The start page is what a folderless editor shows; a file open
        // over it — the settings, most often — takes the keys instead.
        if self.project().is_none()
            && self.editor.is_none()
            && command.is_none_or(|c| c == Command::MarkReviewed)
        {
            let last = self.settings.recent_workspaces.len().saturating_sub(1);
            match key.code {
                KeyCode::Up => self.start_row = self.start_row.saturating_sub(1),
                KeyCode::Down => self.start_row = (self.start_row + 1).min(last),
                KeyCode::Enter => {
                    let chosen = self.settings.recent_workspaces.get(self.start_row).cloned();
                    if let Some(folders) = chosen {
                        self.open_workspace(folders);
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
            // Ctrl+C with something selected is a copy; with nothing it
            // stays the interrupt every program expects.
            if command == Some(Command::Copy) && self.selected_text().is_some() {
                self.copy();
                return;
            }
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
            // In a file, every key is typing but Escape, which closes it,
            // and the bound Ctrl and Alt chords and function keys, which
            // are the editor's wherever the keyboard is.
            let editors_own = command == Some(Command::Close)
                || (command.is_some()
                    && (chord.mods.ctrl || chord.mods.alt || shell_has_no_use_for(&chord)));
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
        self.caret_moved = true;
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
            Command::Quit => {
                // Any workspace with an unsaved file asks before the editor goes.
                let dirty = self
                    .tasks
                    .iter()
                    .position(|s| s.editor.as_ref().is_some_and(|b| b.modified()));
                match dirty {
                    Some(i) => {
                        self.active = i;
                        self.may_close(true);
                    }
                    None => self.should_quit = true,
                }
            }
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
                self.caret_moved = true;
                if let Some(b) = self.editor.as_mut() {
                    b.undo()
                }
            }
            Command::Redo => {
                self.caret_moved = true;
                if let Some(b) = self.editor.as_mut() {
                    b.redo()
                }
            }
            Command::SwapPanes => {
                use yara_core::settings::Side;
                self.settings.agent_side = match self.settings.agent_side {
                    Side::Left => Side::Right,
                    Side::Right => Side::Left,
                };
                let _ = self.settings.save();
            }
            Command::Copy => self.copy(),
            Command::Paste => self.paste(),
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
            Command::NewFolder => self.overlay = Some(Overlay::NewFolder(String::new())),
            Command::RemoveFolder if self.workspace.is_empty() => {
                self.note = Some("this workspace has no folders yet".into())
            }
            Command::RemoveFolder => self.overlay = Some(Overlay::Folders(0)),
            Command::AddFolder => {
                self.overlay = Some(Overlay::AddFolder {
                    dir: self.browse_from(),
                    row: 0,
                    filter: String::new(),
                })
            }
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
            // A task is its folders, so the first thing a task without any
            // needs is one.
            Command::NewTab if self.project().is_none() => self.execute(Command::AddFolder),
            Command::NewTab => self.overlay = Some(Overlay::NewTab(String::new())),
            Command::RenameTab => self.overlay = Some(Overlay::RenameTab(String::new())),
            Command::CloseTab => {
                if self.tasks.len() > 1 {
                    self.tasks.remove(self.active);
                    self.active = self.active.min(self.tasks.len() - 1);
                } else {
                    self.should_quit = true;
                }
            }
            Command::NextTab => {
                self.active = (self.active + 1) % self.tasks.len();
                self.ensure_agent();
            }
            Command::PrevTab => {
                self.active = (self.active + self.tasks.len() - 1) % self.tasks.len();
                self.ensure_agent();
            }
            Command::Close if self.editor.is_some() => {
                if self.may_close(false) {
                    self.close_file(false);
                }
            }
            Command::Close => self.pinned = None,
            Command::ToggleView => {
                self.scroll = 0;
                self.view = match self.view {
                    View::Diff => View::File,
                    View::File => View::Diff,
                }
            }
            Command::FollowLive => {
                self.pinned = None;
                self.scroll = 0;
                self.follow.go_live()
            }
            Command::ScrubBack => {
                self.pinned = None;
                self.scroll = 0;
                self.follow.scrub_back()
            }
            Command::ScrubForward => {
                self.pinned = None;
                self.scroll = 0;
                self.follow.scrub_forward()
            }
            Command::MarkReviewed => {
                self.pinned = None;
                self.scroll = 0;
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

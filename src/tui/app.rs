//! TUI state and event loop.

use std::collections::{BTreeMap, BTreeSet};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use crossterm::event::{
    self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent,
    MouseEventKind,
};
use ratatui::backend::Backend;
use ratatui::layout::Rect;
use ratatui::Terminal;

use crate::core::buffer::{word_at, Buffers};
use crate::core::command::{Chord, Command, Key, Mods, FILE_MENU, HELP_MENU, VIEW_MENU};
use crate::core::diff;
use crate::core::find::Find;
use crate::core::fold::{self, Region};
use crate::core::fs_ops;
use crate::core::git::{self as core_git, GitState};
use crate::core::history::EditKind;
use crate::core::indent;
use crate::core::project::Project;
use crate::core::search::{self, Candidate, Field as SearchField, Search};
use crate::core::settings::{Modifier, Settings};
use crate::core::syntax::Syntax;
use crate::core::theme::{self as core_theme, Theme};
use crate::tui::clipboard::Clipboard;
use crate::tui::icons::{self, Icons};
use crate::tui::menu::{Menu, MenuItem};
use crate::tui::shell::Shell;
use crate::tui::tree::Tree;
use crate::tui::ui;

/// Which pane border is being dragged.
#[derive(PartialEq, Clone, Copy)]
pub enum Splitter {
    Sidebar,
    Shell,
}

#[derive(PartialEq, Clone, Copy)]
pub enum Focus {
    Tree,
    Editor,
    Search,
    Git,
    Find,
    /// The side-by-side diff, which takes the editor's place while it is open.
    Diff,
    Shell,
}

/// A changed file shown side by side: what it was on the left, what it is now
/// on the right.
pub struct Diff {
    /// Path as git reports it, relative to the worktree.
    pub path: String,
    pub rows: Vec<diff::Row>,
    /// First row drawn.
    pub scroll: usize,
}

#[derive(PartialEq, Clone, Copy)]
pub enum SidebarView {
    Files,
    Search,
    Git,
}

/// What a folder browser is being opened for.
#[derive(Clone, Copy, PartialEq)]
pub enum Pick {
    OpenFile,
    OpenFolder,
    AddFolder,
}

/// The menus in the top bar, in the order they are drawn.
#[derive(Clone, Copy, PartialEq)]
pub enum MenuBar {
    File,
    View,
    Help,
}

/// Which strip a dragged tab came from — editor tabs and terminal tabs move
/// within their own strip only.
#[derive(Clone, Copy, PartialEq)]
pub enum TabStrip {
    Editor,
    Terminal,
}

/// A modal question or single-line input laid over the layout.
pub enum Prompt {
    NewFile(PathBuf),
    NewDir(PathBuf),
    Rename(PathBuf),
    MoveTo(PathBuf),
    ConfirmDelete(PathBuf),
    /// Closing buffer `index`, which has unsaved changes.
    ConfirmClose {
        index: usize,
        name: String,
    },
    GitRepo,
    GitWorktree,
    OpenPath,
    OpenFolder,
    /// Typed-path counterpart of the browser's "add folder".
    AddFolderPath,
    /// Renaming terminal session `index`, from its tab's right-click.
    RenameTerminal(usize),
    SaveAs,
    /// The built-in file browser: the terminal frontend's stand-in for the
    /// window's native Open dialog.
    Browse {
        pick: Pick,
        dir: PathBuf,
        /// What the browser lists: folders, plus files when picking a file.
        entries: Vec<(PathBuf, bool)>,
    },
    Themes,
    Recent,
    Help(Vec<String>),
    Goto {
        word: String,
        is_definition: bool,
        candidates: Vec<Candidate>,
    },
}

impl Prompt {
    pub fn title(&self) -> String {
        match self {
            Self::NewFile(dir) => format!("New file in {}", short(dir)),
            Self::NewDir(dir) => format!("New folder in {}", short(dir)),
            Self::Rename(path) => format!("Rename {}", short(path)),
            Self::MoveTo(path) => format!("Move {} into folder", short(path)),
            Self::ConfirmDelete(path) => format!("Delete {}?  (y / n)", short(path)),
            Self::ConfirmClose { name, .. } => {
                format!("Save changes to {name}?  (y = save / n = discard / esc = cancel)")
            }
            Self::GitRepo => "Repository".to_string(),
            Self::GitWorktree => "Worktree".to_string(),
            Self::OpenPath => "Open file (path relative to the project)".to_string(),
            Self::OpenFolder => "Open folder as project".to_string(),
            Self::AddFolderPath => "Add folder to project (path)".to_string(),
            Self::RenameTerminal(index) => format!("Rename terminal {}", index + 1),
            Self::Browse { pick, dir, .. } => {
                let what = match pick {
                    Pick::OpenFile => "Open file",
                    Pick::OpenFolder => "Open folder as project",
                    Pick::AddFolder => "Add folder to project",
                };
                format!(
                    "{what} — {}  (→ enter · ← up · ⏎ pick · tab type)",
                    short(dir)
                )
            }
            Self::SaveAs => "Save as (path relative to the project)".to_string(),
            Self::Themes => "Color theme".to_string(),
            Self::Recent => "Recent projects".to_string(),
            Self::Help(_) => "Key bindings".to_string(),
            Self::Goto {
                word,
                is_definition,
                ..
            } => format!(
                "{} \"{}\"",
                if *is_definition {
                    "Definitions of"
                } else {
                    "References to"
                },
                word
            ),
        }
    }

    /// Whether this prompt takes typed text (as opposed to a selection).
    pub fn is_input(&self) -> bool {
        matches!(
            self,
            Self::NewFile(_)
                | Self::NewDir(_)
                | Self::Rename(_)
                | Self::MoveTo(_)
                | Self::OpenPath
                | Self::OpenFolder
                | Self::AddFolderPath
                | Self::RenameTerminal(_)
                | Self::SaveAs
        )
    }

    pub fn list_len(&self, themes: usize, recent: usize, repos: usize, worktrees: usize) -> usize {
        match self {
            Self::Themes => themes,
            Self::Recent => recent,
            Self::Help(entries) => entries.len(),
            Self::Goto { candidates, .. } => candidates.len(),
            Self::GitRepo => repos,
            Self::GitWorktree => worktrees,
            Self::Browse { dir, entries, .. } => {
                entries.len() + usize::from(dir.parent().is_some())
            }
            _ => 0,
        }
    }
}

/// What the file browser lists in `dir`: folders always, files only when a
/// file is what is being picked.
fn browse_entries(dir: &Path, pick: Pick) -> Vec<(PathBuf, bool)> {
    fs_ops::list_dir(dir)
        .into_iter()
        .filter(|(_, is_dir)| *is_dir || pick == Pick::OpenFile)
        .collect()
}

/// Translates a crossterm key event into a core chord, so the settings map can
/// be consulted. Terminals never report Cmd, so it is left unset.
fn chord_of(key: KeyEvent) -> Option<Chord> {
    let mods = Mods {
        cmd: false,
        ctrl: key.modifiers.contains(KeyModifiers::CONTROL),
        alt: key.modifiers.contains(KeyModifiers::ALT),
        shift: key.modifiers.contains(KeyModifiers::SHIFT),
    };
    let named = !matches!(key.code, KeyCode::Char(_));
    // A bare printable key is ordinary typing, not a shortcut; a named one
    // (F2, Tab, Delete…) can be a binding on its own.
    if !mods.ctrl && !mods.alt && !named {
        return None;
    }
    let key = match key.code {
        KeyCode::Char(c) => Key::Char(c.to_ascii_lowercase()),
        KeyCode::Left => Key::Named("left".into()),
        KeyCode::Right => Key::Named("right".into()),
        KeyCode::Up => Key::Named("up".into()),
        KeyCode::Down => Key::Named("down".into()),
        KeyCode::Enter => Key::Named("enter".into()),
        KeyCode::Tab => Key::Named("tab".into()),
        KeyCode::Esc => Key::Named("esc".into()),
        KeyCode::Home => Key::Named("home".into()),
        KeyCode::End => Key::Named("end".into()),
        KeyCode::PageUp => Key::Named("pageup".into()),
        KeyCode::PageDown => Key::Named("pagedown".into()),
        KeyCode::Delete => Key::Named("delete".into()),
        KeyCode::Backspace => Key::Named("backspace".into()),
        KeyCode::F(n) => Key::Named(format!("f{n}")),
        _ => return None,
    };
    Some(Chord { mods, key })
}

fn short(path: &std::path::Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}

/// Where each pane landed in the last frame, so mouse coordinates can be
/// mapped back to what the user clicked.
#[derive(Default, Clone)]
pub struct Layout {
    pub sidebar: Rect,
    pub sidebar_header: Rect,
    pub tree: Rect,
    /// Find bar hit boxes, so the whole strip works with the mouse.
    pub find_replace_one: Rect,
    pub find_replace_all: Rect,
    pub find_query: Rect,
    pub find_case: Rect,
    pub find_word: Rect,
    pub find_regex: Rect,
    pub find_prev: Rect,
    pub find_next: Rect,
    pub find_close: Rect,
    pub find_replace: Rect,
    pub search_options: Rect,
    pub search_toggles: Rect,
    pub search_action: Rect,
    pub search_exclude: Rect,
    pub search_replace: Rect,
    pub search_input: Rect,
    pub search_list: Rect,
    /// One entry per drawn search row: the flat match index, or `None` for a
    /// file heading.
    pub search_rows: Vec<Option<usize>>,
    /// The draggable border between the sidebar and the editor.
    pub v_split: Rect,
    /// The draggable border above the terminal panel.
    pub h_split: Rect,
    /// The region the panes share, used to clamp a drag.
    pub body: Rect,
    pub tab_files: Rect,
    pub tab_search: Rect,
    pub tab_git: Rect,
    /// Git view rows: the repository and worktree pickers, then the changes.
    pub git_repo: Rect,
    pub git_worktree: Rect,
    pub git_list: Rect,
    /// First change row currently drawn, for mapping clicks when scrolled.
    pub git_list_offset: usize,
    pub tabs: Rect,
    /// (start_x, end_x, buffer index, is_close_button) for each drawn tab.
    pub tab_spans: Vec<(u16, u16, usize, bool)>,
    pub editor: Rect,
    pub gutter: u16,
    pub shell: Rect,
    /// (start_x, end_x, session index, is_close_button) for each shell tab in
    /// the panel header.
    pub shell_tabs: Vec<(u16, u16, usize, bool)>,
    /// Diff tabs in the editor strip: (start, end, index, is_close).
    pub diff_tabs: Vec<(u16, u16, usize, bool)>,
    /// The `+` button in the shell header that opens another session.
    pub shell_new: Rect,
    /// The theme name in the status bar; clicking it opens the theme picker,
    /// as in the window frontend.
    pub status_theme: Rect,
    pub prompt_list: Rect,
    pub menu: Rect,
}

fn hits(rect: Rect, x: u16, y: u16) -> bool {
    rect.width > 0
        && rect.height > 0
        && x >= rect.x
        && x < rect.x + rect.width
        && y >= rect.y
        && y < rect.y + rect.height
}

/// Per-buffer editing state the core doesn't need to know about.
#[derive(Default)]
pub struct EditState {
    pub line: usize,
    pub col: usize,
    /// Index into the visible-line list, not a raw line number.
    pub scroll: usize,
    /// Column the cursor tries to return to when moving vertically.
    pub goal_col: usize,
    /// Header lines whose blocks are collapsed.
    pub folds: BTreeSet<usize>,
    /// Where a selection started, as a character offset; the cursor is its
    /// other end.
    pub anchor: Option<usize>,
}

pub struct App {
    pub project: Project,
    pub tree: Tree,
    pub buffers: Buffers,
    pub edit: Vec<EditState>,
    pub search: Search,
    pub git: GitState,
    /// Open diffs, tabs of their own beside the files.
    pub diffs: Vec<Diff>,
    /// Which diff tab is in front; `None` while a file is.
    pub active_diff: Option<usize>,
    /// Who last touched the line the cursor is on, and what it was read for.
    pub blame: Option<core_git::Blame>,
    blame_key: Option<(PathBuf, usize)>,
    /// Changed lines of the file in front, for the gutter marks.
    pub git_lines: BTreeMap<usize, core_git::LineState>,
    git_lines_key: Option<(PathBuf, usize)>,
    /// Selected row in the git change list.
    pub git_selected: usize,
    pub shell: Shell,
    /// Raised by the shell's reader thread when there is new output to draw.
    shell_dirty: Arc<AtomicBool>,
    pub syntax: Syntax,
    pub themes: Vec<Theme>,
    pub theme_index: usize,
    pub focus: Focus,
    pub sidebar_view: SidebarView,
    pub show_sidebar: bool,
    pub show_shell: bool,
    pub prompt: Option<Prompt>,
    pub prompt_input: String,
    pub prompt_selected: usize,
    pub search_selected: usize,
    /// Find-in-file bar, the terminal twin of the window's.
    pub find: Find,
    pub status: String,
    pub history: Vec<(PathBuf, usize)>,
    pub icons: Icons,
    pub clipboard: Clipboard,
    /// Pane sizes, dragged by their borders just like in the window frontend.
    pub sidebar_width: u16,
    pub shell_height: u16,
    resizing: Option<Splitter>,
    /// True while the left button is dragging out a selection in the editor.
    selecting: bool,
    pub settings: Settings,
    pub layout: Layout,
    /// Rect of the "File" label in the top bar, for clicking it.
    /// Where the top bar's menus sit, in `MenuBar` order.
    pub menu_buttons: [Rect; 3],
    pub menu: Option<Menu>,
    /// Last pointer position, used for hover highlighting.
    pub mouse: Option<(u16, u16)>,
    /// Identifier under the pointer while a navigation modifier is held, as
    /// (line, start column, end column) — drawn underlined, like ⌘-hover in the
    /// GPU frontend.
    pub link: Option<(usize, usize, usize)>,
    /// Row being dragged in the navigator, once the pointer has actually moved.
    pub drag: Option<PathBuf>,
    pub drag_over: Option<PathBuf>,
    /// A tab being dragged along its strip, and the tab it is over.
    pub tab_drag: Option<(TabStrip, usize)>,
    pub tab_drag_over: Option<usize>,
    /// Navigator row the left button went down on, if any. The click only acts
    /// on release, so a press that turns into a drag doesn't also open or
    /// expand the row.
    press: Option<usize>,
    quit: bool,
    /// Rebuilt whenever the active buffer's text changes.
    pub highlight: Vec<Vec<(crate::core::theme::Rgb, bool, String)>>,
    /// Foldable blocks of the active buffer, rebuilt alongside the highlight.
    pub regions: Vec<Region>,
    highlight_key: Option<(PathBuf, usize, String)>,
}

impl App {
    pub fn new(root: Option<PathBuf>) -> Self {
        let themes = core_theme::load_all();
        let (mut settings, settings_error) = Settings::load();
        if let Some(root) = &root {
            settings.push_recent(root);
            let _ = settings.save();
        }
        let theme_index = themes
            .iter()
            .position(|t| t.name == settings.theme)
            .unwrap_or(0);
        let syntax = Syntax::new(themes.get(theme_index).unwrap_or(&Theme::default()));
        let shell_dirty = Arc::new(AtomicBool::new(false));
        let mut app = Self {
            tree: Tree::with_roots(root.iter().cloned().collect()),
            project: Project::opened(root),
            buffers: Buffers::default(),
            edit: Vec::new(),
            search: Search::default(),
            git: GitState::default(),
            diffs: Vec::new(),
            active_diff: None,
            blame: None,
            blame_key: None,
            git_lines: BTreeMap::new(),
            git_lines_key: None,
            git_selected: 0,
            shell: Shell::new(Arc::clone(&shell_dirty)),
            shell_dirty,
            syntax,
            themes,
            theme_index,
            focus: Focus::Tree,
            sidebar_view: SidebarView::Files,
            show_sidebar: settings.show_sidebar,
            show_shell: settings.show_terminal,
            prompt: None,
            prompt_input: String::new(),
            prompt_selected: 0,
            search_selected: 0,
            find: Find::default(),
            status: String::new(),
            history: Vec::new(),
            icons: icons::detect(),
            clipboard: Clipboard::default(),
            sidebar_width: 30,
            shell_height: 10,
            resizing: None,
            selecting: false,
            settings,
            layout: Layout::default(),
            menu_buttons: [Rect::default(); 3],
            menu: None,
            mouse: None,
            link: None,
            drag: None,
            drag_over: None,
            tab_drag: None,
            tab_drag_over: None,
            press: None,
            quit: false,
            highlight: Vec::new(),
            regions: Vec::new(),
            highlight_key: None,
        };
        app.status = settings_error.unwrap_or_default();
        app
    }

    pub fn theme(&self) -> &Theme {
        &self.themes[self.theme_index]
    }

    pub fn run<B: Backend>(mut self, terminal: &mut Terminal<B>) -> io::Result<()> {
        let mut redraw = true;
        while !self.quit {
            // The git view re-polls `git status` on a timer, so it needs a
            // frame even when nothing else happened.
            if self.show_sidebar && self.sidebar_view == SidebarView::Git && self.git.stale() {
                redraw = true;
            }
            if redraw || self.shell_dirty.swap(false, Ordering::Relaxed) {
                self.refresh_highlight();
                self.refresh_git_marks();
                terminal.draw(|frame| ui::draw(frame, &mut self))?;
            }
            // Short poll so shell output appears promptly; the frame is only
            // painted when something actually changed.
            redraw = false;
            if event::poll(Duration::from_millis(30))? {
                redraw = true;
                match event::read()? {
                    Event::Key(key) if key.kind != KeyEventKind::Release => self.on_key(key),
                    Event::Mouse(m) => self.on_mouse(m),
                    Event::Paste(text) => match self.focus {
                        Focus::Shell => self.shell.paste(&text),
                        Focus::Editor => self.paste_text(&text),
                        Focus::Search => {
                            self.search.input_mut().push_str(&text);
                            self.search.run(self.project.roots());
                        }
                        Focus::Find => self.find.query.push_str(&text),
                        Focus::Tree | Focus::Git | Focus::Diff => {}
                    },
                    Event::Resize(..) => {}
                    _ => redraw = false,
                }
                // A tab switch can hide the find bar out from under the
                // keyboard, so the focus is settled before the next frame.
                self.tab_changed();
            }
        }
        Ok(())
    }

    // ----- buffers -------------------------------------------------------

    pub fn edit_state(&mut self) -> &mut EditState {
        let i = self.buffers.active;
        while self.edit.len() <= i {
            self.edit.push(EditState::default());
        }
        &mut self.edit[i]
    }

    pub fn open(&mut self, path: PathBuf) {
        let known = self.buffers.list.iter().any(|b| b.path == path);
        if !self.buffers.open(path.clone()) {
            self.status = format!("cannot open {} as text", short(&path));
            return;
        }
        if !known {
            self.edit.insert(self.buffers.active, EditState::default());
        }
        self.focus = Focus::Editor;
    }

    pub fn jump_to(&mut self, path: PathBuf, line: usize) {
        if let Some(buf) = self.buffers.active() {
            let cur = (buf.path.clone(), self.edit[self.buffers.active].line + 1);
            if cur != (path.clone(), line) {
                self.history.push(cur);
            }
        }
        self.open(path);
        let total = self
            .buffers
            .active()
            .map_or(1, |b| b.text.split('\n').count());
        let state = self.edit_state();
        state.line = line.saturating_sub(1).min(total.saturating_sub(1));
        state.col = 0;
        state.goal_col = 0;
    }

    /// Closes the active tab outright when it is clean; otherwise asks first.
    fn close_tab(&mut self) {
        if self.buffers.is_empty() {
            return;
        }
        let i = self.buffers.active;
        if self.buffers.list[i].modified() {
            self.prompt = Some(Prompt::ConfirmClose {
                index: i,
                name: self.buffers.list[i].name(),
            });
            return;
        }
        self.close_tab_now(i);
    }

    fn close_tab_now(&mut self, i: usize) {
        self.buffers.close(i);
        if i < self.edit.len() {
            self.edit.remove(i);
        }
        // The find bar searches the file that was open; closing it closes the
        // search with it.
        if self.find.open {
            self.find.open = false;
            if self.focus == Focus::Find {
                self.focus = Focus::Editor;
            }
        }
        if self.buffers.is_empty() {
            self.focus = Focus::Tree;
        }
    }

    /// Keeps the gutter marks and the blame line in step with where the cursor
    /// is. Both are a `git` call, so each is made only when something moved.
    pub fn refresh_git_marks(&mut self) {
        let (Some(buf), Some(dir)) = (self.buffers.active(), self.git.dir()) else {
            self.git_lines.clear();
            self.git_lines_key = None;
            self.blame = None;
            self.blame_key = None;
            return;
        };
        let path = buf.path.clone();
        let Ok(relative) = path.strip_prefix(&dir) else {
            self.git_lines.clear();
            self.blame = None;
            return;
        };
        let relative = relative.to_string_lossy().into_owned();

        let lines_key = (path.clone(), buf.text.len());
        if self.git_lines_key.as_ref() != Some(&lines_key) {
            self.git_lines_key = Some(lines_key);
            self.git_lines = core_git::changed_lines(&dir, &relative);
        }

        let line = self.edit.get(self.buffers.active).map_or(0, |s| s.line) + 1;
        let blame_key = (path, line);
        if self.blame_key.as_ref() != Some(&blame_key) {
            self.blame_key = Some(blame_key);
            self.blame = core_git::blame(&dir, &relative, line);
        }
    }

    fn refresh_highlight(&mut self) {
        let Some(buf) = self.buffers.active() else {
            self.highlight.clear();
            self.regions.clear();
            self.highlight_key = None;
            return;
        };
        let key = (
            buf.path.clone(),
            buf.text.len(),
            self.themes[self.theme_index].name.clone(),
        );
        // Length plus path plus theme is a cheap proxy; same-length edits are
        // caught by the explicit invalidation in `mark_dirty`.
        if self.highlight_key.as_ref() == Some(&key) {
            return;
        }
        let mut lines = Vec::new();
        self.syntax
            .highlight_lines(&buf.extension, &buf.text, |regions| {
                lines.push(
                    regions
                        .into_iter()
                        .map(|r| (r.color, r.italic, r.text.trim_end_matches('\n').to_string()))
                        .collect(),
                );
            });
        self.highlight = lines;
        self.highlight_key = Some(key);

        // Folds follow the text: an edit that moves lines around invalidates
        // the headers, so only folds still sitting on a block survive.
        let buf = self.buffers.active().expect("buffer checked above");
        self.regions = fold::regions(&buf.text, &buf.extension);
        let starts = fold::all_starts(&self.regions);
        let index = self.buffers.active;
        if let Some(state) = self.edit.get_mut(index) {
            state.folds.retain(|line| starts.contains(line));
        }
    }

    // ----- folding -------------------------------------------------------

    /// Lines hidden by the active buffer's folds.
    pub fn hidden_lines(&self) -> BTreeSet<usize> {
        match self.edit.get(self.buffers.active) {
            Some(state) => fold::hidden_lines(&self.regions, &state.folds),
            None => BTreeSet::new(),
        }
    }

    /// Real line numbers in display order, folded blocks removed.
    pub fn visible_lines(&self) -> Vec<usize> {
        let hidden = self.hidden_lines();
        (0..self.lines_count())
            .filter(|l| !hidden.contains(l))
            .collect()
    }

    pub fn is_folded(&self, line: usize) -> bool {
        self.edit
            .get(self.buffers.active)
            .is_some_and(|s| s.folds.contains(&line))
    }

    /// Collapses or expands the block headed by `line`, if there is one.
    pub fn toggle_fold(&mut self, line: usize) {
        if fold::region_at(&self.regions, line).is_none() {
            return;
        }
        let cursor = self.edit_state().line;
        let state = self.edit_state();
        if !state.folds.remove(&line) {
            state.folds.insert(line);
        }
        // Never leave the cursor inside something that was just hidden.
        if self.hidden_lines().contains(&cursor) {
            let state = self.edit_state();
            state.line = line;
            state.col = 0;
            state.goal_col = 0;
        }
    }

    /// Folds the innermost block around the cursor, or unfolds it if the cursor
    /// sits on a folded header.
    fn toggle_fold_at_cursor(&mut self) {
        let line = self.edit_state().line;
        if fold::region_at(&self.regions, line).is_some() {
            self.toggle_fold(line);
            return;
        }
        if let Some(header) = fold::context(&self.regions, line, 1).first().copied() {
            self.toggle_fold(header);
        } else {
            self.status = "nothing to fold here".into();
        }
    }

    /// Called after the active tab changes: a find bar belonging to the file
    /// left behind hides, so the keyboard cannot stay in it.
    fn tab_changed(&mut self) {
        if self.focus == Focus::Find && !self.find_showing() {
            self.focus = Focus::Editor;
        }
    }

    /// Whether the find bar is showing: it belongs to one file, and hides
    /// while another tab is in front.
    pub fn find_showing(&self) -> bool {
        match self.buffers.active() {
            Some(buf) => self.find.shows_for(&buf.path),
            None => false,
        }
    }

    fn mark_dirty(&mut self) {
        self.highlight_key = None;
        self.link = None;
        // Match highlighting is drawn from character offsets, so an edit that
        // shifts the text has to shift the hits with it.
        if self.find_showing() {
            if let Some(text) = self.buffers.active().map(|b| b.text.clone()) {
                self.find.refresh(&text);
            }
        }
    }

    // ----- selection -----------------------------------------------------

    /// The selected character range, if any.
    pub fn selection(&self) -> Option<(usize, usize)> {
        let anchor = self.edit.get(self.buffers.active)?.anchor?;
        let cursor = self.cursor_index();
        if anchor == cursor {
            return None;
        }
        Some((anchor.min(cursor), anchor.max(cursor)))
    }

    fn clear_selection(&mut self) {
        if let Some(state) = self.edit.get_mut(self.buffers.active) {
            state.anchor = None;
        }
    }

    /// Starts a selection at the cursor if one is not running already.
    fn begin_selection(&mut self) {
        let cursor = self.cursor_index();
        if let Some(state) = self.edit.get_mut(self.buffers.active) {
            state.anchor.get_or_insert(cursor);
        }
    }

    fn selected_text(&self) -> Option<String> {
        let (start, end) = self.selection()?;
        let buf = self.buffers.active()?;
        Some(buf.text.chars().skip(start).take(end - start).collect())
    }

    /// Removes the selection, leaving the cursor where it began.
    fn delete_selection(&mut self) -> bool {
        let Some((start, end)) = self.selection() else {
            return false;
        };
        let cursor = self.cursor_index();
        let Some(buf) = self.buffers.active_mut() else {
            return false;
        };
        buf.record(EditKind::Bulk, cursor);
        let from = Self::byte_of_char(&buf.text, start);
        let to = Self::byte_of_char(&buf.text, end);
        buf.text.replace_range(from..to, "");
        self.clear_selection();
        self.set_cursor_from_index(start);
        self.mark_dirty();
        true
    }

    /// Undo (`back`) or redo, putting the cursor where the step left it.
    fn step_history(&mut self, back: bool) {
        let cursor = self.cursor_index();
        let Some(buf) = self.buffers.active_mut() else {
            return;
        };
        let moved = if back {
            buf.undo(cursor)
        } else {
            buf.redo(cursor)
        };
        let Some(at) = moved else {
            self.status = if back {
                "nothing to undo".into()
            } else {
                "nothing to redo".into()
            };
            return;
        };
        self.clear_selection();
        self.set_cursor_from_index(at);
        self.mark_dirty();
        if self.find_showing() {
            self.refresh_find();
        }
    }

    fn select_all(&mut self) {
        let total = self.buffers.active().map_or(0, |b| b.text.chars().count());
        if total == 0 {
            return;
        }
        if let Some(state) = self.edit.get_mut(self.buffers.active) {
            state.anchor = Some(0);
        }
        self.set_cursor_from_index(total);
    }

    fn copy(&mut self) {
        if let Some(text) = self.selected_text() {
            self.clipboard.set(text);
            self.status = "copied".into();
        }
    }

    fn cut(&mut self) {
        if let Some(text) = self.selected_text() {
            self.clipboard.set(text);
            self.delete_selection();
            self.status = "cut".into();
        }
    }

    /// Inserts text at the cursor, replacing any selection.
    pub fn paste_text(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }
        self.delete_selection();
        self.insert(text);
    }

    // ----- editing -------------------------------------------------------

    fn lines_count(&self) -> usize {
        self.buffers
            .active()
            .map_or(1, |b| b.text.split('\n').count().max(1))
    }

    fn line_len(&self, line: usize) -> usize {
        self.buffers.active().map_or(0, |b| {
            b.text
                .split('\n')
                .nth(line)
                .map_or(0, |l| l.chars().count())
        })
    }

    /// Char offset of a (line, column) position within the buffer.
    fn char_index_at(&self, line: usize, col: usize) -> usize {
        let Some(buf) = self.buffers.active() else {
            return 0;
        };
        let mut idx = 0;
        for (n, text) in buf.text.split('\n').enumerate() {
            if n == line {
                return idx + col.min(text.chars().count());
            }
            idx += text.chars().count() + 1;
        }
        idx
    }

    /// Char offset of the cursor within the buffer.
    fn cursor_index(&self) -> usize {
        let Some(buf) = self.buffers.active() else {
            return 0;
        };
        let state = &self.edit[self.buffers.active];
        let mut idx = 0;
        for (n, line) in buf.text.split('\n').enumerate() {
            if n == state.line {
                return idx + state.col.min(line.chars().count());
            }
            idx += line.chars().count() + 1;
        }
        idx
    }

    fn set_cursor_from_index(&mut self, index: usize) {
        let Some(buf) = self.buffers.active() else {
            return;
        };
        let mut remaining = index;
        let mut line = 0;
        let mut col = 0;
        for (n, l) in buf.text.split('\n').enumerate() {
            let len = l.chars().count();
            if remaining <= len {
                line = n;
                col = remaining;
                break;
            }
            remaining -= len + 1;
            line = n;
            col = len;
        }
        let state = self.edit_state();
        state.line = line;
        state.col = col;
        state.goal_col = col;
    }

    fn byte_of_char(text: &str, idx: usize) -> usize {
        text.char_indices().nth(idx).map_or(text.len(), |(b, _)| b)
    }

    fn insert(&mut self, s: &str) {
        let index = self.cursor_index();
        // A pasted or otherwise multi-character insert is a step of its own.
        let kind = if s.chars().count() > 1 {
            EditKind::Bulk
        } else {
            EditKind::Insert
        };
        let Some(buf) = self.buffers.active_mut() else {
            return;
        };
        buf.record(kind, index);
        let byte = Self::byte_of_char(&buf.text, index);
        buf.text.insert_str(byte, s);
        self.set_cursor_from_index(index + s.chars().count());
        self.mark_dirty();
    }

    fn backspace(&mut self) {
        let index = self.cursor_index();
        if index == 0 {
            return;
        }
        let Some(buf) = self.buffers.active_mut() else {
            return;
        };
        buf.record(EditKind::Delete, index);
        let start = Self::byte_of_char(&buf.text, index - 1);
        let end = Self::byte_of_char(&buf.text, index);
        buf.text.replace_range(start..end, "");
        self.set_cursor_from_index(index - 1);
        self.mark_dirty();
    }

    fn delete_forward(&mut self) {
        let index = self.cursor_index();
        let Some(buf) = self.buffers.active_mut() else {
            return;
        };
        if index >= buf.text.chars().count() {
            return;
        }
        buf.record(EditKind::Delete, index);
        let start = Self::byte_of_char(&buf.text, index);
        let end = Self::byte_of_char(&buf.text, index + 1);
        buf.text.replace_range(start..end, "");
        self.mark_dirty();
    }

    /// Enter with the same smart-indent rules the GUI uses.
    fn newline(&mut self) {
        let index = self.cursor_index();
        let config = self.settings.indent.clone();
        let Some(buf) = self.buffers.active_mut() else {
            return;
        };
        // Enter closes the run being typed, so a line undoes as a line.
        buf.record(EditKind::Bulk, index);
        let edit = indent::newline_edit(&buf.text, index, &buf.extension, &config);
        let byte = Self::byte_of_char(&buf.text, index);
        buf.text.insert_str(byte, &edit.insert);
        self.set_cursor_from_index(index + edit.cursor_offset);
        self.mark_dirty();
    }

    fn move_cursor(&mut self, dl: isize, dc: isize) {
        if let Some(buf) = self.buffers.active_mut() {
            buf.history.end_run();
        }
        let state_line = self.edit[self.buffers.active].line;
        if dl != 0 {
            // Vertical movement walks the visible lines, stepping over folds.
            let visible = self.visible_lines();
            if visible.is_empty() {
                return;
            }
            let row = visible
                .binary_search(&state_line)
                .unwrap_or_else(|insert| insert.min(visible.len() - 1));
            let target_row = (row as isize + dl).clamp(0, visible.len() as isize - 1) as usize;
            let target = visible[target_row];
            let goal = self.edit[self.buffers.active].goal_col;
            let len = self.line_len(target);
            let state = self.edit_state();
            state.line = target;
            state.col = goal.min(len);
            return;
        }
        let index = self.cursor_index();
        let total = self.buffers.active().map_or(0, |b| b.text.chars().count());
        let next = (index as isize + dc).clamp(0, total as isize) as usize;
        self.set_cursor_from_index(next);
    }

    /// The real line drawn at a screen row, accounting for folds and for the
    /// sticky headers pinned at the top.
    pub fn line_at_row(&self, y: u16) -> Option<usize> {
        let editor = self.layout.editor;
        if y < editor.y || y >= editor.y + editor.height {
            return None;
        }
        let offset = (y - editor.y) as usize;
        let state = self.edit.get(self.buffers.active)?;
        let visible = self.visible_lines();
        let first = visible.get(state.scroll).copied().unwrap_or(0);
        let sticky = fold::context(&self.regions, first, 3)
            .into_iter()
            .filter(|header| *header < first)
            .collect::<Vec<_>>();
        if offset < sticky.len() {
            return Some(sticky[offset]);
        }
        visible.get(state.scroll + offset).copied()
    }

    /// The identifier under a screen point, as (line, start col, end col, word).
    fn word_at_point(&self, x: u16, y: u16) -> Option<(usize, usize, usize, String)> {
        if !hits(self.layout.editor, x, y) || self.buffers.is_empty() {
            return None;
        }
        let line = self.line_at_row(y)?;
        let col = x.checked_sub(self.layout.editor.x + self.layout.gutter)? as usize;
        if col > self.line_len(line) {
            return None;
        }
        let buf = self.buffers.active()?;
        let line_start = self.char_index_at(line, 0);
        let (word, start, end) = word_at(&buf.text, line_start + col)?;
        Some((line, start - line_start, end - line_start, word))
    }

    /// Whether a click carries a configured go-to-definition modifier.
    /// Terminals cannot report Cmd, so the default here is Ctrl or Alt.
    fn is_goto_modifier(&self, modifiers: KeyModifiers) -> bool {
        self.settings
            .goto_modifiers
            .tui
            .iter()
            .any(|wanted| match wanted {
                Modifier::Cmd | Modifier::Ctrl => modifiers.contains(KeyModifiers::CONTROL),
                Modifier::Alt => modifiers.contains(KeyModifiers::ALT),
                Modifier::Shift => modifiers.contains(KeyModifiers::SHIFT),
            })
    }

    fn goto_definition(&mut self) {
        let Some(buf) = self.buffers.active() else {
            return;
        };
        let index = self.cursor_index();
        let Some((word, _, _)) = word_at(&buf.text, index) else {
            self.status = "no identifier under the cursor".into();
            return;
        };
        let current = buf.path.clone();
        self.goto_word(word, current);
    }

    fn goto_word(&mut self, word: String, current: PathBuf) {
        let mut candidates = search::find_definitions(self.project.roots(), &word);
        let is_definition = !candidates.is_empty();
        if !is_definition {
            candidates = search::find_references(self.project.roots(), &word, 100);
        }
        candidates.sort_by_key(|c| (c.path != current, c.path.clone(), c.line));
        match candidates.len() {
            0 => self.status = format!("no definition found for {word}"),
            1 if is_definition => {
                let c = &candidates[0];
                let (path, line) = (c.path.clone(), c.line);
                self.jump_to(path, line);
            }
            _ => {
                self.prompt_selected = 0;
                self.prompt = Some(Prompt::Goto {
                    word,
                    is_definition,
                    candidates,
                });
            }
        }
    }

    // ----- input ---------------------------------------------------------

    // ----- mouse ---------------------------------------------------------

    /// The navigator row under a point, as an index into the visible rows.
    fn tree_row_at(&self, x: u16, y: u16) -> Option<usize> {
        if !hits(self.layout.tree, x, y) {
            return None;
        }
        let index = self.tree.scroll + (y - self.layout.tree.y) as usize;
        (index < self.tree.rows().len()).then_some(index)
    }

    fn on_mouse(&mut self, m: MouseEvent) {
        let (x, y) = (m.column, m.row);
        self.mouse = Some((x, y));
        match m.kind {
            MouseEventKind::Moved => {
                // Holding the navigation modifier underlines the identifier
                // under the pointer, the way ⌘-hover does in the window.
                self.link = if self.is_goto_modifier(m.modifiers) {
                    self.word_at_point(x, y).map(|(l, s, e, _)| (l, s, e))
                } else {
                    None
                };
                if let Some(menu) = &mut self.menu {
                    if hits(self.layout.menu, x, y) {
                        let row = (y - self.layout.menu.y).saturating_sub(1) as usize;
                        if menu.item_at_row(row).is_some() {
                            menu.selected = row;
                        }
                    }
                }
            }
            MouseEventKind::ScrollDown => self.scroll_at(x, y, 3),
            MouseEventKind::ScrollUp => self.scroll_at(x, y, -3),
            MouseEventKind::Down(MouseButton::Right) => self.right_click(x, y),
            MouseEventKind::Down(MouseButton::Left) => {
                self.tab_drag = self.tab_at(x, y);
                if hits(self.layout.v_split, x, y) {
                    self.resizing = Some(Splitter::Sidebar);
                } else if hits(self.layout.h_split, x, y) {
                    self.resizing = Some(Splitter::Shell);
                } else {
                    self.left_click(x, y, m.modifiers);
                }
            }
            MouseEventKind::Drag(MouseButton::Left) if self.resizing.is_some() => {
                self.resize(x, y);
            }
            MouseEventKind::Drag(MouseButton::Left) if self.selecting => {
                self.extend_selection_to(x, y);
            }
            MouseEventKind::Drag(MouseButton::Left) => {
                // Tabs move within their own strip; the highlight follows the
                // pointer while the button is down.
                if let Some((strip, from)) = self.tab_drag {
                    self.tab_drag_over = match self.tab_at(x, y) {
                        Some((over_strip, to)) if over_strip == strip && to != from => Some(to),
                        _ => None,
                    };
                    return;
                }
                // A press on a row only becomes a drag once the pointer leaves
                // it, so ordinary clicks aren't treated as moves.
                if self.drag.is_none() {
                    if let (Some(pressed), Some(index)) = (self.press, self.tree_row_at(x, y)) {
                        if pressed != index {
                            self.drag = self.tree.rows().get(pressed).map(|r| r.path.clone());
                        }
                    }
                }
                self.drag_over = self.drop_target_at(x, y);
            }
            MouseEventKind::Up(MouseButton::Left) => {
                if let (Some((strip, from)), Some(to)) =
                    (self.tab_drag.take(), self.tab_drag_over.take())
                {
                    match strip {
                        TabStrip::Editor => self.buffers.reorder(from, to),
                        TabStrip::Terminal => self.shell.sessions.reorder(from, to),
                    }
                    self.press = None;
                    return;
                }
                self.tab_drag = None;
                self.tab_drag_over = None;
                if self.selecting {
                    self.selecting = false;
                    // A click without movement leaves no selection behind.
                    if self.selection().is_none() {
                        self.clear_selection();
                    }
                    return;
                }
                if self.resizing.take().is_some() {
                    return;
                }
                if self.drag.is_some() {
                    self.finish_drag(x, y);
                } else if let (Some(pressed), Some(index)) = (self.press, self.tree_row_at(x, y)) {
                    if pressed == index {
                        self.activate_tree_row(index);
                    }
                }
                self.press = None;
                self.drag_over = None;
            }
            _ => {}
        }
    }

    /// Applies a splitter drag, keeping both panes usably sized.
    fn resize(&mut self, x: u16, y: u16) {
        let body = self.layout.body;
        match self.resizing {
            Some(Splitter::Sidebar) => {
                let max = body.width.saturating_sub(24).max(12);
                self.sidebar_width = x.saturating_sub(body.x).clamp(12, max);
            }
            Some(Splitter::Shell) => {
                // The status line occupies the last row of `body`, under the
                // editor region; the shell ends just above it.
                let bottom = (body.y + body.height).saturating_sub(1);
                let max = body.height.saturating_sub(6).max(3);
                self.shell_height = bottom.saturating_sub(y).saturating_sub(1).clamp(3, max);
            }
            None => {}
        }
    }

    /// Whether the pointer currently sits on a pane border.
    pub fn hovering_split(&self) -> Option<Splitter> {
        let (x, y) = self.mouse?;
        if self.resizing.is_some() {
            return self.resizing;
        }
        if hits(self.layout.v_split, x, y) {
            Some(Splitter::Sidebar)
        } else if hits(self.layout.h_split, x, y) {
            Some(Splitter::Shell)
        } else {
            None
        }
    }

    /// Moves the cursor to a screen point while keeping the anchor, which is
    /// what dragging out a selection does.
    fn extend_selection_to(&mut self, x: u16, y: u16) {
        let Some(line) = self.line_at_row(y) else {
            return;
        };
        let col = (x.saturating_sub(self.layout.editor.x + self.layout.gutter)) as usize;
        let col = col.min(self.line_len(line));
        let state = self.edit_state();
        state.line = line;
        state.col = col;
        state.goal_col = col;
    }

    fn scroll_at(&mut self, x: u16, y: u16, delta: isize) {
        if self.menu.is_some() {
            return;
        }
        if hits(self.layout.shell, x, y) {
            // Positive delta scrolls down, which means less scrollback.
            self.shell.scroll(-delta);
        } else if hits(self.layout.tree, x, y) {
            self.tree.move_selection(delta);
        } else if hits(self.layout.search_list, x, y) {
            self.search_selected = (self.search_selected as isize + delta).max(0) as usize;
            let total = self.search.total_matches();
            if total > 0 && self.search_selected >= total {
                self.search_selected = total - 1;
            }
        } else if hits(self.layout.git_list, x, y) {
            self.git_selected = (self.git_selected as isize + delta).max(0) as usize;
            let total = self.git.changes.len();
            if total > 0 && self.git_selected >= total {
                self.git_selected = total - 1;
            }
        } else if hits(self.layout.editor, x, y) && !self.buffers.is_empty() {
            self.move_cursor(delta, 0);
        }
    }

    fn left_click(&mut self, x: u16, y: u16, modifiers: KeyModifiers) {
        // A menu swallows the click that dismisses it.
        if self.menu.is_some() {
            if hits(self.layout.menu, x, y) {
                let row = (y - self.layout.menu.y).saturating_sub(1) as usize;
                if let Some(item) = self.menu.as_ref().and_then(|m| m.item_at_row(row)) {
                    self.menu.as_mut().unwrap().selected = row;
                    self.activate_menu_item(item);
                }
            } else {
                self.menu = None;
            }
            return;
        }

        // Prompt list selection.
        if self.prompt.is_some() && hits(self.layout.prompt_list, x, y) {
            let row = (y - self.layout.prompt_list.y) as usize;
            let len = self.prompt.as_ref().map_or(0, |p| {
                p.list_len(
                    self.themes.len(),
                    self.settings.recent_projects.len(),
                    self.git.repos.len(),
                    self.git.worktrees.len(),
                )
            });
            let offset = self.prompt_list_offset();
            if offset + row < len {
                self.prompt_selected = offset + row;
                self.confirm_prompt();
            }
            return;
        }
        if self.prompt.is_some() {
            return;
        }

        // The find bar is fully clickable: fields, option toggles and the
        // arrows beside the match counter.
        if self.find_showing() {
            if hits(self.layout.find_query, x, y) {
                self.focus = Focus::Find;
                self.find.in_replace_field = false;
                return;
            }
            if hits(self.layout.find_replace, x, y) {
                self.focus = Focus::Find;
                self.find.in_replace_field = true;
                return;
            }
            if hits(self.layout.find_case, x, y) {
                self.find.case_sensitive = !self.find.case_sensitive;
                self.refresh_find();
                return;
            }
            if hits(self.layout.find_word, x, y) {
                self.find.whole_word = !self.find.whole_word;
                self.refresh_find();
                return;
            }
            if hits(self.layout.find_regex, x, y) {
                self.find.regex = !self.find.regex;
                self.refresh_find();
                return;
            }
            if hits(self.layout.find_replace_one, x, y) {
                self.find_replace_current();
                return;
            }
            if hits(self.layout.find_replace_all, x, y) {
                self.find_replace_all();
                return;
            }
            if hits(self.layout.find_prev, x, y) {
                self.find_step(-1);
                return;
            }
            if hits(self.layout.find_next, x, y) {
                self.find_step(1);
                return;
            }
            if hits(self.layout.find_close, x, y) {
                self.find.open = false;
                self.focus = Focus::Editor;
                return;
            }
        }
        for (i, which) in [MenuBar::File, MenuBar::View, MenuBar::Help]
            .into_iter()
            .enumerate()
        {
            if hits(self.menu_buttons[i], x, y) {
                // A second click on the same button closes the menu again.
                let open_here = self
                    .menu
                    .as_ref()
                    .is_some_and(|m| m.x == self.menu_buttons[i].x && m.target.is_none());
                self.menu = None;
                if !open_here {
                    self.open_menu_bar_menu(which);
                }
                return;
            }
        }
        if hits(self.layout.tab_files, x, y) {
            self.sidebar_view = SidebarView::Files;
            self.focus = Focus::Tree;
            return;
        }
        if hits(self.layout.tab_search, x, y) {
            self.sidebar_view = SidebarView::Search;
            self.focus = Focus::Search;
            return;
        }
        if hits(self.layout.tab_git, x, y) {
            self.sidebar_view = SidebarView::Git;
            self.focus = Focus::Git;
            return;
        }
        if hits(self.layout.status_theme, x, y) {
            self.prompt_selected = self.theme_index;
            self.prompt = Some(Prompt::Themes);
            return;
        }
        if hits(self.layout.sidebar_header, x, y) {
            return;
        }
        if hits(self.layout.git_repo, x, y) {
            self.focus = Focus::Git;
            self.prompt_selected = self.git.repo;
            self.prompt = Some(Prompt::GitRepo);
            return;
        }
        if hits(self.layout.git_worktree, x, y) {
            self.focus = Focus::Git;
            self.prompt_selected = self.git.worktree;
            self.prompt = Some(Prompt::GitWorktree);
            return;
        }
        if hits(self.layout.git_list, x, y) {
            self.focus = Focus::Git;
            let row = self.layout.git_list_offset + (y - self.layout.git_list.y) as usize;
            if row < self.git.changes.len() {
                self.git_selected = row;
                self.open_git_change();
            }
            return;
        }
        if let Some(index) = self.tree_row_at(x, y) {
            self.focus = Focus::Tree;
            self.tree.selected = index;
            self.press = Some(index);
            return;
        }
        for (rect, field) in [
            (self.layout.search_input, SearchField::Query),
            (self.layout.search_replace, SearchField::Replace),
            (self.layout.search_exclude, SearchField::Exclude),
        ] {
            if hits(rect, x, y) {
                self.focus = Focus::Search;
                self.search.set_field(field);
                return;
            }
        }
        if hits(self.layout.search_action, x, y) {
            self.replace_all_in_project();
            return;
        }
        if hits(self.layout.search_options, x, y) {
            self.focus = Focus::Search;
            self.toggle_search_option(x);
            return;
        }
        if hits(self.layout.search_list, x, y) {
            self.focus = Focus::Search;
            let row = (y - self.layout.search_list.y) as usize;
            if let Some(Some(flat)) = self.layout.search_rows.get(row).copied() {
                self.search_selected = flat;
                let target = self
                    .search
                    .results
                    .iter()
                    .flat_map(|f| f.matches.iter().map(|m| (f.path.clone(), m.line)))
                    .nth(flat);
                if let Some((path, line)) = target {
                    self.open_search_hit(path, line);
                }
            }
            return;
        }
        if hits(self.layout.tabs, x, y) {
            for (start, end, index, is_close) in self.layout.tab_spans.clone() {
                if x >= start && x < end {
                    self.active_diff = None;
                    if is_close {
                        self.buffers.active = index;
                        self.close_tab();
                    } else {
                        self.buffers.active = index;
                        self.focus = Focus::Editor;
                    }
                    return;
                }
            }
            for (start, end, index, is_close) in self.layout.diff_tabs.clone() {
                if x >= start && x < end {
                    if is_close {
                        self.close_diff(index);
                    } else {
                        self.active_diff = Some(index);
                        self.focus = Focus::Diff;
                    }
                    return;
                }
            }
            return;
        }
        if hits(self.layout.shell_new, x, y) {
            let root = self.project.root_or_cwd();
            self.shell.open(&root);
            self.focus = Focus::Shell;
            return;
        }
        if hits(self.layout.shell, x, y) {
            // The header row is the tab strip: switch or close sessions.
            if y == self.layout.shell.y {
                for (start, end, index, is_close) in self.layout.shell_tabs.clone() {
                    if x >= start && x < end {
                        if is_close {
                            self.shell.sessions.close(index);
                            if self.shell.sessions.is_empty() {
                                self.show_shell = false;
                                self.focus = Focus::Editor;
                            }
                        } else {
                            self.shell.sessions.set_active(index);
                            self.focus = Focus::Shell;
                        }
                        return;
                    }
                }
            }
            self.focus = Focus::Shell;
            return;
        }
        if hits(self.layout.editor, x, y) && !self.buffers.is_empty() {
            self.focus = Focus::Editor;
            if self.is_goto_modifier(modifiers) {
                self.link = None;
                match self.word_at_point(x, y) {
                    Some((line, start, _, word)) => {
                        let current = self.buffers.active().map(|b| b.path.clone());
                        let state = self.edit_state();
                        state.line = line;
                        state.col = start;
                        state.goal_col = start;
                        if let Some(current) = current {
                            self.goto_word(word, current);
                        }
                    }
                    None => self.status = "no identifier under the pointer".into(),
                }
                return;
            }
            let Some(line) = self.line_at_row(y) else {
                return;
            };
            // The last gutter column carries the fold marker.
            let marker_x = self.layout.editor.x + self.layout.gutter - 1;
            if x == marker_x {
                self.toggle_fold(line);
                return;
            }
            let col = (x.saturating_sub(self.layout.editor.x + self.layout.gutter)) as usize;
            let col = col.min(self.line_len(line));
            let state = self.edit_state();
            state.line = line;
            state.col = col;
            state.goal_col = col;
            // A press starts a fresh selection; the drag below extends it.
            let anchor = self.cursor_index();
            self.edit_state().anchor = Some(anchor);
            self.selecting = true;
        }
    }

    fn right_click(&mut self, x: u16, y: u16) {
        if self.prompt.is_some() {
            return;
        }
        // A terminal tab's right button renames the session.
        if let Some((TabStrip::Terminal, index)) = self.tab_at(x, y) {
            self.focus = Focus::Shell;
            self.shell.sessions.set_active(index);
            self.prompt_input = if self.shell.sessions.is_named(index) {
                self.shell.sessions.name(index)
            } else {
                String::new()
            };
            self.prompt = Some(Prompt::RenameTerminal(index));
            return;
        }
        if let Some(index) = self.tree_row_at(x, y) {
            self.focus = Focus::Tree;
            self.tree.selected = index;
            let row = &self.tree.rows()[index];
            let (path, is_dir) = (row.path.clone(), row.is_dir);
            let is_root = self.project.is_root(&path);
            let dir = if is_dir {
                path.clone()
            } else {
                path.parent()
                    .map(Path::to_path_buf)
                    .unwrap_or_else(|| self.project.root_or_cwd())
            };
            self.menu = Some(Menu::for_row(Some(path), is_dir, is_root, Some(dir), x, y));
        } else if hits(self.layout.tree, x, y) || hits(self.layout.sidebar, x, y) {
            let root = self.project.root().map(Path::to_path_buf);
            self.menu = Some(Menu::for_row(None, true, false, root, x, y));
        }
    }

    /// The tab under the pointer, if the point is on a tab's label rather than
    /// its close mark.
    fn tab_at(&self, x: u16, y: u16) -> Option<(TabStrip, usize)> {
        if hits(self.layout.tabs, x, y) {
            return self
                .layout
                .tab_spans
                .iter()
                .find(|(start, end, _, is_close)| !is_close && x >= *start && x < *end)
                .map(|(_, _, index, _)| (TabStrip::Editor, *index));
        }
        if y == self.layout.shell.y && hits(self.layout.shell, x, y) {
            return self
                .layout
                .shell_tabs
                .iter()
                .find(|(start, end, _, is_close)| !is_close && x >= *start && x < *end)
                .map(|(_, _, index, _)| (TabStrip::Terminal, *index));
        }
        None
    }

    /// The folder a drop at this point would land in: the folder row under the
    /// pointer, or the project root for empty space in the navigator.
    fn drop_target_at(&self, x: u16, y: u16) -> Option<PathBuf> {
        match self.tree_row_at(x, y) {
            Some(i) => {
                let row = &self.tree.rows()[i];
                Some(if row.is_dir {
                    row.path.clone()
                } else {
                    row.path
                        .parent()
                        .map(Path::to_path_buf)
                        .unwrap_or_else(|| self.project.root_or_cwd())
                })
            }
            None => hits(self.layout.tree, x, y)
                .then(|| self.project.root().map(Path::to_path_buf))
                .flatten(),
        }
    }

    /// Opens a file row, or expands/collapses a folder row.
    fn activate_tree_row(&mut self, index: usize) {
        let Some(row) = self.tree.rows().get(index) else {
            return;
        };
        let (path, is_dir) = (row.path.clone(), row.is_dir);
        self.tree.selected = index;
        if is_dir {
            self.tree.toggle_selected();
        } else {
            self.open(path);
        }
    }

    /// Drops the dragged row onto whatever is under the release point — the
    /// release position is authoritative, not the last motion event.
    fn finish_drag(&mut self, x: u16, y: u16) {
        let dest = self.drop_target_at(x, y);
        let (Some(src), Some(dest)) = (self.drag.take(), dest) else {
            self.drag = None;
            self.drag_over = None;
            return;
        };
        let dest_name = dest.clone();
        match fs_ops::move_into(&src, &dest) {
            Ok(to) => {
                self.buffers.retarget(&src, &to);
                self.tree.expanded.insert(dest);
                self.tree.rebuild_keeping(&to);
                self.status = format!("moved {} into {}", short(&src), short(&dest_name));
            }
            Err(e) => self.status = format!("move failed: {e}"),
        }
    }

    /// First list row the prompt overlay renders, mirroring `ui::draw_prompt`.
    fn prompt_list_offset(&self) -> usize {
        let height = self.layout.prompt_list.height as usize;
        self.prompt_selected
            .saturating_sub(height.saturating_sub(1))
    }

    fn activate_menu_item(&mut self, item: MenuItem) {
        let Some(menu) = self.menu.take() else { return };
        self.prompt_input.clear();
        match item {
            MenuItem::Open => {
                if let Some(path) = menu.target {
                    self.open(path);
                }
            }
            MenuItem::NewFile => self.prompt = Some(Prompt::NewFile(menu.dir)),
            MenuItem::NewDir => self.prompt = Some(Prompt::NewDir(menu.dir)),
            MenuItem::Rename => {
                if let Some(path) = menu.target {
                    self.prompt_input = short(&path);
                    self.prompt = Some(Prompt::Rename(path));
                }
            }
            MenuItem::Move => {
                if let Some(path) = menu.target {
                    self.prompt = Some(Prompt::MoveTo(path));
                }
            }
            MenuItem::Delete => {
                if let Some(path) = menu.target {
                    self.prompt = Some(Prompt::ConfirmDelete(path));
                }
            }
            MenuItem::RemoveFolder => {
                if let Some(path) = menu.target {
                    self.remove_folder(&path);
                }
            }
            MenuItem::Note(_) => {}
            MenuItem::Command(command) => self.execute(command),
        }
    }

    // ----- project folders -------------------------------------------------

    /// Opens the built-in browser — the terminal frontend's stand-in for the
    /// window's native Open dialog.
    fn browse(&mut self, pick: Pick) {
        let dir = self.project.root_or_cwd();
        self.prompt_input.clear();
        self.prompt_selected = 0;
        self.prompt = Some(Prompt::Browse {
            entries: browse_entries(&dir, pick),
            dir,
            pick,
        });
    }

    /// Moves the browser to another directory, keeping the cursor at the top.
    fn browse_to(&mut self, dir: PathBuf) {
        let Some(Prompt::Browse { pick, .. }) = self.prompt.as_ref() else {
            return;
        };
        let pick = *pick;
        self.prompt_selected = 0;
        self.prompt = Some(Prompt::Browse {
            entries: browse_entries(&dir, pick),
            dir,
            pick,
        });
    }

    fn add_folder(&mut self, path: PathBuf) {
        match self.project.add(path) {
            Ok(added) => {
                self.tree.set_roots(self.project.roots().to_vec());
                self.tree.reveal(&added);
                self.settings.push_recent(&added);
                let _ = self.settings.save();
                self.search.run(self.project.roots());
                self.focus = Focus::Tree;
                self.status = format!("added {}", added.display());
            }
            Err(message) => self.status = message,
        }
    }

    fn remove_folder(&mut self, path: &Path) {
        match self.project.remove(path) {
            Ok(()) => {
                self.tree.set_roots(self.project.roots().to_vec());
                self.search.run(self.project.roots());
                self.status = format!("removed {} from the project", path.display());
            }
            Err(message) => self.status = message,
        }
    }

    fn on_key(&mut self, key: KeyEvent) {
        self.link = None;
        if self.menu.is_some() {
            self.menu_key(key);
            return;
        }
        if self.prompt.is_some() {
            self.prompt_key(key);
            return;
        }
        // Anything bound in settings.json wins over pane-local handling.
        if let Some(command) = chord_of(key).and_then(|c| self.settings.tui_command(&c)) {
            if self.command_applies(command) {
                self.execute(command);
                return;
            }
        }

        match key.code {
            // Tab belongs to the shell when it has focus: completion depends
            // on it. Panes are cycled from elsewhere, or with the mouse.
            KeyCode::Tab | KeyCode::BackTab if self.focus == Focus::Shell => {
                self.shell.send_key(key)
            }
            KeyCode::Tab if key.modifiers.contains(KeyModifiers::SHIFT) => self.cycle_focus(-1),
            KeyCode::BackTab => self.cycle_focus(-1),
            // The editor takes Tab as indentation, and the find bar as the
            // switch between its two fields.
            KeyCode::Tab if !matches!(self.focus, Focus::Editor | Focus::Find) => {
                self.cycle_focus(1)
            }
            _ => match self.focus {
                Focus::Tree => self.tree_key(key),
                Focus::Editor => self.editor_key(key),
                Focus::Search => self.search_key(key),
                Focus::Git => self.git_key(key),
                Focus::Find => self.find_key(key),
                Focus::Diff => self.diff_key(key),
                Focus::Shell => self.shell_key(key),
            },
        }
    }

    /// Whether a binding means anything where the keyboard currently is. Keys
    /// like Tab and Enter carry a command *and* a job inside a pane; a pane
    /// that needs the key keeps it.
    fn command_applies(&self, command: Command) -> bool {
        match command {
            // Tab indents in the editor, switches fields in the find bar and
            // completes in the shell.
            Command::NextPane | Command::PrevPane => {
                !matches!(self.focus, Focus::Editor | Focus::Find | Focus::Shell)
            }
            Command::Rename | Command::MoveTo | Command::Delete => self.focus == Focus::Tree,
            Command::FindNext | Command::FindPrev | Command::ReplaceAll => self.find_showing(),
            Command::PickRepository | Command::PickWorktree => self.focus == Focus::Git,
            // The shell takes every ordinary key; only the panel's own
            // bindings are reserved.
            _ => {
                self.focus != Focus::Shell
                    || matches!(
                        command,
                        Command::ToggleTerminal
                            | Command::NewTerminal
                            | Command::CloseTerminal
                            | Command::Quit
                    )
            }
        }
    }

    /// Runs a bound command, whatever triggered it: a key, the File menu, or a
    /// click in the top bar.
    pub fn execute(&mut self, command: Command) {
        match command {
            Command::NewFile => match self.tree.target_dir() {
                Some(dir) => {
                    self.prompt_input.clear();
                    self.prompt = Some(Prompt::NewFile(dir));
                }
                None => self.status = "no folder in the project yet".into(),
            },
            Command::AddFolder => self.browse(Pick::AddFolder),
            Command::OpenFile => self.browse(Pick::OpenFile),
            Command::OpenFolder => self.browse(Pick::OpenFolder),
            Command::OpenRecent => {
                if self.settings.recent_projects.is_empty() {
                    self.status = "no recent projects yet".into();
                } else {
                    self.prompt_selected = 0;
                    self.prompt = Some(Prompt::Recent);
                }
            }
            Command::Save => {
                self.status = if self.buffers.save_active() {
                    self.after_save();
                    "saved".into()
                } else {
                    "nothing to save".into()
                };
            }
            Command::SaveAs => {
                if self.buffers.is_empty() {
                    self.status = "nothing to save".into();
                } else {
                    self.prompt_input.clear();
                    self.prompt = Some(Prompt::SaveAs);
                }
            }
            Command::SaveAll => {
                let previous = self.buffers.active;
                let mut saved = 0;
                for i in 0..self.buffers.list.len() {
                    self.buffers.active = i;
                    if self.buffers.list[i].modified() && self.buffers.save_active() {
                        saved += 1;
                    }
                }
                self.buffers.active = previous;
                self.after_save();
                self.status = format!("saved {saved} file(s)");
            }
            Command::Settings => match self.settings.ensure_file() {
                Ok(path) => {
                    self.open(path);
                    self.status = "editing settings — save to apply".into();
                }
                Err(e) => self.status = format!("cannot open settings: {e}"),
            },
            Command::CloseEditor => self.close_tab(),
            Command::Quit => self.quit = true,
            Command::ToggleSidebar => {
                self.show_sidebar = !self.show_sidebar;
                if !self.show_sidebar && self.focus == Focus::Tree {
                    self.focus = Focus::Editor;
                }
            }
            Command::NewTerminal => {
                self.show_shell = true;
                let root = self.project.root_or_cwd();
                self.shell.open(&root);
                self.focus = Focus::Shell;
            }
            Command::CloseTerminal => {
                self.shell.close_active();
                if self.shell.sessions.is_empty() {
                    self.show_shell = false;
                    self.focus = Focus::Editor;
                }
            }
            Command::ToggleTerminal => {
                self.show_shell = !self.show_shell;
                self.focus = if self.show_shell {
                    Focus::Shell
                } else {
                    Focus::Editor
                };
            }
            Command::FindInFile => {
                if self.buffers.is_empty() {
                    self.status = "open a file first".into();
                } else {
                    self.open_find();
                }
            }
            Command::FocusSearch => {
                // Pressing it again while in the search pane walks the fields,
                // as Tab does in VS Code.
                if self.focus == Focus::Search && self.sidebar_view == SidebarView::Search {
                    self.search.cycle_field(1);
                } else {
                    self.show_sidebar = true;
                    self.sidebar_view = SidebarView::Search;
                    self.focus = Focus::Search;
                    self.search.set_field(SearchField::Query);
                }
            }
            Command::FocusFiles => {
                self.show_sidebar = true;
                self.sidebar_view = SidebarView::Files;
                self.focus = Focus::Tree;
            }
            Command::FocusGit => {
                self.show_sidebar = true;
                self.sidebar_view = SidebarView::Git;
                self.focus = Focus::Git;
            }
            Command::ThemePicker => {
                self.prompt_selected = self.theme_index;
                self.prompt = Some(Prompt::Themes);
            }
            Command::GotoDefinition => self.goto_definition(),
            Command::NewFolder => match self.tree.target_dir() {
                Some(dir) => {
                    self.prompt_input.clear();
                    self.prompt = Some(Prompt::NewDir(dir));
                }
                None => self.status = "no folder in the project yet".into(),
            },
            Command::Rename => {
                if let Some(path) = self.tree.selected_path().map(Path::to_path_buf) {
                    self.prompt_input = short(&path);
                    self.prompt = Some(Prompt::Rename(path));
                }
            }
            Command::MoveTo => {
                if let Some(path) = self.tree.selected_path().map(Path::to_path_buf) {
                    self.prompt_input.clear();
                    self.prompt = Some(Prompt::MoveTo(path));
                }
            }
            Command::Delete => {
                if let Some(path) = self.tree.selected_path().map(Path::to_path_buf) {
                    self.prompt = Some(Prompt::ConfirmDelete(path));
                }
            }
            Command::FindNext => self.find_step(1),
            Command::FindPrev => self.find_step(-1),
            Command::ReplaceAll => self.find_replace_all(),
            Command::NextPane => self.cycle_focus(1),
            Command::PrevPane => self.cycle_focus(-1),
            Command::PickRepository => {
                if !self.git.repos.is_empty() {
                    self.prompt_selected = self.git.repo;
                    self.prompt = Some(Prompt::GitRepo);
                }
            }
            Command::PickWorktree => {
                if !self.git.worktrees.is_empty() {
                    self.prompt_selected = self.git.worktree;
                    self.prompt = Some(Prompt::GitWorktree);
                }
            }
            // The window scales its own font; here the terminal does.
            Command::ZoomIn | Command::ZoomOut | Command::ResetZoom => {
                self.status = "the terminal controls the font size".into()
            }
            Command::Documentation => {
                self.status = format!(
                    "Yara {} — no documentation page yet",
                    env!("CARGO_PKG_VERSION")
                )
            }
            Command::Undo => self.step_history(true),
            Command::Redo => self.step_history(false),
            Command::SelectAll => self.select_all(),
            Command::Copy => self.copy(),
            Command::Cut => self.cut(),
            Command::Paste => {
                let text = self.clipboard.text().to_string();
                if text.is_empty() {
                    self.status = "clipboard is empty".into();
                } else {
                    self.paste_text(&text);
                }
            }
            Command::ToggleFold => self.toggle_fold_at_cursor(),
            Command::FoldAll => {
                let starts = fold::all_starts(&self.regions);
                let line = self.edit_state().line;
                self.edit_state().folds = starts;
                // Keep the cursor visible by lifting it to its outermost header.
                if self.hidden_lines().contains(&line) {
                    let outermost = fold::context(&self.regions, line, usize::MAX)
                        .first()
                        .copied()
                        .unwrap_or(0);
                    let state = self.edit_state();
                    state.line = outermost;
                    state.col = 0;
                }
            }
            Command::UnfoldAll => self.edit_state().folds.clear(),
            Command::GoBack => {
                if let Some((path, line)) = self.history.pop() {
                    self.open(path);
                    let state = self.edit_state();
                    state.line = line.saturating_sub(1);
                    state.col = 0;
                }
            }
            Command::NextTab => {
                if !self.buffers.is_empty() {
                    self.buffers.active = (self.buffers.active + 1) % self.buffers.list.len();
                }
            }
            Command::PrevTab => {
                if !self.buffers.is_empty() {
                    let n = self.buffers.list.len();
                    self.buffers.active = (self.buffers.active + n - 1) % n;
                }
            }
            Command::ContextMenu => self.open_menu_on_selection(),
            Command::FileMenu => self.open_menu_bar_menu(MenuBar::File),
            Command::ViewMenu => self.open_menu_bar_menu(MenuBar::View),
            Command::HelpMenu => self.open_menu_bar_menu(MenuBar::Help),
            Command::Help => {
                self.prompt_selected = 0;
                self.prompt = Some(Prompt::Help(self.help_entries()));
            }
        }
    }

    /// Opens a project-search result: jumps to the line and seeds the find bar
    /// with the same query, so every match in the file is highlighted and the
    /// one that was clicked is the current one.
    fn open_search_hit(&mut self, path: PathBuf, line: usize) {
        self.jump_to(path, line);
        self.find.query = self.search.query.clone();
        self.find.regex = self.search.regex;
        self.find.case_sensitive = self.search.case_sensitive;
        self.find.whole_word = self.search.whole_word;
        if self.find.query.is_empty() {
            self.find.open = false;
        } else if let Some(path) = self.buffers.active().map(|b| b.path.clone()) {
            self.find.open_on(&path);
        }
        if let Some(text) = self.buffers.active().map(|b| b.text.clone()) {
            self.find.refresh(&text);
            if let Some(index) = self.find.hits.iter().position(|h| h.line + 1 == line) {
                self.find.current = index;
            }
        }
    }

    /// Interprets typed input as a path: absolute, `~`-relative, or relative to
    /// the project root.
    fn resolve(&self, input: &str) -> PathBuf {
        let input = input.trim();
        if let Some(rest) = input.strip_prefix("~/") {
            if let Some(home) = std::env::var_os("HOME") {
                return PathBuf::from(home).join(rest);
            }
        }
        let path = PathBuf::from(input);
        if path.is_absolute() {
            path
        } else {
            self.project.root_or_cwd().join(path)
        }
    }

    /// Switches the project to another folder, keeping open buffers.
    fn set_root(&mut self, root: PathBuf) {
        let root = self.project.set_root(root);
        self.tree.set_roots(self.project.roots().to_vec());
        self.search = Search::default();
        self.settings.push_recent(&root);
        let _ = self.settings.save();
        // The shell restarts in the new project directory.
        self.shell.restart();
        self.focus = Focus::Tree;
        self.status = format!("project: {}", root.display());
    }

    /// Re-reads settings after the settings file itself is saved.
    fn after_save(&mut self) {
        // A save changes the git status; show it right away, not on the timer.
        self.git.invalidate();
        let is_settings = self
            .buffers
            .active()
            .zip(Settings::path())
            .is_some_and(|(buf, path)| buf.path == path);
        if !is_settings {
            return;
        }
        let (settings, error) = Settings::load();
        self.settings = settings;
        if let Some(index) = self
            .themes
            .iter()
            .position(|t| t.name == self.settings.theme)
        {
            self.theme_index = index;
            self.syntax.set_theme(&self.themes[index]);
            self.mark_dirty();
        }
        self.icons = icons::detect();
        self.status = error.unwrap_or_else(|| "settings applied".to_string());
    }

    /// Every binding, for the help overlay: chord in a fixed column, then the
    /// action it runs.
    fn help_entries(&self) -> Vec<String> {
        let mut entries: Vec<String> = crate::core::command::ALL
            .iter()
            .filter_map(|command| {
                let chord = self.settings.tui_chord(*command)?;
                Some(format!("{:<16}{}", chord.to_string(), command.label()))
            })
            .collect();
        entries.push(format!("{:<16}{}", "Tab / Shift+Tab", "Switch pane"));
        entries.push(format!("{:<16}{}", "Ctrl/Alt+click", "Go to definition"));
        entries.push(format!("{:<16}{}", "Right click", "Context menu"));
        entries.push(format!("{:<16}{}", "Drag a row", "Move file or folder"));
        entries
    }

    /// Drops one of the top bar's menus open under its button.
    pub fn open_menu_bar_menu(&mut self, which: MenuBar) {
        let settings = self.settings.clone();
        let button = self.menu_buttons[which as usize];
        let (entries, title) = match which {
            MenuBar::File => (FILE_MENU, None),
            MenuBar::View => (VIEW_MENU, None),
            MenuBar::Help => (
                HELP_MENU,
                Some(format!("Yara {}", env!("CARGO_PKG_VERSION"))),
            ),
        };
        self.menu = Some(Menu::commands(
            entries,
            title,
            button.x,
            button.y + 1,
            |command| settings.tui_chord(command).map(|c| c.to_string()),
        ));
    }

    fn cycle_focus(&mut self, delta: isize) {
        let mut panes = Vec::new();
        if self.show_sidebar {
            panes.push(match self.sidebar_view {
                SidebarView::Files => Focus::Tree,
                SidebarView::Search => Focus::Search,
                SidebarView::Git => Focus::Git,
            });
        }
        panes.push(Focus::Editor);
        if self.show_shell {
            panes.push(Focus::Shell);
        }
        let current = panes.iter().position(|f| *f == self.focus).unwrap_or(0) as isize;
        let next = (current + delta).rem_euclid(panes.len() as isize) as usize;
        self.focus = panes[next];
    }

    fn tree_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Down | KeyCode::Char('j') => self.tree.move_selection(1),
            KeyCode::Up | KeyCode::Char('k') => self.tree.move_selection(-1),
            KeyCode::PageDown => self.tree.move_selection(10),
            KeyCode::PageUp => self.tree.move_selection(-10),
            KeyCode::Home => self.tree.selected = 0,
            KeyCode::End => self.tree.selected = self.tree.rows().len().saturating_sub(1),
            KeyCode::Right | KeyCode::Char('l') => {
                if let Some(row) = self.tree.rows().get(self.tree.selected) {
                    if row.is_dir && !self.tree.expanded.contains(&row.path) {
                        self.tree.toggle_selected();
                    }
                }
            }
            KeyCode::Left => {
                if let Some(row) = self.tree.rows().get(self.tree.selected) {
                    if row.is_dir && self.tree.expanded.contains(&row.path) {
                        self.tree.toggle_selected();
                    }
                }
            }
            KeyCode::Enter => {
                let Some(row) = self.tree.rows().get(self.tree.selected) else {
                    return;
                };
                if row.is_dir {
                    self.tree.toggle_selected();
                } else {
                    let path = row.path.clone();
                    self.open(path);
                }
            }
            KeyCode::Char('a') => self.execute(Command::NewFile),
            KeyCode::Char('A') => {
                if let Some(dir) = self.tree.target_dir() {
                    self.prompt_input.clear();
                    self.prompt = Some(Prompt::NewDir(dir));
                }
            }
            KeyCode::Char('r') => {
                if let Some(path) = self.tree.selected_path().map(|p| p.to_path_buf()) {
                    self.prompt_input = short(&path);
                    self.prompt = Some(Prompt::Rename(path));
                }
            }
            // Same menu the right mouse button opens.
            KeyCode::Char('c') | KeyCode::Menu => self.open_menu_on_selection(),
            KeyCode::Char('d') | KeyCode::Delete => {
                if let Some(path) = self.tree.selected_path().map(|p| p.to_path_buf()) {
                    self.prompt = Some(Prompt::ConfirmDelete(path));
                }
            }
            _ => {}
        }
    }

    fn editor_key(&mut self, key: KeyEvent) {
        // Shift with a movement key extends the selection; moving without it
        // drops whatever was selected.
        if matches!(
            key.code,
            KeyCode::Up
                | KeyCode::Down
                | KeyCode::Left
                | KeyCode::Right
                | KeyCode::Home
                | KeyCode::End
                | KeyCode::PageUp
                | KeyCode::PageDown
        ) {
            if key.modifiers.contains(KeyModifiers::SHIFT) {
                self.begin_selection();
            } else {
                self.clear_selection();
            }
        }
        if self.buffers.is_empty() {
            if key.code == KeyCode::Tab {
                self.cycle_focus(1);
            }
            return;
        }
        let alt = key.modifiers.contains(KeyModifiers::ALT);
        if alt {
            match key.code {
                KeyCode::Right => {
                    if !self.buffers.is_empty() {
                        self.buffers.active = (self.buffers.active + 1) % self.buffers.list.len();
                    }
                    return;
                }
                KeyCode::Left => {
                    if !self.buffers.is_empty() {
                        let n = self.buffers.list.len();
                        self.buffers.active = (self.buffers.active + n - 1) % n;
                    }
                    return;
                }
                _ => {}
            }
        }
        match key.code {
            KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.delete_selection();
                self.insert(&c.to_string());
            }
            KeyCode::Enter => {
                self.delete_selection();
                self.newline();
            }
            KeyCode::Backspace => {
                if !self.delete_selection() {
                    self.backspace();
                }
            }
            KeyCode::Delete => {
                if !self.delete_selection() {
                    self.delete_forward();
                }
            }
            KeyCode::Tab => {
                self.delete_selection();
                let unit = self.settings.indent.unit();
                self.insert(&unit);
            }
            KeyCode::Up => self.move_cursor(-1, 0),
            KeyCode::Down => self.move_cursor(1, 0),
            KeyCode::Left => self.move_cursor(0, -1),
            KeyCode::Right => self.move_cursor(0, 1),
            KeyCode::PageUp => self.move_cursor(-20, 0),
            KeyCode::PageDown => self.move_cursor(20, 0),
            KeyCode::Home => {
                let state = self.edit_state();
                state.col = 0;
                state.goal_col = 0;
            }
            KeyCode::End => {
                let line = self.edit[self.buffers.active].line;
                let len = self.line_len(line);
                let state = self.edit_state();
                state.col = len;
                state.goal_col = len;
            }
            _ => {}
        }
        if matches!(key.code, KeyCode::Left | KeyCode::Right | KeyCode::Char(_)) {
            let col = self.edit[self.buffers.active].col;
            self.edit_state().goal_col = col;
        }
    }

    /// The chevron sits at the left of the options row and the three toggles at
    /// its right, so a click is matched against those two boxes.
    fn toggle_search_option(&mut self, x: u16) {
        let toggles = self.layout.search_toggles;
        match x.checked_sub(toggles.x) {
            Some(0..=1) => self.search.case_sensitive = !self.search.case_sensitive,
            Some(3..=4) => self.search.whole_word = !self.search.whole_word,
            Some(6..=7) => self.search.regex = !self.search.regex,
            _ => return,
        }
        self.search.run(self.project.roots());
    }

    /// Rewrites every match in the project, then refreshes open buffers.
    fn replace_all_in_project(&mut self) {
        match self.search.replace_all(self.project.roots()) {
            Ok((count, files)) => {
                for buf in &mut self.buffers.list {
                    if buf.modified() {
                        continue;
                    }
                    if let Ok(text) = std::fs::read_to_string(&buf.path) {
                        buf.saved_text = text.clone();
                        buf.text = text;
                    }
                }
                self.mark_dirty();
                self.status = format!("replaced {count} occurrence(s) in {files} file(s)");
            }
            Err(message) => self.status = message,
        }
    }

    // ----- find in file --------------------------------------------------

    /// Opens the find bar, seeded from the buffer and landing on the match
    /// nearest the cursor.
    pub fn open_find(&mut self) {
        let Some((text, path)) = self
            .buffers
            .active()
            .map(|b| (b.text.clone(), b.path.clone()))
        else {
            return;
        };
        self.find.open_on(&path);
        self.find.refresh(&text);
        let cursor = self.cursor_index();
        self.find.select_near(cursor);
        self.focus = Focus::Find;
        self.reveal_hit();
    }

    /// Moves to the next or previous match.
    pub fn find_step(&mut self, delta: isize) {
        let Some(text) = self.buffers.active().map(|b| b.text.clone()) else {
            return;
        };
        self.find.refresh(&text);
        self.find.step(delta);
        self.reveal_hit();
    }

    /// Unfolds anything hiding the current match and puts the cursor on it.
    fn reveal_hit(&mut self) {
        let Some(hit) = self.find.hit() else { return };
        for header in fold::context(&self.regions, hit.line, usize::MAX) {
            self.edit_state().folds.remove(&header);
        }
        self.set_cursor_from_index(hit.start);
    }

    fn find_replace_current(&mut self) {
        let Some(text) = self.buffers.active().map(|b| b.text.clone()) else {
            return;
        };
        self.find.refresh(&text);
        let at = self.cursor_index();
        if let Some((updated, cursor)) = self.find.replace_current(&text) {
            if let Some(buf) = self.buffers.active_mut() {
                buf.record(EditKind::Bulk, at);
                buf.text = updated.clone();
            }
            self.mark_dirty();
            self.find.refresh(&updated);
            self.find.select_near(cursor);
            self.reveal_hit();
        }
    }

    fn find_replace_all(&mut self) {
        let Some(text) = self.buffers.active().map(|b| b.text.clone()) else {
            return;
        };
        self.find.refresh(&text);
        let at = self.cursor_index();
        if let Some((updated, count)) = self.find.replace_all(&text) {
            if let Some(buf) = self.buffers.active_mut() {
                buf.record(EditKind::Bulk, at);
                buf.text = updated.clone();
            }
            self.mark_dirty();
            self.find.refresh(&updated);
            self.status = format!("replaced {count} occurrence(s)");
        }
    }

    /// Keys while the find bar has focus.
    fn find_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => {
                self.find.open = false;
                self.focus = Focus::Editor;
            }
            // In the replace field Enter applies the replacement and moves on,
            // and Alt+Enter replaces every match at once.
            KeyCode::Enter if key.modifiers.contains(KeyModifiers::ALT) => self.find_replace_all(),
            KeyCode::Enter if self.find.in_replace_field => self.find_replace_current(),
            KeyCode::Enter if key.modifiers.contains(KeyModifiers::SHIFT) => self.find_step(-1),
            KeyCode::Enter => self.find_step(1),
            KeyCode::Tab => self.find.in_replace_field = !self.find.in_replace_field,
            KeyCode::Down => self.find_step(1),
            KeyCode::Up => self.find_step(-1),
            KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                if self.find.in_replace_field {
                    self.find.replace.push(c);
                } else {
                    self.find.query.push(c);
                    self.refresh_find();
                }
            }
            KeyCode::Backspace => {
                if self.find.in_replace_field {
                    self.find.replace.pop();
                } else {
                    self.find.query.pop();
                    self.refresh_find();
                }
            }
            _ => {}
        }
    }

    pub fn refresh_find(&mut self) {
        if let Some(text) = self.buffers.active().map(|b| b.text.clone()) {
            self.find.refresh(&text);
            self.reveal_hit();
        }
    }

    fn search_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.search.input_mut().push(c);
                self.search.run_if_changed(self.project.roots());
                self.search_selected = 0;
            }
            KeyCode::Backspace => {
                self.search.input_mut().pop();
                self.search.run_if_changed(self.project.roots());
                self.search_selected = 0;
            }
            KeyCode::Down => self.search_selected += 1,
            KeyCode::Up => self.search_selected = self.search_selected.saturating_sub(1),
            KeyCode::Enter => {
                let flat: Vec<(PathBuf, usize)> = self
                    .search
                    .results
                    .iter()
                    .flat_map(|f| f.matches.iter().map(|m| (f.path.clone(), m.line)))
                    .collect();
                if let Some((path, line)) = flat.get(self.search_selected).cloned() {
                    self.open_search_hit(path, line);
                }
            }
            KeyCode::Esc => self.focus = Focus::Editor,
            _ => {}
        }
        let total: usize = self.search.total_matches();
        if total > 0 && self.search_selected >= total {
            self.search_selected = total - 1;
        }
    }

    fn shell_key(&mut self, key: KeyEvent) {
        self.shell.send_key(key);
    }

    fn menu_key(&mut self, key: KeyEvent) {
        let Some(menu) = &mut self.menu else { return };
        match key.code {
            KeyCode::Esc => self.menu = None,
            KeyCode::Down | KeyCode::Char('j') => menu.move_selection(1),
            KeyCode::Up | KeyCode::Char('k') => menu.move_selection(-1),
            KeyCode::Enter => {
                if let Some(item) = menu.selected_item() {
                    self.activate_menu_item(item);
                }
            }
            _ => {}
        }
    }

    /// Opens the context menu from the keyboard, on the selected row.
    fn open_menu_on_selection(&mut self) {
        let (target, is_dir, dir) = match self.tree.rows().get(self.tree.selected) {
            Some(row) if row.is_dir => (Some(row.path.clone()), true, Some(row.path.clone())),
            Some(row) => (
                Some(row.path.clone()),
                false,
                Some(
                    row.path
                        .parent()
                        .map(Path::to_path_buf)
                        .unwrap_or_else(|| self.project.root_or_cwd()),
                ),
            ),
            None => (None, true, self.project.root().map(Path::to_path_buf)),
        };
        let is_root = target
            .as_deref()
            .is_some_and(|path| self.project.is_root(path));
        let y = self.layout.tree.y + (self.tree.selected.saturating_sub(self.tree.scroll)) as u16;
        self.menu = Some(Menu::for_row(
            target,
            is_dir,
            is_root,
            dir,
            self.layout.tree.x + 2,
            y,
        ));
    }

    // ----- git view --------------------------------------------------------

    fn git_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Up => self.git_selected = self.git_selected.saturating_sub(1),
            KeyCode::Down => {
                if !self.git.changes.is_empty() {
                    self.git_selected = (self.git_selected + 1).min(self.git.changes.len() - 1);
                }
            }
            KeyCode::Enter => self.open_git_change(),
            KeyCode::Char('r') if !self.git.repos.is_empty() => {
                self.prompt_selected = self.git.repo;
                self.prompt = Some(Prompt::GitRepo);
            }
            KeyCode::Char('w') if !self.git.worktrees.is_empty() => {
                self.prompt_selected = self.git.worktree;
                self.prompt = Some(Prompt::GitWorktree);
            }
            _ => {}
        }
    }

    /// Shows a changed file side by side: what it was against what it is.
    fn open_git_change(&mut self) {
        let Some(dir) = self.git.dir() else { return };
        let Some(change) = self.git.changes.get(self.git_selected).cloned() else {
            return;
        };
        match core_git::diff(&dir, &change) {
            Ok(rows) => {
                let diff = Diff {
                    path: change.path.clone(),
                    rows,
                    scroll: 0,
                };
                match self.diffs.iter().position(|d| d.path == diff.path) {
                    Some(i) => {
                        self.diffs[i] = diff;
                        self.active_diff = Some(i);
                    }
                    None => {
                        self.diffs.push(diff);
                        self.active_diff = Some(self.diffs.len() - 1);
                    }
                }
                self.focus = Focus::Diff;
            }
            Err(message) => self.status = message,
        }
    }

    /// The diff tab in front, if one is.
    pub fn active_diff(&self) -> Option<&Diff> {
        self.active_diff.and_then(|i| self.diffs.get(i))
    }

    pub fn close_diff(&mut self, index: usize) {
        if index >= self.diffs.len() {
            return;
        }
        self.diffs.remove(index);
        self.active_diff = match self.active_diff {
            Some(active) if active == index => None,
            Some(active) if active > index => Some(active - 1),
            other => other,
        };
        if self.active_diff.is_none() && self.focus == Focus::Diff {
            self.focus = Focus::Editor;
        }
    }

    /// Opens the file the diff is showing, in the editor.
    fn open_diff_file(&mut self) {
        let (Some(dir), Some(index)) = (self.git.dir(), self.active_diff) else {
            return;
        };
        let Some(relative) = self.diffs.get(index).map(|d| d.path.clone()) else {
            return;
        };
        let path = dir.join(&relative);
        self.close_diff(index);
        if path.is_file() {
            self.tree.reveal(&path);
            self.open(path);
        } else {
            self.status = format!("gone: {relative}");
        }
    }

    fn diff_key(&mut self, key: KeyEvent) {
        let height = self.layout.editor.height as usize;
        let Some(index) = self.active_diff else {
            return;
        };
        if key.code == KeyCode::Esc {
            self.close_diff(index);
            self.focus = Focus::Git;
            return;
        }
        let Some(diff) = self.diffs.get_mut(index) else {
            return;
        };
        let last = diff.rows.len().saturating_sub(1);
        match key.code {
            KeyCode::Enter => self.open_diff_file(),
            KeyCode::Down => diff.scroll = (diff.scroll + 1).min(last),
            KeyCode::Up => diff.scroll = diff.scroll.saturating_sub(1),
            KeyCode::PageDown => diff.scroll = (diff.scroll + height).min(last),
            KeyCode::PageUp => diff.scroll = diff.scroll.saturating_sub(height),
            KeyCode::Home => diff.scroll = 0,
            KeyCode::End => diff.scroll = last,
            _ => {}
        }
    }

    fn prompt_key(&mut self, key: KeyEvent) {
        let Some(prompt) = &self.prompt else { return };

        if key.code == KeyCode::Esc {
            self.prompt = None;
            self.prompt_input.clear();
            return;
        }

        // Closing a modified buffer offers three ways out, so its `n` means
        // "discard", not the generic "cancel".
        if let Prompt::ConfirmClose { index, .. } = prompt {
            if matches!(key.code, KeyCode::Char('n') | KeyCode::Char('N')) {
                let index = *index;
                self.prompt = None;
                self.close_tab_now(index);
                return;
            }
        }

        // The file browser drives itself: arrows walk the filesystem, Enter
        // picks what the cursor is on.
        if let Prompt::Browse { dir, entries, .. } = prompt {
            let up = dir.parent().map(Path::to_path_buf);
            let row = self.prompt_selected;
            let entry = match &up {
                Some(_) if row == 0 => None,
                Some(_) => entries.get(row - 1).cloned(),
                None => entries.get(row).cloned(),
            };
            let len = entries.len() + usize::from(up.is_some());
            match key.code {
                KeyCode::Down => {
                    if len > 0 {
                        self.prompt_selected = (self.prompt_selected + 1).min(len - 1);
                    }
                }
                KeyCode::Up => self.prompt_selected = self.prompt_selected.saturating_sub(1),
                KeyCode::Left | KeyCode::Backspace => {
                    if let Some(up) = up {
                        self.browse_to(up);
                    }
                }
                KeyCode::Right => match (entry, up) {
                    (Some((path, true)), _) => self.browse_to(path),
                    // The ".." row leads out of the directory either way.
                    (None, Some(up)) => self.browse_to(up),
                    _ => {}
                },
                KeyCode::Enter => self.confirm_prompt(),
                // Typing a path is often quicker than walking to it.
                KeyCode::Tab => {
                    let typed = match prompt {
                        Prompt::Browse {
                            pick: Pick::OpenFile,
                            ..
                        } => Prompt::OpenPath,
                        Prompt::Browse {
                            pick: Pick::OpenFolder,
                            ..
                        } => Prompt::OpenFolder,
                        _ => Prompt::AddFolderPath,
                    };
                    self.prompt_input.clear();
                    self.prompt = Some(typed);
                }
                _ => {}
            }
            return;
        }

        // Selection prompts.
        if !prompt.is_input() {
            let len = prompt.list_len(
                self.themes.len(),
                self.settings.recent_projects.len(),
                self.git.repos.len(),
                self.git.worktrees.len(),
            );
            match key.code {
                KeyCode::Down => {
                    if len > 0 {
                        self.prompt_selected = (self.prompt_selected + 1).min(len - 1);
                    }
                }
                KeyCode::Up => self.prompt_selected = self.prompt_selected.saturating_sub(1),
                KeyCode::Char('y') | KeyCode::Char('Y') => self.confirm_prompt(),
                KeyCode::Char('n') | KeyCode::Char('N') => {
                    self.prompt = None;
                }
                KeyCode::Enter => self.confirm_prompt(),
                _ => {}
            }
            return;
        }

        match key.code {
            KeyCode::Char(c) => self.prompt_input.push(c),
            KeyCode::Backspace => {
                self.prompt_input.pop();
            }
            KeyCode::Enter => self.confirm_prompt(),
            _ => {}
        }
    }

    fn confirm_prompt(&mut self) {
        let Some(prompt) = self.prompt.take() else {
            return;
        };
        let input = std::mem::take(&mut self.prompt_input);
        match prompt {
            Prompt::NewFile(dir) => match fs_ops::create_file(&dir, &input) {
                Ok(path) => {
                    self.tree.expanded.insert(dir);
                    self.tree.reveal(&path);
                    self.open(path);
                }
                Err(e) => self.status = format!("create failed: {e}"),
            },
            Prompt::NewDir(dir) => match fs_ops::create_dir(&dir, &input) {
                Ok(path) => {
                    self.tree.expanded.insert(dir);
                    self.tree.reveal(&path);
                }
                Err(e) => self.status = format!("create failed: {e}"),
            },
            Prompt::Browse { pick, dir, entries } => {
                let up = dir.parent().map(Path::to_path_buf);
                let row = self.prompt_selected;
                let entry = match &up {
                    Some(_) if row == 0 => None,
                    Some(_) => entries.get(row - 1).cloned(),
                    None => entries.get(row).cloned(),
                };
                match entry {
                    // The ".." row only ever navigates.
                    None => {
                        self.prompt = Some(Prompt::Browse { pick, dir, entries });
                        if let Some(up) = up {
                            self.browse_to(up);
                        }
                    }
                    Some((path, true)) => match pick {
                        // A folder is the answer for the folder pickers; for a
                        // file pick, Enter walks into it.
                        Pick::OpenFolder => self.set_root(path),
                        Pick::AddFolder => self.add_folder(path),
                        Pick::OpenFile => {
                            self.prompt = Some(Prompt::Browse { pick, dir, entries });
                            self.browse_to(path);
                        }
                    },
                    Some((path, false)) => self.open(path),
                }
            }
            Prompt::Rename(path) => match fs_ops::rename(&path, &input) {
                Ok(to) => {
                    self.buffers.retarget(&path, &to);
                    self.tree.rebuild_keeping(&to);
                }
                Err(e) => self.status = format!("rename failed: {e}"),
            },
            Prompt::MoveTo(path) => {
                let dest = if input.starts_with('/') {
                    PathBuf::from(&input)
                } else {
                    self.project.root_or_cwd().join(&input)
                };
                match fs_ops::move_into(&path, &dest) {
                    Ok(to) => {
                        self.buffers.retarget(&path, &to);
                        self.tree.expanded.insert(dest);
                        self.tree.rebuild_keeping(&to);
                    }
                    Err(e) => self.status = format!("move failed: {e}"),
                }
            }
            Prompt::ConfirmDelete(path) => match fs_ops::delete(&path) {
                Ok(()) => {
                    self.buffers.close_path(&path);
                    self.edit.truncate(self.buffers.list.len());
                    self.tree.rebuild();
                }
                Err(e) => self.status = format!("delete failed: {e}"),
            },
            Prompt::ConfirmClose { index, name } => {
                if self.buffers.save(index) {
                    self.git.invalidate();
                    self.close_tab_now(index);
                } else {
                    self.status = format!("could not write \"{name}\"");
                }
            }
            Prompt::GitRepo => {
                self.git.select_repo(self.prompt_selected);
                self.git_selected = 0;
            }
            Prompt::GitWorktree => {
                self.git.select_worktree(self.prompt_selected);
                self.git_selected = 0;
            }
            Prompt::OpenPath => {
                let path = self.resolve(&input);
                if path.is_dir() {
                    self.set_root(path);
                } else {
                    self.open(path.clone());
                    self.tree.reveal(&path);
                }
            }
            Prompt::OpenFolder => {
                let path = self.resolve(&input);
                if path.is_dir() {
                    self.set_root(path);
                } else {
                    self.status = format!("not a folder: {}", path.display());
                }
            }
            Prompt::RenameTerminal(index) => self.shell.sessions.rename(index, &input),
            Prompt::AddFolderPath => {
                let path = self.resolve(&input);
                self.add_folder(path);
            }
            Prompt::SaveAs => {
                let path = self.resolve(&input);
                match self.buffers.active_mut() {
                    Some(buf) if !input.trim().is_empty() => {
                        buf.path = path.clone();
                        buf.extension = path
                            .extension()
                            .map(|e| e.to_string_lossy().into_owned())
                            .unwrap_or_default();
                        if self.buffers.save_active() {
                            self.tree.rebuild();
                            self.tree.reveal(&path);
                            self.status = format!("saved as {}", short(&path));
                        } else {
                            self.status = format!("could not write {}", path.display());
                        }
                    }
                    _ => self.status = "nothing to save".into(),
                }
            }
            Prompt::Help(_) => {}
            Prompt::Recent => {
                if let Some(path) = self.settings.recent_projects.get(self.prompt_selected) {
                    let path = path.clone();
                    if path.is_dir() {
                        self.set_root(path);
                    } else {
                        self.status = format!("gone: {}", path.display());
                    }
                }
            }
            Prompt::Themes => {
                self.theme_index = self.prompt_selected.min(self.themes.len() - 1);
                self.syntax.set_theme(&self.themes[self.theme_index]);
                self.mark_dirty();
                self.settings.theme = self.themes[self.theme_index].name.clone();
                let _ = self.settings.save();
                self.status = format!("theme: {}", self.themes[self.theme_index].name);
            }
            Prompt::Goto { candidates, .. } => {
                if let Some(c) = candidates.get(self.prompt_selected) {
                    let (path, line) = (c.path.clone(), c.line);
                    self.jump_to(path, line);
                }
            }
        }
    }
}

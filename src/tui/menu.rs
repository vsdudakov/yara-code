//! Dropdown menus: the navigator's right-click menu and the File menu in the
//! top bar. Both are the same widget with different entries.

use std::path::PathBuf;

use crate::core::command::{Command, FILE_MENU};

#[derive(Clone, Copy, PartialEq)]
pub enum MenuItem {
    Open,
    NewFile,
    NewDir,
    Rename,
    Move,
    Delete,
    /// Drops a folder from the project, leaving it on disk.
    RemoveFolder,
    /// A line that says something rather than doing something — the Help
    /// menu's version.
    Note(&'static str),
    /// An entry of the File menu, carrying the command it runs.
    Command(Command),
}

impl MenuItem {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Command(command) => command.label(),
            Self::Open => "Open",
            Self::NewFile => "New File",
            Self::NewDir => "New Folder",
            Self::Rename => "Rename",
            Self::Move => "Move To...",
            Self::Delete => "Delete",
            Self::RemoveFolder => "Remove Folder from Project",
            Self::Note(text) => text,
        }
    }

    /// Items after which a separator is drawn. File-menu entries carry their
    /// own separators, so only the navigator items answer here.
    fn ends_group(&self) -> bool {
        matches!(self, Self::Open | Self::NewDir | Self::Move | Self::Delete)
    }
}

pub struct Menu {
    /// The row the menu was opened on; `None` for empty space (project root).
    pub target: Option<PathBuf>,
    /// Directory new entries go into.
    pub dir: PathBuf,
    /// Drawn rows; `None` is a separator line.
    rows: Vec<Option<MenuItem>>,
    /// Right-hand hint per row, used for the File menu's key chords.
    shortcuts: Vec<String>,
    pub selected: usize,
    pub x: u16,
    pub y: u16,
}

impl Menu {
    /// `dir` is where new entries would go — `None` when no folder is open at
    /// all, which leaves adding one as the only thing the menu can offer.
    pub fn for_row(
        target: Option<PathBuf>,
        is_dir: bool,
        is_root: bool,
        dir: Option<PathBuf>,
        x: u16,
        y: u16,
    ) -> Self {
        let mut items = match (&target, &dir) {
            (None, None) => Vec::new(),
            (None, Some(_)) => vec![MenuItem::NewFile, MenuItem::NewDir],
            (Some(_), _) if is_dir => vec![
                MenuItem::NewFile,
                MenuItem::NewDir,
                MenuItem::Rename,
                MenuItem::Move,
                MenuItem::Delete,
            ],
            (Some(_), _) => vec![
                MenuItem::Open,
                MenuItem::NewFile,
                MenuItem::NewDir,
                MenuItem::Rename,
                MenuItem::Move,
                MenuItem::Delete,
            ],
        };
        // A project folder is not a plain directory: it can be renamed or
        // deleted on disk, and it can also just leave the project.
        if is_root {
            items.retain(|item| {
                !matches!(
                    item,
                    MenuItem::Rename | MenuItem::Move | MenuItem::Delete
                )
            });
            items.push(MenuItem::RemoveFolder);
        }
        items.push(MenuItem::Command(Command::AddFolder));
        let dir = dir.unwrap_or_default();
        let mut rows = Vec::new();
        for (i, item) in items.iter().enumerate() {
            rows.push(Some(*item));
            if item.ends_group() && i + 1 < items.len() {
                rows.push(None);
            }
        }
        let shortcuts = vec![String::new(); rows.len()];
        Self {
            target,
            dir,
            rows,
            shortcuts,
            selected: 0,
            x,
            y,
        }
    }

    /// One of the top bar's menus, with each entry's current key chord shown
    /// on the right. `title` heads the list where a menu has something to say
    /// about itself — the Help menu's version line.
    pub fn commands(
        entries: &'static [Option<Command>],
        title: Option<String>,
        x: u16,
        y: u16,
        chord_for: impl Fn(Command) -> Option<String>,
    ) -> Self {
        let mut rows = Vec::new();
        let mut shortcuts = Vec::new();
        if let Some(title) = title {
            rows.push(Some(MenuItem::Note(Box::leak(title.into_boxed_str()))));
            shortcuts.push(String::new());
            rows.push(None);
            shortcuts.push(String::new());
        }
        for entry in entries {
            match entry {
                Some(command) => {
                    rows.push(Some(MenuItem::Command(*command)));
                    shortcuts.push(chord_for(*command).unwrap_or_default());
                }
                None => {
                    rows.push(None);
                    shortcuts.push(String::new());
                }
            }
        }
        let mut menu = Self {
            target: None,
            dir: PathBuf::new(),
            rows,
            shortcuts,
            selected: 0,
            x,
            y,
        };
        // Start on the first real entry, not a separator or the note.
        if !matches!(menu.item_at_row(0), Some(MenuItem::Command(_))) {
            menu.move_selection(1);
        }
        menu
    }

    /// The File menu, kept as its own name because the top bar and the key
    /// binding both reach for it.
    pub fn file_menu(x: u16, y: u16, chord_for: impl Fn(Command) -> Option<String>) -> Self {
        Self::commands(FILE_MENU, None, x, y, chord_for)
    }

    pub fn shortcut_at_row(&self, row: usize) -> &str {
        self.shortcuts.get(row).map(String::as_str).unwrap_or("")
    }

    /// The drawn rows: `None` is a separator. Hit-testing, rendering and
    /// keyboard navigation all index into this, so they never disagree.
    pub fn rows(&self) -> &[Option<MenuItem>] {
        &self.rows
    }

    /// The item on a drawn row, or `None` for a separator.
    pub fn item_at_row(&self, row: usize) -> Option<MenuItem> {
        self.rows.get(row).copied().flatten()
    }

    pub fn width(&self) -> u16 {
        let longest = self
            .rows
            .iter()
            .enumerate()
            .filter_map(|(i, row)| {
                let item = (*row)?;
                let shortcut = self.shortcut_at_row(i);
                let gap = if shortcut.is_empty() { 0 } else { 3 };
                Some(item.label().chars().count() + gap + shortcut.chars().count())
            })
            .max()
            .unwrap_or(8);
        (longest + 4) as u16
    }

    pub fn height(&self) -> u16 {
        self.rows.len() as u16 + 2
    }

    /// Moves the highlight over real items, stepping across separators.
    pub fn move_selection(&mut self, delta: isize) {
        let rows = &self.rows;
        let last = rows.len().saturating_sub(1) as isize;
        let mut next = self.selected as isize;
        loop {
            next += delta.signum();
            if next < 0 || next > last {
                return;
            }
            if matches!(rows[next as usize], Some(MenuItem::Note(_))) {
                continue;
            }
            if rows[next as usize].is_some() {
                self.selected = next as usize;
                return;
            }
        }
    }

    /// The highlighted item, if the selection sits on one.
    pub fn selected_item(&self) -> Option<MenuItem> {
        self.item_at_row(self.selected)
    }
}

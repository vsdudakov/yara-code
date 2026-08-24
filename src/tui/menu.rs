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
        }
    }

    /// Items after which a separator is drawn. File-menu entries carry their
    /// own separators, so only the navigator items answer here.
    fn ends_group(&self) -> bool {
        matches!(self, Self::Open | Self::NewDir | Self::Move)
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
    pub fn for_row(target: Option<PathBuf>, is_dir: bool, dir: PathBuf, x: u16, y: u16) -> Self {
        let items = match &target {
            None => vec![MenuItem::NewFile, MenuItem::NewDir],
            Some(_) if is_dir => vec![
                MenuItem::NewFile,
                MenuItem::NewDir,
                MenuItem::Rename,
                MenuItem::Move,
                MenuItem::Delete,
            ],
            Some(_) => vec![
                MenuItem::Open,
                MenuItem::NewFile,
                MenuItem::NewDir,
                MenuItem::Rename,
                MenuItem::Move,
                MenuItem::Delete,
            ],
        };
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

    /// The File menu, with each entry's current key chord shown on the right.
    pub fn file_menu(x: u16, y: u16, chord_for: impl Fn(Command) -> Option<String>) -> Self {
        let mut rows = Vec::new();
        let mut shortcuts = Vec::new();
        for entry in FILE_MENU {
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
        // Start on the first real entry, not a separator.
        if menu.item_at_row(0).is_none() {
            menu.move_selection(1);
        }
        menu
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

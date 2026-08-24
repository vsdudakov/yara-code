//! Flattened view of the project tree — what the navigator actually draws.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use crate::core::fs_ops;

pub struct Row {
    pub path: PathBuf,
    pub is_dir: bool,
    pub depth: usize,
}

pub struct Tree {
    pub root: PathBuf,
    pub expanded: HashSet<PathBuf>,
    pub selected: usize,
    pub scroll: usize,
    rows: Vec<Row>,
}

impl Tree {
    pub fn new(root: PathBuf) -> Self {
        let mut tree = Self {
            root,
            expanded: HashSet::new(),
            selected: 0,
            scroll: 0,
            rows: Vec::new(),
        };
        tree.rebuild();
        tree
    }

    pub fn rows(&self) -> &[Row] {
        &self.rows
    }

    pub fn selected_path(&self) -> Option<&Path> {
        self.rows.get(self.selected).map(|r| r.path.as_path())
    }

    /// The directory new entries should go into: the selected folder itself, or
    /// the parent of the selected file.
    pub fn target_dir(&self) -> PathBuf {
        match self.rows.get(self.selected) {
            Some(row) if row.is_dir => row.path.clone(),
            Some(row) => row.path.parent().unwrap_or(&self.root).to_path_buf(),
            None => self.root.clone(),
        }
    }

    pub fn rebuild(&mut self) {
        let mut rows = Vec::new();
        collect(&self.root, 0, &self.expanded, &mut rows);
        self.rows = rows;
        if self.selected >= self.rows.len() {
            self.selected = self.rows.len().saturating_sub(1);
        }
    }

    /// Rebuilds and keeps the cursor on `path` if it is still visible.
    pub fn rebuild_keeping(&mut self, path: &Path) {
        self.rebuild();
        if let Some(i) = self.rows.iter().position(|r| r.path == path) {
            self.selected = i;
        }
    }

    pub fn move_selection(&mut self, delta: isize) {
        if self.rows.is_empty() {
            return;
        }
        let last = self.rows.len() - 1;
        let next = self.selected as isize + delta;
        self.selected = next.clamp(0, last as isize) as usize;
    }

    pub fn toggle_selected(&mut self) {
        let Some(row) = self.rows.get(self.selected) else {
            return;
        };
        if !row.is_dir {
            return;
        }
        let path = row.path.clone();
        if self.expanded.contains(&path) {
            self.expanded.remove(&path);
        } else {
            self.expanded.insert(path.clone());
        }
        self.rebuild_keeping(&path);
    }

    /// Expands every ancestor of `path` so it becomes visible, then selects it.
    pub fn reveal(&mut self, path: &Path) {
        let mut dir = path.parent();
        while let Some(d) = dir {
            if !d.starts_with(&self.root) && d != self.root {
                break;
            }
            self.expanded.insert(d.to_path_buf());
            if d == self.root {
                break;
            }
            dir = d.parent();
        }
        self.rebuild_keeping(path);
    }

    /// Keeps the selected row inside a viewport `height` rows tall.
    pub fn clamp_scroll(&mut self, height: usize) {
        if height == 0 {
            return;
        }
        if self.selected < self.scroll {
            self.scroll = self.selected;
        } else if self.selected >= self.scroll + height {
            self.scroll = self.selected + 1 - height;
        }
    }
}

fn collect(dir: &Path, depth: usize, expanded: &HashSet<PathBuf>, out: &mut Vec<Row>) {
    for (path, is_dir) in fs_ops::list_dir(dir) {
        let expanded_here = is_dir && expanded.contains(&path);
        out.push(Row {
            path: path.clone(),
            is_dir,
            depth,
        });
        if expanded_here {
            collect(&path, depth + 1, expanded, out);
        }
    }
}

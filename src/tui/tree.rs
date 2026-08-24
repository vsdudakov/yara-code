//! Flattened view of the project tree — what the navigator actually draws.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use crate::core::fs_ops;

pub struct Row {
    pub path: PathBuf,
    pub is_dir: bool,
    pub depth: usize,
    /// A project folder's own header row, drawn only when several are open.
    pub is_root: bool,
}

pub struct Tree {
    /// Project folders, primary first; empty when no folder is open. A single
    /// folder is drawn without a header row, as it always was.
    pub roots: Vec<PathBuf>,
    pub expanded: HashSet<PathBuf>,
    pub selected: usize,
    pub scroll: usize,
    rows: Vec<Row>,
}

impl Tree {
    pub fn new(root: PathBuf) -> Self {
        Self::with_roots(vec![root])
    }

    pub fn with_roots(roots: Vec<PathBuf>) -> Self {
        let mut tree = Self {
            roots,
            expanded: HashSet::new(),
            selected: 0,
            scroll: 0,
            rows: Vec::new(),
        };
        // Added folders start open, so they are visible the moment they land.
        if tree.roots.len() > 1 {
            for root in tree.roots.clone() {
                tree.expanded.insert(root);
            }
        }
        tree.rebuild();
        tree
    }

    /// The primary project folder — where anything without a better anchor
    /// (new files on empty space) goes. `None` with no folder open.
    pub fn root(&self) -> Option<&Path> {
        self.roots.first().map(PathBuf::as_path)
    }

    /// Replaces the folder list, keeping what is expanded and selected.
    pub fn set_roots(&mut self, roots: Vec<PathBuf>) {
        let keep = self.selected_path().map(Path::to_path_buf);
        for root in &roots {
            if roots.len() > 1 {
                self.expanded.insert(root.clone());
            }
        }
        self.roots = roots;
        match keep {
            Some(path) => self.rebuild_keeping(&path),
            None => self.rebuild(),
        }
    }

    pub fn rows(&self) -> &[Row] {
        &self.rows
    }

    pub fn selected_path(&self) -> Option<&Path> {
        self.rows.get(self.selected).map(|r| r.path.as_path())
    }

    /// The directory new entries should go into: the selected folder itself,
    /// or the parent of the selected file. `None` with no folder open, which is
    /// what stops a new file from landing outside the project.
    pub fn target_dir(&self) -> Option<PathBuf> {
        match self.rows.get(self.selected) {
            Some(row) if row.is_dir => Some(row.path.clone()),
            Some(row) => Some(
                row.path
                    .parent()
                    .map(Path::to_path_buf)
                    .unwrap_or_else(|| row.path.clone()),
            ),
            None => self.root().map(Path::to_path_buf),
        }
    }

    pub fn rebuild(&mut self) {
        let mut rows = Vec::new();
        if self.roots.len() == 1 {
            collect(&self.roots[0], 0, &self.expanded, &mut rows);
        } else {
            for root in &self.roots {
                let expanded = self.expanded.contains(root);
                rows.push(Row {
                    path: root.clone(),
                    is_dir: true,
                    depth: 0,
                    is_root: true,
                });
                if expanded {
                    collect(root, 1, &self.expanded, &mut rows);
                }
            }
        }
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
        let Some(root) = self
            .roots
            .iter()
            .find(|r| path.starts_with(r))
            .cloned()
        else {
            return;
        };
        self.expanded.insert(root.clone());
        let mut dir = path.parent();
        while let Some(d) = dir {
            if !d.starts_with(&root) {
                break;
            }
            self.expanded.insert(d.to_path_buf());
            if d == root {
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
            is_root: false,
        });
        if expanded_here {
            collect(&path, depth + 1, expanded, out);
        }
    }
}

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
        let Some(root) = self.roots.iter().find(|r| path.starts_with(r)).cloned() else {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::test_support::Dir;

    /// A folder with a nested one inside, so rows have something to expand.
    fn project(tag: &str) -> Dir {
        let dir = Dir::new(tag);
        dir.file("src/main.rs", "");
        dir.file("src/lib.rs", "");
        dir.file("README.md", "");
        dir
    }

    #[test]
    fn one_folder_is_drawn_without_a_header_row() {
        let dir = project("yara-tree-one");
        let tree = Tree::new(dir.path().to_path_buf());
        let names: Vec<String> = tree
            .rows()
            .iter()
            .map(|r| r.path.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        // Folders first, then files; nothing is expanded to begin with.
        assert_eq!(names, ["src", "README.md"]);
        assert!(tree.rows().iter().all(|r| !r.is_root));
        assert_eq!(tree.root(), Some(dir.path()));
    }

    #[test]
    fn several_folders_each_head_their_own_subtree() {
        let one = project("yara-tree-a");
        let two = project("yara-tree-b");
        let tree = Tree::with_roots(vec![one.path().to_path_buf(), two.path().to_path_buf()]);
        let roots: Vec<&Row> = tree.rows().iter().filter(|r| r.is_root).collect();
        assert_eq!(roots.len(), 2);
        // Added folders start open, so a new folder is not an empty row.
        assert!(tree.rows().len() > 2);
        assert!(tree.rows().iter().any(|r| r.depth == 1));
    }

    #[test]
    fn a_folder_opens_and_closes_under_the_cursor() {
        let dir = project("yara-tree-toggle");
        let mut tree = Tree::new(dir.path().to_path_buf());
        assert_eq!(tree.rows().len(), 2);
        tree.selected = 0; // src
        tree.toggle_selected();
        assert_eq!(tree.rows().len(), 4, "src brought its two files");
        tree.toggle_selected();
        assert_eq!(tree.rows().len(), 2);
        // A file has nothing to toggle.
        tree.selected = 1;
        tree.toggle_selected();
        assert_eq!(tree.rows().len(), 2);
    }

    #[test]
    fn revealing_a_file_expands_everything_above_it() {
        let dir = project("yara-tree-reveal");
        let mut tree = Tree::new(dir.path().to_path_buf());
        let file = dir.path().join("src").join("lib.rs");
        tree.reveal(&file);
        assert_eq!(tree.selected_path(), Some(file.as_path()));
        // A path in no folder of the project is not revealed at all.
        let before = tree.selected;
        tree.reveal(Path::new("/elsewhere/file.rs"));
        assert_eq!(tree.selected, before);
    }

    #[test]
    fn new_entries_go_beside_what_is_selected() {
        let dir = project("yara-tree-target");
        let mut tree = Tree::new(dir.path().to_path_buf());
        // A folder takes them inside itself…
        tree.selected = 0;
        assert_eq!(tree.target_dir(), Some(dir.path().join("src")));
        // …a file, into the folder holding it.
        tree.selected = 1;
        assert_eq!(tree.target_dir(), Some(dir.path().to_path_buf()));
        // With no folder open there is nowhere to put them.
        let empty = Tree::with_roots(Vec::new());
        assert_eq!(empty.target_dir(), None);
        assert_eq!(empty.root(), None);
        assert!(empty.rows().is_empty());
    }

    #[test]
    fn the_cursor_stays_inside_the_list() {
        let dir = project("yara-tree-move");
        let mut tree = Tree::new(dir.path().to_path_buf());
        tree.move_selection(-5);
        assert_eq!(tree.selected, 0, "it stops at the top");
        tree.move_selection(50);
        assert_eq!(tree.selected, tree.rows().len() - 1);
        // An empty tree has nothing to move through.
        let mut empty = Tree::with_roots(Vec::new());
        empty.move_selection(1);
        assert_eq!(empty.selected, 0);
    }

    #[test]
    fn the_view_scrolls_to_keep_the_cursor_visible() {
        let dir = Dir::new("yara-tree-scroll");
        for i in 0..20 {
            dir.file(&format!("file{i:02}.txt"), "");
        }
        let mut tree = Tree::new(dir.path().to_path_buf());
        tree.selected = 15;
        tree.clamp_scroll(5);
        assert_eq!(tree.scroll, 11, "the cursor is the last visible row");
        tree.selected = 2;
        tree.clamp_scroll(5);
        assert_eq!(tree.scroll, 2, "and the first when it moves up");
        // A pane with no height cannot scroll.
        tree.clamp_scroll(0);
        assert_eq!(tree.scroll, 2);
    }

    #[test]
    fn adding_a_folder_keeps_the_cursor_where_it_was() {
        let one = project("yara-tree-set-a");
        let two = project("yara-tree-set-b");
        let mut tree = Tree::new(one.path().to_path_buf());
        let readme = one.path().join("README.md");
        tree.reveal(&readme);
        tree.set_roots(vec![one.path().to_path_buf(), two.path().to_path_buf()]);
        assert_eq!(tree.selected_path(), Some(readme.as_path()));
        assert_eq!(tree.roots.len(), 2);
    }
}

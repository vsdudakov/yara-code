//! The project's files as the sidebar lists them: folders first, then
//! files, each alphabetical, with the opened folders unfolded in place.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

pub struct Row {
    pub path: PathBuf,
    pub is_dir: bool,
    pub depth: usize,
}

pub struct Tree {
    pub root: PathBuf,
    expanded: BTreeSet<PathBuf>,
    pub selected: usize,
    rows: Vec<Row>,
}

/// A folder's entries, folders first, `.git` and the editor's own folder
/// left out.
pub fn list_dir(dir: &Path) -> Vec<(PathBuf, bool)> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut out: Vec<(PathBuf, bool)> = entries
        .filter_map(|e| e.ok())
        .map(|e| {
            let is_dir = e.file_type().is_ok_and(|t| t.is_dir());
            (e.path(), is_dir)
        })
        .filter(|(p, _)| p.file_name().is_none_or(|n| n != ".git" && n != ".ycode"))
        .collect();
    out.sort_by(|(pa, da), (pb, db)| {
        db.cmp(da).then_with(|| {
            let na = pa.file_name().unwrap_or_default().to_ascii_lowercase();
            let nb = pb.file_name().unwrap_or_default().to_ascii_lowercase();
            na.cmp(&nb)
        })
    });
    out
}

/// Every file under `root`, relative and with forward slashes — what the
/// file finder ranks. Stops at a generous cap so a huge tree cannot hang the
/// editor.
pub fn all_files(root: &Path) -> Vec<String> {
    const CAP: usize = 20_000;
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for (path, is_dir) in list_dir(&dir) {
            if is_dir {
                stack.push(path);
            } else if let Ok(rel) = path.strip_prefix(root) {
                out.push(rel.to_string_lossy().replace('\\', "/"));
                if out.len() == CAP {
                    return out;
                }
            }
        }
    }
    out.sort();
    out
}

impl Tree {
    pub fn new(root: PathBuf) -> Self {
        let mut tree = Self {
            root,
            expanded: BTreeSet::new(),
            selected: 0,
            rows: Vec::new(),
        };
        tree.rebuild();
        tree
    }

    pub fn rows(&self) -> &[Row] {
        &self.rows
    }

    pub fn selected_row(&self) -> Option<&Row> {
        self.rows.get(self.selected)
    }

    /// Reads the folders again, keeping the cursor on the same path.
    pub fn rebuild(&mut self) {
        let keep = self.selected_row().map(|r| r.path.clone());
        let mut rows = Vec::new();
        collect(&self.root, 0, &self.expanded, &mut rows);
        self.rows = rows;
        self.selected = keep
            .and_then(|path| self.rows.iter().position(|r| r.path == path))
            .unwrap_or(self.selected.min(self.rows.len().saturating_sub(1)));
    }

    pub fn move_selection(&mut self, delta: isize) {
        let last = self.rows.len().saturating_sub(1) as isize;
        self.selected = (self.selected as isize + delta).clamp(0, last) as usize;
    }

    /// Opens or closes the folder under the cursor; a file is left to the
    /// caller.
    pub fn toggle_selected(&mut self) {
        let Some(row) = self.selected_row().filter(|r| r.is_dir) else {
            return;
        };
        let path = row.path.clone();
        if !self.expanded.remove(&path) {
            self.expanded.insert(path);
        }
        self.rebuild();
    }

    /// Unfolds every folder above `path` and puts the cursor on it.
    pub fn reveal(&mut self, path: &Path) {
        let mut dir = path.parent();
        while let Some(d) = dir.filter(|d| d.starts_with(&self.root) && *d != self.root) {
            self.expanded.insert(d.to_path_buf());
            dir = d.parent();
        }
        self.rebuild();
        if let Some(i) = self.rows.iter().position(|r| r.path == path) {
            self.selected = i;
        }
    }
}

fn collect(dir: &Path, depth: usize, expanded: &BTreeSet<PathBuf>, out: &mut Vec<Row>) {
    for (path, is_dir) in list_dir(dir) {
        let open = is_dir && expanded.contains(&path);
        out.push(Row {
            path: path.clone(),
            is_dir,
            depth,
        });
        if open {
            collect(&path, depth + 1, expanded, out);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::Dir;

    fn project(tag: &str) -> Dir {
        let dir = Dir::new(tag);
        dir.file("src/main.rs", "");
        dir.file("src/lib.rs", "");
        dir.file("README.md", "");
        dir.file(".git/HEAD", "");
        dir
    }

    fn names(tree: &Tree) -> Vec<String> {
        tree.rows()
            .iter()
            .map(|r| {
                format!(
                    "{}{}",
                    "  ".repeat(r.depth),
                    r.path.file_name().unwrap().to_string_lossy()
                )
            })
            .collect()
    }

    #[test]
    fn folders_come_first_closed_and_git_is_hidden() {
        let dir = project("yara-tree");
        let tree = Tree::new(dir.path().to_path_buf());
        assert_eq!(names(&tree), ["src", "README.md"]);
    }

    #[test]
    fn a_folder_opens_and_closes_under_the_cursor_and_a_file_is_revealed() {
        let dir = project("yara-tree-toggle");
        let mut tree = Tree::new(dir.path().to_path_buf());
        tree.toggle_selected();
        assert_eq!(names(&tree), ["src", "  lib.rs", "  main.rs", "README.md"]);
        tree.move_selection(10);
        assert_eq!(tree.selected, 3);
        tree.toggle_selected();
        assert_eq!(tree.rows().len(), 4, "a file does not fold");
        tree.move_selection(-10);
        tree.toggle_selected();
        assert_eq!(names(&tree), ["src", "README.md"]);
        tree.reveal(&dir.path().join("src/main.rs"));
        assert_eq!(tree.selected, 2);
        assert!(tree.selected_row().unwrap().path.ends_with("main.rs"));
    }

    #[test]
    fn every_file_is_listed_relative_for_the_finder() {
        let dir = project("yara-tree-all");
        assert_eq!(
            all_files(dir.path()),
            ["README.md", "src/lib.rs", "src/main.rs"]
        );
    }
}

//! The folders a window is working on. A window can start with none — an empty
//! editor — and "Add Folder to Project" puts folders in it. The first one is
//! the project root; every frontend — navigator, search, go-to-definition —
//! treats the whole list as the project.

use std::path::{Path, PathBuf};

#[derive(Default)]
pub struct Project {
    /// May be empty — that is the editor with no project open. `roots[0]` is
    /// the primary root, the one git, the terminal and relative paths are
    /// anchored to.
    roots: Vec<PathBuf>,
}

impl Project {
    pub fn new(root: PathBuf) -> Self {
        Self {
            roots: vec![canonical(root)],
        }
    }

    /// A project with no folders: the editor opens like this when it is
    /// launched without a path.
    pub fn empty() -> Self {
        Self::default()
    }

    pub fn opened(root: Option<PathBuf>) -> Self {
        match root {
            Some(root) => Self::new(root),
            None => Self::empty(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.roots.is_empty()
    }

    /// The primary folder, or `None` while no folder is open.
    pub fn root(&self) -> Option<&Path> {
        self.roots.first().map(PathBuf::as_path)
    }

    /// Where things anchored to a directory go when no folder is open: the
    /// process's working directory. Used for the shell's cwd and for reading
    /// typed relative paths.
    pub fn root_or_cwd(&self) -> PathBuf {
        match self.roots.first() {
            Some(root) => root.clone(),
            None => std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
        }
    }

    pub fn roots(&self) -> &[PathBuf] {
        &self.roots
    }

    /// True once a second folder is in play, which is what makes the navigator
    /// draw a header row per folder.
    pub fn is_multi_root(&self) -> bool {
        self.roots.len() > 1
    }

    /// Switches the project to a single folder, dropping any added ones.
    pub fn set_root(&mut self, root: PathBuf) -> PathBuf {
        let root = canonical(root);
        self.roots = vec![root.clone()];
        root
    }

    /// Adds another folder. Overlapping folders are refused: one inside the
    /// other would show the same files twice and search them twice.
    pub fn add(&mut self, path: PathBuf) -> Result<PathBuf, String> {
        let path = canonical(path);
        if !path.is_dir() {
            return Err(format!("not a folder: {}", path.display()));
        }
        for root in &self.roots {
            if *root == path {
                return Err(format!("already in the project: {}", name_of(&path)));
            }
            if path.starts_with(root) {
                return Err(format!("already inside {}", name_of(root)));
            }
            if root.starts_with(&path) {
                return Err(format!("would contain {}", name_of(root)));
            }
        }
        self.roots.push(path.clone());
        Ok(path)
    }

    /// Removes a folder from the project; removing the last one leaves the
    /// editor with no project open, which is a valid state.
    pub fn remove(&mut self, path: &Path) -> Result<(), String> {
        match self.roots.iter().position(|r| r == path) {
            Some(i) => {
                self.roots.remove(i);
                Ok(())
            }
            None => Err(format!("not a project folder: {}", name_of(path))),
        }
    }

    pub fn is_root(&self, path: &Path) -> bool {
        self.roots.iter().any(|r| r == path)
    }

    /// The folder `path` belongs to, if any.
    pub fn owner(&self, path: &Path) -> Option<&Path> {
        self.roots
            .iter()
            .find(|r| path.starts_with(r))
            .map(PathBuf::as_path)
    }

    /// The folder name shown on a root row.
    pub fn name_of(path: &Path) -> String {
        name_of(path)
    }

    /// How a path is written in the UI: relative to its folder, prefixed with
    /// the folder's name once more than one is open.
    pub fn display(&self, path: &Path) -> String {
        let Some(root) = self.owner(path) else {
            return path.display().to_string();
        };
        let rest = path
            .strip_prefix(root)
            .unwrap_or(path)
            .display()
            .to_string();
        if !self.is_multi_root() {
            return rest;
        }
        let name = name_of(root);
        if rest.is_empty() {
            name
        } else {
            format!("{name}/{rest}")
        }
    }
}

fn canonical(path: PathBuf) -> PathBuf {
    path.canonicalize().unwrap_or(path)
}

fn name_of(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("yara-project-{tag}"));
        let _ = std::fs::create_dir_all(&dir);
        dir.canonicalize().unwrap_or(dir)
    }

    #[test]
    fn overlapping_folders_are_refused() {
        let root = temp_dir("root");
        let nested = root.join("nested");
        let _ = std::fs::create_dir_all(&nested);
        let mut project = Project::new(root.clone());
        assert!(project.add(nested).is_err(), "a child of a root");
        assert!(project.add(root.clone()).is_err(), "the root itself");
        assert!(
            project.add(root.parent().unwrap().to_path_buf()).is_err(),
            "a parent of a root"
        );
        assert!(!project.is_multi_root());
    }

    /// The path as this platform writes it — Windows separates with `\`.
    fn shown(parts: &[&str]) -> String {
        parts
            .iter()
            .fold(PathBuf::new(), |path, part| path.join(part))
            .display()
            .to_string()
    }

    #[test]
    fn added_folders_show_their_name_in_paths() {
        let root = temp_dir("a");
        let other = temp_dir("b");
        let file = |dir: &PathBuf| dir.join("src").join("main.rs");
        let mut project = Project::new(root.clone());
        assert_eq!(project.display(&file(&root)), shown(&["src", "main.rs"]));
        project.add(other.clone()).unwrap();
        assert!(project.is_multi_root());
        assert_eq!(
            project.display(&file(&other)),
            shown(&["yara-project-b", "src", "main.rs"])
        );
        // The primary root is prefixed too, so the two never read alike.
        assert_eq!(
            project.display(&file(&root)),
            shown(&["yara-project-a", "src", "main.rs"])
        );
    }

    #[test]
    fn folders_can_be_removed_down_to_an_empty_project() {
        let root = temp_dir("keep");
        let other = temp_dir("drop");
        let mut project = Project::new(root.clone());
        project.add(other.clone()).unwrap();
        assert!(project.remove(&other).is_ok());
        assert!(!project.is_multi_root());
        assert!(project.remove(&root).is_ok());
        assert!(project.is_empty());
        assert_eq!(project.root(), None);
        assert!(project.remove(&root).is_err(), "not a project folder now");
    }

    #[test]
    fn an_empty_project_takes_the_first_folder_added() {
        let root = temp_dir("first");
        let mut project = Project::empty();
        assert!(project.is_empty());
        project.add(root.clone()).unwrap();
        assert_eq!(project.root(), Some(root.as_path()));
        assert!(!project.is_multi_root());
    }
}

//! Git integration through the `git` CLI: repository discovery, worktrees,
//! and uncommitted changes — frontend-independent, no extra dependencies.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// How long a cached status stays fresh; views re-poll after this.
pub const REFRESH_EVERY: Duration = Duration::from_secs(2);

/// One changed path, as `git status --porcelain` reports it.
#[derive(Clone, Debug, PartialEq)]
pub struct Change {
    /// The two porcelain columns, e.g. `" M"`, `"A "`, `"??"`.
    pub code: String,
    /// Path relative to the worktree root; for renames, the new name.
    pub path: String,
}

impl Change {
    /// The single letter shown in a change list: the worktree column when it
    /// says anything, the index column otherwise, `U` for untracked.
    pub fn letter(&self) -> char {
        if self.code == "??" {
            return 'U';
        }
        let mut chars = self.code.chars();
        let index = chars.next().unwrap_or(' ');
        let worktree = chars.next().unwrap_or(' ');
        if worktree != ' ' {
            worktree
        } else {
            index
        }
    }
}

/// One worktree of a repository.
#[derive(Clone, Debug, PartialEq)]
pub struct Worktree {
    pub path: PathBuf,
    /// Checked-out branch, or `"detached"`.
    pub branch: String,
}

impl Worktree {
    /// Directory name, for pickers.
    pub fn name(&self) -> String {
        self.path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| self.path.display().to_string())
    }
}

/// Everything behind a git view — the repository and worktree picked, and the
/// cached status — shared by both frontends.
#[derive(Default)]
pub struct GitState {
    /// Project root the repository list was scanned for.
    scanned: Option<PathBuf>,
    pub repos: Vec<PathBuf>,
    pub repo: usize,
    pub worktrees: Vec<Worktree>,
    pub worktree: usize,
    pub changes: Vec<Change>,
    pub error: Option<String>,
    last_refresh: Option<Instant>,
}

impl GitState {
    /// Rescans repositories when the project root changed, and re-polls the
    /// status once the cached one goes stale. Call once per drawn frame.
    pub fn tick(&mut self, root: &Path) {
        if self.scanned.as_deref() != Some(root) {
            self.scanned = Some(root.to_path_buf());
            self.repos = discover_repos(root);
            self.select_repo(0);
        } else if self.stale() {
            self.refresh();
        }
    }

    /// Whether the cached status is due for a re-poll.
    pub fn stale(&self) -> bool {
        self.last_refresh
            .is_none_or(|at| at.elapsed() >= REFRESH_EVERY)
    }

    /// Marks the cached status stale, so the next tick re-reads it right away
    /// (e.g. after a save).
    pub fn invalidate(&mut self) {
        self.last_refresh = None;
    }

    pub fn select_repo(&mut self, index: usize) {
        self.repo = index.min(self.repos.len().saturating_sub(1));
        self.worktree = 0;
        self.worktrees = match self.repos.get(self.repo) {
            Some(repo) => worktrees(repo),
            None => Vec::new(),
        };
        self.refresh();
    }

    pub fn select_worktree(&mut self, index: usize) {
        self.worktree = index.min(self.worktrees.len().saturating_sub(1));
        self.refresh();
    }

    /// The directory whose status is shown: the selected worktree.
    pub fn dir(&self) -> Option<PathBuf> {
        self.worktrees
            .get(self.worktree)
            .map(|w| w.path.clone())
            .or_else(|| self.repos.get(self.repo).cloned())
    }

    fn refresh(&mut self) {
        self.last_refresh = Some(Instant::now());
        let Some(dir) = self.dir() else {
            self.changes.clear();
            self.error = None;
            return;
        };
        match status(&dir) {
            Ok(changes) => {
                self.changes = changes;
                self.error = None;
            }
            Err(message) => {
                self.changes.clear();
                self.error = Some(message);
            }
        }
    }
}

fn git(dir: &Path, args: &[&str]) -> Result<String, String> {
    let out = std::process::Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .map_err(|e| format!("git unavailable: {e}"))?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    } else {
        let err = String::from_utf8_lossy(&out.stderr);
        Err(err.lines().next().unwrap_or("git failed").to_string())
    }
}

/// The top of the repository containing `dir`, if there is one.
pub fn repo_root(dir: &Path) -> Option<PathBuf> {
    let out = git(dir, &["rev-parse", "--show-toplevel"]).ok()?;
    let line = out.lines().next()?.trim();
    if line.is_empty() {
        None
    } else {
        Some(PathBuf::from(line))
    }
}

/// Repositories the project can see: the one holding the root itself, plus
/// every first-level subdirectory that is a repository of its own.
pub fn discover_repos(root: &Path) -> Vec<PathBuf> {
    let mut repos = Vec::new();
    if let Some(repo) = repo_root(root) {
        repos.push(repo);
    }
    if let Ok(rd) = std::fs::read_dir(root) {
        let mut subs: Vec<PathBuf> = rd
            .filter_map(|e| e.ok().map(|e| e.path()))
            // `.git` is a directory in a main worktree and a file in linked
            // worktrees and submodules; both mark a repository.
            .filter(|p| p.is_dir() && p.join(".git").exists())
            .collect();
        subs.sort();
        for sub in subs {
            let repo = repo_root(&sub).unwrap_or(sub);
            if !repos.contains(&repo) {
                repos.push(repo);
            }
        }
    }
    repos
}

/// Worktrees of `repo`, the main one first.
pub fn worktrees(repo: &Path) -> Vec<Worktree> {
    match git(repo, &["worktree", "list", "--porcelain"]) {
        Ok(out) => parse_worktrees(&out),
        Err(_) => vec![Worktree {
            path: repo.to_path_buf(),
            branch: String::new(),
        }],
    }
}

fn parse_worktrees(out: &str) -> Vec<Worktree> {
    let mut list = Vec::new();
    let mut current: Option<Worktree> = None;
    for line in out.lines() {
        if let Some(path) = line.strip_prefix("worktree ") {
            list.extend(current.take());
            current = Some(Worktree {
                path: PathBuf::from(path),
                branch: String::new(),
            });
        } else if let Some(branch) = line.strip_prefix("branch ") {
            if let Some(w) = &mut current {
                w.branch = branch.strip_prefix("refs/heads/").unwrap_or(branch).to_string();
            }
        } else if line == "detached" {
            if let Some(w) = &mut current {
                w.branch = "detached".into();
            }
        }
    }
    list.extend(current);
    list
}

/// Uncommitted changes in the worktree at `dir`.
pub fn status(dir: &Path) -> Result<Vec<Change>, String> {
    git(dir, &["status", "--porcelain"]).map(|out| parse_status(&out))
}

fn parse_status(out: &str) -> Vec<Change> {
    out.lines()
        .filter(|line| line.len() > 3 && line.is_char_boundary(2))
        .map(|line| {
            let code = line[..2].to_string();
            let rest = &line[3..];
            // Renames read `R  old -> new`; the new name is the live one.
            let path = match rest.split_once(" -> ") {
                Some((_, new)) => new,
                None => rest,
            };
            Change {
                code,
                path: unquote(path),
            }
        })
        .collect()
}

/// Porcelain quotes paths with special characters; undo the common escapes.
fn unquote(path: &str) -> String {
    let Some(inner) = path.strip_prefix('"').and_then(|p| p.strip_suffix('"')) else {
        return path.to_string();
    };
    let mut out = String::with_capacity(inner.len());
    let mut chars = inner.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some(other) => out.push(other),
            None => {}
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_lines_parse() {
        let out = " M src/main.rs\n?? notes.txt\nR  old.rs -> new.rs\nD  gone.rs\n";
        let changes = parse_status(out);
        assert_eq!(changes.len(), 4);
        assert_eq!(changes[0].path, "src/main.rs");
        assert_eq!(changes[0].letter(), 'M');
        assert_eq!(changes[1].letter(), 'U');
        assert_eq!(changes[2].path, "new.rs");
        assert_eq!(changes[2].letter(), 'R');
        assert_eq!(changes[3].letter(), 'D');
    }

    #[test]
    fn quoted_paths_unescape() {
        assert_eq!(unquote("\"with space\""), "with space");
        assert_eq!(unquote("\"a\\\"b\""), "a\"b");
        assert_eq!(unquote("plain.rs"), "plain.rs");
    }

    #[test]
    fn worktree_listing_parses() {
        let out = "worktree /repo\nHEAD abc\nbranch refs/heads/main\n\n\
                   worktree /repo-wt\nHEAD def\ndetached\n";
        let list = parse_worktrees(out);
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].branch, "main");
        assert_eq!(list[0].name(), "repo");
        assert_eq!(list[1].branch, "detached");
    }
}

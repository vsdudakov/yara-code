//! Git integration through the `git` CLI: repository discovery, worktrees,
//! and uncommitted changes — frontend-independent, no extra dependencies.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use crate::core::diff;

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
    /// How a path stands against the last commit, for the navigator's colors.
    /// `None` for anything git has nothing to say about.
    pub fn state_of(&self, path: &Path) -> Option<FileState> {
        let dir = self.dir()?;
        let relative = path
            .strip_prefix(&dir)
            .ok()?
            .to_string_lossy()
            .replace('\\', "/");
        if let Some(change) = self.changes.iter().find(|change| change.path == relative) {
            return Some(FileState::of(change));
        }
        // An untracked directory is reported as `pkg/` rather than file by
        // file, so everything inside it is untracked too.
        self.changes
            .iter()
            .find(|change| change.path.ends_with('/') && relative.starts_with(&change.path))
            .map(FileState::of)
    }

    /// Whether anything under this folder changed, so a collapsed folder still
    /// shows that something inside it did.
    pub fn folder_touched(&self, path: &Path) -> bool {
        let Some(dir) = self.dir() else { return false };
        let Ok(relative) = path.strip_prefix(&dir) else {
            return false;
        };
        let prefix = format!("{}/", relative.to_string_lossy().replace('\\', "/"));
        !prefix.is_empty()
            && self
                .changes
                .iter()
                .any(|change| change.path.starts_with(&prefix))
    }

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
        return None;
    }
    // git answers with forward slashes even on Windows; canonicalising brings
    // it into the same shape as the paths the navigator hands us, so prefix
    // comparisons work there too.
    let path = PathBuf::from(line);
    Some(path.canonicalize().unwrap_or(path))
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

/// Same reason as `repo_root`: a worktree path from git is compared against
/// filesystem paths.
fn canonical(path: PathBuf) -> PathBuf {
    path.canonicalize().unwrap_or(path)
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
                w.branch = branch
                    .strip_prefix("refs/heads/")
                    .unwrap_or(branch)
                    .to_string();
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
/// How a file stands against the last commit, for painting its name in the
/// navigator.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum FileState {
    Modified,
    Added,
    Deleted,
    Untracked,
}

impl FileState {
    fn of(change: &Change) -> Self {
        match change.letter() {
            'U' => Self::Untracked,
            'A' | 'C' => Self::Added,
            'D' => Self::Deleted,
            _ => Self::Modified,
        }
    }
}

/// What each changed line of a file is, by line number in the file as it
/// stands. A removal marks the line it was removed *before*.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum LineState {
    Added,
    Modified,
    Removed,
}

/// The side-by-side diff of one changed path. Untracked files have no old
/// version, and deleted ones no new one; both still show as a whole file.
pub fn diff(dir: &Path, change: &Change) -> Result<Vec<diff::Row>, String> {
    let path = change.path.clone();
    if change.code == "??" {
        let text = std::fs::read_to_string(dir.join(&path))
            .map_err(|e| format!("cannot read {path}: {e}"))?;
        return Ok(diff::all_added(&text));
    }
    if change.letter() == 'D' {
        let text = git(dir, &["show", &format!("HEAD:{path}")])?;
        return Ok(diff::all_removed(&text));
    }
    // A context wide enough to cover any file makes the diff the whole file,
    // which is what a two-pane view shows.
    let out = git(
        dir,
        &[
            "diff",
            "--no-color",
            "--no-ext-diff",
            "-U1000000",
            "HEAD",
            "--",
            &path,
        ],
    )?;
    if out.trim().is_empty() {
        // Staged-only changes are not in `diff HEAD` when the worktree matches
        // the index; fall back to the file as it stands.
        let text = std::fs::read_to_string(dir.join(&path))
            .map_err(|e| format!("cannot read {path}: {e}"))?;
        return Ok(diff::all_added(&text));
    }
    Ok(diff::from_unified(&out))
}

/// Who last touched a line, and in what.
#[derive(Clone, Debug, PartialEq)]
pub struct Blame {
    /// Short commit hash.
    pub commit: String,
    pub author: String,
    /// How long ago, in words: "3 days ago".
    pub when: String,
    pub summary: String,
    /// Pull request the commit came in through, when its message says so.
    pub pr: Option<String>,
}

impl Blame {
    /// One line for a status bar.
    pub fn line(&self) -> String {
        let mut out = format!("{} · {} · {}", self.commit, self.author, self.when);
        if let Some(pr) = &self.pr {
            out.push_str(&format!(" · #{pr}"));
        }
        if !self.summary.is_empty() {
            out.push_str(&format!(" · {}", self.summary));
        }
        out
    }
}

/// Who last changed line `line` (1-based) of `path`. `None` for a line that is
/// not committed yet, or outside a repository.
pub fn blame(dir: &Path, path: &str, line: usize) -> Option<Blame> {
    let range = format!("{line},{line}");
    let out = git(dir, &["blame", "--porcelain", "-L", &range, "--", path]).ok()?;
    let mut lines = out.lines();
    let header = lines.next()?;
    let commit = header.split_whitespace().next()?.to_string();
    // All zeros is git's way of saying "not committed yet".
    if commit.chars().all(|c| c == '0') {
        return Some(Blame {
            commit: "uncommitted".into(),
            author: "you".into(),
            when: "not committed yet".into(),
            summary: String::new(),
            pr: None,
        });
    }
    let mut author = String::new();
    let mut when = String::new();
    let mut summary = String::new();
    for entry in lines {
        if let Some(rest) = entry.strip_prefix("author ") {
            author = rest.to_string();
        } else if let Some(rest) = entry.strip_prefix("author-time ") {
            when = rest.parse().ok().map(ago).unwrap_or_default();
        } else if let Some(rest) = entry.strip_prefix("summary ") {
            summary = rest.to_string();
        }
    }
    let pr = pull_request(&summary);
    Some(Blame {
        commit: commit.chars().take(8).collect(),
        author,
        when,
        summary,
        pr,
    })
}

/// The pull request a commit message names — GitHub's `(#12)` on a squash, or
/// `Merge pull request #12`.
fn pull_request(summary: &str) -> Option<String> {
    let at = summary.find('#')?;
    let digits: String = summary[at + 1..]
        .chars()
        .take_while(char::is_ascii_digit)
        .collect();
    (!digits.is_empty()).then_some(digits)
}

/// A unix timestamp as "3 days ago".
fn ago(time: u64) -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(time);
    let seconds = now.saturating_sub(time);
    // Each unit hands over as soon as one of the next is complete, so an hour
    // old never reads as "60 minutes ago".
    let (count, unit) = match seconds {
        0..=90 => return "just now".to_string(),
        91..=3_599 => (seconds / 60, "minute"),
        3_600..=86_399 => (seconds / 3_600, "hour"),
        86_400..=2_591_999 => (seconds / 86_400, "day"),
        2_592_000..=31_535_999 => (seconds / 2_592_000, "month"),
        _ => (seconds / 31_536_000, "year"),
    };
    let plural = if count == 1 { "" } else { "s" };
    format!("{count} {unit}{plural} ago")
}

/// Changed lines of one file, keyed by line number in the working copy — what
/// the editor's gutter marks.
pub fn changed_lines(dir: &Path, path: &str) -> BTreeMap<usize, LineState> {
    let mut marks = BTreeMap::new();
    let Ok(out) = git(
        dir,
        &[
            "diff",
            "--no-color",
            "--no-ext-diff",
            "-U0",
            "HEAD",
            "--",
            path,
        ],
    ) else {
        return marks;
    };
    for row in diff::from_unified(&out) {
        match (row.kind, &row.right, &row.left) {
            (diff::Kind::Added, Some(right), _) => {
                marks.insert(right.line, LineState::Added);
            }
            (diff::Kind::Changed, Some(right), _) => {
                marks.insert(right.line, LineState::Modified);
            }
            (diff::Kind::Removed, _, Some(_)) => {
                // The gap where lines were taken out: mark the line that
                // closed over it, or the last one when the tail went.
                let at = marks.keys().next_back().copied().unwrap_or(0) + 1;
                marks.entry(at).or_insert(LineState::Removed);
            }
            _ => {}
        }
    }
    marks
}

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
    #[test]
    fn a_pull_request_is_read_out_of_the_summary() {
        assert_eq!(
            super::pull_request("Fix the thing (#412)").as_deref(),
            Some("412")
        );
        assert_eq!(
            super::pull_request("Merge pull request #7 from a/b").as_deref(),
            Some("7")
        );
        assert_eq!(super::pull_request("no number here"), None);
        assert_eq!(super::pull_request("issue #"), None);
    }

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

#[cfg(test)]
mod git_tests {
    use super::*;
    use crate::core::test_support::Dir;

    /// A repository with one commit, so blame and diff have something to say.
    fn repo(tag: &str) -> Dir {
        let dir = Dir::new(tag);
        let run = |args: &[&str]| {
            let out = std::process::Command::new("git")
                .arg("-C")
                .arg(dir.path())
                .args(args)
                .output()
                .expect("git is on PATH");
            assert!(out.status.success(), "git {args:?}: {:?}", out);
        };
        run(&["init", "-q", "-b", "main"]);
        run(&["config", "user.email", "test@example.com"]);
        run(&["config", "user.name", "Test"]);
        run(&["config", "commit.gpgsign", "false"]);
        dir.file("kept.txt", "one\ntwo\nthree\n");
        run(&["add", "-A"]);
        run(&["commit", "-qm", "Seed the tree (#42)"]);
        dir
    }

    #[test]
    fn status_letters_come_from_the_porcelain_columns() {
        assert_eq!(
            Change {
                code: "??".into(),
                path: "x".into()
            }
            .letter(),
            'U'
        );
        // The worktree column speaks first, the index column otherwise.
        assert_eq!(
            Change {
                code: " M".into(),
                path: "x".into()
            }
            .letter(),
            'M'
        );
        assert_eq!(
            Change {
                code: "A ".into(),
                path: "x".into()
            }
            .letter(),
            'A'
        );
    }

    #[test]
    fn porcelain_lines_become_changes() {
        let changes =
            parse_status(" M src/main.rs\n?? new.txt\nR  old.rs -> new.rs\n\"quoted\\tname\"\n");
        assert_eq!(changes[0].path, "src/main.rs");
        assert_eq!(changes[1].code, "??");
        // A rename is listed under the name it has now.
        assert_eq!(changes[2].path, "new.rs");
        assert_eq!(unquote("\"a\\tb\""), "a\tb");
        assert_eq!(unquote("plain"), "plain");
    }

    #[test]
    fn a_worktree_listing_is_read_into_paths_and_branches() {
        let parsed = parse_worktrees(
            "worktree /work/main\nHEAD abc\nbranch refs/heads/main\n\n\
             worktree /work/detached\nHEAD def\ndetached\n\n",
        );
        assert_eq!(parsed[0].branch, "main");
        assert_eq!(parsed[0].name(), "main");
        assert_eq!(parsed[1].branch, "detached");
    }

    #[test]
    fn a_repository_reports_what_changed_and_who_wrote_it() {
        let dir = repo("yara-git-status");
        std::fs::write(dir.path().join("kept.txt"), "one\nTWO\nthree\nfour\n").unwrap();
        dir.file("fresh.txt", "new\n");

        let root = repo_root(dir.path()).expect("inside a repository");
        assert_eq!(root, dir.path());
        assert!(discover_repos(dir.path()).contains(&root));
        assert_eq!(worktrees(dir.path())[0].branch, "main");

        let changes = status(dir.path()).unwrap();
        let modified = changes.iter().find(|c| c.path == "kept.txt").unwrap();
        assert_eq!(modified.letter(), 'M');
        assert_eq!(
            changes
                .iter()
                .find(|c| c.path == "fresh.txt")
                .unwrap()
                .letter(),
            'U'
        );

        // The diff pairs the replaced line and leaves the added one alone.
        let rows = diff(dir.path(), modified).unwrap();
        let changed = rows.iter().find(|r| r.kind == diff::Kind::Changed).unwrap();
        assert_eq!(changed.left.as_ref().unwrap().text, "two");
        assert_eq!(changed.right.as_ref().unwrap().text, "TWO");
        assert!(rows.iter().any(|r| r.kind == diff::Kind::Added));

        // An untracked file has no old version at all.
        let fresh = changes.iter().find(|c| c.path == "fresh.txt").unwrap();
        let rows = diff(dir.path(), fresh).unwrap();
        assert!(rows.iter().all(|r| r.kind == diff::Kind::Added));

        // The gutter marks: line 2 changed, line 4 arrived.
        let marks = changed_lines(dir.path(), "kept.txt");
        assert_eq!(marks.get(&2), Some(&LineState::Modified));
        assert_eq!(marks.get(&4), Some(&LineState::Added));

        // Blame names the commit, its author and the pull request it mentions.
        let first = blame(dir.path(), "kept.txt", 1).unwrap();
        assert_eq!(first.author, "Test");
        assert_eq!(first.pr.as_deref(), Some("42"));
        assert!(first.summary.starts_with("Seed the tree"));
        assert!(first.line().contains("#42"));
        // A line that is not committed yet says so.
        let uncommitted = blame(dir.path(), "kept.txt", 4).unwrap();
        assert_eq!(uncommitted.when, "not committed yet");
    }

    #[test]
    fn a_deleted_file_shows_as_wholly_removed() {
        let dir = repo("yara-git-deleted");
        std::fs::remove_file(dir.path().join("kept.txt")).unwrap();
        let changes = status(dir.path()).unwrap();
        let gone = changes.iter().find(|c| c.path == "kept.txt").unwrap();
        assert_eq!(gone.letter(), 'D');
        let rows = diff(dir.path(), gone).unwrap();
        assert!(rows.iter().all(|r| r.kind == diff::Kind::Removed));
        assert_eq!(rows.len(), 3);
    }

    #[test]
    fn outside_a_repository_nothing_is_claimed() {
        let dir = Dir::new("yara-git-none");
        assert!(repo_root(dir.path()).is_none());
        assert!(status(dir.path()).is_err());
        assert!(changed_lines(dir.path(), "nothing.txt").is_empty());
        assert!(blame(dir.path(), "nothing.txt", 1).is_none());
        assert!(discover_repos(dir.path()).is_empty());
    }

    #[test]
    fn the_state_tracks_the_repository_it_was_pointed_at() {
        let dir = repo("yara-git-state");
        std::fs::write(dir.path().join("kept.txt"), "one\nTWO\nthree\n").unwrap();
        let mut state = GitState::default();
        state.tick(dir.path());
        assert!(!state.repos.is_empty());
        assert_eq!(state.dir().as_deref(), Some(dir.path()));
        assert!(state.changes.iter().any(|c| c.path == "kept.txt"));

        let file = dir.path().join("kept.txt");
        assert_eq!(state.state_of(&file), Some(FileState::Modified));
        assert_eq!(state.state_of(Path::new("/elsewhere/x")), None);

        // A folder wears the mark of what changed beneath it.
        let nested = dir.file("pkg/inner.txt", "x\n");
        state.invalidate();
        state.tick(dir.path());
        assert!(state.folder_touched(&dir.path().join("pkg")));
        assert!(!state.folder_touched(&dir.path().join("nowhere")));
        assert_eq!(state.state_of(&nested), Some(FileState::Untracked));
    }

    #[test]
    fn ages_read_as_words() {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        assert_eq!(ago(now), "just now");
        assert_eq!(ago(now - 600), "10 minutes ago");
        assert_eq!(ago(now - 7200), "2 hours ago");
        assert_eq!(ago(now - 86_400 * 3), "3 days ago");
        assert_eq!(ago(now - 2_592_000 * 2), "2 months ago");
        assert_eq!(ago(now - 31_536_000 * 4), "4 years ago");
        assert_eq!(ago(now - 3600), "1 hour ago", "one of anything is singular");
    }
}

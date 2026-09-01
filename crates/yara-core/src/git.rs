//! Git through the `git` CLI, no libgit2: the repository behind a project,
//! what has changed in it against its main branch, and the watcher that
//! turns the agent's edits into timeline events.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::follow::EditEvent;
use crate::tree;

/// The repository a project lives in.
#[derive(Clone, Debug, PartialEq)]
pub struct Repo {
    pub root: PathBuf,
    /// The main working copy's root — `root` itself unless this is a
    /// linked worktree. Work on a worktree's behalf that need not run inside
    /// it runs here: on Windows a folder that is some process's working
    /// directory cannot be removed, and a background `gh` or the very git
    /// that removes the worktree would hold it.
    pub main: PathBuf,
    /// The checked-out branch, or `detached`.
    pub branch: String,
    /// The folder's name when it is a linked worktree; the main working
    /// copy has none.
    pub worktree: Option<String>,
    /// What changes are measured against: the merge base with the main
    /// branch when there is one to diverge from, else HEAD.
    pub base: String,
}

/// Runs git in `dir`. Without optional locks: the watcher asks git every
/// half second, and a status that refreshed the index would leave a lock
/// in the way of the user's own git.
fn git(dir: &Path, args: &[&str]) -> Result<String, String> {
    let out = std::process::Command::new("git")
        .args(["--no-optional-locks", "-C"])
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

/// A path as the system spells it, with Windows' `\\?\` prefix taken off:
/// git does not take that shape on a command line, and neither do the
/// comparisons the editor makes against paths it was given.
pub fn canonical(path: &Path) -> PathBuf {
    let full = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    match full.to_string_lossy().strip_prefix(r"\\?\") {
        Some(rest) => PathBuf::from(rest),
        None => full,
    }
}

fn first_line(out: String) -> String {
    out.lines().next().unwrap_or("").trim().to_string()
}

/// The repository holding `dir`, if there is one. `main_branch` is the
/// branch changes are measured against; empty means the branch the main
/// working copy has checked out, or `main`.
pub fn open(dir: &Path, main_branch: &str) -> Option<Repo> {
    let top = first_line(git(dir, &["rev-parse", "--show-toplevel"]).ok()?);
    if top.is_empty() {
        return None;
    }
    let root = canonical(&PathBuf::from(&top));
    let branch = match first_line(git(&root, &["branch", "--show-current"]).unwrap_or_default()) {
        b if b.is_empty() => "detached".to_string(),
        b => b,
    };
    // The first worktree git lists is the main working copy, and its branch
    // is what changes are measured against when the settings name none.
    let list = git(&root, &["worktree", "list", "--porcelain"]).unwrap_or_default();
    let mut entries = list.split("\n\n").filter(|e| !e.trim().is_empty());
    let main = entries.next().unwrap_or_default();
    let main_root = main
        .lines()
        .find_map(|l| l.strip_prefix("worktree "))
        .map(|p| canonical(&PathBuf::from(p)))
        .unwrap_or_else(|| root.clone());
    // A linked worktree keeps a `.git` file pointing at the repository; the
    // main working copy keeps the repository itself in a folder. That is a
    // surer test than comparing paths, which git spells its own way.
    let worktree = root
        .join(".git")
        .is_file()
        .then(|| root.file_name().map(|n| n.to_string_lossy().into_owned()))
        .flatten();
    let main_branch = if main_branch.is_empty() {
        main.lines()
            .find_map(|l| l.strip_prefix("branch refs/heads/"))
            .unwrap_or("main")
            .to_string()
    } else {
        main_branch.to_string()
    };
    let base = git(&root, &["merge-base", "HEAD", &main_branch])
        .map(first_line)
        .ok()
        .filter(|b| !b.is_empty())
        .unwrap_or_else(|| "HEAD".to_string());
    Some(Repo {
        root,
        main: main_root,
        branch,
        worktree,
        base,
    })
}

/// A worktree in `dir` for the workspace, named after it with spaces and
/// slashes made dashes. One that is already there is simply used; a branch
/// of that name that already exists is checked out rather than made.
pub fn worktree_add(repo: &Repo, dir: &Path, name: &str) -> Result<PathBuf, String> {
    let slug: Vec<&str> = name
        .split(|c: char| c.is_whitespace() || c == '/')
        .filter(|part| !part.is_empty())
        .collect();
    let slug = slug.join("-");
    if slug.is_empty() {
        return Err("a workspace needs a name".into());
    }
    let path = dir.join(&slug);
    if path.join(".git").exists() {
        return Ok(canonical(&path));
    }
    let _ = std::fs::create_dir_all(dir);
    let branch_exists = git(&repo.root, &["rev-parse", "--verify", "--quiet", &slug]).is_ok();
    let path_text = path.to_string_lossy().into_owned();
    let args: Vec<&str> = if branch_exists {
        vec!["worktree", "add", "-q", &path_text, &slug]
    } else {
        vec!["worktree", "add", "-q", "-b", &slug, &path_text]
    };
    git(&repo.root, &args)?;
    Ok(canonical(&path))
}

/// The repositories inside `root` — a folder that holds several of them,
/// as a bench of related projects often is. Two levels deep, so a worktree
/// under `.worktrees/` is found as well as a plain `backend/`.
pub fn discover_repos(root: &Path) -> Vec<PathBuf> {
    const VENDOR: [&str; 6] = ["node_modules", "target", "dist", "build", "vendor", "venv"];
    let mut found = Vec::new();
    let children = |dir: &Path| -> Vec<PathBuf> {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return Vec::new();
        };
        let mut out: Vec<PathBuf> = entries
            .flatten()
            .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
            .map(|e| e.path())
            .filter(|p| {
                p.file_name()
                    .is_none_or(|n| n != ".git" && !VENDOR.iter().any(|v| n == *v))
            })
            .collect();
        out.sort();
        out
    };
    for child in children(root) {
        if child.join(".git").exists() {
            found.push(child);
            continue;
        }
        // A folder of worktrees is not a repository itself; what is inside
        // it may be.
        for grandchild in children(&child) {
            if grandchild.join(".git").exists() {
                found.push(grandchild);
            }
        }
    }
    found
}

/// The linked worktrees of the repository — every working copy but the
/// main one — as folders.
pub fn worktrees(repo: &Repo) -> Vec<PathBuf> {
    let list = git(&repo.root, &["worktree", "list", "--porcelain"]).unwrap_or_default();
    list.split("\n\n")
        .skip(1)
        .filter_map(|entry| entry.lines().find_map(|l| l.strip_prefix("worktree ")))
        .map(|p| canonical(&PathBuf::from(p)))
        .collect()
}

/// Removes a linked worktree, folder and all; its branch stays. Git runs in
/// the main working copy, never in the folder it is taking away.
pub fn worktree_remove(repo: &Repo, path: &Path) -> Result<(), String> {
    git(
        &repo.main,
        &["worktree", "remove", "--force", &path.to_string_lossy()],
    )
    .map(|_| ())
}

/// The pull request the branch is on, as `#number title`, through the `gh`
/// CLI when it is installed and logged in; nothing otherwise, and nothing
/// for a detached head. Slow, so it is asked off the drawing thread — and
/// from the main working copy, so a worktree can go while it is asked.
pub fn pull_request(repo: &Repo) -> Option<String> {
    if repo.branch == "detached" {
        return None;
    }
    let out = std::process::Command::new("gh")
        .args([
            "pr",
            "view",
            &repo.branch,
            "--json",
            "number,title",
            "-q",
            r##""#\(.number) \(.title)""##,
        ])
        .current_dir(&repo.main)
        .output()
        .ok()?;
    let line = first_line(String::from_utf8_lossy(&out.stdout).into_owned());
    (out.status.success() && line.starts_with('#')).then_some(line)
}

/// One changed path, against the base: `A`dded, `M`odified, `D`eleted, or
/// `U`ntracked, with how many lines went each way.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Change {
    pub letter: char,
    pub path: String,
    pub added: usize,
    pub removed: usize,
}

/// Everything that differs from the base, the working tree included.
pub fn changes(repo: &Repo) -> Result<Vec<Change>, String> {
    let mut out = Vec::new();
    let names = git(
        &repo.root,
        &["diff", "--name-status", "--no-renames", &repo.base],
    )?;
    let counts = git(
        &repo.root,
        &["diff", "--numstat", "--no-renames", &repo.base],
    )?;
    let counts: BTreeMap<&str, (usize, usize)> = counts
        .lines()
        .filter_map(|l| {
            let mut parts = l.split('\t');
            let added = parts.next()?.parse().unwrap_or(0);
            let removed = parts.next()?.parse().unwrap_or(0);
            Some((parts.next()?, (added, removed)))
        })
        .collect();
    for line in names.lines() {
        let Some((status, path)) = line.split_once('\t') else {
            continue;
        };
        let (added, removed) = counts.get(path).copied().unwrap_or((0, 0));
        out.push(Change {
            letter: status.chars().next().unwrap_or('M'),
            path: path.to_string(),
            added,
            removed,
        });
    }
    let status = git(&repo.root, &["status", "--porcelain", "-uall"])?;
    for path in status.lines().filter_map(|l| l.strip_prefix("?? ")) {
        let path = path.trim_matches('"');
        let added = std::fs::read_to_string(repo.root.join(path))
            .map(|t| t.lines().count())
            .unwrap_or(0);
        out.push(Change {
            letter: 'U',
            path: path.to_string(),
            added,
            removed: 0,
        });
    }
    out.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(out)
}

/// Who last touched a line, as `git blame` tells it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Blame {
    pub author: String,
    /// When, in seconds since the epoch.
    pub time: u64,
    pub summary: String,
}

/// The last commit to touch line `line` (counted from one) of `path`, with
/// whitespace changes looked past. A line nobody has committed has none.
pub fn blame(repo: &Repo, path: &Path, line: usize) -> Option<Blame> {
    let range = format!("{line},{line}");
    let out = git(
        &repo.root,
        &[
            "blame",
            "--porcelain",
            "-w",
            "-L",
            &range,
            "--",
            &path.to_string_lossy(),
        ],
    )
    .ok()?;
    let mut lines = out.lines();
    let hash = lines.next()?.split(' ').next()?;
    if hash.chars().all(|c| c == '0') {
        return None;
    }
    let mut blame = Blame {
        author: String::new(),
        time: 0,
        summary: String::new(),
    };
    for l in lines {
        if let Some(v) = l.strip_prefix("author ") {
            blame.author = v.to_string();
        } else if let Some(v) = l.strip_prefix("author-time ") {
            blame.time = v.trim().parse().unwrap_or(0);
        } else if let Some(v) = l.strip_prefix("summary ") {
            blame.summary = v.to_string();
        }
    }
    Some(blame)
}

/// How long ago `then` was, from `now`, the way a person says it.
pub fn ago(then: u64, now: u64) -> String {
    let seconds = now.saturating_sub(then);
    let (count, unit) = match seconds {
        s if s < 60 => return "just now".into(),
        s if s < 3_600 => (s / 60, "minute"),
        s if s < 86_400 => (s / 3_600, "hour"),
        s if s < 30 * 86_400 => (s / 86_400, "day"),
        s if s < 365 * 86_400 => (s / (30 * 86_400), "month"),
        s => (s / (365 * 86_400), "year"),
    };
    let plural = if count == 1 { "" } else { "s" };
    format!("{count} {unit}{plural} ago")
}

/// The unified diff of one path against the base; an untracked file reads
/// as wholly added.
pub fn file_diff(repo: &Repo, path: &str) -> Result<String, String> {
    let diff = git(
        &repo.root,
        &[
            "diff",
            "--no-color",
            "--no-ext-diff",
            &repo.base,
            "--",
            path,
        ],
    )?;
    if !diff.trim().is_empty() {
        return Ok(diff);
    }
    let text = std::fs::read_to_string(repo.root.join(path)).map_err(|e| format!("{path}: {e}"))?;
    Ok(text.lines().map(|l| format!("+{l}\n")).collect())
}

/// The unified diff between two texts, from git itself so the hunks are the
/// ones a reviewer expects. Git exits 1 when the texts differ, which is not
/// a failure here.
pub fn unified(old: &str, new: &str) -> String {
    // A folder of its own per call: two diffs at once must not share one.
    static COUNTER: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("ycode-diff-{}-{n}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    let (a, b) = (dir.join("a"), dir.join("b"));
    if std::fs::write(&a, old).is_err() || std::fs::write(&b, new).is_err() {
        return String::new();
    }
    let out = std::process::Command::new("git")
        .args(["diff", "--no-index", "--no-color", "--no-ext-diff", "--"])
        .arg(&a)
        .arg(&b)
        .output();
    let _ = std::fs::remove_dir_all(&dir);
    out.map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
        .unwrap_or_default()
}

/// Every file under `root` with when it was last written, folded into one
/// number. Cheap next to reading them: no reading at all.
fn fingerprint(root: &Path, rules: &[String]) -> u64 {
    use std::hash::{Hash, Hasher};
    // A folder the size of a home directory would cost a second a poll;
    // past this many files the walk gives up and says so, so a caller does
    // not take a partial answer for the whole truth.
    const CAP: usize = 20_000;
    let mut seen = 0usize;
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let name = entry.file_name();
            if tree::ignored(rules, &name.to_string_lossy()) {
                continue;
            }
            let Ok(meta) = entry.metadata() else { continue };
            if meta.is_dir() {
                stack.push(entry.path());
            } else {
                entry.path().hash(&mut hasher);
                meta.len().hash(&mut hasher);
                if let Ok(modified) = meta.modified() {
                    modified.hash(&mut hasher);
                }
                seen += 1;
                if seen == CAP {
                    // Too big to watch this way; every poll counts as movement
                    // rather than pretending the folder is still.
                    return rand_like();
                }
            }
        }
    }
    hasher.finish()
}

/// A number that differs every time, for a folder too big to fingerprint.
fn rand_like() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0)
}

/// What git says has changed, and when those files were last written: the
/// whole of what a repository's watcher needs, and one process to get it.
fn repo_fingerprint(repo: &Repo) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    // Untracked folders are left collapsed: naming every file inside one
    // is what makes a status slow on a big working tree.
    let status = git(&repo.root, &["status", "--porcelain"]).unwrap_or_default();
    status.hash(&mut hasher);
    for line in status.lines() {
        let path = line.get(3..).unwrap_or("").trim_matches('"');
        if let Ok(meta) = std::fs::metadata(repo.root.join(path)) {
            meta.len().hash(&mut hasher);
            if let Ok(modified) = meta.modified() {
                modified.hash(&mut hasher);
            }
        }
    }
    hasher.finish()
}

/// Watches a folder: each poll, every file whose text moved since the last
/// one becomes an edit — the step the agent just took. The first poll only
/// takes stock. A folder that is a repository is asked of git, which is
/// quick and knows what is committed; one that is not is read whole.
#[derive(Default)]
pub struct Watcher {
    seen: Option<BTreeMap<String, String>>,
    /// The folder's files and their modification times as of the last poll,
    /// so the files are only read when something on disk actually moved.
    fingerprint: Option<u64>,
}

impl Watcher {
    /// Whether anything in the folder moved since the last look; the first
    /// look counts as movement, so the stock is taken. A repository is
    /// asked of git — a walk of a big working tree costs more than the one
    /// process does, and misses nothing git already knows.
    pub fn moved(&mut self, root: &Path, repo: Option<&Repo>, rules: &[String]) -> bool {
        let now = match repo {
            Some(repo) => repo_fingerprint(repo),
            None => fingerprint(root, rules),
        };
        let moved = self.fingerprint != Some(now);
        self.fingerprint = Some(now);
        moved
    }

    /// The edits made in `root` since the last poll. `repo` is the folder's
    /// repository when it has one — then only what differs from the base is
    /// watched, rather than every file in the folder.
    pub fn poll(&mut self, root: &Path, repo: Option<&Repo>, rules: &[String]) -> Vec<EditEvent> {
        let mut paths: Vec<String> = match repo {
            Some(repo) => changes(repo)
                .unwrap_or_default()
                .into_iter()
                .map(|c| c.path)
                .collect(),
            // A folder outside git is read whole, so it is watched only
            // while it is small enough to read.
            None => {
                let all = tree::all_files_with(root, rules);
                if all.len() > 2_000 {
                    return Vec::new();
                }
                all
            }
        };
        // A path put back the way it was is a change too, so what was seen
        // before is looked at again even when git no longer lists it.
        paths.extend(self.seen.iter().flat_map(|seen| seen.keys().cloned()));
        paths.sort();
        paths.dedup();
        let now: BTreeMap<String, String> = paths
            .into_iter()
            .map(|path| {
                let text = std::fs::read_to_string(root.join(&path)).unwrap_or_default();
                (path, text)
            })
            .collect();
        let edits = match &self.seen {
            None => Vec::new(),
            Some(seen) => now
                .iter()
                .filter_map(|(path, text)| {
                    // A path not seen before starts from its committed
                    // version where there is one, so the first edit to a
                    // clean file is that edit alone.
                    let old = match (seen.get(path), repo) {
                        (Some(old), _) => old.clone(),
                        (None, Some(repo)) => {
                            git(&repo.root, &["show", &format!("{}:{path}", repo.base)])
                                .unwrap_or_default()
                        }
                        (None, None) => String::new(),
                    };
                    // The path an edit carries is the whole one: a task
                    // holds several folders, and a name alone would not say
                    // which.
                    (old != *text)
                        .then(|| EditEvent::from_unified(root.join(path), &unified(&old, text)))
                })
                .collect(),
        };
        self.seen = Some(now);
        edits
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::Dir;

    fn repo(tag: &str) -> (Dir, Repo) {
        let dir = Dir::new(tag);
        for args in [
            vec!["init", "-q", "-b", "main"],
            vec!["config", "user.email", "t@t"],
            vec!["config", "user.name", "t"],
        ] {
            git(dir.path(), &args).unwrap();
        }
        dir.file("a.txt", "one\ntwo\n");
        git(dir.path(), &["add", "."]).unwrap();
        git(dir.path(), &["commit", "-q", "-m", "first"]).unwrap();
        let repo = open(dir.path(), "").unwrap();
        (dir, repo)
    }

    #[test]
    fn a_line_is_blamed_on_the_commit_that_wrote_it_and_an_unsaved_one_on_nobody() {
        let (dir, repo) = repo("yara-blame");
        let who = blame(&repo, Path::new("a.txt"), 2).unwrap();
        assert_eq!((who.author.as_str(), who.summary.as_str()), ("t", "first"));
        assert!(who.time > 0);
        dir.file("a.txt", "one\ntwo\nthree\n");
        assert_eq!(blame(&repo, Path::new("a.txt"), 3), None);
        assert_eq!(blame(&repo, Path::new("missing.txt"), 1), None);
        assert_eq!(ago(100, 130), "just now");
        assert_eq!(ago(0, 3_600), "1 hour ago");
        assert_eq!(ago(0, 3 * 86_400), "3 days ago");
        assert_eq!(ago(0, 400 * 86_400), "1 year ago");
    }

    #[test]
    fn a_folder_that_is_no_repository_is_watched_all_the_same() {
        let dir = Dir::new("yara-watch-plain");
        dir.file("notes.md", "one\n");
        let mut watcher = Watcher::default();
        assert!(
            watcher
                .poll(dir.path(), None, &tree::default_ignores())
                .is_empty(),
            "stock taken"
        );
        dir.file("notes.md", "one\ntwo\n");
        dir.file("fresh.txt", "hello\n");
        let edits = watcher.poll(dir.path(), None, &tree::default_ignores());
        assert_eq!(edits.len(), 2);
        assert_eq!(edits[0].path, dir.path().join("fresh.txt"));
        assert_eq!(edits[1].path, dir.path().join("notes.md"));
        assert_eq!((edits[1].added(), edits[1].removed()), (1, 0));
        assert!(
            watcher
                .poll(dir.path(), None, &tree::default_ignores())
                .is_empty(),
            "reported once"
        );
    }

    #[test]
    fn two_texts_diff_the_way_git_diffs_them() {
        let diff = unified("a\nb\nc\n", "a\nc\nd\n");
        assert!(diff.contains("@@ -1,3 +1,3 @@"), "{diff}");
        assert!(diff.contains("-b\n") && diff.contains("+d\n"));
        assert_eq!(unified("same\n", "same\n"), "");
    }

    #[test]
    fn outside_a_repository_nothing_is_claimed() {
        let dir = Dir::new("yara-git-none");
        assert_eq!(open(dir.path(), ""), None);
    }

    #[test]
    fn a_repository_knows_its_branch_and_measures_against_main() {
        let (dir, repo) = repo("yara-git-open");
        assert_eq!(repo.root, dir.path());
        assert_eq!(repo.branch, "main");
        assert_eq!(repo.worktree, None);
        assert!(changes(&repo).unwrap().is_empty());

        git(dir.path(), &["checkout", "-q", "-b", "feature"]).unwrap();
        dir.file("a.txt", "one\n2\nthree\n");
        dir.file("new.txt", "x\ny\n");
        git(dir.path(), &["add", "."]).unwrap();
        git(dir.path(), &["commit", "-q", "-m", "work"]).unwrap();
        std::fs::remove_file(dir.path().join("a.txt")).unwrap();
        dir.file("b.txt", "untracked\n");
        let repo = open(dir.path(), "main").unwrap();
        assert_eq!(repo.branch, "feature");
        assert_ne!(repo.base, "HEAD", "the merge base with main");
        let changed = changes(&repo).unwrap();
        let summary: Vec<(char, &str, usize, usize)> = changed
            .iter()
            .map(|c| (c.letter, c.path.as_str(), c.added, c.removed))
            .collect();
        assert_eq!(
            summary,
            [
                ('D', "a.txt", 0, 2),
                ('U', "b.txt", 1, 0),
                ('A', "new.txt", 2, 0)
            ]
        );
        assert_eq!(file_diff(&repo, "b.txt").unwrap(), "+untracked\n");
        assert!(file_diff(&repo, "new.txt").unwrap().contains("+x\n+y\n"));
    }

    #[test]
    fn a_linked_worktree_is_named_and_a_named_base_branch_wins() {
        let (dir, _) = repo("yara-git-worktree");
        let wt = dir.path().join("wt");
        git(
            dir.path(),
            &["worktree", "add", "-q", "-b", "agent", wt.to_str().unwrap()],
        )
        .unwrap();
        let repo = open(&wt, "").unwrap();
        assert_eq!(repo.worktree.as_deref(), Some("wt"));
        assert_eq!(repo.branch, "agent");
        let explicit = open(&wt, "main").unwrap();
        assert_eq!(explicit.base, repo.base);
        let missing = open(&wt, "no-such-branch").unwrap();
        assert_eq!(missing.base, "HEAD", "an unknown base falls back to HEAD");
    }

    #[test]
    fn the_repositories_inside_a_folder_of_them_are_found() {
        let dir = Dir::new("yara-git-bench");
        let (backend, _) = {
            let path = dir.path().join("backend");
            std::fs::create_dir_all(&path).unwrap();
            for args in [
                vec!["init", "-q", "-b", "main"],
                vec!["config", "user.email", "t@t"],
                vec!["config", "user.name", "t"],
            ] {
                git(&path, &args).unwrap();
            }
            std::fs::write(path.join("a.txt"), "one\n").unwrap();
            git(&path, &["add", "."]).unwrap();
            git(&path, &["commit", "-q", "-m", "first"]).unwrap();
            (path.clone(), open(&path, "").unwrap())
        };
        std::fs::create_dir_all(dir.path().join("notes")).unwrap();
        let repo = open(&backend, "main").unwrap();
        let nested = dir.path().join(".worktrees/backend-task");
        worktree_add(&repo, &dir.path().join(".worktrees"), "backend-task").unwrap();

        let found = discover_repos(dir.path());
        assert!(found.contains(&backend), "{found:?}");
        assert!(
            found.iter().any(|p| p.ends_with("backend-task")),
            "a worktree two levels down: {found:?}"
        );
        assert!(nested.is_dir());
        assert!(
            !found.iter().any(|p| p.ends_with("notes")),
            "not a repository"
        );
    }

    #[test]
    fn a_worktree_is_added_on_a_branch_of_its_name() {
        let (dir, repo) = repo("yara-git-add-worktree");
        let trees = dir.path().join("trees");
        let path = worktree_add(&repo, &trees, " task/login flow ").unwrap();
        assert_eq!(path, canonical(&trees.join("task-login-flow")));
        let added = open(&path, "main").unwrap();
        assert_eq!(added.branch, "task-login-flow");
        assert_eq!(added.worktree.as_deref(), Some("task-login-flow"));
        assert_eq!(added.main, repo.root, "it knows the main working copy");
        assert_eq!(repo.main, repo.root);
        assert!(worktree_add(&repo, &trees, " ").is_err());
        // The same name again is the same workspace, not an error.
        assert_eq!(
            worktree_add(&repo, &trees, "task/login flow").unwrap(),
            path
        );
        // A branch that exists without a worktree is checked out, not remade.
        git(dir.path(), &["branch", "-q", "older"]).unwrap();
        let older = worktree_add(&repo, &trees, "older").unwrap();
        let older_repo = open(&older, "main").unwrap();
        assert_eq!(older_repo.branch, "older");
        let mut listed = worktrees(&repo);
        listed.sort();
        assert_eq!(listed, [older.clone(), path.clone()]);
        // Asked of the worktree itself, git still runs in the main copy.
        worktree_remove(&older_repo, &older).unwrap();
        assert_eq!(worktrees(&repo), [path]);
        assert!(!older.exists());
    }

    #[test]
    fn the_watcher_reports_each_new_edit_once_and_never_what_was_already_there() {
        let (dir, repo) = repo("yara-git-watch");
        let root = dir.path().to_path_buf();
        let mut watcher = Watcher::default();
        let rules = tree::default_ignores();
        let poll = |w: &mut Watcher| w.poll(&root, Some(&repo), &rules);
        assert!(poll(&mut watcher).is_empty(), "the first poll takes stock");
        assert!(poll(&mut watcher).is_empty(), "nothing moved");
        // A clean file's first edit is measured from its committed version.
        dir.file("a.txt", "one\ntwo\nthree\n");
        let edits = poll(&mut watcher);
        assert_eq!(edits.len(), 1);
        assert_eq!((edits[0].added(), edits[0].removed()), (1, 0));

        dir.file("a.txt", "one\nthree\n");
        dir.file("fresh.txt", "hello\n");
        let edits = poll(&mut watcher);
        assert_eq!(edits.len(), 2);
        assert_eq!(edits[0].path, dir.path().join("a.txt"));
        // The step just taken — one line gone — not the distance from main.
        assert_eq!((edits[0].added(), edits[0].removed()), (0, 1));
        assert_eq!(edits[0].hunks[0].lines[1].text, "two");
        assert_eq!(edits[1].path, dir.path().join("fresh.txt"));
        assert_eq!(edits[1].added(), 1);
        assert!(poll(&mut watcher).is_empty(), "reported once");

        // Putting the file back is an edit too, even though git is quiet.
        dir.file("a.txt", "one\ntwo\n");
        let edits = poll(&mut watcher);
        assert_eq!(edits.len(), 1);
        assert_eq!((edits[0].added(), edits[0].removed()), (1, 1));
        // With nothing written since, nothing moved and git is not asked.
        assert!(
            watcher.moved(&root, Some(&repo), &rules),
            "the first look takes stock"
        );
        assert!(!watcher.moved(&root, Some(&repo), &rules));
        dir.file("b.txt", "new\n");
        assert!(watcher.moved(&root, Some(&repo), &rules));
        // A second edit to a file git already lists is movement too.
        dir.file("b.txt", "new and more\n");
        assert!(watcher.moved(&root, Some(&repo), &rules));
    }
}

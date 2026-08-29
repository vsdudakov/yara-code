//! Git through the `git` CLI, no libgit2: the repository behind a project,
//! what has changed in it against its main branch, and the watcher that
//! turns the agent's edits into timeline events.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::follow::EditEvent;

/// The repository a project lives in.
#[derive(Clone, Debug, PartialEq)]
pub struct Repo {
    pub root: PathBuf,
    /// The checked-out branch, or `detached`.
    pub branch: String,
    /// The folder's name when it is a linked worktree; the main working
    /// copy has none.
    pub worktree: Option<String>,
    /// What changes are measured against: the merge base with the main
    /// branch when there is one to diverge from, else HEAD.
    pub base: String,
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
    let root = PathBuf::from(&top);
    let root = root.canonicalize().unwrap_or(root);
    let branch = match first_line(git(&root, &["branch", "--show-current"]).unwrap_or_default()) {
        b if b.is_empty() => "detached".to_string(),
        b => b,
    };
    // The main working copy is the first worktree git lists; a folder that
    // is not it is a linked worktree.
    let list = git(&root, &["worktree", "list", "--porcelain"]).unwrap_or_default();
    let mut entries = list.split("\n\n").filter(|e| !e.trim().is_empty());
    let main = entries.next().unwrap_or_default();
    let main_path = main
        .lines()
        .find_map(|l| l.strip_prefix("worktree "))
        .map(|p| {
            PathBuf::from(p)
                .canonicalize()
                .unwrap_or_else(|_| PathBuf::from(p))
        });
    let worktree = (main_path.as_deref() != Some(&root))
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
        branch,
        worktree,
        base,
    })
}

/// A new linked worktree in `dir`, on a new branch, both named after the
/// workspace: what the user typed, with spaces and slashes made dashes so
/// that it is a folder name and a branch name.
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
    let _ = std::fs::create_dir_all(dir);
    git(
        &repo.root,
        &[
            "worktree",
            "add",
            "-q",
            "-b",
            &slug,
            &path.to_string_lossy(),
        ],
    )?;
    Ok(path.canonicalize().unwrap_or(path))
}

/// The pull request the branch is on, as `#number title`, through the `gh`
/// CLI when it is installed and logged in; nothing otherwise. Slow, so it
/// is asked off the drawing thread.
pub fn pull_request(root: &Path) -> Option<String> {
    let out = std::process::Command::new("gh")
        .args([
            "pr",
            "view",
            "--json",
            "number,title",
            "-q",
            r##""#\(.number) \(.title)""##,
        ])
        .current_dir(root)
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

/// Watches the working tree: each poll, every changed path whose text moved
/// since the last poll becomes an edit — the step the agent just took, not
/// the whole distance from the base. The first poll only takes stock: what
/// was already on the branch is the CHANGES list's business.
#[derive(Default)]
pub struct Watcher {
    seen: Option<BTreeMap<String, String>>,
}

impl Watcher {
    pub fn poll(&mut self, repo: &Repo) -> Vec<EditEvent> {
        let Ok(changes) = changes(repo) else {
            return Vec::new();
        };
        let mut paths: Vec<String> = changes.into_iter().map(|c| c.path).collect();
        // A path put back the way it was is a change too, so what was seen
        // before is looked at again even when git no longer lists it.
        paths.extend(self.seen.iter().flat_map(|seen| seen.keys().cloned()));
        paths.sort();
        paths.dedup();
        let now: BTreeMap<String, String> = paths
            .into_iter()
            .map(|path| {
                let text = std::fs::read_to_string(repo.root.join(&path)).unwrap_or_default();
                (path, text)
            })
            .collect();
        let edits = match &self.seen {
            None => Vec::new(),
            Some(seen) => now
                .iter()
                .filter_map(|(path, text)| {
                    // A path not seen before starts from its base version,
                    // so the first edit to a clean file is that edit alone.
                    let old = match seen.get(path) {
                        Some(old) => old.clone(),
                        None => git(&repo.root, &["show", &format!("{}:{path}", repo.base)])
                            .unwrap_or_default(),
                    };
                    (old != *text).then(|| EditEvent::from_unified(path, &unified(&old, text)))
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
    fn a_worktree_is_added_on_a_branch_of_its_name() {
        let (dir, repo) = repo("yara-git-add-worktree");
        let trees = dir.path().join("trees");
        let path = worktree_add(&repo, &trees, " task/login flow ").unwrap();
        assert_eq!(path, trees.join("task-login-flow").canonicalize().unwrap());
        let added = open(&path, "main").unwrap();
        assert_eq!(added.branch, "task-login-flow");
        assert_eq!(added.worktree.as_deref(), Some("task-login-flow"));
        assert!(worktree_add(&repo, &trees, " ").is_err());
        assert!(
            worktree_add(&repo, &trees, "task/login flow").is_err(),
            "taken"
        );
    }

    #[test]
    fn the_watcher_reports_each_new_edit_once_and_never_what_was_already_there() {
        let (dir, repo) = repo("yara-git-watch");
        let mut watcher = Watcher::default();
        assert!(watcher.poll(&repo).is_empty(), "the first poll takes stock");
        assert!(watcher.poll(&repo).is_empty(), "nothing moved");
        // A clean file's first edit is measured from its committed version.
        dir.file("a.txt", "one\ntwo\nthree\n");
        let edits = watcher.poll(&repo);
        assert_eq!(edits.len(), 1);
        assert_eq!((edits[0].added(), edits[0].removed()), (1, 0));

        dir.file("a.txt", "one\nthree\n");
        dir.file("fresh.txt", "hello\n");
        let edits = watcher.poll(&repo);
        assert_eq!(edits.len(), 2);
        assert_eq!(edits[0].path, PathBuf::from("a.txt"));
        // The step just taken — one line gone — not the distance from main.
        assert_eq!((edits[0].added(), edits[0].removed()), (0, 1));
        assert_eq!(edits[0].hunks[0].lines[1].text, "two");
        assert_eq!(edits[1].path, PathBuf::from("fresh.txt"));
        assert_eq!(edits[1].added(), 1);
        assert!(watcher.poll(&repo).is_empty(), "reported once");

        // Putting the file back is an edit too, even though git is quiet.
        dir.file("a.txt", "one\ntwo\n");
        let edits = watcher.poll(&repo);
        assert_eq!(edits.len(), 1);
        assert_eq!((edits[0].added(), edits[0].removed()), (1, 1));
    }
}

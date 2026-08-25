//! Frontend-independent editor logic, shared by the GPU window and the TUI.

pub mod buffer;
pub mod clipboard;
pub mod command;
pub mod diff;
pub mod find;
pub mod fold;
pub mod fs_ops;
pub mod fuzzy;
pub mod git;
pub mod glob;
pub mod history;
pub mod indent;
pub mod markdown;
pub mod project;

#[cfg(feature = "pty")]
pub mod pty;
pub mod search;
pub mod settings;
pub mod syntax;
#[cfg(test)]
pub mod test_support;
pub mod theme;
pub mod update;

use std::path::PathBuf;

/// Where the editor keeps its own files: `%APPDATA%` on Windows, and the XDG
/// config directory — or `~/.config` — everywhere else.
pub fn config_dir() -> Option<PathBuf> {
    // An explicit directory wins over every convention: it is how tests keep
    // their settings out of the user's, and how a portable install carries
    // its own.
    if let Some(dir) = std::env::var_os("YARA_CONFIG_DIR") {
        return Some(PathBuf::from(dir));
    }
    if cfg!(windows) {
        if let Some(appdata) = std::env::var_os("APPDATA") {
            return Some(PathBuf::from(appdata).join("yara-code"));
        }
    }
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
        .or_else(|| std::env::var_os("USERPROFILE").map(|h| PathBuf::from(h).join(".config")))?;
    Some(base.join("yara-code"))
}

/// The documentation site, opened from Help → Documentation.
pub const DOCUMENTATION: &str = "https://vsdudakov.github.io/yara-code/";

/// Hands a URL to the desktop's own browser. Returns false when there is
/// nothing to hand it to — a headless machine, say — so the caller can say so
/// rather than pretending it worked.
pub fn open_url(url: &str) -> bool {
    #[cfg(target_os = "macos")]
    let (program, args): (&str, &[&str]) = ("open", &[]);
    #[cfg(target_os = "windows")]
    let (program, args): (&str, &[&str]) = ("cmd", &["/C", "start", ""]);
    #[cfg(all(unix, not(target_os = "macos")))]
    let (program, args): (&str, &[&str]) = ("xdg-open", &[]);

    std::process::Command::new(program)
        .args(args)
        .arg(url)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .is_ok()
}

/// Whether a status message reports something that failed, as opposed to
/// something that merely happened or is worth knowing. Both frontends colour
/// their status bar by this, so a failure looks like one in each.
pub fn is_failure(message: &str) -> bool {
    const FAILED: [&str; 7] = [
        "failed",
        "could not",
        "cannot",
        "no such",
        "not a ",
        "gone:",
        "unavailable",
    ];
    let lower = message.to_ascii_lowercase();
    FAILED.iter().any(|word| lower.contains(word))
}

/// `1 file`, `3 files`: a count with its noun, so no message has to say
/// "file(s)".
pub fn count(n: usize, noun: &str) -> String {
    if n == 1 {
        format!("1 {noun}")
    } else {
        format!("{n} {noun}s")
    }
}

/// What Save All has to say afterwards. A file that could not be written is
/// named, so the count never hides a failure.
pub fn save_all_report(saved: usize, failed: &[String]) -> String {
    if failed.is_empty() {
        format!("saved {}", count(saved, "file"))
    } else {
        format!(
            "saved {}; could not write {}",
            count(saved, "file"),
            failed.join(", ")
        )
    }
}

/// Resolves the project root a frontend was launched with. Without a path
/// argument the editor opens with no project at all, and the user picks a
/// folder from the File menu.
pub fn project_root(arg: Option<PathBuf>) -> Option<PathBuf> {
    arg.map(|root| root.canonicalize().unwrap_or(root))
}

/// What a launch path means: a folder is the project root, a file is opened in
/// a tab with its folder as the root.
#[derive(Default)]
pub struct Launch {
    pub root: Option<PathBuf>,
    pub file: Option<PathBuf>,
}

/// Reads the launch argument as either a folder to open or a file to open.
/// A path that names a file becomes that file in front, with its parent folder
/// as the project; anything else — a folder, or a path that does not exist yet
/// — is the project root, as `project_root` gives it.
pub fn launch(arg: Option<PathBuf>) -> Launch {
    let Some(path) = arg else {
        return Launch::default();
    };
    let path = path.canonicalize().unwrap_or(path);
    if path.is_file() {
        Launch {
            root: path.parent().map(std::path::Path::to_path_buf),
            file: Some(path),
        }
    } else {
        Launch {
            root: Some(path),
            file: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_explicit_config_directory_wins() {
        // Set for this process only; the other tests read their own.
        let dir = crate::core::test_support::Dir::new("yara-config-override");
        let _lock = crate::core::test_support::ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        std::env::set_var("YARA_CONFIG_DIR", dir.path());
        assert_eq!(config_dir().as_deref(), Some(dir.path()));
        std::env::remove_var("YARA_CONFIG_DIR");
        assert!(
            config_dir().is_some(),
            "a convention answers when nothing is set"
        );
    }

    #[test]
    fn a_count_names_its_noun_in_the_right_number() {
        assert_eq!(count(1, "file"), "1 file");
        assert_eq!(count(0, "result"), "0 results");
        assert_eq!(count(12, "occurrence"), "12 occurrences");
    }

    #[test]
    fn a_failure_is_told_from_a_notice_by_its_wording() {
        assert!(is_failure("could not write main.rs"));
        assert!(is_failure("terminal failed: no pty"));
        assert!(is_failure("not a git repository"));
        assert!(!is_failure("saved 3 files"));
        assert!(!is_failure("notable.txt opened"));
    }

    #[test]
    fn no_path_argument_means_no_project() {
        assert_eq!(project_root(None), None);
    }

    #[test]
    fn a_path_argument_is_made_absolute() {
        let dir = crate::core::test_support::Dir::new("yara-root-arg");
        let root = project_root(Some(dir.path().to_path_buf())).unwrap();
        assert!(root.is_absolute());
        assert_eq!(root, dir.path());
        // A path that does not exist cannot be canonicalised, and is kept as
        // given rather than dropped — the editor reports it later.
        let missing = PathBuf::from("/definitely/not/here");
        assert_eq!(project_root(Some(missing.clone())), Some(missing));
    }

    #[test]
    fn a_file_argument_opens_the_file_in_its_folder() {
        let dir = crate::core::test_support::Dir::new("yara-launch-file");
        let file = dir.file("main.rs", "fn main() {}\n");
        let launched = launch(Some(file.clone()));
        assert_eq!(
            launched.file.as_deref(),
            Some(file.canonicalize().unwrap().as_path())
        );
        assert_eq!(
            launched.root,
            file.canonicalize()
                .unwrap()
                .parent()
                .map(std::path::Path::to_path_buf)
        );
    }

    #[test]
    fn a_folder_argument_is_the_root_and_opens_nothing() {
        let dir = crate::core::test_support::Dir::new("yara-launch-dir");
        let launched = launch(Some(dir.path().to_path_buf()));
        assert_eq!(launched.root.as_deref(), Some(dir.path()));
        assert!(launched.file.is_none());
    }
}

//! Frontend-independent editor logic, shared by the GPU window and the TUI.

pub mod buffer;
pub mod command;
pub mod diff;
pub mod find;
pub mod fold;
pub mod fs_ops;
pub mod git;
pub mod glob;
pub mod history;
pub mod indent;
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

/// Resolves the project root a frontend was launched with. Without a path
/// argument the editor opens with no project at all, and the user picks a
/// folder from the File menu.
pub fn project_root(arg: Option<PathBuf>) -> Option<PathBuf> {
    arg.map(|root| root.canonicalize().unwrap_or(root))
}

#[cfg(test)]
mod tests {
    use super::*;

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
}

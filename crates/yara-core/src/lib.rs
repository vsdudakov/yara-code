//! Yara Code's core, rewritten for v1: everything the terminal frontend
//! shows but does not itself draw. Ported from the legacy `src/core` one
//! module at a time, each with its tests; nothing here may depend on it.

pub mod buffer;
pub mod command;
pub mod follow;
pub mod fuzzy;
pub mod git;
pub mod glob;
pub mod history;
pub mod keyboard;
pub mod pty;
pub mod search;
pub mod settings;
pub mod syntax;
#[cfg(test)]
pub mod test_support;
pub mod theme;
pub mod tree;

use std::path::PathBuf;

/// Where the editor keeps its own files: `%APPDATA%` on Windows, and the XDG
/// config directory — or `~/.config` — everywhere else. The folder is named
/// after the command, `ycode`, rather than after the repository.
pub fn config_dir() -> Option<PathBuf> {
    // An explicit directory wins over every convention: it is how tests keep
    // their settings out of the user's, and how a portable install carries
    // its own.
    if let Some(dir) = std::env::var_os("YARA_CONFIG_DIR") {
        return Some(PathBuf::from(dir));
    }
    if cfg!(windows) {
        if let Some(appdata) = std::env::var_os("APPDATA") {
            return Some(PathBuf::from(appdata).join("ycode"));
        }
    }
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
        .or_else(|| std::env::var_os("USERPROFILE").map(|h| PathBuf::from(h).join(".config")))?;
    Some(base.join("ycode"))
}

/// The user's home folder, as the platform spells it.
pub fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .filter(|home| home.is_dir())
}

/// The documentation site, opened from Help → Documentation.
pub const DOCUMENTATION: &str = "https://vsdudakov.github.io/yara-code/";

/// Hands a URL to the desktop's own browser; false when there is nothing to
/// hand it to — a headless machine, say — so the caller can say so.
pub fn open_url(url: &str) -> bool {
    let (program, args): (&str, &[&str]) = if cfg!(target_os = "macos") {
        ("open", &[])
    } else if cfg!(windows) {
        ("cmd", &["/C", "start", ""])
    } else {
        ("xdg-open", &[])
    };
    std::process::Command::new(program)
        .args(args)
        .arg(url)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_explicit_config_directory_wins_over_every_convention() {
        let _lock = test_support::ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        std::env::set_var("YARA_CONFIG_DIR", "/somewhere/else");
        assert_eq!(config_dir(), Some(PathBuf::from("/somewhere/else")));
        std::env::remove_var("YARA_CONFIG_DIR");
        let dir = config_dir().expect("a config directory on every platform");
        assert!(dir.ends_with("ycode"), "{}", dir.display());
    }
}

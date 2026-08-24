//! Frontend-independent editor logic, shared by the GPU window and the TUI.

pub mod buffer;
pub mod command;
pub mod find;
pub mod fold;
pub mod fs_ops;
pub mod git;
pub mod glob;
pub mod indent;

#[cfg(feature = "pty")]
pub mod pty;
pub mod search;
pub mod settings;
pub mod syntax;
pub mod theme;

use std::path::PathBuf;

/// Resolves the project root a frontend was launched with, defaulting to the
/// working directory.
pub fn project_root(arg: Option<PathBuf>) -> PathBuf {
    let root = arg.unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
    root.canonicalize().unwrap_or(root)
}

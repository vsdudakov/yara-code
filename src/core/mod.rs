//! Frontend-independent editor logic, shared by the GPU window and the TUI.

pub mod buffer;
pub mod command;
pub mod find;
pub mod fold;
pub mod fs_ops;
pub mod git;
pub mod glob;
pub mod indent;
pub mod project;

#[cfg(feature = "pty")]
pub mod pty;
pub mod search;
pub mod settings;
pub mod syntax;
pub mod theme;

use std::path::PathBuf;

/// Resolves the project root a frontend was launched with. Without a path
/// argument the editor opens with no project at all, and the user picks a
/// folder from the File menu.
pub fn project_root(arg: Option<PathBuf>) -> Option<PathBuf> {
    arg.map(|root| root.canonicalize().unwrap_or(root))
}

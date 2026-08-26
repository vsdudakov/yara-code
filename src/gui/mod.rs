//! GPU-accelerated window frontend (egui on wgpu).

pub mod app;
pub mod diff;
pub mod editor;
pub mod file_tree;
pub mod fold_view;
pub mod fonts;
pub mod git;
pub mod highlight;
pub mod keys;
/// The system menu bar, which only macOS has.
#[cfg(target_os = "macos")]
pub mod mac_menu;
pub mod preview;
pub mod terminal;
pub mod theme;

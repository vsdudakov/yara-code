//! Yara — a minimal code editor with two frontends over one core:
//! `gui` draws a GPU-accelerated window, `tui` draws inside a terminal.

pub mod core;

#[cfg(feature = "gui")]
pub mod gui;

#[cfg(feature = "tui")]
pub mod tui;

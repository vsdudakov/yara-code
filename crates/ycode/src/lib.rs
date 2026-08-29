//! The v1 terminal frontend. `app` holds state and takes input, `ui` paints
//! it; tests drive both through ratatui's test backend and read the frame.

pub mod app;
pub mod keys;
pub mod theme;
pub mod ui;

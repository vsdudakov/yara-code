//! Terminal frontend (ratatui on crossterm). Runs anywhere a shell does,
//! including over SSH on a headless server.

pub mod app;
pub mod clipboard;
pub mod icons;
pub mod menu;
pub mod shell;
pub mod theme;
pub mod tree;
pub mod ui;

use std::io;
use std::path::PathBuf;

use crossterm::event::{
    DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture,
};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};

/// Sets up the alternate screen, runs the editor, and restores the terminal
/// even if the app returns an error.
pub fn run(root: Option<PathBuf>) -> io::Result<()> {
    enable_raw_mode()?;
    let mut out = io::stdout();
    execute!(
        out,
        EnterAlternateScreen,
        EnableMouseCapture,
        EnableBracketedPaste
    )?;
    let backend = ratatui::backend::CrosstermBackend::new(out);
    let mut terminal = ratatui::Terminal::new(backend)?;

    let result = app::App::new(root).run(&mut terminal);

    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture,
        DisableBracketedPaste
    )?;
    terminal.show_cursor()?;
    result
}

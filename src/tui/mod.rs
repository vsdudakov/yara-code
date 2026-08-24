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
    KeyboardEnhancementFlags, PopKeyboardEnhancementFlags, PushKeyboardEnhancementFlags,
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
    // Without the kitty keyboard protocol a terminal cannot tell Ctrl+Shift+S
    // from Ctrl+S. Where it is available, ask for it: that is what makes the
    // VS Code-style second tier of bindings work.
    let enhanced = crossterm::terminal::supports_keyboard_enhancement().unwrap_or(false);
    if enhanced {
        execute!(
            out,
            PushKeyboardEnhancementFlags(KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES)
        )?;
    }

    let backend = ratatui::backend::CrosstermBackend::new(out);
    let mut terminal = ratatui::Terminal::new(backend)?;

    let result = app::App::new(root).run(&mut terminal);

    disable_raw_mode()?;
    if enhanced {
        execute!(terminal.backend_mut(), PopKeyboardEnhancementFlags)?;
    }
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture,
        DisableBracketedPaste
    )?;
    terminal.show_cursor()?;
    result
}

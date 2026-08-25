//! Terminal frontend (ratatui on crossterm). Runs anywhere a shell does,
//! including over SSH on a headless server.

pub mod app;
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
/// Puts the terminal back the way it was found. It lives on the stack of
/// `run`, so it runs on a normal return, an early `?`, and a panic unwinding
/// through — the user never gets their shell back in raw mode.
struct RestoreTerminal {
    enhanced: bool,
}

impl Drop for RestoreTerminal {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let mut out = io::stdout();
        if self.enhanced {
            let _ = execute!(out, PopKeyboardEnhancementFlags);
        }
        let _ = execute!(
            out,
            LeaveAlternateScreen,
            DisableMouseCapture,
            DisableBracketedPaste,
            crossterm::cursor::SetCursorStyle::DefaultUserShape,
            crossterm::cursor::Show
        );
    }
}

pub fn run(root: Option<PathBuf>, file: Option<PathBuf>) -> io::Result<()> {
    // A panic inside the draw loop would otherwise print into the alternate
    // screen and vanish with it. Restore first, then report.
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = disable_raw_mode();
        let _ = execute!(
            io::stdout(),
            LeaveAlternateScreen,
            DisableMouseCapture,
            DisableBracketedPaste,
            crossterm::cursor::SetCursorStyle::DefaultUserShape,
            crossterm::cursor::Show
        );
        default_hook(info);
    }));

    enable_raw_mode()?;
    let mut out = io::stdout();
    execute!(
        out,
        EnterAlternateScreen,
        EnableMouseCapture,
        EnableBracketedPaste,
        // A bar that blinks, the way an editor's caret does everywhere else —
        // the window frontend draws one, and a block that sits still reads as
        // a selection rather than a place to type.
        crossterm::cursor::SetCursorStyle::BlinkingBar
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

    let _restore = RestoreTerminal { enhanced };
    let backend = ratatui::backend::CrosstermBackend::new(out);
    let mut terminal = ratatui::Terminal::new(backend)?;

    let mut app = app::App::new(root);
    if let Some(file) = file {
        app.open(file);
    }
    if !enhanced {
        app.status = "this terminal cannot tell Ctrl+Shift from Ctrl — \
                      rebind those chords in settings.json"
            .into();
    }
    app.run(&mut terminal)
}

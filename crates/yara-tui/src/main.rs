//! The `ycode-next` command: the v1 terminal frontend, opened on the folder
//! given on the command line or on no project at all.

use std::io;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use crossterm::event::{
    self, Event, KeyboardEnhancementFlags, PopKeyboardEnhancementFlags,
    PushKeyboardEnhancementFlags,
};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;
use yara_tui::app::App;
use yara_tui::ui;

/// How long a quiet loop waits before looking for the agent's output again.
const IDLE: Duration = Duration::from_millis(50);

fn main() -> io::Result<()> {
    let mut app = App::load(std::env::args_os().nth(1).map(PathBuf::from));
    app.start_agent();
    app.refresh();

    enable_raw_mode()?;
    let mut out = io::stdout();
    execute!(out, EnterAlternateScreen)?;
    // Without the kitty keyboard protocol a terminal cannot tell Ctrl+Shift+S
    // from Ctrl+S; where it is available, ask for it.
    let enhanced = crossterm::terminal::supports_keyboard_enhancement().unwrap_or(false);
    if enhanced {
        execute!(
            out,
            PushKeyboardEnhancementFlags(KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES)
        )?;
    } else {
        app.note = Some(
            "this terminal cannot tell Ctrl+Shift from Ctrl — rebind those chords in settings.json"
                .into(),
        );
    }
    let mut terminal = Terminal::new(CrosstermBackend::new(io::stdout()))?;
    let result = run(&mut terminal, &mut app);
    if enhanced {
        let _ = execute!(io::stdout(), PopKeyboardEnhancementFlags);
    }
    let _ = execute!(io::stdout(), LeaveAlternateScreen);
    disable_raw_mode()?;
    result
}

fn run(terminal: &mut Terminal<CrosstermBackend<io::Stdout>>, app: &mut App) -> io::Result<()> {
    let refresh_every = Duration::from_millis(app.settings.refresh_ms.max(100));
    let mut last_refresh = Instant::now();
    let mut redraw = true;
    while !app.should_quit {
        if last_refresh.elapsed() >= refresh_every {
            app.refresh();
            last_refresh = Instant::now();
        }
        if redraw || app.take_dirty() {
            terminal.draw(|frame| ui::draw(frame, app))?;
            redraw = false;
        }
        if event::poll(IDLE)? {
            match event::read()? {
                Event::Key(key) => app.handle_key(key),
                Event::Resize(..) => {}
                _ => continue,
            }
            redraw = true;
        }
    }
    Ok(())
}

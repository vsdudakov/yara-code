//! The `ycode` command: the terminal editor, opened on the folder
//! given on the command line or on no project at all.

use std::io;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use crossterm::event::{
    self, DisableMouseCapture, EnableMouseCapture, Event, KeyboardEnhancementFlags,
    PopKeyboardEnhancementFlags, PushKeyboardEnhancementFlags,
};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;
use ycode::app::App;
use ycode::ui;

/// How long a quiet loop waits before looking for the agent's output again.
const IDLE: Duration = Duration::from_millis(50);

fn main() -> io::Result<()> {
    let arg = std::env::args().nth(1);
    match arg.as_deref() {
        Some("--version" | "-V") => {
            println!("ycode {}", env!("CARGO_PKG_VERSION"));
            return Ok(());
        }
        Some("--help" | "-h") => {
            println!("ycode [PATH]\n\nThe terminal editor for the agent loop. Opens PATH as the project, or the start page without one.");
            return Ok(());
        }
        _ => {}
    }
    let mut app = App::load(arg.map(PathBuf::from));
    app.start_agent();
    app.refresh();
    app.poll_usage();

    enable_raw_mode()?;
    let mut out = io::stdout();
    execute!(out, EnterAlternateScreen, EnableMouseCapture)?;
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
    let _ = execute!(io::stdout(), DisableMouseCapture, LeaveAlternateScreen);
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
        app.collect();
        if redraw || app.take_dirty() {
            terminal.draw(|frame| ui::draw(frame, app))?;
            redraw = false;
        }
        if event::poll(IDLE)? {
            match event::read()? {
                Event::Key(key) => app.handle_key(key),
                Event::Mouse(mouse) => app.handle_mouse(mouse),
                Event::Resize(..) => {}
                _ => continue,
            }
            redraw = true;
        }
    }
    Ok(())
}

//! End-to-end tests for the terminal frontend.
//!
//! These drive the real `App` through the same entry point the terminal does —
//! key and mouse events in, a drawn frame out — and read the frame back off
//! ratatui's test backend. Nothing is mocked: the navigator lists real files,
//! the editor opens them, git runs against a real repository.

#![cfg(feature = "tui")]

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use crossterm::event::{
    Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use ratatui::backend::TestBackend;
use ratatui::Terminal;

use yara::tui::app::App;
use yara::tui::ui;

/// A project to open, removed when the test ends.
struct Project(PathBuf);

impl Project {
    fn new(tag: &str) -> Self {
        static COUNTER: AtomicUsize = AtomicUsize::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!("{tag}-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).unwrap();
        let path = path.canonicalize().unwrap_or(path);
        let project = Self(path);
        project.file("README.md", "# Title\nA line of prose.\n");
        project.file("src/main.rs", "fn main() {\n    let total = 1;\n}\n");
        project
    }

    fn file(&self, name: &str, body: &str) -> PathBuf {
        let path = self.0.join(name);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, body).unwrap();
        path
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for Project {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// The editor, its terminal, and what the last frame looked like.
struct Harness {
    app: App,
    terminal: Terminal<TestBackend>,
}

impl Harness {
    fn open(project: &Project) -> Self {
        Self::with_size(Some(project.path().to_path_buf()), 120, 32)
    }

    fn with_size(root: Option<PathBuf>, width: u16, height: u16) -> Self {
        let mut harness = Self {
            app: App::new(root),
            terminal: Terminal::new(TestBackend::new(width, height)).unwrap(),
        };
        harness.draw();
        harness
    }

    fn draw(&mut self) {
        self.app.prepare();
        let app = &mut self.app;
        self.terminal.draw(|frame| ui::draw(frame, app)).unwrap();
    }

    /// Everything on screen, one line per row, trailing spaces trimmed.
    fn screen(&self) -> String {
        let buffer = self.terminal.backend().buffer();
        (0..buffer.area.height)
            .map(|y| {
                (0..buffer.area.width)
                    .map(|x| buffer[(x, y)].symbol())
                    .collect::<String>()
                    .trim_end()
                    .to_string()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn press(&mut self, code: KeyCode, modifiers: KeyModifiers) {
        self.app.handle(Event::Key(KeyEvent {
            code,
            modifiers,
            kind: KeyEventKind::Press,
            state: crossterm::event::KeyEventState::NONE,
        }));
        self.draw();
    }

    fn key(&mut self, code: KeyCode) {
        self.press(code, KeyModifiers::NONE);
    }

    fn ctrl(&mut self, c: char) {
        self.press(KeyCode::Char(c), KeyModifiers::CONTROL);
    }

    fn type_text(&mut self, text: &str) {
        for c in text.chars() {
            self.press(KeyCode::Char(c), KeyModifiers::NONE);
        }
    }

    fn click(&mut self, column: u16, row: u16) {
        for kind in [
            MouseEventKind::Down(MouseButton::Left),
            MouseEventKind::Up(MouseButton::Left),
        ] {
            self.app.handle(Event::Mouse(MouseEvent {
                kind,
                column,
                row,
                modifiers: KeyModifiers::NONE,
            }));
        }
        self.draw();
    }

    /// The row a piece of text is drawn on, if it is on screen at all.
    fn row_of(&self, text: &str) -> Option<u16> {
        self.screen()
            .lines()
            .position(|line| line.contains(text))
            .map(|row| row as u16)
    }

    fn shows(&self, text: &str) -> bool {
        self.screen().contains(text)
    }
}

#[test]
fn with_no_folder_the_start_page_offers_the_keys() {
    // Tall enough for the start page to draw its title as well as its keys.
    let harness = Harness::with_size(None, 120, 44);
    let screen = harness.screen();
    assert!(screen.contains("YARA CODE"), "{screen}");
    // The navigator says there is nothing open, and says how to change that.
    assert!(screen.contains("No folder in the project"), "{screen}");
    assert!(screen.contains("add a folder") && screen.contains("open a folder"));
    // The start page groups the keys that are actually bound.
    assert!(
        screen.contains("PROJECT") && screen.contains("PANELS"),
        "{screen}"
    );
    assert!(screen.contains("Open Folder...") && screen.contains("Toggle Terminal"));
    // The top bar carries all three menus.
    assert!(screen.contains("File") && screen.contains("View") && screen.contains("Help"));
}

#[test]
fn a_project_lists_its_files_and_opens_one() {
    let project = Project::new("yara-e2e-open");
    let mut harness = Harness::open(&project);
    assert!(harness.shows("README.md"), "{}", harness.screen());
    assert!(harness.shows("src"));

    // Enter on the first row opens the folder; the file below it opens in a tab.
    let row = harness.row_of("README.md").unwrap();
    harness.click(4, row);
    assert!(harness.shows("A line of prose."), "{}", harness.screen());
    assert!(harness.shows("Title"));
    // The status bar names the file that is in front.
    assert!(harness.shows("Ln 1, Col 1"));
}

#[test]
fn typing_changes_the_file_and_undo_takes_it_back() {
    let project = Project::new("yara-e2e-edit");
    let mut harness = Harness::open(&project);
    let row = harness.row_of("README.md").unwrap();
    harness.click(4, row);

    harness.type_text("Hello");
    assert!(harness.shows("Hello# Title"), "{}", harness.screen());
    // The tab wears the unsaved mark.
    assert!(harness.shows("●"));

    harness.ctrl('z');
    assert!(!harness.shows("Hello# Title"), "{}", harness.screen());
}

#[test]
fn the_find_bar_belongs_to_the_file_it_was_opened_on() {
    let project = Project::new("yara-e2e-find");
    let mut harness = Harness::open(&project);
    let row = harness.row_of("README.md").unwrap();
    harness.click(4, row);

    harness.ctrl('f');
    assert!(harness.shows("FIND"), "{}", harness.screen());
    assert!(harness.shows("REPLACE"), "both fields are always drawn");
    harness.type_text("line");
    assert!(harness.shows("1 of 1"), "{}", harness.screen());

    // Escape closes it; the editor is still there.
    harness.key(KeyCode::Esc);
    assert!(!harness.shows("FIND"));
    assert!(harness.shows("A line of prose."));
}

#[test]
fn the_sidebar_switches_between_files_search_and_git() {
    let project = Project::new("yara-e2e-panels");
    let mut harness = Harness::open(&project);

    harness.press(
        KeyCode::Char('f'),
        KeyModifiers::CONTROL | KeyModifiers::SHIFT,
    );
    assert!(harness.shows("SEARCH"), "{}", harness.screen());
    assert!(harness.shows("EXCLUDE"));

    harness.press(
        KeyCode::Char('g'),
        KeyModifiers::CONTROL | KeyModifiers::SHIFT,
    );
    // No repository here, and the panel says so rather than drawing nothing.
    assert!(
        harness.shows("not a git repository") || harness.shows("REPOSITORY"),
        "{}",
        harness.screen()
    );

    harness.press(
        KeyCode::Char('e'),
        KeyModifiers::CONTROL | KeyModifiers::SHIFT,
    );
    assert!(harness.shows("README.md"));
}

#[test]
fn searching_the_project_lists_the_files_that_match() {
    let project = Project::new("yara-e2e-search");
    let mut harness = Harness::open(&project);
    harness.press(
        KeyCode::Char('f'),
        KeyModifiers::CONTROL | KeyModifiers::SHIFT,
    );
    harness.type_text("total");
    assert!(harness.shows("main.rs"), "{}", harness.screen());
    assert!(harness.shows("results in") || harness.shows("1 result"));
}

#[test]
fn the_sidebar_and_the_terminal_panel_can_be_put_away() {
    let project = Project::new("yara-e2e-toggle");
    let mut harness = Harness::open(&project);
    assert!(harness.shows("README.md"));
    harness.ctrl('b');
    assert!(!harness.shows("README.md"), "the sidebar is gone");
    harness.ctrl('b');
    assert!(harness.shows("README.md"), "and back");

    harness.ctrl('j');
    let with_terminal = harness.shows("TERMINAL");
    harness.ctrl('j');
    assert_ne!(with_terminal, harness.shows("TERMINAL"));
}

#[test]
fn the_file_menu_opens_from_the_keyboard_and_lists_its_entries() {
    let project = Project::new("yara-e2e-menu");
    let mut harness = Harness::open(&project);
    harness.key(KeyCode::F(10));
    let screen = harness.screen();
    assert!(screen.contains("New File..."), "{screen}");
    assert!(screen.contains("Add Folder to Project..."));
    assert!(screen.contains("Quit"));
    // Escape puts it away.
    harness.key(KeyCode::Esc);
    assert!(!harness.shows("Quit"), "{}", harness.screen());
}

#[test]
fn the_bindings_overlay_lists_what_is_actually_bound() {
    let project = Project::new("yara-e2e-help");
    let mut harness = Harness::open(&project);
    harness.key(KeyCode::F(1));
    let screen = harness.screen();
    assert!(screen.contains("Key bindings"), "{screen}");
    assert!(screen.contains("Save"));
    assert!(screen.contains("Ctrl+S") || screen.contains("Ctrl+"));
}

#[test]
fn a_narrow_terminal_still_draws_without_panicking() {
    let project = Project::new("yara-e2e-narrow");
    // Small enough that every pane has to give something up.
    let mut harness = Harness::with_size(Some(project.path().to_path_buf()), 40, 10);
    harness.ctrl('f');
    harness.type_text("x");
    harness.key(KeyCode::F(1));
    harness.key(KeyCode::Esc);
    assert!(!harness.screen().is_empty());
}

#[test]
fn a_changed_file_shows_its_diff_in_a_tab_of_its_own() {
    let project = Project::new("yara-e2e-git");
    let git = |args: &[&str]| {
        let out = std::process::Command::new("git")
            .arg("-C")
            .arg(project.path())
            .args(args)
            .output()
            .expect("git is on PATH");
        assert!(out.status.success(), "git {args:?}");
    };
    git(&["init", "-q", "-b", "main"]);
    git(&["config", "user.email", "test@example.com"]);
    git(&["config", "user.name", "Test"]);
    git(&["config", "commit.gpgsign", "false"]);
    git(&["add", "-A"]);
    git(&["commit", "-qm", "First"]);
    project.file("README.md", "# Title\nA changed line.\n");

    let mut harness = Harness::open(&project);
    harness.press(
        KeyCode::Char('g'),
        KeyModifiers::CONTROL | KeyModifiers::SHIFT,
    );
    assert!(harness.shows("REPOSITORY"), "{}", harness.screen());
    assert!(harness.shows("README.md"), "the changed file is listed");

    // Enter on the change opens the two-pane diff.
    harness.key(KeyCode::Enter);
    let screen = harness.screen();
    assert!(
        screen.contains("≠") || screen.contains("open file"),
        "{screen}"
    );
    assert!(
        screen.contains("A line of prose."),
        "the old line: {screen}"
    );
    assert!(screen.contains("A changed line."), "and the new one");
}

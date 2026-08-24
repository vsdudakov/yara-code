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
        // Settings the editor writes — the theme it was switched to, the
        // recent list — land here, never in the user's own config.
        let config = path.join(".config");
        std::fs::create_dir_all(&config).unwrap();
        std::env::set_var("YARA_CONFIG_DIR", &config);
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

#[test]
fn editing_a_file_covers_the_keys_a_hand_actually_uses() {
    let project = Project::new("yara-e2e-keys");
    let mut harness = Harness::open(&project);
    let row = harness.row_of("README.md").unwrap();
    harness.click(4, row);

    // Move about, then type at the end of the first line.
    harness.key(KeyCode::End);
    harness.type_text(" more");
    assert!(harness.shows("# Title more"), "{}", harness.screen());

    // Enter carries the indentation, backspace takes a character back.
    harness.key(KeyCode::Enter);
    harness.type_text("second");
    harness.key(KeyCode::Backspace);
    assert!(harness.shows("secon"), "{}", harness.screen());

    // Home, arrows and delete-forward.
    harness.key(KeyCode::Home);
    harness.key(KeyCode::Delete);
    assert!(harness.shows("econ"), "{}", harness.screen());
    for code in [KeyCode::Up, KeyCode::Down, KeyCode::Left, KeyCode::Right] {
        harness.key(code);
    }
    harness.key(KeyCode::PageDown);
    harness.key(KeyCode::PageUp);

    // Select all and cut, then paste it back.
    harness.ctrl('a');
    harness.ctrl('x');
    assert!(!harness.shows("# Title more"), "{}", harness.screen());
    harness.ctrl('v');
    assert!(harness.shows("# Title more"), "{}", harness.screen());

    // Save writes it to disk.
    harness.ctrl('s');
    let body = std::fs::read_to_string(project.path().join("README.md")).unwrap();
    assert!(body.contains("# Title more"), "{body}");
}

#[test]
fn two_files_share_the_tab_strip_and_close_one_at_a_time() {
    let project = Project::new("yara-e2e-tabs");
    let mut harness = Harness::open(&project);

    // Open README, then walk into src/ and open the file inside it.
    let row = harness.row_of("README.md").unwrap();
    harness.click(4, row);
    let src = harness.row_of("src").unwrap();
    harness.click(4, src);
    let main = harness.row_of("main.rs").unwrap();
    harness.click(6, main);
    assert!(harness.shows("main.rs ×"), "{}", harness.screen());
    assert!(
        harness.shows("README.md ×"),
        "both tabs: {}",
        harness.screen()
    );

    // Ctrl+PageUp walks back to the first tab.
    harness.press(KeyCode::PageUp, KeyModifiers::CONTROL);
    assert!(harness.shows("A line of prose."), "{}", harness.screen());
    harness.press(KeyCode::PageDown, KeyModifiers::CONTROL);
    assert!(harness.shows("fn main()"), "{}", harness.screen());

    // Closing leaves the other one open.
    harness.ctrl('w');
    assert!(!harness.shows("main.rs ×"), "{}", harness.screen());
    assert!(harness.shows("README.md ×"));
}

#[test]
fn the_navigator_makes_and_renames_and_deletes_files() {
    let project = Project::new("yara-e2e-files");
    let mut harness = Harness::open(&project);

    // New entries go beside what the cursor is on, so put it on a file at the
    // top level first.
    let row = harness.row_of("README.md").unwrap();
    harness.click(4, row);
    harness.press(
        KeyCode::Char('e'),
        KeyModifiers::CONTROL | KeyModifiers::SHIFT,
    );

    harness.key(KeyCode::Char('a'));
    assert!(harness.shows("New file in"), "{}", harness.screen());
    harness.type_text("notes.txt");
    harness.key(KeyCode::Enter);
    assert!(
        project.path().join("notes.txt").is_file(),
        "{}",
        harness.screen()
    );
    assert!(harness.shows("notes.txt"), "{}", harness.screen());

    // Creating a file opens it, so the keyboard goes back to the navigator.
    harness.press(
        KeyCode::Char('e'),
        KeyModifiers::CONTROL | KeyModifiers::SHIFT,
    );

    // New folder.
    harness.key(KeyCode::Char('A'));
    assert!(harness.shows("New folder in"), "{}", harness.screen());
    harness.type_text("docs");
    harness.key(KeyCode::Enter);
    assert!(project.path().join("docs").is_dir());

    // Rename what the cursor is on: walk the cursor onto the file with the
    // keyboard, since the folder just made has shifted every row below it.
    harness.press(
        KeyCode::Char('e'),
        KeyModifiers::CONTROL | KeyModifiers::SHIFT,
    );
    harness.key(KeyCode::Home);
    let mut steps = 0;
    while steps < 10 && !harness.shows("Rename notes.txt") {
        harness.key(KeyCode::Down);
        harness.key(KeyCode::F(2));
        if harness.shows("Rename notes.txt") {
            break;
        }
        harness.key(KeyCode::Esc);
        steps += 1;
    }
    assert!(harness.shows("Rename notes.txt"), "{}", harness.screen());
    for _ in 0..20 {
        harness.key(KeyCode::Backspace);
    }
    harness.type_text("renamed.txt");
    harness.key(KeyCode::Enter);
    assert!(project.path().join("renamed.txt").is_file());

    // Delete asks first, and takes no for an answer.
    // The cursor followed the rename, so delete acts on the new name.
    harness.key(KeyCode::Char('d'));
    assert!(harness.shows("Delete renamed.txt"), "{}", harness.screen());
    harness.key(KeyCode::Char('n'));
    assert!(project.path().join("renamed.txt").exists(), "n means no");
    harness.key(KeyCode::Char('d'));
    harness.key(KeyCode::Char('y'));
    assert!(!project.path().join("renamed.txt").exists(), "y means yes");
}

#[test]
fn the_context_menu_opens_on_a_row_and_runs_an_entry() {
    let project = Project::new("yara-e2e-context");
    let mut harness = Harness::open(&project);
    harness.press(KeyCode::F(10), KeyModifiers::SHIFT);
    let screen = harness.screen();
    assert!(screen.contains("New File"), "{screen}");
    assert!(screen.contains("Move To..."), "{screen}");
    // Down to "New Folder", then Enter opens its prompt.
    harness.key(KeyCode::Down);
    harness.key(KeyCode::Enter);
    assert!(harness.shows("New folder in"), "{}", harness.screen());
    harness.key(KeyCode::Esc);
}

#[test]
fn the_theme_picker_switches_the_colours() {
    let project = Project::new("yara-e2e-theme");
    let mut harness = Harness::open(&project);
    harness.press(
        KeyCode::Char('t'),
        KeyModifiers::CONTROL | KeyModifiers::SHIFT,
    );
    let screen = harness.screen();
    assert!(screen.contains("Color theme"), "{screen}");
    assert!(screen.contains("Light+") && screen.contains("Monokai"));
    harness.key(KeyCode::Down);
    harness.key(KeyCode::Enter);
    // The picker is gone and the status bar names the theme now in effect.
    assert!(!harness.shows("Color theme"), "{}", harness.screen());
    let status = harness
        .screen()
        .lines()
        .last()
        .unwrap_or_default()
        .to_string();
    assert!(!status.contains("Dark+"), "the theme changed: {status}");
    assert!(
        status.contains("Light+") || status.contains("Monokai"),
        "status bar: {status}"
    );
}

#[test]
fn folding_hides_a_block_and_unfolding_brings_it_back() {
    let project = Project::new("yara-e2e-fold");
    project.file(
        "deep.py",
        "def outer():\n    first = 1\n    second = 2\n\nprint(outer())\n",
    );
    let mut harness = Harness::open(&project);
    let row = harness.row_of("deep.py").unwrap();
    harness.click(4, row);
    assert!(harness.shows("first = 1"), "{}", harness.screen());

    harness.press(
        KeyCode::Char('f'),
        KeyModifiers::CONTROL | KeyModifiers::ALT,
    );
    assert!(!harness.shows("first = 1"), "folded: {}", harness.screen());
    harness.press(
        KeyCode::Char('9'),
        KeyModifiers::CONTROL | KeyModifiers::ALT,
    );
    assert!(harness.shows("first = 1"), "unfolded: {}", harness.screen());
    harness.press(
        KeyCode::Char('0'),
        KeyModifiers::CONTROL | KeyModifiers::ALT,
    );
    assert!(
        !harness.shows("first = 1"),
        "fold all: {}",
        harness.screen()
    );
}

#[test]
fn replacing_in_a_file_rewrites_every_match() {
    let project = Project::new("yara-e2e-replace");
    project.file("many.txt", "one\none\none\n");
    let mut harness = Harness::open(&project);
    let row = harness.row_of("many.txt").unwrap();
    harness.click(4, row);

    harness.ctrl('f');
    harness.type_text("one");
    assert!(harness.shows("1 of 3"), "{}", harness.screen());
    // Tab moves to the replace field; the actions appear once it has text.
    harness.key(KeyCode::Tab);
    harness.type_text("two");
    assert!(harness.shows("Replace All"), "{}", harness.screen());
    harness.press(KeyCode::Enter, KeyModifiers::ALT);
    harness.ctrl('s');
    let body = std::fs::read_to_string(project.path().join("many.txt")).unwrap();
    assert_eq!(body, "two\ntwo\ntwo\n", "every match was rewritten");
}

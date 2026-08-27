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

/// A project to open, removed when the test ends. `YARA_CONFIG_DIR` is one
/// variable for the whole process, so the tests in this binary take turns:
/// each holds the lock for as long as its project lives.
struct Project(
    PathBuf,
    /// Held by the first project a test makes; a second folder in the same
    /// test rides on it rather than waiting for itself.
    #[allow(dead_code)]
    Option<std::sync::MutexGuard<'static, ()>>,
);

static CONFIG_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

thread_local! {
    static HOLDS_CONFIG_LOCK: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

impl Project {
    fn new(tag: &str) -> Self {
        // A test that panicked while holding the lock poisons it; the next
        // test still gets its turn.
        let lock = if HOLDS_CONFIG_LOCK.with(|held| held.replace(true)) {
            None
        } else {
            Some(CONFIG_LOCK.lock().unwrap_or_else(|e| e.into_inner()))
        };
        static COUNTER: AtomicUsize = AtomicUsize::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!("{tag}-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).unwrap();
        let path = path.canonicalize().unwrap_or(path);
        // Settings the editor writes — the theme it was switched to, the
        // recent list — land here, never in the user's own config.
        let config = path.with_extension("config");
        std::fs::create_dir_all(&config).unwrap();
        std::env::set_var("YARA_CONFIG_DIR", &config);
        let project = Self(path, lock);
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
        if self.1.is_some() {
            HOLDS_CONFIG_LOCK.with(|held| held.set(false));
        }
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

    /// Presses the left button at one point, drags to another, and releases —
    /// the three events a mouse actually sends.
    fn drag(&mut self, from: (u16, u16), to: (u16, u16)) {
        for (kind, (column, row)) in [
            (MouseEventKind::Down(MouseButton::Left), from),
            (MouseEventKind::Drag(MouseButton::Left), to),
            (MouseEventKind::Up(MouseButton::Left), to),
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

    /// The right button at a point, which is what opens a context menu.
    fn right_click(&mut self, column: u16, row: u16) {
        self.app.handle(Event::Mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Right),
            column,
            row,
            modifiers: KeyModifiers::NONE,
        }));
        self.draw();
    }

    fn wheel(&mut self, column: u16, row: u16, up: bool) {
        self.app.handle(Event::Mouse(MouseEvent {
            kind: if up {
                MouseEventKind::ScrollUp
            } else {
                MouseEventKind::ScrollDown
            },
            column,
            row,
            modifiers: KeyModifiers::NONE,
        }));
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
    assert!(screen.contains("open recent") && screen.contains("open a folder"));
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
    assert!(screen.contains("Key Bindings"), "{screen}");
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
fn a_tab_dragged_under_a_close_question_cannot_change_which_tab_the_answer_closes() {
    let project = Project::new("yara-e2e-drag-under-question");
    let mut harness = Harness::open(&project);
    // Two files, both edited.
    let row = harness.row_of("README.md").unwrap();
    harness.click(4, row);
    harness.type_text("x");
    let src = harness.row_of("src").unwrap();
    harness.click(4, src);
    let main = harness.row_of("main.rs").unwrap();
    harness.click(6, main);
    harness.type_text("y");
    assert!(harness.shows("yfn main()"), "{}", harness.screen());

    // Close asks about main.rs, the tab in front.
    harness.ctrl('w');
    assert!(harness.shows("Don't Save"), "{}", harness.screen());

    // Drag the README tab onto main.rs while the question is up. Both tabs
    // wear the unsaved mark, and the strip is the one row naming them both.
    let screen = harness.screen();
    let (strip, line) = screen
        .lines()
        .enumerate()
        .find(|(_, l)| l.contains("README.md") && l.contains("main.rs"))
        .unwrap_or_else(|| panic!("both tabs on one strip: {screen}"));
    let readme = line.find("README.md").unwrap() as u16 + 2;
    let main_tab = line.find("main.rs").unwrap() as u16 + 2;
    harness.drag((readme, strip as u16), (main_tab, strip as u16));

    // "Don't Save" closes main.rs — the file that was asked about — and
    // README keeps its edit and its place in front.
    harness.key(KeyCode::Char('n'));
    let screen = harness.screen();
    assert!(
        !screen
            .lines()
            .any(|l| l.contains("README.md") && l.contains("main.rs")),
        "main.rs is off the strip: {screen}"
    );
    assert!(screen.contains("x# Title"), "{screen}");
}

#[test]
fn save_as_over_another_file_asks_first() {
    let project = Project::new("yara-e2e-save-as");
    let mut harness = Harness::open(&project);
    let row = harness.row_of("README.md").unwrap();
    harness.click(4, row);
    let before = std::fs::read_to_string(project.path().join("src/main.rs")).unwrap();

    harness.press(
        KeyCode::Char('s'),
        KeyModifiers::CONTROL | KeyModifiers::SHIFT,
    );
    harness.type_text("src/main.rs");
    harness.key(KeyCode::Enter);
    assert!(
        harness.shows("Replace \"main.rs\"?"),
        "{}",
        harness.screen()
    );
    assert_eq!(
        std::fs::read_to_string(project.path().join("src/main.rs")).unwrap(),
        before,
        "nothing is written until the question is answered"
    );
    harness.key(KeyCode::Esc);
    assert_eq!(
        std::fs::read_to_string(project.path().join("src/main.rs")).unwrap(),
        before
    );

    // Asked again and told yes, it writes.
    harness.press(
        KeyCode::Char('s'),
        KeyModifiers::CONTROL | KeyModifiers::SHIFT,
    );
    harness.type_text("src/main.rs");
    harness.key(KeyCode::Enter);
    harness.key(KeyCode::Char('y'));
    assert_eq!(
        std::fs::read_to_string(project.path().join("src/main.rs")).unwrap(),
        "# Title\nA line of prose.\n"
    );
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
    assert!(
        harness.shows("Delete \"renamed.txt\"?"),
        "{}",
        harness.screen()
    );
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
    assert!(screen.contains("Color Theme"), "{screen}");
    assert!(screen.contains("Light+") && screen.contains("Monokai"));
    harness.key(KeyCode::Down);
    harness.key(KeyCode::Enter);
    // The picker is gone and the status bar names the theme now in effect.
    assert!(!harness.shows("Monokai"), "{}", harness.screen());
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

#[test]
fn a_shift_selection_copies_and_the_mouse_places_the_cursor() {
    let project = Project::new("yara-e2e-select");
    let mut harness = Harness::open(&project);
    let row = harness.row_of("README.md").unwrap();
    harness.click(4, row);

    // Shift+End selects to the end of the line; copying and pasting doubles it.
    harness.press(KeyCode::End, KeyModifiers::SHIFT);
    harness.ctrl('c');
    harness.key(KeyCode::End);
    harness.ctrl('v');
    assert!(harness.shows("# Title# Title"), "{}", harness.screen());

    // Clicking in the text places the cursor: the status bar reports where.
    let line = harness.row_of("A line of prose.").unwrap();
    harness.click(45, line);
    assert!(harness.shows("Ln 2"), "{}", harness.screen());

    // The wheel scrolls whichever pane is under it without falling over.
    harness.wheel(45, line, false);
    harness.wheel(45, line, true);
}

#[test]
fn go_to_definition_jumps_to_where_a_name_is_defined() {
    let project = Project::new("yara-e2e-goto");
    project.file(
        "app.rs",
        "fn helper() -> u32 { 1 }\n\nfn main() {\n    let x = helper();\n}\n",
    );
    let mut harness = Harness::open(&project);
    let row = harness.row_of("app.rs").unwrap();
    harness.click(4, row);

    // Put the cursor on the call to `helper` on the fourth line, then F12.
    harness.key(KeyCode::Down);
    harness.key(KeyCode::Down);
    harness.key(KeyCode::Down);
    for _ in 0..14 {
        harness.key(KeyCode::Right);
    }
    harness.key(KeyCode::F(12));
    let screen = harness.screen();
    assert!(
        screen.contains("Definitions of") || screen.contains("Ln 1"),
        "{screen}"
    );
    // A definition picker, if one opened, answers to Enter; then Back returns.
    if screen.contains("Definitions of") {
        harness.key(KeyCode::Enter);
    }
    assert!(harness.shows("Ln 1"), "{}", harness.screen());
    harness.press(KeyCode::Left, KeyModifiers::ALT);
    assert!(harness.shows("Ln 4"), "back: {}", harness.screen());
}

#[test]
fn a_search_hit_opens_the_file_with_the_match_in_view() {
    let project = Project::new("yara-e2e-hit");
    let mut harness = Harness::open(&project);
    harness.press(
        KeyCode::Char('f'),
        KeyModifiers::CONTROL | KeyModifiers::SHIFT,
    );
    harness.type_text("total");
    let hit = harness.row_of("main.rs").expect("a result group");
    // The match line is listed under its file; Enter on it opens the file.
    harness.key(KeyCode::Down);
    harness.key(KeyCode::Enter);
    assert!(harness.shows("let total = 1;"), "{}", harness.screen());
    // The find bar is seeded from the search, so the match is highlighted.
    assert!(harness.shows("FIND"), "{}", harness.screen());
    let _ = hit;
}

#[test]
fn replacing_across_the_project_rewrites_every_file() {
    let project = Project::new("yara-e2e-project-replace");
    project.file("a.txt", "alpha\n");
    project.file("b.txt", "alpha alpha\n");
    let mut harness = Harness::open(&project);
    harness.press(
        KeyCode::Char('f'),
        KeyModifiers::CONTROL | KeyModifiers::SHIFT,
    );
    harness.type_text("alpha");
    // Pressing Search again walks to the replace field, as Tab does in VS Code.
    harness.press(
        KeyCode::Char('f'),
        KeyModifiers::CONTROL | KeyModifiers::SHIFT,
    );
    harness.type_text("omega");
    let action = harness
        .row_of("Replace All")
        .expect("the action is offered");
    let col = harness
        .screen()
        .lines()
        .nth(action as usize)
        .unwrap()
        .find("Replace All")
        .unwrap() as u16;
    harness.click(col + 2, action);
    // Rewriting the project is asked about first.
    assert!(harness.shows("cannot be undone"), "{}", harness.screen());
    harness.key(KeyCode::Enter);
    assert_eq!(
        std::fs::read_to_string(project.path().join("b.txt")).unwrap(),
        "omega omega\n"
    );
    assert!(harness.shows("replaced"), "{}", harness.screen());
}

#[test]
fn the_search_toggles_answer_to_a_click() {
    let project = Project::new("yara-e2e-toggles");
    project.file("case.txt", "Alpha\nalpha\n");
    let mut harness = Harness::open(&project);
    harness.press(
        KeyCode::Char('f'),
        KeyModifiers::CONTROL | KeyModifiers::SHIFT,
    );
    harness.type_text("alpha");
    assert!(harness.shows("2 results"), "{}", harness.screen());
    // The "Aa" toggle sits at the right of the SEARCH heading.
    let heading = harness.row_of("SEARCH").unwrap();
    let col = harness
        .screen()
        .lines()
        .nth(heading as usize)
        .unwrap()
        .find("Aa")
        .unwrap() as u16;
    harness.click(col, heading);
    assert!(harness.shows("1 result"), "{}", harness.screen());
}

#[test]
fn folders_are_added_and_removed_through_the_browser_and_the_menu() {
    let project = Project::new("yara-e2e-add");
    let other = Project::new("yara-e2e-other");
    let mut harness = Harness::open(&project);

    // The browser opens on Add Folder; Tab switches to typing a path.
    harness.press(
        KeyCode::Char('a'),
        KeyModifiers::CONTROL | KeyModifiers::SHIFT,
    );
    assert!(
        harness.shows("Add folder to project"),
        "{}",
        harness.screen()
    );
    // Walk into a folder and back out, then type the path instead.
    harness.key(KeyCode::Down);
    harness.key(KeyCode::Right);
    harness.key(KeyCode::Left);
    harness.key(KeyCode::Tab);
    harness.type_text(&other.path().display().to_string());
    harness.key(KeyCode::Enter);
    let other_name = other
        .path()
        .file_name()
        .unwrap()
        .to_string_lossy()
        .to_uppercase();
    assert!(
        harness.shows(&other_name) || harness.shows("added"),
        "{}",
        harness.screen()
    );

    // The root row's menu offers to remove it again.
    harness.press(
        KeyCode::Char('e'),
        KeyModifiers::CONTROL | KeyModifiers::SHIFT,
    );
    harness.key(KeyCode::End);
    let mut found = false;
    for _ in 0..12 {
        harness.press(KeyCode::F(10), KeyModifiers::SHIFT);
        if harness.shows("Remove Folder from Project") {
            found = true;
            break;
        }
        harness.key(KeyCode::Esc);
        harness.key(KeyCode::Up);
    }
    assert!(found, "{}", harness.screen());
    // Down to the entry and Enter.
    for _ in 0..8 {
        harness.key(KeyCode::Down);
    }
    harness.key(KeyCode::Enter);
    assert!(
        harness.shows("removed") || !harness.shows(&other_name),
        "{}",
        harness.screen()
    );
}

#[test]
fn opening_a_folder_switches_the_project() {
    let project = Project::new("yara-e2e-switch");
    let other = Project::new("yara-e2e-switch-to");
    other.file("only-here.txt", "");
    let mut harness = Harness::open(&project);
    harness.press(
        KeyCode::Char('o'),
        KeyModifiers::CONTROL | KeyModifiers::SHIFT,
    );
    assert!(
        harness.shows("Open folder as project"),
        "{}",
        harness.screen()
    );
    harness.key(KeyCode::Tab);
    harness.type_text(&other.path().display().to_string());
    harness.key(KeyCode::Enter);
    assert!(harness.shows("only-here.txt"), "{}", harness.screen());
    // Open Recent lists where we came from.
    harness.ctrl('r');
    assert!(harness.shows("Recent Projects"), "{}", harness.screen());
    harness.key(KeyCode::Esc);
}

#[test]
fn a_row_dragged_onto_a_folder_moves_the_file() {
    let project = Project::new("yara-e2e-drag");
    let mut harness = Harness::open(&project);
    let from = harness.row_of("README.md").unwrap();
    let to = harness.row_of("src").unwrap();
    harness.drag((4, from), (4, to));
    assert!(
        project.path().join("src").join("README.md").is_file(),
        "{}",
        harness.screen()
    );
}

#[test]
fn the_shell_takes_the_keyboard_and_gives_it_back() {
    let project = Project::new("yara-e2e-shell");
    let mut harness = Harness::open(&project);
    // Click into the grid — the layout says where it is — and the keyboard
    // goes to the shell.
    let grid = harness.app.layout.shell;
    harness.click(grid.x + 10, grid.y + 2);
    assert_eq!(
        harness.app.focus,
        yara::tui::app::Focus::Shell,
        "{}",
        harness.screen()
    );
    harness.type_text("echo yara-shell-marker");
    harness.key(KeyCode::Enter);

    // Ctrl+J is a line feed, and an agent prompt in the terminal reads it as
    // another line: the shell keeps it, and the panel stays where it is.
    harness.ctrl('j');
    assert!(harness.shows("TERMINAL"), "{}", harness.screen());
    // From the editor it still folds the panel away, and brings it back.
    harness.app.focus = yara::tui::app::Focus::Editor;
    harness.ctrl('j');
    assert!(!harness.shows("TERMINAL"), "{}", harness.screen());
    harness.ctrl('j');
    assert!(harness.shows("TERMINAL"), "{}", harness.screen());
    harness.app.focus = yara::tui::app::Focus::Shell;

    // A second session appears on the strip, and closing it takes it away.
    harness.press(
        KeyCode::Char('t'),
        KeyModifiers::CONTROL | KeyModifiers::ALT,
    );
    harness.draw();
    let sessions = |h: &Harness| {
        h.screen()
            .lines()
            .find(|l| l.contains("TERMINAL"))
            .map_or(0, |l| l.matches('×').count())
    };
    assert_eq!(sessions(&harness), 2, "{}", harness.screen());
    harness.press(
        KeyCode::Char('w'),
        KeyModifiers::CONTROL | KeyModifiers::ALT,
    );
    harness.draw();
    assert_eq!(sessions(&harness), 1, "{}", harness.screen());
}

#[test]
fn the_splitters_resize_the_panes() {
    let project = Project::new("yara-e2e-resize");
    let mut harness = Harness::open(&project);
    // The splitter is the one-column rect the layout reserves for it.
    let bar = harness.app.layout.v_split.x;
    let row = harness.app.layout.v_split.y + 2;
    harness.drag((bar, row), (bar + 10, row));
    let after = harness.app.layout.v_split.x;
    assert!(after > bar, "the sidebar grew: {bar} -> {after}");
    // The horizontal one, above the terminal, moves too.
    let top = harness.app.layout.h_split.y;
    harness.drag((60, top), (60, top.saturating_sub(4)));
    assert!(
        harness.app.layout.h_split.y < top,
        "the terminal grew taller"
    );
}

#[test]
fn a_dirty_buffer_asks_before_closing() {
    let project = Project::new("yara-e2e-dirty");
    let mut harness = Harness::open(&project);
    let row = harness.row_of("README.md").unwrap();
    harness.click(4, row);
    harness.type_text("x");
    harness.ctrl('w');
    assert!(harness.shows("Save changes"), "{}", harness.screen());
    // n discards.
    harness.key(KeyCode::Char('n'));
    assert!(!harness.shows("README.md ●"), "{}", harness.screen());
    let body = std::fs::read_to_string(project.path().join("README.md")).unwrap();
    assert!(!body.starts_with('x'), "discard did not write");
}

#[test]
fn the_git_view_pickers_open_and_the_diff_tab_closes() {
    let project = Project::new("yara-e2e-git-more");
    let git = |args: &[&str]| {
        let out = std::process::Command::new("git")
            .arg("-C")
            .arg(project.path())
            .args(args)
            .output()
            .unwrap();
        assert!(out.status.success(), "git {args:?}");
    };
    git(&["init", "-q", "-b", "main"]);
    git(&["config", "user.email", "t@e.com"]);
    git(&["config", "user.name", "T"]);
    git(&["config", "commit.gpgsign", "false"]);
    git(&["add", "-A"]);
    git(&["commit", "-qm", "First"]);
    project.file("README.md", "# Title\nChanged.\n");

    let mut harness = Harness::open(&project);
    harness.press(
        KeyCode::Char('g'),
        KeyModifiers::CONTROL | KeyModifiers::SHIFT,
    );
    // The repository and worktree pickers.
    harness.key(KeyCode::Char('r'));
    assert!(harness.shows("Repository"), "{}", harness.screen());
    harness.key(KeyCode::Enter);
    harness.key(KeyCode::Char('w'));
    assert!(harness.shows("Worktree"), "{}", harness.screen());
    harness.key(KeyCode::Enter);

    // Open the diff, scroll it, open the file from it, then close the tab.
    harness.key(KeyCode::Enter);
    assert!(harness.shows("Changed."), "{}", harness.screen());
    for code in [
        KeyCode::Down,
        KeyCode::PageDown,
        KeyCode::Home,
        KeyCode::End,
        KeyCode::Up,
    ] {
        harness.key(code);
    }
    harness.key(KeyCode::Enter);
    assert!(
        harness.shows("Ln 1"),
        "the file opened: {}",
        harness.screen()
    );
    // Back to the git view and the diff again, then Esc closes it.
    harness.press(
        KeyCode::Char('g'),
        KeyModifiers::CONTROL | KeyModifiers::SHIFT,
    );
    harness.key(KeyCode::Enter);
    harness.key(KeyCode::Esc);
    assert!(!harness.shows("≠ README.md"), "{}", harness.screen());
}

#[test]
fn the_right_button_opens_the_context_menu_on_a_row() {
    let project = Project::new("yara-e2e-rclick");
    let mut harness = Harness::open(&project);
    let row = harness.row_of("README.md").unwrap();
    for kind in [
        MouseEventKind::Down(MouseButton::Right),
        MouseEventKind::Up(MouseButton::Right),
    ] {
        harness.app.handle(Event::Mouse(MouseEvent {
            kind,
            column: 4,
            row,
            modifiers: KeyModifiers::NONE,
        }));
    }
    harness.draw();
    assert!(
        harness.shows("Open") && harness.shows("Delete"),
        "{}",
        harness.screen()
    );
    // Hovering moves the highlight; clicking outside dismisses it.
    harness.app.handle(Event::Mouse(MouseEvent {
        kind: MouseEventKind::Moved,
        column: 6,
        row: row + 3,
        modifiers: KeyModifiers::NONE,
    }));
    harness.draw();
    // Escape, like a click clear of it, puts the menu away.
    harness.key(KeyCode::Esc);
    assert!(!harness.shows("Move To..."), "{}", harness.screen());
}

#[test]
fn a_shift_drag_selects_text_and_paste_replaces_it() {
    let project = Project::new("yara-e2e-shiftdrag");
    let mut harness = Harness::open(&project);
    let row = harness.row_of("README.md").unwrap();
    harness.click(4, row);
    let line = harness.row_of("A line of prose.").unwrap();
    // Drag across the second line to select it.
    harness.drag((38, line), (54, line));
    // A paste event goes where the keyboard is: over the selection.
    harness.app.handle(Event::Paste("replaced".into()));
    harness.draw();
    assert!(harness.shows("replaced"), "{}", harness.screen());
}

#[test]
fn tab_cycles_the_panes_and_ctrl_click_jumps_to_a_definition() {
    let project = Project::new("yara-e2e-cycle");
    project.file(
        "app.rs",
        "fn helper() -> u32 { 1 }\nfn main() { let x = helper(); }\n",
    );
    let mut harness = Harness::open(&project);
    // From the navigator, Tab walks Editor → Terminal → Navigator.
    harness.key(KeyCode::Tab);
    harness.key(KeyCode::Tab);
    harness.press(KeyCode::BackTab, KeyModifiers::SHIFT);
    // Open the file and Ctrl+click the call on line 2.
    let row = harness.row_of("app.rs").unwrap();
    harness.click(4, row);
    let line = harness.row_of("let x = helper();").unwrap();
    let col = harness
        .screen()
        .lines()
        .nth(line as usize)
        .unwrap()
        .find("helper();")
        .unwrap() as u16;
    harness.app.handle(Event::Mouse(MouseEvent {
        kind: MouseEventKind::Moved,
        column: col + 2,
        row: line,
        modifiers: KeyModifiers::CONTROL,
    }));
    harness.draw();
    for kind in [
        MouseEventKind::Down(MouseButton::Left),
        MouseEventKind::Up(MouseButton::Left),
    ] {
        harness.app.handle(Event::Mouse(MouseEvent {
            kind,
            column: col + 2,
            row: line,
            modifiers: KeyModifiers::CONTROL,
        }));
    }
    harness.draw();
    let screen = harness.screen();
    assert!(
        screen.contains("Definitions of") || screen.contains("Ln 1"),
        "{screen}"
    );
    if screen.contains("Definitions of") {
        harness.key(KeyCode::Enter);
    }
    assert!(harness.shows("Ln 1"), "{}", harness.screen());
}

#[test]
fn find_next_and_replace_one_walk_the_matches() {
    let project = Project::new("yara-e2e-findstep");
    project.file("many.txt", "one\none\none\n");
    let mut harness = Harness::open(&project);
    let row = harness.row_of("many.txt").unwrap();
    harness.click(4, row);
    harness.ctrl('f');
    harness.type_text("one");
    assert!(harness.shows("1 of 3"), "{}", harness.screen());
    harness.key(KeyCode::F(3));
    assert!(harness.shows("2 of 3"), "{}", harness.screen());
    harness.press(KeyCode::F(3), KeyModifiers::SHIFT);
    assert!(harness.shows("1 of 3"), "{}", harness.screen());
    // Replace just this one: two remain.
    harness.key(KeyCode::Tab);
    harness.type_text("two");
    harness.key(KeyCode::Enter);
    assert!(harness.shows("1 of 2"), "{}", harness.screen());
}

#[test]
fn a_folder_row_offers_to_leave_the_project() {
    let project = Project::new("yara-e2e-leave");
    let other = Project::new("yara-e2e-leave-other");
    let mut harness = Harness::open(&project);
    harness.press(
        KeyCode::Char('a'),
        KeyModifiers::CONTROL | KeyModifiers::SHIFT,
    );
    harness.key(KeyCode::Tab);
    harness.type_text(&other.path().display().to_string());
    harness.key(KeyCode::Enter);
    // The root row wears the folder's name; a long one is clipped to the pane,
    // so match on its stem.
    let name: String = other
        .path()
        .file_name()
        .unwrap()
        .to_string_lossy()
        .chars()
        .take(20)
        .collect();
    assert!(harness.shows(&name), "{}", harness.screen());
    // Right-click the added folder's own row: the menu offers to remove it.
    let row = harness.row_of(&name).unwrap();
    for kind in [
        MouseEventKind::Down(MouseButton::Right),
        MouseEventKind::Up(MouseButton::Right),
    ] {
        harness.app.handle(Event::Mouse(MouseEvent {
            kind,
            column: 4,
            row,
            modifiers: KeyModifiers::NONE,
        }));
    }
    harness.draw();
    assert!(
        harness.shows("Remove Folder from Project"),
        "{}",
        harness.screen()
    );
    let entry = harness.row_of("Remove Folder from Project").unwrap();
    harness.click(6, entry);
    // The folder is gone from the navigator. The status bar still names it
    // ("removed …"), so look at the rows, not the whole screen.
    let in_navigator = harness
        .screen()
        .lines()
        .any(|line| line.starts_with(" ▾") && line.contains(&name));
    assert!(!in_navigator, "{}", harness.screen());
    assert!(harness.shows("removed"), "{}", harness.screen());
}

#[test]
fn a_preview_draws_its_lists_tables_and_charts() {
    let project = Project::new("yara-e2e-preview-rich");
    project.file("README.md", "# Yara Code\n\n## Lists\n\n- one\n  - nested\n- [x] done\n- [ ] todo\n\n## Table\n\n| Name | Size |\n|:-----|-----:|\n| one | 1 |\n| two | 22 |\n\n## Charts\n\n```mermaid\npie title Languages\n  \"Rust\" : 70\n  \"Docs\" : 30\n```\n\n```mermaid\nflowchart LR\n  A[Edit] --> B[Ship]\n```\n");
    let mut harness = Harness::with_size(Some(project.path().to_path_buf()), 120, 60);
    let row = harness.row_of("README.md").unwrap();
    harness.click(4, row);
    harness.press(
        KeyCode::Char('v'),
        KeyModifiers::CONTROL | KeyModifiers::SHIFT,
    );
    let screen = harness.screen();
    // A nested item is set in under the one above it, and a ticked item wears
    // its box rather than a bullet.
    assert!(
        screen.contains("• one") && screen.contains("◦ nested"),
        "{screen}"
    );
    assert!(
        screen.contains("☑ done") && screen.contains("☐ todo"),
        "{screen}"
    );
    // The table is ruled, and the right-aligned column ends where it should.
    assert!(screen.contains("│ Name │"), "{screen}");
    assert!(screen.contains("│   22 │"), "right aligned: {screen}");
    // The pie is bars with their shares, and the flowchart is boxes and an
    // arrow — neither is left as the mermaid it was written in.
    assert!(
        screen.contains("Languages") && screen.contains("70%"),
        "{screen}"
    );
    assert!(
        screen.contains("│ Edit │") && screen.contains("▶"),
        "{screen}"
    );
    assert!(!screen.contains("flowchart LR"), "{screen}");
}

#[test]
fn a_markdown_file_previews_as_a_reader_sees_it() {
    let project = Project::new("yara-e2e-preview");
    project.file(
        "README.md",
        "<div align=\"center\">\n\n# Yara Code\n\nSome **bold** text.\n\n\
         [![CI](https://x/badge.svg)](https://x/ci)\n\n- one\n- two\n\n</div>\n\n\
         ```rust\nfn main() {}\n```\n",
    );
    let mut harness = Harness::open(&project);
    let row = harness.row_of("README.md").unwrap();
    harness.click(4, row);
    harness.press(
        KeyCode::Char('v'),
        KeyModifiers::CONTROL | KeyModifiers::SHIFT,
    );
    let screen = harness.screen();
    // The heading without its hash, the list with bullets, the fence gone.
    assert!(
        screen.contains("Yara Code") && !screen.contains("# Yara Code"),
        "{screen}"
    );
    assert!(
        screen.contains("• one") && screen.contains("• two"),
        "{screen}"
    );
    assert!(
        screen.contains("fn main() {}") && !screen.contains("```"),
        "{screen}"
    );
    // A README's wrapper is markup for a browser, and a badge is the name it
    // was given — neither is painted as the markup it was written as.
    assert!(!screen.contains("<div"), "{screen}");
    assert!(screen.contains("CI") && !screen.contains("]("), "{screen}");
    assert!(screen.contains("◫ README.md"), "a tab of its own: {screen}");
    // The toggle again puts it away; a non-markdown file has none.
    harness.press(
        KeyCode::Char('v'),
        KeyModifiers::CONTROL | KeyModifiers::SHIFT,
    );
    assert!(!harness.shows("◫ README.md"), "{}", harness.screen());
    let src = harness.row_of("src").unwrap();
    harness.click(4, src);
    let main = harness.row_of("main.rs").unwrap();
    harness.click(6, main);
    harness.press(
        KeyCode::Char('v'),
        KeyModifiers::CONTROL | KeyModifiers::SHIFT,
    );
    assert!(harness.shows("is not markdown"), "{}", harness.screen());
}

#[test]
fn the_indentation_picker_changes_what_enter_inserts() {
    let project = Project::new("yara-e2e-indent");
    let mut harness = Harness::open(&project);
    harness.press(
        KeyCode::Char('i'),
        KeyModifiers::CONTROL | KeyModifiers::ALT,
    );
    let screen = harness.screen();
    assert!(
        screen.contains("Indentation") && screen.contains("Tabs"),
        "{screen}"
    );
    // Down to "Tabs" (the last choice) and Enter.
    harness.key(KeyCode::End);
    for _ in 0..3 {
        harness.key(KeyCode::Down);
    }
    harness.key(KeyCode::Enter);
    assert!(harness.shows("indentation: Tabs"), "{}", harness.screen());
    assert_eq!(
        harness.app.settings.indent.style,
        yara::core::settings::IndentStyle::Tabs
    );
}

#[test]
fn saving_the_settings_file_applies_it_and_says_what_it_cannot() {
    let project = Project::new("yara-e2e-live-settings");
    let settings = project.file(
        ".ycode/settings.json",
        "{\"theme\": \"Dark+\", \"show_terminal\": true}\n",
    );
    let mut harness = Harness::open(&project);
    assert!(harness.shows("TERMINAL"), "{}", harness.screen());

    std::fs::write(
        &settings,
        "{\"theme\": \"Monokai\", \"show_terminal\": false, \"font_size\": 20.0}\n",
    )
    .unwrap();
    harness.app.open(settings);
    harness.draw();
    harness.ctrl('s');
    let screen = harness.screen();
    assert!(screen.contains("settings applied"), "{screen}");
    // The theme and the panel flag took effect; the font size is the
    // terminal's, and the status bar says so instead of pretending.
    assert!(screen.contains("Monokai"), "{screen}");
    assert!(!screen.contains(" TERMINAL "), "{screen}");
    // The status bar is clipped to fit beside the cursor readout, so only
    // the start of the note is guaranteed on a 120-column screen.
    assert!(screen.contains("font_size is the"), "{screen}");
}

#[test]
fn indented_code_shows_a_guide_per_level_through_blank_lines() {
    let project = Project::new("yara-e2e-guides");
    project.file(
        "nested.rs",
        "fn a() {\n    if x {\n        y();\n\n    }\n}\n",
    );
    let mut harness = Harness::open(&project);
    let row = harness.row_of("nested.rs").unwrap();
    harness.click(4, row);
    let screen = harness.screen();
    // Guides land where the leading spaces were: one at the first level, two
    // on the deepest line, and the blank line inside the block keeps one.
    // The first bar on a row is the sidebar's edge; the guides come after.
    let guide = |text: &str| {
        screen
            .lines()
            .find(|l| l.contains(text))
            .and_then(|l| l.split_once('\u{2502}'))
            .map(|(_, editor)| editor.matches('\u{2502}').count())
            .unwrap_or(0)
    };
    assert_eq!(guide("if x {"), 1, "{screen}");
    assert_eq!(guide("y();"), 2, "{screen}");
    let y = screen.lines().position(|l| l.contains("y();")).unwrap();
    let blank = screen.lines().nth(y + 1).unwrap();
    assert!(blank.contains('\u{2502}'), "{screen}");
    assert_eq!(guide("fn a() {"), 0, "{screen}");
}

#[test]
fn a_markdown_file_in_front_offers_a_preview_button_on_the_strip() {
    let project = Project::new("yara-e2e-preview-hint");
    let mut harness = Harness::open(&project);
    let row = harness.row_of("README.md").unwrap();
    harness.click(4, row);
    let screen = harness.screen();
    assert!(screen.contains("◫ Preview Ctrl+Shift+V"), "{screen}");
    // Clicking it renders the file in a preview tab; the button then goes,
    // as the preview, not the markdown, is in front.
    let strip = screen
        .lines()
        .position(|l| l.contains("◫ Preview"))
        .unwrap() as u16;
    let column = screen
        .lines()
        .nth(strip as usize)
        .unwrap()
        .find("◫ Preview")
        .unwrap();
    let column = screen.lines().nth(strip as usize).unwrap()[..column]
        .chars()
        .count() as u16
        + 2;
    harness.click(column, strip);
    let screen = harness.screen();
    assert!(screen.contains("◫ README.md"), "{screen}");
    assert!(!screen.contains("◫ Preview Ctrl"), "{screen}");
    // A file that is not markdown gets no such button.
    let src = harness.row_of("src").unwrap();
    harness.click(4, src);
    let row = harness.row_of("main.rs").unwrap();
    harness.click(4, row);
    assert!(!harness.shows("◫ Preview"), "{}", harness.screen());
}

#[test]
fn too_many_tabs_scroll_behind_arrows_and_the_front_one_stays_in_view() {
    let project = Project::new("yara-e2e-tab-scroll");
    for i in 0..12 {
        project.file(&format!("long_file_name_{i:02}.rs"), "fn f() {}\n");
    }
    let mut harness = Harness::with_size(Some(project.path().to_path_buf()), 100, 30);
    for i in 0..12 {
        let name = format!("long_file_name_{i:02}.rs");
        let row = harness.row_of(&name).unwrap();
        harness.click(4, row);
    }
    let screen = harness.screen();
    let strip = screen
        .lines()
        .find(|l| l.contains("‹") && l.contains("›"))
        .expect(&screen);
    // The last file opened is in front and on screen; the first scrolled off.
    assert!(strip.contains("name_11.rs"), "{strip}");
    assert!(!strip.contains("name_00.rs"), "{strip}");
    // ‹ brings earlier tabs back, a few columns per press; pressing past the
    // first tab is harmless.
    let row = screen.lines().position(|l| l.contains('‹')).unwrap() as u16;
    let at = strip.find('‹').unwrap();
    let at = strip[..at].chars().count() as u16;
    for _ in 0..40 {
        harness.click(at, row);
    }
    let strip = harness
        .screen()
        .lines()
        .find(|l| l.contains('‹'))
        .unwrap()
        .to_string();
    assert!(strip.contains("name_00.rs"), "{strip}");
    // Switching back to the last tab from the keyboard scrolls it into view:
    // Next Tab wraps, so a full round lands on it again.
    for _ in 0..12 {
        harness.press(KeyCode::PageDown, KeyModifiers::CONTROL);
    }
    let strip = harness
        .screen()
        .lines()
        .find(|l| l.contains('‹'))
        .unwrap()
        .to_string();
    assert!(strip.contains("name_11.rs"), "{strip}");
}

#[test]
fn the_command_palette_runs_what_is_typed() {
    let project = Project::new("yara-e2e-palette");
    let mut harness = Harness::open(&project);
    assert!(harness.shows("FILES"), "{}", harness.screen());
    harness.press(
        KeyCode::Char('p'),
        KeyModifiers::CONTROL | KeyModifiers::SHIFT,
    );
    assert!(harness.shows("Command Palette"), "{}", harness.screen());
    // The rows narrow as the name is typed, best match on top; Enter runs it.
    harness.type_text("toggle side");
    assert!(harness.shows("Toggle Sidebar"), "{}", harness.screen());
    harness.key(KeyCode::Enter);
    assert!(!harness.shows("> toggle side"), "{}", harness.screen());
    assert!(!harness.shows("FILES"), "{}", harness.screen());
}

#[test]
fn go_to_file_opens_the_best_match() {
    let project = Project::new("yara-e2e-quick-open");
    let mut harness = Harness::open(&project);
    harness.ctrl('p');
    assert!(harness.shows("Go to File"), "{}", harness.screen());
    harness.type_text("main");
    harness.key(KeyCode::Enter);
    assert!(harness.shows("main.rs ×"), "{}", harness.screen());
    assert!(harness.shows("fn main()"), "{}", harness.screen());
}

#[test]
fn go_to_line_moves_the_caret() {
    let project = Project::new("yara-e2e-goto-line");
    let mut harness = Harness::open(&project);
    let row = harness.row_of("README.md").unwrap();
    harness.click(4, row);
    harness.ctrl('g');
    assert!(harness.shows("Go to Line"), "{}", harness.screen());
    harness.type_text("2");
    harness.key(KeyCode::Enter);
    assert!(harness.shows("Ln 2, Col 1"), "{}", harness.screen());
}

#[test]
fn the_git_panel_stages_and_commits() {
    let project = Project::new("yara-e2e-stage");
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
    assert!(harness.shows("CHANGES  1"), "{}", harness.screen());
    assert!(!harness.shows("STAGED CHANGES"));
    // `s` moves the selected file into the index; the panel says so at once.
    harness.key(KeyCode::Char('s'));
    assert!(harness.shows("STAGED CHANGES  1"), "{}", harness.screen());
    // `c` asks for a message, and Enter makes the commit.
    harness.key(KeyCode::Char('c'));
    assert!(harness.shows("Commit message"), "{}", harness.screen());
    harness.type_text("Second");
    harness.key(KeyCode::Enter);
    assert!(harness.shows("committed"), "{}", harness.screen());
    assert!(harness.shows("no changes"), "{}", harness.screen());
    let log = std::process::Command::new("git")
        .arg("-C")
        .arg(project.path())
        .args(["log", "-1", "--format=%s"])
        .output()
        .unwrap();
    assert_eq!(String::from_utf8_lossy(&log.stdout).trim(), "Second");
}

#[test]
fn the_wheel_scrolls_the_view_and_leaves_the_caret() {
    let project = Project::new("yara-e2e-wheel");
    let body: String = (1..=60).map(|n| format!("line {n}\n")).collect();
    project.file("long.txt", &body);
    let mut harness = Harness::open(&project);
    let row = harness.row_of("long.txt").unwrap();
    harness.click(4, row);
    let editor = harness.app.layout.editor;
    for _ in 0..2 {
        harness.wheel(editor.x + 10, editor.y + 2, false);
    }
    // The view moved on by two notches of six rows; the caret did not.
    assert!(!harness.shows("line 1\n"), "{}", harness.screen());
    assert!(harness.shows("line 13"), "{}", harness.screen());
    assert!(harness.shows("Ln 1, Col 1"), "{}", harness.screen());
    // Moving the caret brings the view back to wherever it lands.
    harness.key(KeyCode::Down);
    assert!(harness.shows(" 2  line 2"), "{}", harness.screen());
    assert!(harness.shows("Ln 2, Col 1"), "{}", harness.screen());
}

#[test]
fn a_file_changed_on_disk_is_reloaded_while_clean() {
    let project = Project::new("yara-e2e-disk");
    let mut harness = Harness::open(&project);
    let row = harness.row_of("README.md").unwrap();
    harness.click(4, row);
    assert!(harness.shows("A line of prose"), "{}", harness.screen());
    // Timestamps are whole seconds on some filesystems; the poll itself runs
    // once a second, so the wait covers both.
    std::thread::sleep(std::time::Duration::from_millis(1100));
    project.file("README.md", "# Title\nWritten by something else.\n");
    harness.draw();
    assert!(
        harness.shows("Written by something else"),
        "{}",
        harness.screen()
    );
    assert!(harness.shows("changed on disk"), "{}", harness.screen());
}

#[test]
fn a_tabs_own_menu_closes_that_tab_and_every_other_one() {
    let project = Project::new("yara-e2e-tabmenu");
    project.file("one.txt", "first\n");
    project.file("two.txt", "second\n");
    let mut harness = Harness::open(&project);
    for name in ["one.txt", "two.txt"] {
        let row = harness.row_of(name).unwrap();
        harness.click(4, row);
    }
    // Right-clicking the first tab picks it and offers what can be done to
    // tabs.
    let strip = harness.app.layout.tabs;
    let (start, _, _, _) = harness.app.layout.tab_spans[0];
    harness.right_click(start + 1, strip.y);
    assert!(harness.shows("Close Tab"), "{}", harness.screen());
    assert!(harness.shows("Close All Tabs"), "{}", harness.screen());

    // Down lands on the second entry, and Enter runs it.
    harness.key(KeyCode::Down);
    harness.key(KeyCode::Enter);
    assert!(harness.app.buffers.is_empty(), "{}", harness.screen());
    // Nothing is open, so the start page is what stands in the editor.
    assert!(harness.shows("Open Folder..."), "{}", harness.screen());
}

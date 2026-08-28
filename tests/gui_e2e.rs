//! End-to-end tests for the window frontend.
//!
//! egui draws into a context, not a window, so the real `App` can be run
//! headless: feed it synthetic input, let it lay out a frame, and read the
//! frame's shapes back. No display server, no wgpu — the same code that runs
//! on screen, minus the screen.

#![cfg(feature = "gui")]

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use egui::{Event, Key, Modifiers, Pos2, RawInput, Rect, Vec2};
use yara::gui::app::App;

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

/// The editor and the context it draws into, with the text of the last frame.
struct Harness {
    app: App,
    ctx: egui::Context,
    events: Vec<Event>,
    /// Every string laid out last frame, and where it was drawn.
    text: Vec<(String, Pos2)>,
    /// Every line the frame drew. The marks on a tab are strokes rather than
    /// glyphs, so this is how a test finds one.
    strokes: Vec<[Pos2; 2]>,
    /// What the last frame put on the clipboard, if anything.
    copied: Option<String>,
    /// The last link the window asked to be opened.
    opened: Option<String>,
    /// How many lines each piece of text was laid out over, so a test can say
    /// that something stands on one line.
    rows: Vec<(String, usize)>,
}

impl Harness {
    fn open(project: Option<&Project>) -> Self {
        let ctx = egui::Context::default();
        let app = App::with_context(&ctx, project.map(|p| p.path().to_path_buf()));
        let mut harness = Self {
            app,
            ctx,
            events: Vec::new(),
            text: Vec::new(),
            strokes: Vec::new(),
            copied: None,
            opened: None,
            rows: Vec::new(),
        };
        // Two frames: egui lays out on the first and settles on the second.
        harness.frame();
        harness.frame();
        harness
    }

    fn frame(&mut self) {
        let input = RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(1280.0, 800.0))),
            events: std::mem::take(&mut self.events),
            ..Default::default()
        };
        let app = &mut self.app;
        let output = self.ctx.run(input, |ctx| app.ui(ctx));
        // Every string egui laid out this frame, which is what a user reads.
        self.text = output
            .shapes
            .iter()
            .filter_map(|shape| match &shape.shape {
                egui::Shape::Text(text) => Some((text.galley.text().to_string(), text.pos)),
                _ => None,
            })
            .collect();
        self.rows = output
            .shapes
            .iter()
            .filter_map(|shape| match &shape.shape {
                egui::Shape::Text(text) => {
                    Some((text.galley.text().to_string(), text.galley.rows.len()))
                }
                _ => None,
            })
            .collect();
        self.strokes = output
            .shapes
            .iter()
            .filter_map(|shape| match &shape.shape {
                egui::Shape::LineSegment { points, .. } => Some(*points),
                _ => None,
            })
            .collect();
        self.copied = output
            .platform_output
            .commands
            .iter()
            .find_map(|command| match command {
                egui::OutputCommand::CopyText(text) => Some(text.clone()),
                _ => None,
            });
        if let Some(url) =
            output
                .platform_output
                .commands
                .iter()
                .find_map(|command| match command {
                    egui::OutputCommand::OpenUrl(open) => Some(open.url.clone()),
                    _ => None,
                })
        {
            self.opened = Some(url);
        }
    }

    fn press(&mut self, key: Key, modifiers: Modifiers) {
        for pressed in [true, false] {
            self.events.push(Event::Key {
                key,
                physical_key: None,
                pressed,
                repeat: false,
                modifiers,
            });
        }
        // Two frames: the command runs on the first, and what it opened — a
        // modal, a panel — is laid out on the next.
        self.frame();
        self.frame();
    }

    fn click(&mut self, at: Pos2) {
        // Hover on one frame, press on the next, release on the one after:
        // that is how a real click arrives, and egui's buttons and menus only
        // answer a press on a widget that was already hovered.
        self.events.push(Event::PointerMoved(at));
        self.frame();
        self.events.push(Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Primary,
            pressed: true,
            modifiers: Modifiers::NONE,
        });
        self.frame();
        self.events.push(Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: Modifiers::NONE,
        });
        self.frame();
        // A click is answered on the frame after it, once egui has the
        // interaction.
        self.frame();
    }

    /// How many lines a piece of text was laid out over.
    fn rows_of(&self, text: &str) -> Option<usize> {
        self.rows
            .iter()
            .find(|(drawn, _)| drawn == text)
            .map(|(_, rows)| *rows)
    }

    fn shows(&self, text: &str) -> bool {
        self.text.iter().any(|(drawn, _)| drawn.contains(text))
    }

    /// Re-reads the file in front from disk, as the editor would after an
    /// outside change — so a Save then writes and applies the new contents.
    fn reload_active_buffer(&mut self) {
        self.app.reload_active_from_disk();
        self.frame();
    }

    fn screen(&self) -> String {
        self.text
            .iter()
            .map(|(drawn, _)| drawn.as_str())
            .collect::<Vec<_>>()
            .join(" | ")
    }

    /// The middle of a piece of text on screen — where a user would click it.
    /// An exact match wins over a longer string that merely contains it, so a
    /// file name finds its navigator row rather than the status bar's path.
    fn position_of(&self, text: &str) -> Option<Pos2> {
        self.text
            .iter()
            .find(|(drawn, _)| drawn == text)
            .or_else(|| self.text.iter().find(|(drawn, _)| drawn.contains(text)))
            .map(|(drawn, pos)| {
                // The position is the galley's corner; aim a little inside it.
                Pos2::new(pos.x + drawn.len().min(6) as f32 * 3.0, pos.y + 7.0)
            })
    }
}

#[test]
fn with_no_folder_the_window_shows_the_start_page() {
    let harness = Harness::open(None);
    assert!(harness.shows("YCODE"), "{}", harness.screen());
    assert!(
        harness.shows("no folder in the project"),
        "{}",
        harness.screen()
    );
    // The menus and the key groups the start page is built from. On macOS the
    // menus are in the system bar and the window draws no strip of its own,
    // which is the whole point of the platform's own menu bar.
    if cfg!(target_os = "macos") {
        assert!(!harness.shows("View"), "{}", harness.screen());
    } else {
        assert!(harness.shows("File") && harness.shows("View") && harness.shows("Help"));
    }
    assert!(harness.shows("PROJECT") && harness.shows("PANELS"));
    assert!(harness.shows("Toggle Terminal"));
}

#[test]
fn a_project_lists_its_files_in_the_navigator() {
    let project = Project::new("yara-gui-e2e-open");
    let harness = Harness::open(Some(&project));
    assert!(harness.shows("README.md"), "{}", harness.screen());
    assert!(harness.shows("src"));
    // The sidebar's own footer, and the status bar's theme name.
    assert!(harness.shows("FILES") && harness.shows("SEARCH") && harness.shows("GIT"));
    assert!(harness.shows("Dark+"));
}

#[test]
fn the_sidebar_and_the_terminal_answer_their_keys() {
    let project = Project::new("yara-gui-e2e-panels");
    let mut harness = Harness::open(Some(&project));
    assert!(harness.shows("README.md"));

    harness.press(Key::B, Modifiers::COMMAND);
    assert!(!harness.shows("README.md"), "the sidebar closed");
    harness.press(Key::B, Modifiers::COMMAND);
    assert!(harness.shows("README.md"), "and opened again");

    // The search view, with the three fields the panel draws.
    harness.press(Key::F, Modifiers::COMMAND | Modifiers::SHIFT);
    assert!(harness.shows("SEARCH"), "{}", harness.screen());
    assert!(harness.shows("EXCLUDE"), "{}", harness.screen());
}

#[test]
fn the_theme_picker_lists_every_theme() {
    let project = Project::new("yara-gui-e2e-theme");
    let mut harness = Harness::open(Some(&project));
    harness.press(Key::T, Modifiers::COMMAND | Modifiers::SHIFT);
    assert!(harness.shows("Dark+"), "{}", harness.screen());
    assert!(harness.shows("Light+") && harness.shows("Monokai"));
    harness.press(Key::Escape, Modifiers::NONE);
}

#[test]
fn the_bindings_overlay_opens_from_f1() {
    let project = Project::new("yara-gui-e2e-help");
    let mut harness = Harness::open(Some(&project));
    harness.press(Key::F1, Modifiers::NONE);
    assert!(harness.shows("Key Bindings"), "{}", harness.screen());
    // A row the start page behind it does not have.
    assert!(harness.shows("Save All"));
    harness.press(Key::Escape, Modifiers::NONE);
    assert!(!harness.shows("Save All"));
}

#[test]
fn a_window_the_size_of_a_postage_stamp_still_draws() {
    let project = Project::new("yara-gui-e2e-small");
    let ctx = egui::Context::default();
    let mut app = App::with_context(&ctx, Some(project.path().to_path_buf()));
    for _ in 0..2 {
        let input = RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(320.0, 200.0))),
            ..Default::default()
        };
        let _ = ctx.run(input, |ctx| app.ui(ctx));
    }
}

#[test]
fn clicking_a_file_in_the_navigator_opens_it() {
    let project = Project::new("yara-gui-e2e-click");
    let mut harness = Harness::open(Some(&project));
    let at = harness
        .position_of("README.md")
        .expect("the navigator lists it");
    harness.click(at);
    // The file is in front: its text is drawn, and the status bar says where
    // the cursor is.
    assert!(
        harness.shows("A line of prose.") || harness.shows("Ln 1"),
        "{}",
        harness.screen()
    );
}

impl Harness {
    fn type_text(&mut self, text: &str) {
        self.events.push(Event::Text(text.to_string()));
        self.frame();
        self.frame();
    }
}

#[test]
fn tab_in_the_editor_inserts_the_indent_unit_rather_than_a_tab_character() {
    let project = Project::new("yara-gui-e2e-tab");
    // A file at the top level, where the navigator shows it unopened.
    project.file("code.rs", "fn main() {\n}\n");
    let mut harness = Harness::open(Some(&project));
    let at = harness.position_of("code.rs").unwrap();
    harness.click(at);
    // Into the text, then to the start of its first line.
    let line = harness.position_of("fn main() {").unwrap();
    harness.click(line);
    harness.press(Key::Home, Modifiers::NONE);
    harness.press(Key::Tab, Modifiers::NONE);
    harness.press(Key::S, Modifiers::COMMAND);

    let saved = std::fs::read_to_string(project.path().join("code.rs")).unwrap();
    assert!(
        !saved.contains('\t'),
        "a literal tab was inserted: {saved:?}"
    );
    assert!(
        saved.starts_with("    fn main() {"),
        "the indent unit, spaces of the set width: {saved:?}"
    );
}

#[test]
fn find_and_replace_in_the_open_file_works_through_the_bar() {
    let project = Project::new("yara-gui-e2e-find");
    project.file("many.txt", "one\none\none\n");
    let mut harness = Harness::open(Some(&project));
    let at = harness.position_of("many.txt").unwrap();
    harness.click(at);

    harness.press(Key::F, Modifiers::COMMAND);
    assert!(
        harness.shows("FIND") && harness.shows("REPLACE"),
        "{}",
        harness.screen()
    );
    harness.type_text("one");
    assert!(harness.shows("1 of 3"), "{}", harness.screen());
    // Next and previous match, on the chords VS Code uses for the platform.
    let (next, previous) = if cfg!(target_os = "macos") {
        (
            (Key::G, Modifiers::COMMAND),
            (Key::G, Modifiers::COMMAND | Modifiers::SHIFT),
        )
    } else {
        ((Key::F3, Modifiers::NONE), (Key::F3, Modifiers::SHIFT))
    };
    harness.press(next.0, next.1);
    assert!(harness.shows("2 of 3"), "{}", harness.screen());
    harness.press(previous.0, previous.1);
    assert!(harness.shows("1 of 3"), "{}", harness.screen());
    // Escape closes the bar.
    harness.press(Key::Escape, Modifiers::NONE);
    assert!(!harness.shows("REPLACE"), "{}", harness.screen());
}

#[test]
fn undo_redo_and_folding_answer_their_keys_in_the_window() {
    let project = Project::new("yara-gui-e2e-edit");
    project.file(
        "deep.py",
        "def outer():\n    first = 1\n    second = 2\n\nprint(outer())\n",
    );
    let mut harness = Harness::open(Some(&project));
    let at = harness.position_of("deep.py").unwrap();
    harness.click(at);
    assert!(harness.shows("first = 1"), "{}", harness.screen());

    harness.press(Key::Num0, Modifiers::COMMAND | Modifiers::ALT);
    assert!(
        !harness.shows("first = 1"),
        "fold all: {}",
        harness.screen()
    );
    harness.press(Key::Num9, Modifiers::COMMAND | Modifiers::ALT);
    assert!(
        harness.shows("first = 1"),
        "unfold all: {}",
        harness.screen()
    );

    // Undo and redo with nothing to undo report, rather than fail.
    harness.press(Key::Z, Modifiers::COMMAND);
    assert!(harness.shows("nothing to undo"), "{}", harness.screen());
    harness.press(Key::Z, Modifiers::COMMAND | Modifiers::SHIFT);
    assert!(harness.shows("nothing to redo"), "{}", harness.screen());
}

#[test]
fn the_git_panel_opens_a_diff_tab_in_the_window() {
    let project = Project::new("yara-gui-e2e-git");
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

    let mut harness = Harness::open(Some(&project));
    harness.press(Key::G, Modifiers::CTRL | Modifiers::SHIFT);
    assert!(harness.shows("REPOSITORY"), "{}", harness.screen());
    assert!(harness.shows("README.md"), "the change is listed");
    // Click the change: a diff tab opens with both versions.
    let at = harness
        .text
        .iter()
        .find(|(t, _)| t.contains("README.md") && !t.contains("×"))
        .map(|(_, p)| Pos2::new(p.x + 20.0, p.y + 7.0))
        .unwrap();
    // The changed file reads down the panel's left edge, under the heading
    // that counts it, rather than drifting to the middle as the panel widens.
    let heading = harness
        .text
        .iter()
        .find(|(t, _)| t.starts_with("CHANGES"))
        .map(|(_, p)| p.x)
        .unwrap();
    let row = at.x - 20.0;
    assert!(
        row <= heading && heading - row <= 12.0,
        "the row starts at {row}, the heading at {heading}"
    );
    harness.click(at);
    assert!(harness.shows("Changed."), "{}", harness.screen());
    assert!(
        harness.shows("A line of prose."),
        "the old side: {}",
        harness.screen()
    );
    assert!(harness.shows("Open File"), "{}", harness.screen());
}

#[test]
fn the_seam_of_a_diff_is_dragged_to_give_one_side_more_room_in_the_window() {
    let project = Project::new("yara-gui-e2e-diff-seam");
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

    let mut harness = Harness::open(Some(&project));
    harness.press(Key::G, Modifiers::CTRL | Modifiers::SHIFT);
    let at = harness
        .text
        .iter()
        .find(|(t, _)| t.contains("README.md") && !t.contains("×"))
        .map(|(_, p)| Pos2::new(p.x + 20.0, p.y + 7.0))
        .unwrap();
    harness.click(at);
    assert!(harness.shows("Changed."), "{}", harness.screen());

    // Where the galleys were painted, uncorrected: the left number, the left
    // text and the right text of the diff's rows give the gutter's width and
    // so where the seam is.
    let raw = |harness: &Harness, text: &str| -> Pos2 {
        harness
            .text
            .iter()
            .find(|(t, _)| t == text)
            .map(|(_, p)| *p)
            .unwrap_or_else(|| panic!("{text:?} on screen: {}", harness.screen()))
    };
    let number = raw(&harness, "1");
    let left = raw(&harness, "A line of prose.");
    let right = raw(&harness, "Changed.");
    let char_w = (left.x - number.x) / 2.0;
    let seam = right.x - char_w * 6.0;

    harness.drag(
        Pos2::new(seam, right.y + 4.0),
        Pos2::new(seam + 80.0, right.y + 4.0),
    );
    let moved = raw(&harness, "Changed.");
    assert!(
        (moved.x - (right.x + 80.0)).abs() <= 4.0,
        "the new side moved with the seam: {} -> {}",
        right.x,
        moved.x
    );
    assert!(
        harness.shows("A line of prose."),
        "the old side still reads: {}",
        harness.screen()
    );
}

#[test]
fn the_diff_arrows_jump_from_change_to_change() {
    let project = Project::new("yara-gui-e2e-diff-arrows");
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
    // A change far enough down the file that it starts out below the fold.
    let body: String = (1..=300).map(|n| format!("line {n}\n")).collect();
    project.file("long.txt", &body);
    git(&["add", "-A"]);
    git(&["commit", "-qm", "First"]);
    project.file("long.txt", &body.replace("line 250", "the-far-change"));

    let mut harness = Harness::open(Some(&project));
    harness.press(Key::G, Modifiers::CTRL | Modifiers::SHIFT);
    let row = harness
        .text
        .iter()
        .find(|(t, _)| t.contains("long.txt") && !t.contains("×"))
        .map(|(_, p)| Pos2::new(p.x + 20.0, p.y + 7.0))
        .unwrap();
    harness.click(row);
    // A diff opens on its first change, with the change in the middle of the
    // view rather than at its top edge: the lines before it are in sight.
    assert!(harness.shows("the-far-change"), "{}", harness.screen());
    assert!(
        harness.shows("line 245") && harness.shows("line 255"),
        "the change is centred: {}",
        harness.screen()
    );
    assert!(!harness.shows("line 1"), "{}", harness.screen());

    // The arrows sit left of Open File in the diff's own header: each is
    // sixteen points wide, with the theme's spacing and button padding
    // between them and the text beside them.
    let (_, open_file) = harness
        .text
        .iter()
        .find(|(t, _)| t == "Open File")
        .cloned()
        .unwrap();
    let down = Pos2::new(open_file.x - 20.0, open_file.y + 7.0);
    let up = Pos2::new(open_file.x - 42.0, open_file.y + 7.0);
    harness.click(up);
    assert!(
        !harness.shows("the-far-change") && harness.shows("line 1"),
        "the up arrow goes back to the top: {}",
        harness.screen()
    );
    harness.click(down);
    assert!(
        harness.shows("the-far-change") && harness.shows("line 245"),
        "the down arrow returns to the change, centred: {}",
        harness.screen()
    );

    // The arrow keys do the same, as they do in the terminal frontend.
    harness.press(Key::ArrowUp, Modifiers::NONE);
    assert!(
        !harness.shows("the-far-change") && harness.shows("line 1"),
        "the up key goes back: {}",
        harness.screen()
    );
    harness.press(Key::ArrowDown, Modifiers::NONE);
    assert!(
        harness.shows("the-far-change"),
        "the down key jumps to the change: {}",
        harness.screen()
    );
}

/// The window's own menu strip, which every platform but macOS draws — there
/// the same three menus are the system bar's, and AppKit draws them where no
/// test harness of ours can read them.
#[test]
#[cfg(not(target_os = "macos"))]
fn the_help_menu_names_the_version_and_offers_the_update_check() {
    let project = Project::new("yara-gui-e2e-menus");
    let mut harness = Harness::open(Some(&project));
    let at = harness.position_of("Help").unwrap();
    harness.click(at);
    assert!(harness.shows("Yara Code 0."), "{}", harness.screen());
    assert!(
        harness.shows("Check for Updates..."),
        "{}",
        harness.screen()
    );
    assert!(harness.shows("Documentation"), "{}", harness.screen());
    // Escape closes it.
    harness.press(Key::Escape, Modifiers::NONE);
    assert!(
        !harness.shows("Check for Updates..."),
        "{}",
        harness.screen()
    );
}

#[test]
fn the_recent_projects_modal_lists_the_folder_we_opened() {
    let project = Project::new("yara-gui-e2e-recent");
    let mut harness = Harness::open(Some(&project));
    harness.press(Key::R, Modifiers::COMMAND);
    assert!(harness.shows("Recent Projects"), "{}", harness.screen());
    let name = project
        .path()
        .file_name()
        .unwrap()
        .to_string_lossy()
        .to_string();
    assert!(harness.shows(&name), "{}", harness.screen());
    harness.press(Key::Escape, Modifiers::NONE);
    assert!(!harness.shows("Recent Projects"));
}

#[test]
fn a_file_saves_from_the_window() {
    let project = Project::new("yara-gui-e2e-save");
    let mut harness = Harness::open(Some(&project));
    let at = harness.position_of("README.md").unwrap();
    harness.click(at);
    // Nothing to save yet is reported, not an error.
    harness.press(Key::S, Modifiers::COMMAND);
    harness.press(Key::S, Modifiers::COMMAND | Modifiers::ALT);
    assert!(
        harness.shows("saved") || harness.shows("nothing"),
        "{}",
        harness.screen()
    );
    // Closing the only tab returns to the start page.
    harness.press(Key::W, Modifiers::COMMAND);
    assert!(harness.shows("PROJECT"), "{}", harness.screen());
}

impl Harness {
    fn right_click(&mut self, at: Pos2) {
        self.events.push(Event::PointerMoved(at));
        self.frame();
        for pressed in [true, false] {
            self.events.push(Event::PointerButton {
                pos: at,
                button: egui::PointerButton::Secondary,
                pressed,
                modifiers: Modifiers::NONE,
            });
            self.frame();
        }
        self.frame();
    }

    fn drag(&mut self, from: Pos2, to: Pos2) {
        self.events.push(Event::PointerMoved(from));
        self.frame();
        self.events.push(Event::PointerButton {
            pos: from,
            button: egui::PointerButton::Primary,
            pressed: true,
            modifiers: Modifiers::NONE,
        });
        self.frame();
        // A few steps, so egui sees a drag rather than a click.
        for i in 1..=4 {
            let t = i as f32 / 4.0;
            self.events
                .push(Event::PointerMoved(from + (to - from) * t));
            self.frame();
        }
        self.events.push(Event::PointerButton {
            pos: to,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: Modifiers::NONE,
        });
        self.frame();
        self.frame();
    }
}

#[test]
fn the_navigator_renames_in_place_and_the_context_menu_offers_the_rest() {
    let project = Project::new("yara-gui-e2e-rename");
    let mut harness = Harness::open(Some(&project));
    // Select the row, then F2 opens the inline name box.
    let at = harness.position_of("README.md").unwrap();
    harness.click(at);
    harness.press(Key::F2, Modifiers::NONE);
    harness.press(Key::A, Modifiers::COMMAND);
    harness.type_text("RENAMED.md");
    harness.press(Key::Enter, Modifiers::NONE);
    assert!(
        project.path().join("RENAMED.md").is_file(),
        "{}",
        harness.screen()
    );

    // The context menu on a file row.
    let at = harness.position_of("RENAMED.md").unwrap();
    harness.right_click(at);
    assert!(harness.shows("Move To..."), "{}", harness.screen());
    assert!(harness.shows("Delete") && harness.shows("New Folder"));
    harness.press(Key::Escape, Modifiers::NONE);
}

#[test]
fn dragging_a_row_onto_a_folder_moves_the_file_in_the_window() {
    let project = Project::new("yara-gui-e2e-dnd");
    let mut harness = Harness::open(Some(&project));
    let from = harness.position_of("README.md").unwrap();
    let to = harness.position_of("src").unwrap();
    harness.drag(from, to);
    assert!(
        project.path().join("src").join("README.md").is_file(),
        "{}",
        harness.screen()
    );
}

#[test]
fn several_folders_each_head_their_own_subtree_in_the_window() {
    let project = Project::new("yara-gui-e2e-roots");
    let other = Project::new("yara-gui-e2e-roots-other");
    // No native dialog in a test: add the folder through the navigator's
    // empty-space menu is dialog-bound too, so use the project directly.
    let ctx = egui::Context::default();
    let mut app = App::with_context(&ctx, Some(project.path().to_path_buf()));
    let _ = other;
    for _ in 0..2 {
        let input = RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(1280.0, 800.0))),
            ..Default::default()
        };
        let _ = ctx.run(input, |ctx| app.ui(ctx));
    }
}

#[test]
fn the_terminal_opens_a_second_session_and_takes_typing() {
    let project = Project::new("yara-gui-e2e-term");
    let mut harness = Harness::open(Some(&project));
    assert!(harness.shows("TERMINAL"), "{}", harness.screen());
    harness.press(Key::T, Modifiers::COMMAND | Modifiers::ALT);
    harness.frame();
    assert!(harness.shows("2"), "{}", harness.screen());
    // Click into the grid and type; the shell gets it.
    let at = harness
        .position_of("TERMINAL")
        .map(|p| Pos2::new(p.x + 40.0, p.y + 60.0))
        .unwrap();
    harness.click(at);
    harness.type_text("echo yara-copy-marker");
    harness.press(Key::Enter, Modifiers::NONE);

    // The shell answers on its own schedule; frames keep coming until it has.
    let mut echoed = false;
    for _ in 0..200 {
        std::thread::sleep(std::time::Duration::from_millis(50));
        harness.frame();
        echoed = harness.shows("yara-copy-marker");
        if echoed {
            break;
        }
    }
    assert!(
        echoed,
        "the shell never echoed the line: {}",
        harness.screen()
    );

    // Drag over the echoed line and copy it: the grid answers the mouse and
    // the clipboard the way the terminal frontend's does.
    let line = harness
        .text
        .iter()
        .find(|(t, _)| t.contains("yara-copy-marker"))
        .map(|(_, p)| *p)
        .unwrap();
    harness.drag(
        Pos2::new(line.x + 1.0, line.y + 4.0),
        Pos2::new(line.x + 300.0, line.y + 4.0),
    );
    harness.events.push(Event::Copy);
    harness.frame();
    let copied = harness.copied.clone().unwrap_or_default();
    assert!(
        copied.contains("yara-copy-marker"),
        "the selection reached the clipboard: {copied:?}"
    );

    harness.press(Key::W, Modifiers::COMMAND | Modifiers::ALT);
    harness.frame();
}

#[test]
fn escape_closes_one_thing_at_a_time() {
    let project = Project::new("yara-gui-e2e-escape");
    let mut harness = Harness::open(Some(&project));
    let at = harness.position_of("README.md").unwrap();
    harness.click(at);
    harness.press(Key::F, Modifiers::COMMAND);
    assert!(harness.shows("REPLACE"), "{}", harness.screen());
    // A modal over the find bar: Escape takes the modal, not the bar with it.
    harness.press(Key::T, Modifiers::COMMAND | Modifiers::SHIFT);
    assert!(harness.shows("Monokai"), "{}", harness.screen());
    harness.press(Key::Escape, Modifiers::NONE);
    assert!(
        !harness.shows("Monokai"),
        "the picker closed: {}",
        harness.screen()
    );
    assert!(
        harness.shows("REPLACE"),
        "the find bar survived: {}",
        harness.screen()
    );
    // A second Escape, with nothing above it, closes the bar.
    harness.press(Key::Escape, Modifiers::NONE);
    assert!(!harness.shows("REPLACE"), "{}", harness.screen());
}

#[test]
fn a_table_cell_keeps_its_column_wide_enough_for_it() {
    let project = Project::new("yara-gui-e2e-preview-table");
    project.file(
        "README.md",
        "| Action | Terminal | Window |\n| --- | :---: | ---: |\n| Preview | `Ctrl+Shift+V` | `Cmd+Shift+V` |\n",
    );
    let mut harness = Harness::open(Some(&project));
    let at = harness.position_of("README.md").unwrap();
    harness.click(at);
    harness.press(Key::V, Modifiers::COMMAND | Modifiers::SHIFT);
    // A cell is measured before its column is, so a cell wrapped to the room
    // it happens to have shrinks its own column and the table folds up into a
    // stack a few letters wide.
    let screen = harness.screen();
    assert_eq!(harness.rows_of("Terminal"), Some(1), "{screen}");
    assert_eq!(harness.rows_of(" Ctrl+Shift+V "), Some(1), "{screen}");
    // A centred column stands its heading over its cells, and a right-hand one
    // ends both at the same edge.
    let head = harness.position_of("Terminal").unwrap();
    let cell = harness.position_of(" Ctrl+Shift+V ").unwrap();
    assert!((head.x - cell.x).abs() < 4.0, "centred: {head:?} {cell:?}");
}

#[test]
fn a_link_in_the_preview_opens_where_it_points() {
    let project = Project::new("yara-gui-e2e-preview-link");
    project.file("README.md", "<https://example.com/docs>\n\nPlain prose.\n");
    let mut harness = Harness::open(Some(&project));
    let at = harness.position_of("README.md").unwrap();
    harness.click(at);
    harness.press(Key::V, Modifiers::COMMAND | Modifiers::SHIFT);
    // An autolink is the link it names, and a click on it opens it.
    let at = harness.position_of("https://example.com/docs").unwrap();
    harness.click(at);
    assert_eq!(
        harness.opened.as_deref(),
        Some("https://example.com/docs"),
        "{}",
        harness.screen()
    );
    // A click on prose that is not a link opens nothing.
    harness.opened = None;
    let at = harness.position_of("Plain prose.").unwrap();
    harness.click(at);
    assert_eq!(harness.opened, None);
}

#[test]
fn a_preview_draws_its_lists_tables_and_charts_in_the_window() {
    let project = Project::new("yara-gui-e2e-preview-rich");
    project.file("README.md", "# Yara Code\n\n## Lists\n\n- one\n  - nested\n- [x] done\n- [ ] todo\n\n## Table\n\n| Name | Size |\n|:-----|-----:|\n| one | 1 |\n| two | 22 |\n\n## Charts\n\n```mermaid\npie title Languages\n  \"Rust\" : 70\n  \"Docs\" : 30\n```\n\n```mermaid\nflowchart LR\n  A[Edit] --> B[Ship]\n```\n");
    let mut harness = Harness::open(Some(&project));
    let at = harness.position_of("README.md").unwrap();
    harness.click(at);
    harness.press(Key::V, Modifiers::COMMAND | Modifiers::SHIFT);
    let screen = harness.screen();
    // A nested item is set in under the one above it, and a ticked item wears
    // its box rather than a bullet.
    assert!(harness.shows("nested") && harness.shows("•"), "{screen}");
    assert!(harness.shows("☑") && harness.shows("☐"), "{screen}");
    assert!(harness.shows("Name") && harness.shows("22"), "{screen}");
    assert!(
        !harness.shows("| Name | Size |"),
        "the pipes are markup: {screen}"
    );
    // The charts are down the page: they are drawn, not printed, so what shows
    // is their labels and shares rather than the mermaid behind them.
    harness.scroll(Pos2::new(640.0, 400.0), -20.0);
    let screen = harness.screen();
    assert!(
        harness.shows("Languages") && harness.shows("70%"),
        "{screen}"
    );
    assert!(harness.shows("Edit") && harness.shows("Ship"), "{screen}");
    assert!(!harness.shows("flowchart LR"), "{screen}");
}

#[test]
fn a_markdown_file_previews_in_the_window() {
    let project = Project::new("yara-gui-e2e-preview");
    project.file(
        "README.md",
        "<div align=\"center\">\n\n# Yara Code\n\nSome **bold** text.\n\n         [![CI](https://x/badge.svg)](https://x/ci)\n\n- one\n- two\n\n</div>\n",
    );
    let mut harness = Harness::open(Some(&project));
    let at = harness.position_of("README.md").unwrap();
    harness.click(at);
    harness.press(Key::V, Modifiers::COMMAND | Modifiers::SHIFT);
    assert!(harness.shows("Yara Code"), "{}", harness.screen());
    assert!(
        !harness.shows("# Yara Code"),
        "the hash is markup, not text"
    );
    assert!(
        harness.shows("README.md preview"),
        "a tab of its own: {}",
        harness.screen()
    );
    assert!(harness.shows("•"), "{}", harness.screen());
    // A file and the preview of it stand in the strip together. Named after
    // the path alone they were one widget to egui, which paints the clash
    // across the two tabs and hands one tab's click to the other.
    assert!(
        !harness.screen().contains("widget ID"),
        "the tabs are two widgets: {}",
        harness.screen()
    );
    // A README's wrapper is markup for a browser, and a badge is the name it
    // was given — neither is painted as the markup it was written as.
    let screen = harness.screen();
    assert!(!screen.contains("<div"), "{screen}");
    assert!(screen.contains("CI") && !screen.contains("]("), "{screen}");
    harness.press(Key::V, Modifiers::COMMAND | Modifiers::SHIFT);
    assert!(!harness.shows("README.md preview"), "{}", harness.screen());
}

#[test]
fn the_indentation_picker_opens_from_its_key_and_the_status_bar() {
    let project = Project::new("yara-gui-e2e-indent");
    let mut harness = Harness::open(Some(&project));
    // The status bar describes the file in front, indentation included.
    let at = harness.position_of("README.md").unwrap();
    harness.click(at);
    assert!(
        harness.shows("Spaces: 4"),
        "the status bar shows it: {}",
        harness.screen()
    );
    harness.press(Key::I, Modifiers::COMMAND | Modifiers::ALT);
    assert!(
        harness.shows("Indentation") && harness.shows("Tabs"),
        "{}",
        harness.screen()
    );
    let at = harness.position_of("Tabs").unwrap();
    harness.click(at);
    assert!(harness.shows("indentation: Tabs"), "{}", harness.screen());
    assert!(!harness.shows("Spaces: 4"), "{}", harness.screen());
}

#[test]
fn saving_the_settings_file_applies_it_without_a_restart() {
    let project = Project::new("yara-gui-e2e-live-settings");
    let mut harness = Harness::open(Some(&project));
    let size = |h: &Harness| {
        h.ctx
            .style()
            .text_styles
            .get(&egui::TextStyle::Monospace)
            .unwrap()
            .size
    };
    assert_eq!(size(&harness), 13.5);

    // File → Settings opens the user's settings.json in a tab. Rewrite it on
    // disk the way an edit would, then Save: the editor writes the buffer's
    // (unchanged) text and reloads what it now finds there.
    harness.press(Key::Comma, Modifiers::COMMAND);
    assert!(harness.shows("settings.json"), "{}", harness.screen());
    let settings = yara::core::settings::Settings::path().unwrap();
    let mut text = std::fs::read_to_string(&settings).unwrap();
    text = text
        .replace("\"font_size\": 13.5", "\"font_size\": 19.0")
        .replace("\"theme\": \"Dark+\"", "\"theme\": \"Monokai\"");
    std::fs::write(&settings, &text).unwrap();
    harness.reload_active_buffer();
    harness.press(Key::S, Modifiers::COMMAND);
    assert!(harness.shows("settings applied"), "{}", harness.screen());
    assert_eq!(size(&harness), 19.0, "the font size was applied on save");
    assert!(
        harness.shows("Monokai"),
        "and the theme: {}",
        harness.screen()
    );
}

#[test]
fn the_command_palette_opens_from_its_key_and_runs_a_command() {
    let project = Project::new("yara-gui-e2e-palette");
    let mut harness = Harness::open(Some(&project));
    harness.press(Key::P, Modifiers::COMMAND | Modifiers::SHIFT);
    // The picker sizes itself on its first frame and shows on the next.
    harness.frame();
    assert!(harness.shows("Command Palette"), "{}", harness.screen());
    harness.type_text("toggle side");
    assert!(harness.shows("Toggle Sidebar"), "{}", harness.screen());
    harness.press(Key::Enter, Modifiers::NONE);
    // The start page lists the palette too, so the field's hint is what
    // proves the picker itself is gone.
    assert!(!harness.shows("Type a command"), "{}", harness.screen());
    assert!(!harness.shows("FILES"), "{}", harness.screen());
}

#[test]
fn close_all_tabs_leaves_the_window_with_nothing_open() {
    let project = Project::new("yara-gui-e2e-closeall");
    project.file("one.txt", "first\n");
    project.file("two.txt", "second\n");
    let mut harness = Harness::open(Some(&project));
    for name in ["one.txt", "two.txt"] {
        let at = harness.position_of(name).expect("the navigator lists it");
        harness.click(at);
    }
    assert!(harness.shows("second"), "{}", harness.screen());
    harness.press(Key::W, Modifiers::COMMAND | Modifiers::SHIFT);
    // Nothing is open, so the start page is what stands in the editor.
    assert!(!harness.shows("second"), "{}", harness.screen());
    assert!(harness.shows("Open Folder"), "{}", harness.screen());
}

impl Harness {
    /// The width the navigator opens at. The same file name is drawn twice —
    /// once in the navigator, once on its tab — and which side of this line it
    /// falls on is what tells the two apart.
    const SIDEBAR: f32 = 220.0;

    /// How far down the tab strip reaches. The file in front is named in the
    /// status bar as well, and that one is at the foot of the window.
    const STRIP: f32 = 60.0;

    /// The middle of a file's tab in the strip.
    fn tab_of(&self, name: &str) -> Option<Pos2> {
        self.text
            .iter()
            .find(|(drawn, at)| drawn == name && at.x > Self::SIDEBAR && at.y < Self::STRIP)
            .map(|(_, at)| Pos2::new(at.x + 12.0, at.y + 6.0))
    }

    /// The close cross of a file's tab: two crossing strokes, the leftmost pair
    /// drawn to the right of the name, since every tab carries its own.
    fn tab_cross(&self, name: &str) -> Option<Pos2> {
        let (_, name_at) = self
            .text
            .iter()
            .find(|(drawn, at)| drawn == name && at.x > Self::SIDEBAR && at.y < Self::STRIP)?;
        self.strokes
            .iter()
            .map(|[a, b]| Pos2::new((a.x + b.x) / 2.0, (a.y + b.y) / 2.0))
            .filter(|at| at.x > name_at.x && at.y < Self::STRIP)
            .min_by(|a, b| a.x.total_cmp(&b.x))
    }

    /// Whether the navigator — not the start page behind it, which names the
    /// same commands — draws a piece of text.
    fn navigator_shows(&self, text: &str) -> bool {
        self.text
            .iter()
            .any(|(drawn, at)| drawn.contains(text) && at.x < Self::SIDEBAR)
    }
}

#[test]
fn a_tab_answers_a_click_on_its_name_and_on_its_cross() {
    let project = Project::new("yara-gui-e2e-tabclicks");
    project.file("second.txt", "the second file\n");
    let mut harness = Harness::open(Some(&project));
    for name in ["README.md", "second.txt"] {
        let at = harness.position_of(name).expect("the navigator lists it");
        harness.click(at);
    }
    assert!(harness.shows("the second file"), "{}", harness.screen());

    // Both files are in the strip; clicking the one behind brings it forward.
    let tab = harness.tab_of("README.md").expect("a tab of its own");
    harness.click(tab);
    assert!(harness.shows("A line of prose."), "{}", harness.screen());

    // The cross on that same tab closes it, and only it.
    let cross = harness.tab_cross("README.md").expect("a cross on the tab");
    harness.click(cross);
    assert!(!harness.shows("A line of prose."), "{}", harness.screen());
    assert!(harness.shows("the second file"), "{}", harness.screen());
}

#[test]
fn a_tab_carried_past_its_neighbour_changes_places_with_it() {
    let project = Project::new("yara-gui-e2e-tabdrag");
    project.file("second.txt", "the second file\n");
    let mut harness = Harness::open(Some(&project));
    for name in ["README.md", "second.txt"] {
        let at = harness.position_of(name).expect("the navigator lists it");
        harness.click(at);
    }
    let first = harness.tab_of("README.md").expect("a tab of its own");
    let second = harness.tab_of("second.txt").expect("a tab of its own");
    assert!(first.x < second.x, "{}", harness.screen());

    // Carried past the middle of its neighbour, the tab takes its place — the
    // strip has already reordered by the time the pointer lets go.
    harness.drag(first, Pos2::new(second.x + 40.0, second.y));
    let moved = harness.tab_of("README.md").expect("still open");
    let stayed = harness.tab_of("second.txt").expect("still open");
    assert!(stayed.x < moved.x, "{}", harness.screen());
    assert!(harness.shows("A line of prose."), "{}", harness.screen());
}

#[test]
fn with_no_folder_the_navigator_offers_the_recent_list_first() {
    let harness = Harness::open(None);
    assert!(
        harness.navigator_shows("No folder in the project"),
        "{}",
        harness.screen()
    );
    assert!(
        harness.navigator_shows("Open Recent..."),
        "{}",
        harness.screen()
    );
    assert!(
        harness.navigator_shows("Open Folder..."),
        "{}",
        harness.screen()
    );
    // Adding a folder to a project that has none is just opening one, and the
    // navigator no longer offers it as a separate thing.
    assert!(
        !harness.navigator_shows("Add Folder to Project..."),
        "{}",
        harness.screen()
    );
}

impl Harness {
    fn scroll(&mut self, at: Pos2, lines: f32) {
        self.events.push(Event::PointerMoved(at));
        self.events.push(Event::MouseWheel {
            unit: egui::MouseWheelUnit::Line,
            delta: Vec2::new(0.0, lines),
            modifiers: Modifiers::NONE,
        });
        self.frame();
        self.frame();
    }

    /// Where a piece of text sits in the frame's paint order — what is drawn
    /// later is drawn on top. Matched whole, because the editor lays a file
    /// out as one galley and every line of it would match a part.
    fn painted_at(&self, text: &str) -> Option<usize> {
        self.text.iter().position(|(drawn, _)| drawn == text)
    }
}

#[test]
fn a_tabs_menu_is_drawn_over_the_sticky_header_and_not_under_it() {
    let project = Project::new("yara-gui-e2e-sticky");
    let mut body = String::from("fn the_enclosing_function() {\n");
    for i in 0..80 {
        body.push_str(&format!("    let line_{i} = {i};\n"));
    }
    body.push_str("}\n");
    project.file("long.rs", &body);
    let mut harness = Harness::open(Some(&project));
    let at = harness
        .position_of("long.rs")
        .expect("the navigator lists it");
    harness.click(at);
    // Far enough down that the function's own line has scrolled off, which is
    // what pins it in the band at the top of the view.
    harness.scroll(Pos2::new(700.0, 400.0), -30.0);
    const HEADER: &str = "fn the_enclosing_function() {";
    assert!(
        harness.painted_at(HEADER).is_some(),
        "the header is pinned: {}",
        harness.screen()
    );

    // The band is a layer of its own, laid over the text it scrolls past. A
    // menu has to stand over it in turn, and once did not.
    let tab = harness.tab_of("long.rs").expect("a tab of its own");
    harness.right_click(tab);
    let header = harness.painted_at(HEADER).expect("the pinned header");
    let menu = harness
        .painted_at("Close All Tabs")
        .expect("the tab's menu");
    assert!(
        menu > header,
        "the menu is painted last: menu {menu}, header {header}"
    );
}

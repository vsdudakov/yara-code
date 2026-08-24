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

/// The editor and the context it draws into, with the text of the last frame.
struct Harness {
    app: App,
    ctx: egui::Context,
    events: Vec<Event>,
    text: Vec<String>,
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
                egui::Shape::Text(text) => Some(text.galley.text().to_string()),
                _ => None,
            })
            .collect();
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
        self.events.push(Event::PointerMoved(at));
        self.events.push(Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Primary,
            pressed: true,
            modifiers: Modifiers::NONE,
        });
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

    fn shows(&self, text: &str) -> bool {
        self.text.iter().any(|drawn| drawn.contains(text))
    }

    fn screen(&self) -> String {
        self.text.join(" | ")
    }
}

#[test]
fn with_no_folder_the_window_shows_the_start_page() {
    let harness = Harness::open(None);
    assert!(harness.shows("YARA CODE"), "{}", harness.screen());
    assert!(
        harness.shows("no folder in the project"),
        "{}",
        harness.screen()
    );
    // The menus and the key groups the start page is built from.
    assert!(harness.shows("File") && harness.shows("View") && harness.shows("Help"));
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
    assert!(harness.shows("key bindings"), "{}", harness.screen());
    assert!(harness.shows("Save"));
    harness.press(Key::Escape, Modifiers::NONE);
    assert!(!harness.shows("key bindings"));
}

#[test]
fn zooming_changes_the_code_font_and_resets() {
    let project = Project::new("yara-gui-e2e-zoom");
    let mut harness = Harness::open(Some(&project));
    let size = |harness: &Harness| {
        harness
            .ctx
            .style()
            .text_styles
            .get(&egui::TextStyle::Monospace)
            .unwrap()
            .size
    };
    let before = size(&harness);
    harness.press(Key::Equals, Modifiers::COMMAND);
    assert!(size(&harness) > before, "zoom in");
    harness.press(Key::Num0, Modifiers::COMMAND);
    assert_eq!(size(&harness), before, "reset puts it back");
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

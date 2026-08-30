//! The frame, read back off ratatui's test backend: the same `App` and `draw`
//! the terminal runs, with keys fed in and the drawn text asserted on.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::backend::TestBackend;
use ratatui::Terminal;
use yara_core::follow::EditEvent;
use yara_core::settings::{Settings, Side};
use yara_core::theme::Theme;
use ycode::app::{App, Focus, Overlay};
use ycode::ui;

fn frame(app: &mut App) -> Vec<String> {
    let mut terminal = Terminal::new(TestBackend::new(100, 24)).unwrap();
    terminal.draw(|frame| ui::draw(frame, app)).unwrap();
    let buffer = terminal.backend().buffer();
    (0..buffer.area.height)
        .map(|y| {
            (0..buffer.area.width)
                .map(|x| buffer[(x, y)].symbol())
                .collect::<String>()
        })
        .collect()
}

fn text(app: &mut App) -> String {
    frame(app).join("\n")
}

/// An app with the keyboard on the follow pane, which most tests drive.
fn following(settings: Settings) -> App {
    let mut app = App::with_settings(Some(scratch()), settings, Theme::default());
    app.focus = Focus::Follow;
    app
}

/// A folder that is a project but not a repository: the panes without git.
fn scratch() -> std::path::PathBuf {
    let path = std::env::temp_dir().join("yara-frame-scratch");
    let _ = std::fs::create_dir_all(&path);
    path
}

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

fn edit(path: &str, diff: &str) -> EditEvent {
    EditEvent::from_unified(path, diff)
}

#[test]
fn an_empty_session_shows_both_panes_and_waits_for_an_edit() {
    let mut app = App::new(Some("/work/demo".into()));
    let rows = frame(&mut app);
    assert!(
        rows[0].contains("YARA  File  Help  │  demo  [+]"),
        "{}",
        rows[0]
    );
    assert!(
        rows[0].contains("○ claude — exited"),
        "no agent was started"
    );
    assert!(rows[1].contains("AGENT · claude"));
    assert!(rows[1].contains("FOLLOW · LIVE"));
    assert!(rows[1].find("AGENT").unwrap() < rows[1].find("FOLLOW").unwrap());
    let all = rows.join("\n");
    assert!(all.contains("waiting for the agent's first edit"));
    assert!(all.contains("no edits yet"));
    assert!(
        rows[23].contains("F5 palette  F3 search  F1 keys  v"),
        "the hints that fit, then the version: {}",
        rows[23]
    );
    assert!(
        rows[0].contains(" demo  [+]"),
        "one tab, named by its folder: {}",
        rows[0]
    );
}

#[test]
fn the_settings_place_the_agent_name_its_pane_and_the_sidebar() {
    let settings = Settings {
        agent: "codex".into(),
        agent_side: Side::Right,
        agent_width: 30,
        show_sidebar: true,
        sidebar_width: 20,
        ..Settings::default()
    };
    let mut app = App::with_settings(Some(scratch()), settings, Theme::default());
    let rows = frame(&mut app);
    assert!(rows[1].contains("AGENT · codex"));
    // The tree keeps to the edge away from the agent.
    assert!(rows[1].find("FILES").unwrap() < rows[1].find("FOLLOW").unwrap());
    assert!(rows[1].find("FOLLOW").unwrap() < rows[1].find("AGENT").unwrap());
    let mut app = App::with_settings(
        Some(scratch()),
        Settings {
            show_sidebar: true,
            ..Settings::default()
        },
        Theme::default(),
    );
    let rows = frame(&mut app);
    assert!(rows[1].find("AGENT").unwrap() < rows[1].find("FOLLOW").unwrap());
    assert!(rows[1].find("FOLLOW").unwrap() < rows[1].find("FILES").unwrap());
    assert!(rows[21].contains("^B hide · ⏎ open"), "{}", rows[21]);
}

#[test]
fn a_rebound_key_moves_the_action_and_its_hint() {
    let mut settings = Settings::default();
    settings
        .keys
        .map
        .insert("follow_live".into(), "L".parse().unwrap());
    let mut app = following(settings);
    app.record_edit(edit("a.rs", "+one\n"));
    app.record_edit(edit("b.rs", "+two\n"));
    app.handle_key(key(KeyCode::Left));
    assert!(text(&mut app).contains("[ l → live ]"));
    app.handle_key(key(KeyCode::Char('f')));
    assert!(!app.follow.is_live(), "f is no longer bound");
    app.handle_key(key(KeyCode::Char('l')));
    assert!(app.follow.is_live());
}

#[cfg(unix)]
#[test]
fn the_agent_runs_in_its_pane_takes_the_keys_and_f6_hands_them_to_follow() {
    let settings = Settings {
        agent: "cat".into(),
        ..Settings::default()
    };
    let mut app = App::with_settings(Some(scratch()), settings, Theme::default());
    app.start_agent();
    assert!(app.agent_running());
    let rows = frame(&mut app);
    assert!(rows[0].contains("● cat — running"));
    assert!(rows[1].contains("AGENT · cat"));

    // With the agent focused, `f` is typed at it rather than going live.
    app.focus = Focus::Agent;
    app.record_edit(edit("a.rs", "+one\n"));
    app.follow.scrub_back();
    for c in "hey".chars() {
        app.handle_key(key(KeyCode::Char(c)));
    }
    app.handle_key(key(KeyCode::Enter));
    app.handle_key(key(KeyCode::Char('f')));
    assert!(!app.follow.is_live());
    let start = std::time::Instant::now();
    let mut screen = String::new();
    while start.elapsed() < std::time::Duration::from_secs(5) && screen.matches("hey").count() < 2 {
        std::thread::sleep(std::time::Duration::from_millis(20));
        screen = app.agent.as_ref().unwrap().with_screen(|s| s.contents());
    }
    assert_eq!(
        screen.matches("hey").count(),
        2,
        "cat echoed it: {screen:?}"
    );
    // The frame paints the agent's screen, and the dirty flag asked for it.
    assert!(app.take_dirty());
    let mut terminal = Terminal::new(TestBackend::new(100, 24)).unwrap();
    terminal.draw(|frame| ui::draw(frame, &mut app)).unwrap();
    let painted = terminal
        .backend()
        .buffer()
        .content
        .iter()
        .map(|c| c.symbol())
        .collect::<String>();
    assert!(painted.contains("hey"), "{painted}");

    // F6 moves the keyboard; now `f` is the follow pane's.
    app.handle_key(key(KeyCode::F(6)));
    assert_eq!(app.focus, Focus::Follow);
    app.handle_key(key(KeyCode::Char('f')));
    assert!(app.follow.is_live());
    app.handle_key(key(KeyCode::F(6)));
    assert_eq!(app.focus, Focus::Agent);
    // A function key is the editor's even with the agent focused; a Ctrl
    // chord the agent uses itself is not.
    app.handle_key(KeyEvent::new(KeyCode::Char('r'), KeyModifiers::CONTROL));
    assert!(
        app.overlay.is_none() && app.note.is_none(),
        "Ctrl+R went to cat"
    );
    app.handle_key(key(KeyCode::F(4)));
    assert!(app.overlay.is_some(), "CHANGES opened");
}

#[test]
fn an_agent_that_cannot_start_is_a_note_not_a_crash() {
    let settings = Settings {
        agent: "".into(),
        ..Settings::default()
    };
    let mut app = App::with_settings(Some(scratch()), settings, Theme::default());
    app.start_agent();
    assert!(app.agent.is_none());
    assert!(text(&mut app).contains("no agent command"));
}

#[test]
fn an_edit_lands_in_the_follow_pane_with_its_diff_and_counts() {
    let mut app = following(Settings::default());
    app.record_edit(edit(
        "src/main.rs",
        "@@ -1,2 +1,3 @@\n fn main() {\n-    let x = 1;\n+    let total = 1;\n+    let extra = 2;\n",
    ));
    let all = text(&mut app);
    assert!(all.contains("src/main.rs +2 −1"));
    assert!(all.contains("edits ◉ [1/1]"));
    assert!(all.contains("← → scrub · f live · ⏎ mark reviewed"));
    assert!(all.contains("[ v: diff ]"));
    assert!(all.contains("    1   fn main() {"));
    assert!(all.contains("    2 −     let x = 1;"));
    assert!(all.contains("    2 +     let total = 1;"));
    assert!(all.contains("◆ 1 unreviewed"));
}

#[test]
fn scrubbing_back_pauses_the_pane_and_f_brings_it_back_live() {
    let mut app = following(Settings::default());
    app.record_edit(edit("a.rs", "+one\n"));
    app.record_edit(edit("b.rs", "+two\n"));
    app.handle_key(key(KeyCode::Left));
    let all = text(&mut app);
    assert!(all.contains("FOLLOW · PAUSED"));
    assert!(all.contains("[ f → live ]"));
    assert!(all.contains("a.rs +1 −0"));
    assert!(all.contains("edits ◉● [1/2]"));
    app.handle_key(key(KeyCode::Char('f')));
    let all = text(&mut app);
    assert!(all.contains("FOLLOW · LIVE"));
    assert!(!all.contains("→ live ]"));
    assert!(all.contains("edits ●◉ [2/2]"));
}

#[test]
fn enter_reviews_the_edit_and_the_status_bar_counts_down() {
    let mut app = following(Settings::default());
    app.record_edit(edit("a.rs", "+one\n"));
    app.record_edit(edit("b.rs", "+two\n"));
    app.handle_key(key(KeyCode::Enter));
    let all = text(&mut app);
    assert!(all.contains("◆ 1 unreviewed"));
    assert!(all.contains("edits ◉○ [1/2]"));
    app.handle_key(key(KeyCode::Enter));
    let all = text(&mut app);
    assert!(all.contains("✓ all reviewed"));
    assert!(all.contains("[✓ reviewed]"));
    assert!(all.contains("FOLLOW · LIVE"));
}

#[test]
fn a_long_timeline_windows_to_the_configured_ticks() {
    let settings = Settings {
        timeline_ticks: 5,
        ..Settings::default()
    };
    let mut app = following(settings);
    for i in 0..20 {
        app.record_edit(edit(&format!("f{i}.rs"), "+x\n"));
    }
    assert!(text(&mut app).contains("edits ‥●●●●◉ [20/20]"));
}

#[test]
fn ctrl_b_shows_the_files_and_ctrl_q_asks_to_quit() {
    let mut app = following(Settings::default());
    assert!(!text(&mut app).contains("FILES"));
    app.handle_key(KeyEvent::new(KeyCode::Char('b'), KeyModifiers::CONTROL));
    assert!(text(&mut app).contains("FILES"));
    app.handle_key(KeyEvent::new(KeyCode::Char('q'), KeyModifiers::CONTROL));
    assert!(app.should_quit);
}

#[test]
fn checking_for_updates_says_so_and_the_version_chip_sits_in_the_status_bar() {
    let mut app = following(Settings::default());
    let all = text(&mut app);
    assert!(
        all.contains(&format!("v{} ", yara_core::update::CURRENT)),
        "{all}"
    );
    app.execute(yara_core::command::Command::CheckForUpdates);
    assert!(text(&mut app).contains("checking for updates…"));
}

/// A project that is a repository with one commit on `main`, removed with
/// the test. Git's own configuration is set so a commit works anywhere.
struct Repo(std::path::PathBuf);

impl Repo {
    fn new(tag: &str) -> Self {
        let path = std::env::temp_dir().join(format!("{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).unwrap();
        let path = path.canonicalize().unwrap();
        let repo = Self(path);
        for args in [
            vec!["init", "-q", "-b", "main"],
            vec!["config", "user.email", "t@t"],
            vec!["config", "user.name", "t"],
        ] {
            repo.git(&args);
        }
        repo.file("src/main.rs", "fn main() {\n    let x = 1;\n}\n");
        repo.git(&["add", "."]);
        repo.git(&["commit", "-q", "-m", "first"]);
        repo
    }

    fn git(&self, args: &[&str]) {
        let status = std::process::Command::new("git")
            .arg("-C")
            .arg(&self.0)
            .args(args)
            .status()
            .unwrap();
        assert!(status.success(), "git {args:?}");
    }

    fn file(&self, name: &str, body: &str) {
        let path = self.0.join(name);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, body).unwrap();
    }
}

impl Drop for Repo {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn the_agents_edits_reach_the_timeline_from_the_working_tree() {
    let repo = Repo::new("yara-frame-git");
    let mut app = App::with_settings(Some(repo.0.clone()), Settings::default(), Theme::default());
    app.focus = Focus::Follow;
    app.refresh();
    let all = text(&mut app);
    assert!(all.contains("⎇ main"), "{all}");
    assert!(all.contains("waiting for the agent's first edit"));

    repo.file("src/main.rs", "fn main() {\n    let total = 1;\n}\n");
    app.refresh();
    let all = text(&mut app);
    assert!(all.contains("src/main.rs +1 −1"), "{all}");
    assert!(all.contains("    2 −     let x = 1;"));
    assert!(all.contains("    2 +     let total = 1;"));
    assert!(all.contains("◆ 1 unreviewed"));
    assert!(
        all.contains("…"),
        "the long path is shortened, not the rest"
    );
    assert!(all.contains("+1 −1"), "the status bar counts against main");

    // The file view shows the file as it stands, the new line marked.
    app.handle_key(key(KeyCode::Char('v')));
    let all = text(&mut app);
    assert!(all.contains("[ v: file ]"));
    assert!(all.contains("▎    2      let total = 1;"), "{all}");
    assert!(all.contains("     1  fn main() {"));
}

#[test]
fn changes_lists_what_differs_from_main_and_opens_a_files_diff() {
    let repo = Repo::new("yara-frame-changes");
    repo.git(&["checkout", "-q", "-b", "feature"]);
    repo.file("README.md", "# Title\n");
    repo.git(&["add", "."]);
    repo.git(&["commit", "-q", "-m", "docs"]);
    let settings = Settings {
        base_branch: "main".into(),
        ..Settings::default()
    };
    let mut app = App::with_settings(Some(repo.0.clone()), settings, Theme::default());
    app.focus = Focus::Follow;
    app.refresh();
    repo.file("src/main.rs", "fn main() {}\n");
    app.refresh();
    app.handle_key(key(KeyCode::F(4)));
    let all = text(&mut app);
    assert!(all.contains("CHANGES"));
    assert!(all.contains(" A README.md"), "{all}");
    assert!(all.contains(" M src/main.rs"));
    assert!(all.contains("vs main · 2 files · +2 −3 · ⏎ open diff · Esc close"));

    app.handle_key(key(KeyCode::Down));
    app.handle_key(key(KeyCode::Enter));
    let all = text(&mut app);
    assert!(!all.contains("CHANGES ·"), "the overlay closed");
    assert!(all.contains("FOLLOW · CHANGES"));
    assert!(all.contains("[ Esc → timeline ]"));
    assert!(
        all.contains("src/main.rs +1 −3"),
        "the whole distance from main: {all}"
    );
    app.handle_key(key(KeyCode::Esc));
    assert!(text(&mut app).contains("FOLLOW · LIVE"));

    app.handle_key(key(KeyCode::F(4)));
    app.handle_key(key(KeyCode::Esc));
    assert!(app.overlay.is_none());
}

#[test]
fn a_new_tab_is_an_agent_in_a_worktree_of_its_own_and_tabs_are_named_by_their_work() {
    let repo = Repo::new("yara-frame-tabs");
    let settings = Settings {
        agent: "cat".into(),
        worktrees_dir: repo.0.join("trees").to_string_lossy().into_owned(),
        ..Settings::default()
    };
    let mut app = App::with_settings(Some(repo.0.clone()), settings, Theme::default());
    app.focus = Focus::Follow;
    app.refresh();
    assert!(
        frame(&mut app)[0].contains(" main  [+]"),
        "named by its branch"
    );

    app.handle_key(key(KeyCode::F(7)));
    assert!(text(&mut app).contains("NEW TASK"));
    for c in "task/login".chars() {
        app.handle_key(key(KeyCode::Char(c)));
    }
    app.handle_key(key(KeyCode::Backspace));
    app.handle_key(key(KeyCode::Char('n')));
    assert!(text(&mut app).contains("task/login█"));
    app.handle_key(key(KeyCode::Enter));
    assert_eq!(app.tasks.len(), 2);
    assert_eq!(app.active, 1);
    assert_eq!(app.repo().unwrap().branch, "task-login");
    let rows = frame(&mut app);
    assert!(rows[0].contains(" main   task/login  [+]"), "{}", rows[0]);
    assert!(rows[0].contains("● cat — running"), "an agent of its own");
    let repo_name = repo.0.file_name().unwrap().to_string_lossy().into_owned();
    assert!(repo
        .0
        .join(format!("trees/{repo_name}/task-login/src/main.rs"))
        .exists());

    // Each tab keeps its own timeline: an edit in the worktree lands only
    // in the tab that watches it.
    std::fs::write(
        repo.0
            .join(format!("trees/{repo_name}/task-login/src/main.rs")),
        "fn main() {}\n",
    )
    .unwrap();
    app.refresh();
    assert_eq!(app.follow.len(), 1);
    assert_eq!(app.tasks[0].follow.len(), 0);

    app.handle_key(key(KeyCode::F(2)));
    for c in "PR 42".chars() {
        app.handle_key(key(KeyCode::Char(c)));
    }
    app.handle_key(key(KeyCode::Enter));
    assert!(frame(&mut app)[0].contains(" main   PR 42  [+]"));

    let with_ctrl = |c| KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL);
    app.handle_key(with_ctrl('k'));
    assert_eq!(app.active, 0);
    app.handle_key(with_ctrl('l'));
    assert_eq!(app.active, 1);
    app.handle_key(KeyEvent::new(KeyCode::Char('w'), KeyModifiers::CONTROL));
    assert_eq!(app.tasks.len(), 1);
    assert!(!app.should_quit);
    app.handle_key(KeyEvent::new(KeyCode::Char('w'), KeyModifiers::CONTROL));
    assert!(app.should_quit, "closing the last tab is quitting");
}

#[test]
fn files_open_in_the_editor_type_save_and_close_back_to_follow() {
    let repo = Repo::new("yara-frame-edit");
    let mut app = App::with_settings(Some(repo.0.clone()), Settings::default(), Theme::default());
    app.refresh();
    let ctrl = |c| KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL);
    app.handle_key(ctrl('b'));
    assert_eq!(app.focus, Focus::Files);
    let all = text(&mut app);
    assert!(all.contains("▸ src"), "{all}");
    app.handle_key(key(KeyCode::Enter));
    assert!(text(&mut app).contains("▾ src"));
    app.handle_key(key(KeyCode::Down));
    app.handle_key(key(KeyCode::Enter));
    assert_eq!(app.focus, Focus::Editor);
    let all = text(&mut app);
    assert!(all.contains(" EDIT "), "{all}");
    assert!(all.contains("src/main.rs") && all.contains("Rust"), "{all}");
    assert!(all.contains("    1  fn main() {"));
    assert!(!all.contains("main.rs ●"), "clean");
    // The caret is a drawn cell that blinks with the app's own clock.
    let mut terminal = Terminal::new(TestBackend::new(100, 24)).unwrap();
    terminal.draw(|frame| ui::draw(frame, &mut app)).unwrap();
    let (cx, cy) = (app.hits.editor.x, app.hits.editor.y);
    let lit = terminal.backend().buffer()[(cx, cy)].bg;
    assert_eq!(lit, ycode::theme::color(app.theme.ui.cursor));
    app.blink();
    terminal.draw(|frame| ui::draw(frame, &mut app)).unwrap();
    assert_ne!(terminal.backend().buffer()[(cx, cy)].bg, lit);

    // Typing goes into the file — `f` is not follow's here.
    app.handle_key(key(KeyCode::Char('f')));
    app.handle_key(key(KeyCode::Enter));
    let all = text(&mut app);
    assert!(all.contains("main.rs ●"), "dirty: {all}");
    assert!(all.contains("    1  f"));
    assert!(all.contains("    2  fn main() {"));
    app.handle_key(ctrl('z'));
    assert!(!text(&mut app).contains("main.rs ●"), "undone");
    app.handle_key(ctrl('y'));
    assert!(text(&mut app).contains("main.rs ●"), "redone");
    app.handle_key(ctrl('s'));
    let all = text(&mut app);
    assert!(all.contains("✓ saved"), "{all}");
    // The editor's own chords still reach the editor from a file: the tree
    // opens, the palette opens, a plain letter is typing.
    app.handle_key(ctrl('b'));
    assert!(!app.show_sidebar, "the tree hid, from inside the file");
    app.handle_key(ctrl('b'));
    assert!(app.show_sidebar && app.focus == Focus::Files);
    app.focus = Focus::Editor;
    app.handle_key(key(KeyCode::F(5)));
    assert!(matches!(app.overlay, Some(Overlay::Palette(..))));
    app.handle_key(key(KeyCode::Esc));
    let before = app.editor.as_ref().unwrap().text.clone();
    app.handle_key(key(KeyCode::Char('v')));
    assert_ne!(
        app.editor.as_ref().unwrap().text,
        before,
        "a letter is typing"
    );
    app.handle_key(ctrl('z'));
    assert!(std::fs::read_to_string(repo.0.join("src/main.rs"))
        .unwrap()
        .starts_with("f\nfn main"));
    // The agent's edits are told apart from the user's own: the tree tints
    // what changed and the timeline shows the save.
    app.refresh();
    assert!(text(&mut app).contains("main.rs ●"), "{}", text(&mut app));
    app.handle_key(key(KeyCode::Esc));
    assert_eq!(app.focus, Focus::Follow);
    assert!(text(&mut app).contains("FOLLOW · LIVE"));
    assert_eq!(app.follow.len(), 1);
}

#[test]
fn ctrl_p_finds_a_file_by_a_few_letters() {
    let repo = Repo::new("yara-frame-quick");
    repo.file("docs/guide.md", "# guide\n");
    let mut app = App::with_settings(Some(repo.0.clone()), Settings::default(), Theme::default());
    app.focus = Focus::Follow;
    app.handle_key(KeyEvent::new(KeyCode::Char('p'), KeyModifiers::CONTROL));
    let all = text(&mut app);
    assert!(all.contains("GO TO FILE"), "{all}");
    assert!(all.contains("docs/guide.md") && all.contains("src/main.rs"));
    for c in "gd".chars() {
        app.handle_key(key(KeyCode::Char(c)));
    }
    let all = text(&mut app);
    assert!(all.contains("> gd█"));
    assert!(!all.contains("src/main.rs"));
    app.handle_key(key(KeyCode::Enter));
    let shown = text(&mut app);
    assert_eq!(app.focus, Focus::Editor, "{shown}");
    assert!(shown.contains("guide.md"), "{shown}");
    assert!(
        app.tree
            .as_ref()
            .unwrap()
            .selected_row()
            .unwrap()
            .path
            .ends_with("guide.md"),
        "revealed"
    );
}

#[test]
fn the_palette_search_keys_menus_and_recent_open_and_do_their_work() {
    let repo = Repo::new("yara-frame-overlays");
    let mut app = App::with_settings(Some(repo.0.clone()), Settings::default(), Theme::default());
    app.focus = Focus::Follow;

    // The palette runs a command found by a few letters.
    app.handle_key(key(KeyCode::F(5)));
    let overlay = format!("{:?}", app.overlay);
    let all = text(&mut app);
    assert!(
        all.contains("COMMAND PALETTE") && all.contains("type a command…"),
        "{overlay}\n{all}"
    );
    for c in "togf".chars() {
        app.handle_key(key(KeyCode::Char(c)));
    }
    let all = text(&mut app);
    assert!(all.contains(" Toggle Files") && all.contains("^B"), "{all}");
    app.handle_key(key(KeyCode::Enter));
    assert!(app.show_sidebar && app.overlay.is_none());

    // Search lists path:line text and opens the hit on its line.
    app.handle_key(key(KeyCode::F(3)));
    for c in "let".chars() {
        app.handle_key(key(KeyCode::Char(c)));
    }
    let all = text(&mut app);
    assert!(all.contains("SEARCH PROJECT"), "{all}");
    assert!(all.contains(" src/main.rs:2  let x = 1;"), "{all}");
    assert!(all.contains("1 matches in 1 files · exclude: target, node_modules, .*"));
    app.handle_key(key(KeyCode::Enter));
    assert_eq!(app.focus, Focus::Editor);
    assert_eq!(app.editor.as_ref().unwrap().line_col().0, 1);
    app.handle_key(key(KeyCode::Esc));

    // F1 lists every command with a dotted leader to its chord.
    app.handle_key(key(KeyCode::F(1)));
    let all = text(&mut app);
    assert!(all.contains("KEY BINDINGS"), "{all}");
    assert!(all.contains(" Save ") && all.contains("· Ctrl+S"), "{all}");
    app.handle_key(key(KeyCode::Esc));

    // F10 drops the File menu under its word; Right moves to Help.
    app.handle_key(key(KeyCode::F(10)));
    let rows = frame(&mut app);
    assert!(rows[1].contains("┌ File "), "{}", rows[1]);
    assert!(
        rows[2].contains("Open Recent Workspace…") && rows[2].contains("^R"),
        "{}",
        rows[2]
    );
    app.handle_key(key(KeyCode::Right));
    let all = text(&mut app);
    assert!(
        all.contains("┌ Help ") && all.contains("Documentation"),
        "{all}"
    );
    app.handle_key(key(KeyCode::Down));
    app.handle_key(key(KeyCode::Enter));
    assert!(
        matches!(app.overlay, Some(Overlay::Keys(_))),
        "Help → Key Bindings"
    );
    app.handle_key(key(KeyCode::Esc));

    // Ctrl+R lists recent projects.
    app.settings.push_recent(std::slice::from_ref(&repo.0));
    app.handle_key(KeyEvent::new(KeyCode::Char('r'), KeyModifiers::CONTROL));
    let all = text(&mut app);
    assert!(
        all.contains("OPEN RECENT") && all.contains("yara-frame-overlays"),
        "{all}"
    );
    app.handle_key(key(KeyCode::Esc));
    assert!(app.overlay.is_none());
}

#[test]
fn the_start_page_lists_recent_projects_and_enter_opens_one() {
    let repo = Repo::new("yara-frame-start");
    let mut settings = Settings::default();
    settings.push_recent(std::slice::from_ref(&repo.0));
    settings.push_recent(&[std::path::PathBuf::from("/somewhere/else")]);
    let mut app = App::with_settings(None, settings, Theme::default());
    let all = text(&mut app);
    assert!(
        all.contains("the terminal editor for the agent loop"),
        "{all}"
    );
    assert!(all.contains("RECENT"));
    assert!(
        all.contains("▸ /somewhere/else") && all.contains("⏎"),
        "{all}"
    );
    assert!(all.contains("⏎ open project · ^P go to file · F1 keys"));
    assert!(!all.contains("FOLLOW"), "no panes without a project");

    let lock = std::env::temp_dir().join(format!("yara-frame-start-config-{}", std::process::id()));
    std::env::set_var("YARA_CONFIG_DIR", &lock);
    app.handle_key(key(KeyCode::Down));
    app.handle_key(key(KeyCode::Enter));
    std::env::remove_var("YARA_CONFIG_DIR");
    let _ = std::fs::remove_dir_all(&lock);
    assert_eq!(app.project(), Some(repo.0.as_path()));
    assert_eq!(
        app.settings.recent_workspaces[0],
        vec![repo.0.clone()],
        "moved to the front"
    );
    let all = text(&mut app);
    assert!(
        all.contains("FOLLOW · LIVE") && all.contains("⎇ main"),
        "{all}"
    );
}

#[cfg(unix)]
#[test]
fn agent_usage_is_polled_from_the_configured_commands_and_shown_as_bars() {
    let settings = Settings {
        usage_commands: std::collections::BTreeMap::from([(
            "claude".to_string(),
            r#"echo '{"plan":"Max","percent":85,"detail":"1.2M tokens","reset":"resets in 3h"}'"#
                .to_string(),
        )]),
        ..Settings::default()
    };
    let mut app = following(settings);
    app.handle_key(key(KeyCode::F(8)));
    assert!(text(&mut app).contains("asking the agents…"));
    let start = std::time::Instant::now();
    while app.usage.is_none() && start.elapsed().as_secs() < 5 {
        std::thread::sleep(std::time::Duration::from_millis(20));
        app.collect();
    }
    let all = text(&mut app);
    assert!(all.contains("AGENT USAGE"), "{all}");
    assert!(
        all.contains(" claude  Max       ▰▰▰▰▰▰▰▰▰▱  85%  1.2M tokens"),
        "{all}"
    );
    assert!(
        all.contains("resets in 3h")
            && all.contains("polled from each agent CLI · refreshed 0s ago")
    );
    app.handle_key(key(KeyCode::Esc));
    assert!(text(&mut app).contains("◐ claude 85%"), "the header chip");
}

#[test]
fn the_theme_picker_switches_the_theme_and_the_mouse_reaches_the_chrome() {
    use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
    let repo = Repo::new("yara-frame-mouse");
    let mut app = App::with_settings(Some(repo.0.clone()), Settings::default(), Theme::default());
    app.focus = Focus::Follow;
    app.refresh();
    app.handle_key(key(KeyCode::F(9)));
    let all = text(&mut app);
    assert!(
        all.contains("THEME") && all.contains(" Dark Modern"),
        "{all}"
    );
    app.handle_key(key(KeyCode::Enter));
    assert_eq!(app.theme.name, "Dark Modern");

    let click = |x, y| MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: x,
        row: y,
        modifiers: KeyModifiers::NONE,
    };
    // A click on "File" drops the menu; one on the backdrop closes it.
    frame(&mut app);
    let (menu, _) = app.hits.menus[0];
    app.handle_mouse(click(menu.x, menu.y));
    assert!(matches!(app.overlay, Some(Overlay::Menu(0, _))));
    frame(&mut app);
    app.handle_mouse(click(90, 20));
    assert!(app.overlay.is_none());
    // A click in the agent pane moves the keyboard there; on a tick, back.
    frame(&mut app);
    app.handle_mouse(click(app.hits.agent.x + 2, app.hits.agent.y + 2));
    assert_eq!(app.focus, Focus::Agent);
    repo.file("src/main.rs", "fn main() {}\n");
    app.refresh();
    repo.file("README.md", "hi\n");
    app.refresh();
    frame(&mut app);
    let (tick, index) = app.hits.ticks[0];
    app.handle_mouse(click(tick.x, tick.y));
    assert_eq!(
        (app.focus, app.follow.cursor(), index),
        (Focus::Follow, 0, 0)
    );
    assert!(!app.follow.is_live());
    // The counter jumps to the next unreviewed edit; the live button goes live.
    frame(&mut app);
    let counter = app.hits.counter;
    app.handle_mouse(click(counter.x, counter.y));
    assert_eq!(app.follow.cursor(), 1);
    app.follow.scrub_back();
    frame(&mut app);
    let live = app.hits.live;
    app.handle_mouse(click(live.x + 2, live.y));
    assert!(app.follow.is_live());
    // A file row opens the file.
    app.handle_key(KeyEvent::new(KeyCode::Char('b'), KeyModifiers::CONTROL));
    frame(&mut app);
    let (row, _) = app.hits.file_rows[0];
    app.handle_mouse(click(row.x + 3, row.y));
    frame(&mut app);
    let (row, _) = app.hits.file_rows[1];
    app.handle_mouse(click(row.x + 3, row.y));
    assert_eq!(app.focus, Focus::Editor);
    assert!(app.editor.as_ref().unwrap().path.ends_with("main.rs"));
}

#[test]
fn a_new_file_is_made_where_the_files_cursor_is_and_settings_opens_its_own_file() {
    let repo = Repo::new("yara-frame-newfile");
    let mut app = App::with_settings(Some(repo.0.clone()), Settings::default(), Theme::default());
    app.focus = Focus::Follow;
    app.handle_key(KeyEvent::new(KeyCode::Char('n'), KeyModifiers::CONTROL));
    assert!(text(&mut app).contains("NEW FILE"));
    for c in "notes.md".chars() {
        app.handle_key(key(KeyCode::Char(c)));
    }
    app.handle_key(key(KeyCode::Enter));
    assert!(
        repo.0.join("notes.md").exists(),
        "{:?} {:?} {:?} {:?}",
        app.note,
        app.overlay,
        app.editor.as_ref().map(|b| b.path.clone()),
        app.project()
    );
    assert_eq!(app.focus, Focus::Editor);
    assert!(text(&mut app).contains("notes.md"));

    let config =
        std::env::temp_dir().join(format!("yara-frame-newfile-config-{}", std::process::id()));
    std::env::set_var("YARA_CONFIG_DIR", &config);
    app.execute(yara_core::command::Command::Settings);
    std::env::remove_var("YARA_CONFIG_DIR");
    let shown = text(&mut app);
    let _ = std::fs::remove_dir_all(&config);
    assert!(shown.contains("settings.json"), "{shown}");
    assert!(app.editor.as_ref().unwrap().text.contains("\"agent\""));
}

#[cfg(unix)]
#[test]
fn without_a_usage_command_agent_usage_asks_the_agent_itself() {
    let settings = Settings {
        agent: "cat".into(),
        usage_slash: std::collections::BTreeMap::from([("cat".to_string(), "/status".to_string())]),
        ..Settings::default()
    };
    let mut app = following(settings);
    app.start_agent();
    app.handle_key(key(KeyCode::F(8)));
    assert_eq!(app.focus, Focus::Agent);
    let start = std::time::Instant::now();
    let mut screen = String::new();
    while start.elapsed().as_secs() < 5 && !screen.contains("/status") {
        std::thread::sleep(std::time::Duration::from_millis(20));
        screen = app.agent.as_ref().unwrap().with_screen(|s| s.contents());
    }
    assert!(screen.contains("/status"), "typed at the agent: {screen:?}");
}

#[test]
fn the_wheel_scrolls_the_diff_and_moves_the_editor_caret() {
    use crossterm::event::{MouseEvent, MouseEventKind};
    let repo = Repo::new("yara-frame-wheel");
    let mut app = App::with_settings(Some(repo.0.clone()), Settings::default(), Theme::default());
    app.focus = Focus::Follow;
    app.refresh();
    let body: String = (1..=60).map(|i| format!("line {i}\n")).collect();
    repo.file("src/main.rs", &body);
    app.refresh();
    let wheel = |kind, x, y| MouseEvent {
        kind,
        column: x,
        row: y,
        modifiers: KeyModifiers::NONE,
    };
    frame(&mut app);
    let follow = app.hits.follow;
    let all = text(&mut app);
    assert!(all.contains("    1 + line 1"), "{all}");
    for _ in 0..4 {
        app.handle_mouse(wheel(
            MouseEventKind::ScrollDown,
            follow.x + 3,
            follow.y + 3,
        ));
    }
    let all = text(&mut app);
    assert!(
        !all.contains("    1 + line 1") && all.contains("   13 + line 13"),
        "{all}"
    );
    for _ in 0..40 {
        app.handle_mouse(wheel(
            MouseEventKind::ScrollDown,
            follow.x + 3,
            follow.y + 3,
        ));
    }
    assert!(
        text(&mut app).contains("   60 + line 60"),
        "stops at the end"
    );
    app.handle_key(key(KeyCode::Left));
    assert_eq!(app.scroll, 0, "a follow key starts at the top again");

    app.open_file(&repo.0.join("src/main.rs"));
    frame(&mut app);
    for _ in 0..2 {
        app.handle_mouse(wheel(
            MouseEventKind::ScrollDown,
            follow.x + 3,
            follow.y + 3,
        ));
    }
    let all = text(&mut app);
    assert!(
        all.contains("    7  line 7") && !all.contains("    1  line 1"),
        "{all}"
    );
    assert_eq!(
        app.editor.as_ref().unwrap().line_col().0,
        0,
        "the caret stays"
    );
    app.handle_mouse(wheel(MouseEventKind::ScrollUp, follow.x + 3, follow.y + 3));
    assert!(text(&mut app).contains("    4  line 4"));
    // Typing brings the view back to the caret.
    app.handle_key(key(KeyCode::Char('x')));
    let all = text(&mut app);
    assert!(all.contains("    1  xline 1"), "{:?} {all}", app.focus);
}

#[test]
fn a_file_opened_from_the_tree_by_mouse_scrolls_both_ways_and_a_click_keeps_it_open() {
    use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
    let repo = Repo::new("yara-frame-wheel-tree");
    let body: String = (1..=80).map(|i| format!("line {i}\n")).collect();
    repo.file("src/main.rs", &body);
    let mut app = App::with_settings(
        Some(repo.0.clone()),
        Settings {
            show_sidebar: true,
            ..Settings::default()
        },
        Theme::default(),
    );
    app.refresh();
    let ev = |kind, x, y| MouseEvent {
        kind,
        column: x,
        row: y,
        modifiers: KeyModifiers::NONE,
    };
    frame(&mut app);
    let (row, _) = app.hits.file_rows[0];
    app.handle_mouse(ev(
        MouseEventKind::Down(MouseButton::Left),
        row.x + 3,
        row.y,
    ));
    frame(&mut app);
    let (row, _) = app.hits.file_rows[1];
    app.handle_mouse(ev(
        MouseEventKind::Down(MouseButton::Left),
        row.x + 3,
        row.y,
    ));
    assert!(app.editor.is_some());
    frame(&mut app);
    let follow = app.hits.follow;
    for _ in 0..60 {
        app.handle_mouse(ev(MouseEventKind::ScrollDown, follow.x + 5, follow.y + 5));
    }
    let all = text(&mut app);
    assert!(all.contains("line 80"), "{all}");
    for _ in 0..10 {
        app.handle_mouse(ev(MouseEventKind::ScrollUp, follow.x + 5, follow.y + 5));
    }
    let all = text(&mut app);
    assert!(!all.contains("line 80"), "scrolled back up: {all}");
    app.handle_mouse(ev(
        MouseEventKind::Down(MouseButton::Left),
        follow.x + 5,
        follow.y + 5,
    ));
    assert!(app.editor.is_some(), "a click in the editor keeps it open");
    assert_eq!(app.focus, Focus::Editor);
    // A click in the text puts the caret there: the fourth visible row, at
    // its third column; the view does not jump.
    frame(&mut app);
    let (editor, top) = (app.hits.editor, app.scroll as usize);
    app.handle_mouse(ev(
        MouseEventKind::Down(MouseButton::Left),
        editor.x + 3,
        editor.y + 3,
    ));
    assert_eq!(app.editor.as_ref().unwrap().line_col(), (top + 3, 3));
    frame(&mut app);
    assert_eq!(app.scroll as usize, top);
}

#[test]
fn a_drag_selects_text_and_ctrl_c_copies_it_without_the_gutter() {
    use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
    let repo = Repo::new("yara-frame-select");
    let mut app = App::with_settings(Some(repo.0.clone()), Settings::default(), Theme::default());
    app.focus = Focus::Follow;
    app.open_file(&repo.0.join("src/main.rs"));
    frame(&mut app);
    let ev = |kind, x, y| MouseEvent {
        kind,
        column: x,
        row: y,
        modifiers: KeyModifiers::NONE,
    };
    let e = app.hits.editor;
    app.handle_mouse(ev(MouseEventKind::Down(MouseButton::Left), e.x, e.y));
    app.handle_mouse(ev(
        MouseEventKind::Drag(MouseButton::Left),
        e.x + 30,
        e.y + 1,
    ));
    let painted = {
        let mut terminal = Terminal::new(TestBackend::new(100, 24)).unwrap();
        terminal.draw(|frame| ui::draw(frame, &mut app)).unwrap();
        terminal.backend().buffer()[(e.x + 2, e.y)].bg
    };
    assert_eq!(
        painted,
        ycode::theme::color(app.theme.ui.selected_bg),
        "lit"
    );
    assert_eq!(
        app.selected_text().as_deref(),
        Some("fn main() {\n    let x = 1;")
    );
    app.handle_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL));
    assert!(app.note.as_deref().unwrap().starts_with("copied 2 lines"));
    assert!(app.selection.is_none());
    // Typing drops a selection.
    app.handle_mouse(ev(MouseEventKind::Down(MouseButton::Left), e.x, e.y));
    app.handle_mouse(ev(MouseEventKind::Drag(MouseButton::Left), e.x + 3, e.y));
    app.handle_key(key(KeyCode::Right));
    assert!(app.selection.is_none());
}

#[test]
fn the_panes_move_to_the_other_side_from_the_palette_and_the_seam_drags_the_width() {
    use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
    let config =
        std::env::temp_dir().join(format!("yara-frame-swap-config-{}", std::process::id()));
    std::env::set_var("YARA_CONFIG_DIR", &config);
    let mut app = following(Settings::default());
    let rows = frame(&mut app);
    assert!(rows[1].find("AGENT").unwrap() < rows[1].find("FOLLOW").unwrap());
    app.execute(yara_core::command::Command::SwapPanes);
    let rows = frame(&mut app);
    assert!(rows[1].find("FOLLOW").unwrap() < rows[1].find("AGENT").unwrap());
    assert_eq!(app.settings.agent_side, Side::Right);
    app.execute(yara_core::command::Command::SwapPanes);

    let ev = |kind, x, y| MouseEvent {
        kind,
        column: x,
        row: y,
        modifiers: KeyModifiers::NONE,
    };
    frame(&mut app);
    let seam = app.hits.seam;
    assert_eq!(seam.width, 1);
    app.handle_mouse(ev(
        MouseEventKind::Down(MouseButton::Left),
        seam.x,
        seam.y + 3,
    ));
    app.handle_mouse(ev(MouseEventKind::Drag(MouseButton::Left), 60, seam.y + 3));
    app.handle_mouse(ev(MouseEventKind::Up(MouseButton::Left), 60, seam.y + 3));
    assert_eq!(app.settings.agent_width, 60);
    let rows = frame(&mut app);
    assert!(rows[1].find("FOLLOW").unwrap() >= 58, "{}", rows[1]);
    app.handle_mouse(ev(
        MouseEventKind::Down(MouseButton::Left),
        app.hits.seam.x,
        5,
    ));
    app.handle_mouse(ev(MouseEventKind::Drag(MouseButton::Left), 2, 5));
    assert_eq!(app.settings.agent_width, 20, "never narrower than a fifth");
    // The tree's seam sets the tree's width; the tree keeps a column of air.
    app.handle_key(KeyEvent::new(KeyCode::Char('b'), KeyModifiers::CONTROL));
    frame(&mut app);
    let tree_seam = app.hits.tree_seam;
    assert_eq!(tree_seam.width, 1);
    assert_eq!(tree_seam.x + 1, app.hits.files.x);
    app.handle_mouse(ev(MouseEventKind::Down(MouseButton::Left), tree_seam.x, 5));
    app.handle_mouse(ev(MouseEventKind::Drag(MouseButton::Left), 79, 5));
    app.handle_mouse(ev(MouseEventKind::Up(MouseButton::Left), 79, 5));
    assert_eq!(app.settings.sidebar_width, 20);
    std::env::remove_var("YARA_CONFIG_DIR");
    let _ = std::fs::remove_dir_all(&config);
}

#[test]
fn what_the_mouse_rests_on_lights_up_and_a_seam_shows_itself() {
    use crossterm::event::{MouseEvent, MouseEventKind};
    let repo = Repo::new("yara-frame-hover");
    let mut app = App::with_settings(
        Some(repo.0.clone()),
        Settings {
            show_sidebar: true,
            ..Settings::default()
        },
        Theme::default(),
    );
    frame(&mut app);
    let moved = |x, y| MouseEvent {
        kind: MouseEventKind::Moved,
        column: x,
        row: y,
        modifiers: KeyModifiers::NONE,
    };
    let (row, _) = app.hits.file_rows[0];
    app.handle_mouse(moved(row.x + 2, row.y));
    let mut terminal = Terminal::new(TestBackend::new(100, 24)).unwrap();
    terminal.draw(|frame| ui::draw(frame, &mut app)).unwrap();
    assert_eq!(
        terminal.backend().buffer()[(row.x + 2, row.y)].bg,
        ycode::theme::color(app.theme.ui.hover_bg)
    );
    let seam = app.hits.seam;
    app.handle_mouse(moved(seam.x, seam.y + 4));
    terminal.draw(|frame| ui::draw(frame, &mut app)).unwrap();
    assert_eq!(
        terminal.backend().buffer()[(seam.x, seam.y + 4)].symbol(),
        "┃"
    );
    assert_eq!(
        terminal.backend().buffer()[(seam.x, seam.y + 9)].symbol(),
        "┃",
        "the whole seam"
    );
}

#[test]
fn a_right_click_on_a_tab_renames_the_task_or_deletes_it() {
    use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
    let repo = Repo::new("yara-frame-tabmenu");
    let trees = repo.0.join("trees");
    let settings = Settings {
        worktrees_dir: trees.to_string_lossy().into_owned(),
        ..Settings::default()
    };
    let mut app = App::with_settings(Some(repo.0.clone()), settings, Theme::default());
    app.focus = Focus::Follow;
    assert_eq!(
        app.tasks.len(),
        1,
        "one workspace, whatever worktrees exist"
    );
    app.handle_key(key(KeyCode::F(7)));
    for c in "review".chars() {
        app.handle_key(key(KeyCode::Char(c)));
    }
    app.handle_key(key(KeyCode::Enter));
    assert_eq!(app.tasks.len(), 2);
    let rows = frame(&mut app);
    assert!(rows[0].contains(" main   review  [+]"), "{}", rows[0]);

    let (tab, _) = app.hits.tabs[1];
    let right = MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Right),
        column: tab.x + 1,
        row: tab.y,
        modifiers: KeyModifiers::NONE,
    };
    app.handle_mouse(right);
    let all = text(&mut app);
    assert!(
        all.contains("Rename…") && all.contains("Delete Task") && all.contains("Close"),
        "{all}"
    );
    app.handle_key(key(KeyCode::Enter));
    assert!(matches!(app.overlay, Some(Overlay::RenameTab(_))));
    for c in "pr 7".chars() {
        app.handle_key(key(KeyCode::Char(c)));
    }
    app.handle_key(key(KeyCode::Enter));
    assert!(frame(&mut app)[0].contains(" main   pr 7  [+]"));

    app.handle_mouse(right);
    app.handle_key(key(KeyCode::Down));
    app.handle_key(key(KeyCode::Enter));
    assert_eq!(app.tasks.len(), 1, "the task's tab is gone");
    let repo_name = repo.0.file_name().unwrap().to_string_lossy().into_owned();
    assert!(
        !trees.join(&repo_name).join("review").exists(),
        "and so is its worktree"
    );
    // The repository itself is not a worktree to delete.
    frame(&mut app);
    let (tab, _) = app.hits.tabs[0];
    app.handle_mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Right),
        column: tab.x + 1,
        row: tab.y,
        modifiers: KeyModifiers::NONE,
    });
    app.handle_key(key(KeyCode::Down));
    app.handle_key(key(KeyCode::Enter));
    assert_eq!(app.tasks.len(), 1);
    assert!(app.note.as_deref().unwrap().contains("no worktree"));
}

#[test]
fn a_task_holds_a_folder_of_every_repository_it_touches() {
    let backend = Repo::new("yara-frame-backend");
    let frontend = Repo::new("yara-frame-frontend");
    frontend.file("src/app.js", "export const app = 1;\n");
    frontend.git(&["add", "."]);
    frontend.git(&["commit", "-q", "-m", "front"]);
    let mut app = App::with_settings(
        Some(backend.0.clone()),
        Settings::default(),
        Theme::default(),
    );
    app.focus = Focus::Follow;
    // The walk starts beside the task's folder; type to narrow, Enter to
    // step in, Enter on the first row to add what the walk stands on.
    app.execute(yara_core::command::Command::AddFolder);
    assert!(matches!(app.overlay, Some(Overlay::AddFolder { .. })));
    let name = frontend
        .0
        .file_name()
        .unwrap()
        .to_string_lossy()
        .into_owned();
    for c in name.chars() {
        app.handle_key(key(KeyCode::Char(c)));
    }
    let all = text(&mut app);
    assert!(
        all.contains("ADD FOLDER") && all.contains(&format!("▸ {name}")),
        "{all}"
    );
    app.handle_key(key(KeyCode::Enter));
    app.handle_key(key(KeyCode::Up));
    app.handle_key(key(KeyCode::Up));
    app.handle_key(key(KeyCode::Enter));
    assert_eq!(app.folders.len(), 2, "{:?}", app.note);

    // An edit in either folder lands on the one timeline, named by its folder.
    backend.file("src/main.rs", "fn main() { serve() }\n");
    frontend.file("src/app.js", "export const app = 2;\n");
    app.refresh();
    assert_eq!(app.follow.len(), 2);
    let all = text(&mut app);
    let backend_name = backend
        .0
        .file_name()
        .unwrap()
        .to_string_lossy()
        .into_owned();
    assert!(
        all.contains(&format!("{backend_name}/src/main.rs")) || all.contains("src/app.js"),
        "{all}"
    );

    // CHANGES heads each folder with its branch and counts.
    app.handle_key(key(KeyCode::F(4)));
    let all = text(&mut app);
    assert!(
        all.contains(&backend_name) && all.contains("⎇ main"),
        "{all}"
    );
    assert!(all.contains("vs main · 2 files"), "{all}");
    app.handle_key(key(KeyCode::Esc));

    // The tree heads each folder too, and the finder reaches both.
    app.handle_key(KeyEvent::new(KeyCode::Char('b'), KeyModifiers::CONTROL));
    let all = text(&mut app);
    assert!(all.contains(&backend_name), "{all}");
    let hits = app.quick_open_hits("appjs");
    assert!(hits.iter().any(|h| h.ends_with("src/app.js")), "{hits:?}");
    app.handle_key(KeyEvent::new(KeyCode::Char('p'), KeyModifiers::CONTROL));
    for c in "appjs".chars() {
        app.handle_key(key(KeyCode::Char(c)));
    }
    app.handle_key(key(KeyCode::Enter));
    assert!(
        app.editor.as_ref().unwrap().path.starts_with(&frontend.0),
        "opened from the other folder"
    );
    // The status bar says how many folders the workspace holds.
    assert!(text(&mut app).contains("+1 folders"));
}

#[test]
fn closing_a_dirty_file_asks_first_and_so_does_quitting() {
    let repo = Repo::new("yara-frame-dirty");
    let mut app = App::with_settings(Some(repo.0.clone()), Settings::default(), Theme::default());
    app.open_file(&repo.0.join("src/main.rs"));
    app.handle_key(key(KeyCode::Char('q')));
    app.handle_key(key(KeyCode::Esc));
    let all = text(&mut app);
    assert!(
        all.contains("UNSAVED CHANGES") && all.contains("main.rs has unsaved changes"),
        "{all}"
    );
    assert!(app.editor.is_some());
    app.handle_key(key(KeyCode::Esc));
    assert!(app.overlay.is_none() && app.editor.is_some(), "stayed");
    app.handle_key(key(KeyCode::Esc));
    app.handle_key(key(KeyCode::Char('n')));
    assert!(app.editor.is_none(), "discarded");
    assert!(std::fs::read_to_string(repo.0.join("src/main.rs"))
        .unwrap()
        .starts_with("fn main"));

    app.open_file(&repo.0.join("src/main.rs"));
    app.handle_key(key(KeyCode::Char('q')));
    app.handle_key(KeyEvent::new(KeyCode::Char('q'), KeyModifiers::CONTROL));
    assert!(!app.should_quit && matches!(app.overlay, Some(Overlay::CloseFile { quit: true })));
    app.handle_key(key(KeyCode::Char('y')));
    assert!(app.should_quit);
    assert!(std::fs::read_to_string(repo.0.join("src/main.rs"))
        .unwrap()
        .starts_with("qfn main"));
}

#[test]
fn a_tab_dragged_over_another_takes_its_place() {
    use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
    let repo = Repo::new("yara-frame-drag-tab");
    let settings = Settings {
        worktrees_dir: repo.0.join("trees").to_string_lossy().into_owned(),
        ..Settings::default()
    };
    let mut app = App::with_settings(Some(repo.0.clone()), settings, Theme::default());
    app.focus = Focus::Follow;
    app.handle_key(key(KeyCode::F(7)));
    for c in "second".chars() {
        app.handle_key(key(KeyCode::Char(c)));
    }
    app.handle_key(key(KeyCode::Enter));
    assert!(frame(&mut app)[0].contains(" main   second  [+]"));
    let ev = |kind, x, y| MouseEvent {
        kind,
        column: x,
        row: y,
        modifiers: KeyModifiers::NONE,
    };
    let (first, _) = app.hits.tabs[0];
    let (second, _) = app.hits.tabs[1];
    app.handle_mouse(ev(
        MouseEventKind::Down(MouseButton::Left),
        first.x + 1,
        first.y,
    ));
    app.handle_mouse(ev(
        MouseEventKind::Drag(MouseButton::Left),
        second.x + 1,
        second.y,
    ));
    app.handle_mouse(ev(
        MouseEventKind::Up(MouseButton::Left),
        second.x + 1,
        second.y,
    ));
    assert!(
        frame(&mut app)[0].contains(" second   main  [+]"),
        "{}",
        frame(&mut app)[0]
    );
    assert_eq!(app.active, 1, "the dragged tab stays active");
    assert!(app.dragging_tab.is_none());
}

#[test]
fn a_task_watches_folders_that_are_no_repository_at_all() {
    let dir = std::env::temp_dir().join(format!("yara-frame-plain-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::write(dir.join("src/notes.md"), "one\n").unwrap();
    let dir = dir.canonicalize().unwrap();
    let mut app = App::with_settings(Some(dir.clone()), Settings::default(), Theme::default());
    app.focus = Focus::Follow;
    app.refresh();
    assert!(app.repo().is_none(), "no git here");
    std::fs::write(dir.join("src/notes.md"), "one\ntwo\n").unwrap();
    app.refresh();
    assert_eq!(app.follow.len(), 1);
    let all = text(&mut app);
    assert!(all.contains("src/notes.md +1 −0"), "{all}");
    assert!(all.contains("+ two"), "{all}");
    // A new task on the same folders is another agent with its own timeline.
    app.handle_key(key(KeyCode::F(7)));
    for c in "second look".chars() {
        app.handle_key(key(KeyCode::Char(c)));
    }
    app.handle_key(key(KeyCode::Enter));
    assert_eq!(app.tasks.len(), 2);
    assert_eq!(app.project(), Some(dir.as_path()));
    assert!(frame(&mut app)[0].contains(" second look  [+]"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_new_task_without_a_project_asks_for_a_folder() {
    let repo = Repo::new("yara-frame-newtask-empty");
    let mut app = App::with_settings(None, Settings::default(), Theme::default());
    app.handle_key(key(KeyCode::F(7)));
    assert!(
        text(&mut app).contains("ADD FOLDER"),
        "a task needs a folder first"
    );
    let config = std::env::temp_dir().join(format!("yara-newtask-config-{}", std::process::id()));
    std::env::set_var("YARA_CONFIG_DIR", &config);
    // Start the walk in the folder that holds the repository.
    app.overlay = Some(Overlay::AddFolder {
        dir: repo.0.parent().unwrap().to_path_buf(),
        row: 0,
        filter: String::new(),
    });
    let name = repo.0.file_name().unwrap().to_string_lossy().into_owned();
    for c in name.chars() {
        app.handle_key(key(KeyCode::Char(c)));
    }
    app.handle_key(key(KeyCode::Enter));
    app.handle_key(key(KeyCode::Up));
    app.handle_key(key(KeyCode::Up));
    app.handle_key(key(KeyCode::Enter));
    std::env::remove_var("YARA_CONFIG_DIR");
    let _ = std::fs::remove_dir_all(&config);
    assert_eq!(app.project(), Some(repo.0.as_path()));
    assert_eq!(
        app.tasks.len(),
        1,
        "it opened here rather than in a new tab"
    );
}

#[test]
fn settings_open_over_the_start_page() {
    let config = std::env::temp_dir().join(format!("yara-frame-settings-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&config);
    std::env::set_var("YARA_CONFIG_DIR", &config);
    let mut app = App::with_settings(None, Settings::default(), Theme::default());
    app.handle_key(key(KeyCode::F(12)));
    let all = text(&mut app);
    assert_eq!(app.focus, Focus::Editor);
    assert!(
        all.contains(" EDIT ") && all.contains("settings.json"),
        "{all}"
    );
    assert!(all.contains("\"agent\""), "the file itself: {all}");
    assert!(!all.contains("the terminal editor for the agent loop"));
    // Typing goes into the file, not to the start page's list.
    app.handle_key(key(KeyCode::Down));
    app.handle_key(key(KeyCode::Char('x')));
    assert!(app.editor.as_ref().unwrap().modified());
    app.handle_key(key(KeyCode::Esc));
    app.handle_key(key(KeyCode::Char('n')));
    assert!(text(&mut app).contains("the terminal editor for the agent loop"));
    std::env::remove_var("YARA_CONFIG_DIR");
    let _ = std::fs::remove_dir_all(&config);
}

#[test]
fn the_tree_makes_files_and_folders_from_a_right_click() {
    use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
    let repo = Repo::new("yara-frame-treemenu");
    let mut app = App::with_settings(
        Some(repo.0.clone()),
        Settings {
            show_sidebar: true,
            ..Settings::default()
        },
        Theme::default(),
    );
    app.refresh();
    frame(&mut app);
    let (row, _) = app.hits.file_rows[0]; // src
    let right = MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Right),
        column: row.x + 2,
        row: row.y,
        modifiers: KeyModifiers::NONE,
    };
    app.handle_mouse(right);
    let state = format!(
        "{:?} files={:?} rows={:?}",
        app.overlay,
        app.hits.files,
        app.hits.file_rows.first()
    );
    let all = text(&mut app);
    assert!(
        all.contains("New File") && all.contains("New Folder"),
        "{state}\n{all}"
    );
    app.handle_key(key(KeyCode::Enter));
    for c in "notes.md".chars() {
        app.handle_key(key(KeyCode::Char(c)));
    }
    app.handle_key(key(KeyCode::Enter));
    assert!(
        repo.0.join("src/notes.md").exists(),
        "made where the cursor was"
    );
    assert_eq!(app.focus, Focus::Editor);
    app.handle_key(key(KeyCode::Esc));

    app.handle_mouse(right);
    app.handle_key(key(KeyCode::Down));
    app.handle_key(key(KeyCode::Enter));
    assert!(text(&mut app).contains("NEW FOLDER"));
    for c in "assets".chars() {
        app.handle_key(key(KeyCode::Char(c)));
    }
    app.handle_key(key(KeyCode::Enter));
    assert!(repo.0.join("src/assets").is_dir());
    assert!(app.editor.is_none(), "a folder is not opened for editing");
}

#[test]
fn a_workspace_holds_the_folders_and_its_tasks_work_in_worktrees_of_them() {
    let backend = Repo::new("yara-frame-ws-backend");
    let frontend = Repo::new("yara-frame-ws-frontend");
    let trees = backend.0.join("trees");
    let settings = Settings {
        worktrees_dir: trees.to_string_lossy().into_owned(),
        ..Settings::default()
    };
    let config = std::env::temp_dir().join(format!("yara-ws-config-{}", std::process::id()));
    std::env::set_var("YARA_CONFIG_DIR", &config);
    let mut app = App::with_workspace(vec![backend.0.clone()], settings, Theme::default());
    app.focus = Focus::Follow;
    app.refresh();

    // A folder joins the workspace, and every task takes it up.
    app.overlay = Some(Overlay::AddFolder {
        dir: frontend.0.parent().unwrap().to_path_buf(),
        // The row the typing would have left the cursor on: the first
        // folder the filter kept.
        row: 2,
        filter: frontend
            .0
            .file_name()
            .unwrap()
            .to_string_lossy()
            .into_owned(),
    });
    app.handle_key(key(KeyCode::Enter));
    app.handle_key(key(KeyCode::Up));
    app.handle_key(key(KeyCode::Up));
    app.handle_key(key(KeyCode::Enter));
    assert_eq!(app.workspace.len(), 2, "{:?}", app.note);
    assert_eq!(app.folders.len(), 2, "the task works in both");

    // A task of its own gets a worktree of every repository in it.
    app.handle_key(key(KeyCode::F(7)));
    for c in "login flow".chars() {
        app.handle_key(key(KeyCode::Char(c)));
    }
    app.handle_key(key(KeyCode::Enter));
    assert_eq!(app.tasks.len(), 2);
    assert_eq!(app.folders.len(), 2);
    for folder in &app.folders {
        assert!(
            folder.path.starts_with(&trees),
            "{} {:?}",
            folder.path.display(),
            app.note
        );
        assert_eq!(folder.repo.as_ref().unwrap().branch, "login-flow");
    }
    assert_eq!(
        app.tasks[0].folders[0].path, backend.0,
        "the first task stays put"
    );

    // Taking a folder out of the workspace takes it from every task.
    app.execute(yara_core::command::Command::RemoveFolder);
    let all = text(&mut app);
    assert!(all.contains("WORKSPACE FOLDERS"), "{all}");
    app.handle_key(key(KeyCode::Down));
    app.handle_key(key(KeyCode::Enter));
    assert_eq!(app.workspace.len(), 1);
    assert_eq!(app.folders.len(), 1);
    assert_eq!(app.tasks[0].folders.len(), 1);
    // The workspace is what the recent list remembers.
    assert_eq!(app.settings.recent_workspaces[0], app.workspace);
    std::env::remove_var("YARA_CONFIG_DIR");
    let _ = std::fs::remove_dir_all(&config);
}

//! The frame, read back off ratatui's test backend: the same `App` and `draw`
//! the terminal runs, with keys fed in and the drawn text asserted on.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::backend::TestBackend;
use ratatui::Terminal;
use yara_core::follow::EditEvent;
use yara_core::settings::{Settings, Side};
use yara_core::theme::Theme;
use yara_tui::app::{App, Focus};
use yara_tui::ui;

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
    let mut app = App::with_settings(None, settings, Theme::default());
    app.focus = Focus::Follow;
    app
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
        rows[0].contains("YARA") && rows[0].contains("/work/demo"),
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
    assert!(rows[23].contains("F6 pane  ^⇧G changes  ^B files  ^⇧P palette  ^⇧F search  F1 keys"));
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
    let mut app = App::with_settings(None, settings, Theme::default());
    let rows = frame(&mut app);
    assert!(rows[1].contains("FILES"));
    assert!(rows[1].contains("AGENT · codex"));
    assert!(rows[1].find("FOLLOW").unwrap() < rows[1].find("AGENT").unwrap());
    assert!(rows[21].contains("^B hide · ⏎ open"));
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
    let mut app = App::with_settings(None, settings, Theme::default());
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
    // Ctrl+Shift+G is the editor's even with the agent focused.
    app.handle_key(KeyEvent::new(
        KeyCode::Char('g'),
        KeyModifiers::CONTROL | KeyModifiers::SHIFT,
    ));
    assert!(app.overlay.is_some(), "CHANGES opened");
}

#[test]
fn an_agent_that_cannot_start_is_a_note_not_a_crash() {
    let settings = Settings {
        agent: "".into(),
        ..Settings::default()
    };
    let mut app = App::with_settings(None, settings, Theme::default());
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
fn a_command_not_built_yet_says_so_in_the_status_bar() {
    let mut app = following(Settings::default());
    app.handle_key(key(KeyCode::F(1)));
    assert!(text(&mut app).contains("Key Bindings is not here yet"));
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
    assert!(all.contains("src/main.rs +1 −1"));
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
    app.handle_key(KeyEvent::new(
        KeyCode::Char('g'),
        KeyModifiers::CONTROL | KeyModifiers::SHIFT,
    ));
    let all = text(&mut app);
    assert!(all.contains("CHANGES"));
    assert!(all.contains(" A README.md"), "{all}");
    assert!(all.contains(" M src/main.rs"));
    assert!(all.contains("git status vs main · 2 files · +2 −3 · ⏎ open diff · Esc close"));

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

    app.handle_key(KeyEvent::new(
        KeyCode::Char('g'),
        KeyModifiers::CONTROL | KeyModifiers::SHIFT,
    ));
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

    let ctrl_shift = |c| {
        KeyEvent::new(
            KeyCode::Char(c),
            KeyModifiers::CONTROL | KeyModifiers::SHIFT,
        )
    };
    app.handle_key(ctrl_shift('n'));
    assert!(text(&mut app).contains("NEW AGENT"));
    for c in "task/login".chars() {
        app.handle_key(key(KeyCode::Char(c)));
    }
    app.handle_key(key(KeyCode::Backspace));
    app.handle_key(key(KeyCode::Char('n')));
    assert!(text(&mut app).contains("task/login█"));
    app.handle_key(key(KeyCode::Enter));
    assert_eq!(app.sessions.len(), 2);
    assert_eq!(app.active, 1);
    let rows = frame(&mut app);
    assert!(rows[0].contains("⌥ worktree: task-login"), "{}", rows[0]);
    assert!(rows[0].contains(" main   task/login  [+]"), "{}", rows[0]);
    assert!(rows[0].contains("● cat — running"), "an agent of its own");
    assert!(repo.0.join("trees/task-login/src/main.rs").exists());

    // Each tab keeps its own timeline: an edit in the worktree lands only
    // in the tab that watches it.
    std::fs::write(
        repo.0.join("trees/task-login/src/main.rs"),
        "fn main() {}\n",
    )
    .unwrap();
    app.refresh();
    assert_eq!(app.follow.len(), 1);
    assert_eq!(app.sessions[0].follow.len(), 0);

    app.handle_key(key(KeyCode::F(2)));
    for c in "PR 42".chars() {
        app.handle_key(key(KeyCode::Char(c)));
    }
    app.handle_key(key(KeyCode::Enter));
    assert!(frame(&mut app)[0].contains(" main   PR 42  [+]"));

    app.handle_key(KeyEvent::new(KeyCode::PageUp, KeyModifiers::CONTROL));
    assert_eq!(app.active, 0);
    app.handle_key(KeyEvent::new(KeyCode::PageDown, KeyModifiers::CONTROL));
    assert_eq!(app.active, 1);
    app.handle_key(ctrl_shift('w'));
    assert_eq!(app.sessions.len(), 1);
    assert!(!app.should_quit);
    app.handle_key(ctrl_shift('w'));
    assert!(app.should_quit, "closing the last tab is quitting");
}

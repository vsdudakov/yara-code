//! Draws the documentation's screenshots: every scene is the real `App`
//! painted onto ratatui's test backend, written out as SVG — text, colours
//! and all, in the theme the editor ships with. `make shots` runs it.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use ratatui::style::{Color, Modifier};
use ratatui::Terminal;
use yara_core::settings::Settings;
use yara_core::theme::Theme;
use ycode::app::{App, Focus};
use ycode::ui;

const COLS: u16 = 120;
const ROWS: u16 = 34;

fn main() {
    let out = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| "docs/assets/shots".into());
    std::fs::create_dir_all(&out).unwrap();
    let repo = Repo::new();
    // A stand-in agent: a script called `claude` that prints a transcript
    // and waits, so the pane shows what a session looks like.
    let transcript = repo.file(
        "../.transcript",
        "\x1b[1m> fix the login redirect\x1b[0m\n\n\
         I'll look at how the session is checked first.\n\n\
         \x1b[38;5;180m● Read src/auth.rs\x1b[0m\n\
         \x1b[38;5;180m✳ Edit src/auth.rs (+5 −1)\x1b[0m\n\
         \x1b[38;5;180m✳ Edit src/routes.rs (+2 −0)\x1b[0m\n\n\
         The redirect now keeps the original path. Running the tests.\n\n\
         \x1b[38;5;108m✓ 14 tests passed\x1b[0m\n\n> \n",
    );
    let script = repo.root.parent().unwrap().join("claude");
    std::fs::write(
        &script,
        format!("#!/bin/sh\ncat {}\nsleep 60\n", transcript.display()),
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    let agent = script.display().to_string();

    // The start page, with a couple of projects remembered.
    let mut settings = Settings::default();
    settings.push_recent(Path::new("/Users/you/code/checkout"));
    settings.push_recent(Path::new("/Users/you/code/yara-code"));
    let mut app = App::with_settings(None, settings.clone(), Theme::default());
    shot(&out, "start", &mut app);

    // The main loop, with the agent's transcript in its pane and two edits.
    settings.agent = agent;
    let mut app = App::with_settings(Some(repo.root.clone()), settings.clone(), Theme::default());
    app.start_agent();
    wait_for_agent(&mut app, "tests passed");
    app.refresh();
    repo.file(
        "src/auth.rs",
        "pub fn check(session: &Session, path: &str) -> Redirect {\n    if session.expired() {\n        return Redirect::login_then(path);\n    }\n    if !session.valid() {\n        return Redirect::login();\n    }\n    Redirect::none()\n}\n",
    );
    app.refresh();
    repo.file(
        "src/routes.rs",
        "pub fn routes() -> Router {\n    Router::new()\n        .get(\"/login\", login)\n        .get(\"/logout\", logout)\n}\n",
    );
    app.refresh();
    app.focus = Focus::Follow;
    shot(&out, "hero", &mut app);

    app.handle_key(plain(KeyCode::Left));
    shot(&out, "paused", &mut app);
    app.handle_key(plain(KeyCode::Enter));
    app.handle_key(plain(KeyCode::Char('v')));
    shot(&out, "file-view", &mut app);
    app.handle_key(plain(KeyCode::Char('v')));

    app.handle_key(ctrl_shift('g'));
    shot(&out, "changes", &mut app);
    app.handle_key(plain(KeyCode::Esc));

    app.handle_key(ctrl_shift('p'));
    for c in "fol".chars() {
        app.handle_key(plain(KeyCode::Char(c)));
    }
    shot(&out, "palette", &mut app);
    app.handle_key(plain(KeyCode::Esc));

    app.handle_key(ctrl_shift('f'));
    for c in "redirect".chars() {
        app.handle_key(plain(KeyCode::Char(c)));
    }
    shot(&out, "search", &mut app);
    app.handle_key(plain(KeyCode::Esc));

    app.handle_key(plain(KeyCode::F(1)));
    shot(&out, "keys", &mut app);
    app.handle_key(plain(KeyCode::Esc));

    app.handle_key(plain(KeyCode::F(10)));
    shot(&out, "menu", &mut app);
    app.handle_key(plain(KeyCode::Esc));

    app.handle_key(ctrl('b'));
    app.handle_key(plain(KeyCode::Enter));
    app.handle_key(plain(KeyCode::Down));
    app.handle_key(plain(KeyCode::Enter));
    shot(&out, "edit", &mut app);
    app.handle_key(plain(KeyCode::Esc));
    app.handle_key(ctrl('b'));

    // A second agent in a worktree of its own.
    app.handle_key(ctrl_shift('n'));
    for c in "logout flow".chars() {
        app.handle_key(plain(KeyCode::Char(c)));
    }
    shot(&out, "new-tab", &mut app);
    app.handle_key(plain(KeyCode::Enter));
    wait_for_agent(&mut app, "tests passed");
    app.focus = Focus::Agent;
    shot(&out, "tabs", &mut app);

    // What the agents have used of their plans.
    app.settings.usage_commands = [
        ("claude", r#"echo '{"plan":"Max","percent":62,"detail":"1.2M tokens · 340 requests","reset":"resets in 3h 20m"}'"#),
        ("codex", r#"echo '{"plan":"Pro","percent":18,"detail":"210k tokens","reset":"resets tomorrow"}'"#),
    ]
    .into_iter()
    .map(|(a, c)| (a.to_string(), c.to_string()))
    .collect();
    app.execute(yara_core::command::Command::AgentUsage);
    let start = Instant::now();
    while app.usage.is_none() && start.elapsed() < Duration::from_secs(5) {
        std::thread::sleep(Duration::from_millis(20));
        app.collect();
    }
    shot(&out, "usage", &mut app);
}

fn plain(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

fn ctrl(c: char) -> KeyEvent {
    KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL)
}

fn ctrl_shift(c: char) -> KeyEvent {
    KeyEvent::new(
        KeyCode::Char(c),
        KeyModifiers::CONTROL | KeyModifiers::SHIFT,
    )
}

/// Waits until the agent's screen shows `text`, so a shot never catches the
/// transcript half-printed.
fn wait_for_agent(app: &mut App, text: &str) {
    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(5) {
        let screen = app
            .agent
            .as_ref()
            .map(|pty| pty.with_screen(|s| s.contents()))
            .unwrap_or_default();
        if screen.contains(text) {
            return;
        }
        std::thread::sleep(Duration::from_millis(30));
    }
}

fn shot(out: &Path, name: &str, app: &mut App) {
    let mut terminal = Terminal::new(TestBackend::new(COLS, ROWS)).unwrap();
    // Twice: the first frame sizes the agent's grid, the second paints it.
    terminal.draw(|frame| ui::draw(frame, app)).unwrap();
    std::thread::sleep(Duration::from_millis(80));
    terminal.draw(|frame| ui::draw(frame, app)).unwrap();
    let svg = svg(terminal.backend().buffer(), &app.theme);
    std::fs::write(out.join(format!("{name}.svg")), svg).unwrap();
    eprintln!("wrote {name}.svg");
}

/// The frame as SVG: one rectangle per run of background, one text element
/// per run of foreground, in a monospace stack every browser has.
fn svg(buffer: &Buffer, theme: &Theme) -> String {
    const CW: f32 = 8.4;
    const CH: f32 = 18.0;
    let (w, h) = (buffer.area.width, buffer.area.height);
    let bg = theme.ui.bg;
    let mut out = String::new();
    let _ = write!(
        out,
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{}" height="{}" viewBox="0 0 {0} {1}" font-family="SF Mono, Menlo, Consolas, 'DejaVu Sans Mono', monospace" font-size="14">"#,
        w as f32 * CW + 16.0,
        h as f32 * CH + 16.0
    );
    let _ = write!(
        out,
        r#"<rect width="100%" height="100%" rx="8" fill="{}"/>"#,
        hex(bg)
    );
    for y in 0..h {
        // Backgrounds first, as runs.
        let mut x = 0;
        while x < w {
            let cell = &buffer[(x, y)];
            let colour = cell.bg;
            let start = x;
            while x < w && buffer[(x, y)].bg == colour {
                x += 1;
            }
            if let Some(c) = rgb(colour) {
                if c != bg {
                    let _ = write!(
                        out,
                        r#"<rect x="{}" y="{}" width="{}" height="{CH}" fill="{}"/>"#,
                        8.0 + start as f32 * CW,
                        8.0 + y as f32 * CH,
                        (x - start) as f32 * CW,
                        hex(c)
                    );
                }
            }
        }
        // Then the text, as runs of one style.
        let mut x = 0;
        while x < w {
            let cell = &buffer[(x, y)];
            let (fg_c, mods) = (cell.fg, cell.modifier);
            let start = x;
            let mut text = String::new();
            while x < w && buffer[(x, y)].fg == fg_c && buffer[(x, y)].modifier == mods {
                text.push_str(buffer[(x, y)].symbol());
                x += 1;
            }
            if text.trim().is_empty() {
                continue;
            }
            let colour = rgb(fg_c).unwrap_or(theme.ui.fg);
            let weight = if mods.contains(Modifier::BOLD) {
                " font-weight=\"bold\""
            } else {
                ""
            };
            let style = if mods.contains(Modifier::ITALIC) {
                " font-style=\"italic\""
            } else {
                ""
            };
            let _ = write!(
                out,
                r#"<text x="{}" y="{}" fill="{}" xml:space="preserve"{weight}{style}>{}</text>"#,
                8.0 + start as f32 * CW,
                8.0 + y as f32 * CH + 13.5,
                hex(colour),
                escape(&text)
            );
        }
    }
    out.push_str("</svg>\n");
    out
}

fn rgb(colour: Color) -> Option<(u8, u8, u8)> {
    match colour {
        Color::Rgb(r, g, b) => Some((r, g, b)),
        _ => None,
    }
}

fn hex(c: (u8, u8, u8)) -> String {
    format!("#{:02x}{:02x}{:02x}", c.0, c.1, c.2)
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// A repository with one commit on main, removed when the run ends.
struct Repo {
    root: PathBuf,
}

impl Repo {
    fn new() -> Self {
        let root = std::env::temp_dir()
            .join(format!("ycode-shots-{}", std::process::id()))
            .join("checkout");
        let _ = std::fs::remove_dir_all(root.parent().unwrap());
        std::fs::create_dir_all(&root).unwrap();
        let root = root.canonicalize().unwrap();
        let repo = Self { root };
        for args in [
            vec!["init", "-q", "-b", "main"],
            vec!["config", "user.email", "you@example.com"],
            vec!["config", "user.name", "you"],
        ] {
            repo.git(&args);
        }
        repo.file(
            "src/auth.rs",
            "pub fn check(session: &Session, path: &str) -> Redirect {\n    if !session.valid() {\n        return Redirect::login();\n    }\n    Redirect::none()\n}\n",
        );
        repo.file(
            "src/routes.rs",
            "pub fn routes() -> Router {\n    Router::new()\n        .get(\"/login\", login)\n}\n",
        );
        repo.file(
            "src/main.rs",
            "mod auth;\nmod routes;\n\nfn main() {\n    serve(routes::routes());\n}\n",
        );
        repo.file(
            "README.md",
            "# checkout\n\nThe login flow, and the tests for it.\n",
        );
        repo.file(
            "Cargo.toml",
            "[package]\nname = \"checkout\"\nversion = \"0.4.1\"\n",
        );
        repo.file(".gitignore", ".transcript\ntarget\n");
        repo.git(&["add", "."]);
        repo.git(&["commit", "-q", "-m", "first"]);
        repo.git(&["checkout", "-q", "-b", "feature/login-redirect"]);
        repo
    }

    fn git(&self, args: &[&str]) {
        let status = std::process::Command::new("git")
            .arg("-C")
            .arg(&self.root)
            .args(args)
            .status()
            .unwrap();
        assert!(status.success(), "git {args:?}");
    }

    fn file(&self, name: &str, body: &str) -> PathBuf {
        let path = self.root.join(name);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, body).unwrap();
        path
    }
}

impl Drop for Repo {
    fn drop(&mut self) {
        // The repository, its worktrees and the stand-in agent all live in
        // the one folder.
        let _ = std::fs::remove_dir_all(self.root.parent().unwrap());
    }
}

//! Draws the documentation's screenshots: every scene is the real `App`
//! painted onto ratatui's test backend and written out as SVG — text,
//! colours and all, in the theme the editor ships with. `make shots` runs
//! it, from a bench of two repositories and a stand-in agent.

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
use ycode::app::{App, Focus, Overlay};
use ycode::ui;

const COLS: u16 = 120;
const ROWS: u16 = 34;

fn main() {
    let out = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| "docs/assets/shots".into());
    std::fs::create_dir_all(&out).unwrap();
    let bench = Bench::new();

    // The start page, with a workspace or two remembered.
    let mut settings = Settings::default();
    settings.push_recent(&[PathBuf::from("/Users/you/code/checkout")]);
    settings.push_recent(&[
        PathBuf::from("/Users/you/code/orders"),
        PathBuf::from("/Users/you/code/orders-web"),
    ]);
    let mut app = App::with_workspace(Vec::new(), settings.clone(), Theme::default());
    shot(&out, "start", &mut app);

    // The loop itself: the agent's session, and the diff of its last edit.
    settings.agent = bench.agent.display().to_string();
    settings.shell = "sh".into();
    let mut app = App::with_workspace(vec![bench.root.clone()], settings, Theme::default());
    app.start_agent();
    wait_for_agent(&mut app, "tests passed");
    app.refresh();
    bench.write(
        "backend/src/auth.rs",
        "pub fn check(session: &Session, path: &str) -> Redirect {\n    if session.expired() {\n        return Redirect::login_then(path);\n    }\n    if !session.valid() {\n        return Redirect::login();\n    }\n    Redirect::none()\n}\n",
    );
    app.refresh();
    bench.write(
        "frontend/src/login.js",
        "export function login(next) {\n  const to = next ?? location.pathname;\n  return post(\"/login\", { to });\n}\n",
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
    app.handle_key(plain(KeyCode::Char('f')));

    // CHANGES, headed by each repository of the bench.
    app.handle_key(plain(KeyCode::F(4)));
    shot(&out, "changes", &mut app);
    app.handle_key(plain(KeyCode::Esc));

    // A second task, in worktrees of its own.
    app.handle_key(plain(KeyCode::F(7)));
    for c in "logout flow".chars() {
        app.handle_key(plain(KeyCode::Char(c)));
    }
    shot(&out, "new-task", &mut app);
    app.handle_key(plain(KeyCode::Enter));
    wait_for_agent(&mut app, "tests passed");
    app.focus = Focus::Agent;
    shot(&out, "tasks", &mut app);
    // Back to the first task, which has the work in it.
    app.handle_key(ctrl('k'));

    // The shell under the agent.
    app.handle_key(ctrl('t'));
    if let Some(pty) = app.terminal.as_mut() {
        pty.write(b"cd backend && git status --short && git log --oneline -3\n");
    }
    std::thread::sleep(Duration::from_millis(600));
    shot(&out, "terminal", &mut app);
    app.handle_key(ctrl('t'));

    // The tree, and a file open in the follow pane's place.
    app.handle_key(ctrl('b'));
    app.handle_key(plain(KeyCode::Enter));
    app.handle_key(ctrl('p'));
    for c in "authrs".chars() {
        app.handle_key(plain(KeyCode::Char(c)));
    }
    app.handle_key(plain(KeyCode::Enter));
    app.handle_key(plain(KeyCode::Down));
    app.handle_key(plain(KeyCode::Down));
    app.handle_key(plain(KeyCode::Right));
    shot(&out, "edit", &mut app);
    app.handle_key(plain(KeyCode::Esc));
    app.handle_key(ctrl('b'));

    // The overlays.
    app.handle_key(plain(KeyCode::F(5)));
    for c in "fol".chars() {
        app.handle_key(plain(KeyCode::Char(c)));
    }
    shot(&out, "palette", &mut app);
    app.handle_key(plain(KeyCode::Esc));

    app.handle_key(plain(KeyCode::F(3)));
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

    app.overlay = Some(Overlay::AddFolder {
        dir: bench.root.clone(),
        row: 2,
        filter: String::new(),
    });
    shot(&out, "add-folder", &mut app);
    app.handle_key(plain(KeyCode::Esc));

    // What the agents have used of their plans.
    app.settings.usage_commands = [
        (
            "claude",
            r#"echo '{"plan":"Max","percent":62,"detail":"1.2M tokens · 340 requests","reset":"resets in 3h 20m"}'"#,
        ),
        (
            "codex",
            r#"echo '{"plan":"Pro","percent":18,"detail":"210k tokens","reset":"resets tomorrow"}'"#,
        ),
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
    std::thread::sleep(Duration::from_millis(120));
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
            let colour = buffer[(x, y)].bg;
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
            let (fg_c, mods) = (buffer[(x, y)].fg, buffer[(x, y)].modifier);
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

/// A bench: one folder holding two repositories and the stand-in agent,
/// removed when the run ends.
struct Bench {
    root: PathBuf,
    agent: PathBuf,
}

impl Bench {
    fn new() -> Self {
        let base = std::env::temp_dir().join(format!("ycode-shots-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let root = base.join("checkout");
        std::fs::create_dir_all(&root).unwrap();
        let root = yara_core::git::canonical(&root);
        let bench = Self {
            agent: base.join("claude"),
            root,
        };
        bench.repo(
            "backend",
            &[
                ("src/auth.rs", "pub fn check(session: &Session, path: &str) -> Redirect {\n    if !session.valid() {\n        return Redirect::login();\n    }\n    Redirect::none()\n}\n"),
                ("src/routes.rs", "pub fn routes() -> Router {\n    Router::new().get(\"/login\", login)\n}\n"),
                ("Cargo.toml", "[package]\nname = \"backend\"\nversion = \"0.4.1\"\n"),
            ],
        );
        bench.repo(
            "frontend",
            &[
                (
                    "src/login.js",
                    "export function login() {\n  return post(\"/login\", {});\n}\n",
                ),
                (
                    "package.json",
                    "{ \"name\": \"frontend\", \"version\": \"0.4.1\" }\n",
                ),
            ],
        );
        bench.write(
            "README.md",
            "# checkout\n\nThe login flow, backend and front.\n",
        );
        bench.write("up.sh", "#!/bin/sh\ndocker compose up -d\n");

        // The agent: a script that prints a session and waits.
        let transcript = base.join("transcript");
        std::fs::write(
            &transcript,
            "\x1b[1m> fix the login redirect\x1b[0m\n\n\
             I'll look at how the session is checked first.\n\n\
             \x1b[38;5;110m● Read backend/src/auth.rs\x1b[0m\n\
             \x1b[38;5;110m✳ Edit backend/src/auth.rs (+5 −1)\x1b[0m\n\
             \x1b[38;5;110m✳ Edit frontend/src/login.js (+3 −1)\x1b[0m\n\n\
             The redirect keeps the original path now, and the form sends it.\n\n\
             \x1b[38;5;108m✓ 14 tests passed\x1b[0m\n\n> \n",
        )
        .unwrap();
        std::fs::write(
            &bench.agent,
            format!("#!/bin/sh\ncat {}\nsleep 120\n", transcript.display()),
        )
        .unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&bench.agent, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        bench
    }

    fn repo(&self, name: &str, files: &[(&str, &str)]) {
        let path = self.root.join(name);
        std::fs::create_dir_all(&path).unwrap();
        for args in [
            vec!["init", "-q", "-b", "main"],
            vec!["config", "user.email", "you@example.com"],
            vec!["config", "user.name", "you"],
        ] {
            git(&path, &args);
        }
        for (file, body) in files {
            self.write(&format!("{name}/{file}"), body);
        }
        git(&path, &["add", "."]);
        git(&path, &["commit", "-q", "-m", "first"]);
        git(&path, &["checkout", "-q", "-b", "feature/login-redirect"]);
    }

    fn write(&self, name: &str, body: &str) {
        let path = self.root.join(name);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, body).unwrap();
    }
}

fn git(dir: &Path, args: &[&str]) {
    let status = std::process::Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .status()
        .unwrap();
    assert!(status.success(), "git {args:?}");
}

impl Drop for Bench {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(self.root.parent().unwrap());
    }
}

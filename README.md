<div align="center">

# Yara Code

**A lightweight code editor for agent-driven development — a terminal UI and a
GPU window that mirror each other, in one small Rust binary.**

[![CI](https://github.com/vsdudakov/yara-code/actions/workflows/ci.yml/badge.svg)](https://github.com/vsdudakov/yara-code/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/vsdudakov/yara-code?sort=semver)](https://github.com/vsdudakov/yara-code/releases)
[![Docs](https://img.shields.io/badge/docs-vsdudakov.github.io%2Fyara--code-blue.svg)](https://vsdudakov.github.io/yara-code/)
[![Coverage](https://img.shields.io/badge/coverage-81%25-green.svg)](#development)
[![Rust](https://img.shields.io/badge/rust-stable-orange.svg)](https://www.rust-lang.org/)
[![License: MIT](https://img.shields.io/badge/license-MIT-green.svg)](LICENSE)

📖 **Documentation: [vsdudakov.github.io/yara-code](https://vsdudakov.github.io/yara-code/)**

![Yara Code, the terminal frontend](assets/tui.gif)

</div>

---

## Why Yara Code exists

You write code with an agent now — **Claude Code**, **Cursor CLI**, **Codex
CLI**, **Aider** — and the agent lives in a terminal. What you actually do all
day is *read*: open the files it touched, read the diff, fix a line it got
wrong, run the tests. A full IDE is the wrong tool for that job — it spends its
startup time, its memory and its screen on a language server, a debugger, a
marketplace and a git GUI you no longer use.

Yara Code is the small editor for that loop. It **views files and diffs, edits a few
lines, and stays out of the way** — with the agent right there in the built-in
terminal.

- **No LSP.** No indexing, no background language servers, no "loading
  workspace". Go-to-definition is a fast keyword jump; your agent knows the
  codebase better than an index would.
- **No git UI to learn.** Status, diffs and blame are *shown*; every git command
  you actually run, you run in the terminal, where you already run them.
- **Nothing else.** No plugin marketplace, no debugger, no telemetry, no
  Electron. One Rust crate, ~15k lines, a single binary, ~40 ms to a drawn
  frame.
- **Two frontends that mirror each other.** `ycode` is a terminal UI that runs
  over SSH; `ycode-gui` is the same editor drawn by the GPU. Same panes, same
  menus, same keys — switch between a remote box and your desktop without
  changing habits.
- **Three themes, on purpose.** Dark+, Light+ and Monokai ship built in; any
  VS Code theme JSON works if you want another. Syntax highlighting for 75+
  languages through syntect.

If you want a lightweight **alternative to VS Code and Zed** for reviewing what
an AI agent wrote — and nothing more — that is exactly what this is.

## Install

```bash
brew install vsdudakov/tap/ycode   # installs both commands at once

ycode ~/code/project               # the terminal editor
ycode-gui ~/code/project           # the same editor, in a window
```

Every install — Homebrew, `.deb`, `.rpm`, the AUR package, the plain archives —
puts **both** `ycode` and `ycode-gui` on your `PATH`. The editor is called
Yara Code; the commands are `ycode`, because `yara` already belongs to VirusTotal's
malware scanner in every package manager.

On Linux, every release also carries a `.deb`, an `.rpm` and a `PKGBUILD`:

```bash
sudo apt install ./ycode_X.Y.Z-1_amd64.deb   # Debian, Ubuntu, Mint
sudo dnf install ./ycode-X.Y.Z-1.x86_64.rpm  # Fedora, RHEL, openSUSE
makepkg -si                                  # Arch, from the release PKGBUILD
```

Plain binaries for macOS (Apple Silicon and Intel), Linux x86_64 and Windows x64
are on the [releases page](https://github.com/vsdudakov/yara-code/releases/latest);
`cargo build --release` builds both from source. On a server, build the terminal
frontend alone — it pulls no graphics stack at all:

```bash
cargo build --release --no-default-features --features tui
```

## What it does

| | |
| --- | --- |
| 📂 **Read what the agent changed** | Changed files tinted in the navigator, changed lines marked in the gutter, a **side-by-side diff** in a tab of its own, blame for the line under the cursor — commit, author, age, and the pull request its message names. |
| ✏️ **Fix a line, not a project** | Tabs, smart indent, folding with sticky scroll, undo/redo over everything (including Replace All), find & replace in the file and across the project. |
| 🖥 **The agent stays in view** | A real login shell on a pseudo-terminal in both frontends — tab completion, full-screen programs, 5000 lines of scrollback, named and reorderable session tabs. |
| 🗂 **Several folders, one window** | "Add Folder to Project" — search, go-to-definition and the navigator treat them as one project. |
| 🎨 **Themes and syntax** | Three built-in themes; any VS Code color theme JSON; 75 syntect grammars plus bundled TypeScript, TOML, Kotlin, Swift, Dart, Dockerfile, Protobuf and GraphQL. |
| ⌨️ **Keys you already know** | VS Code bindings, `Ctrl` for `Cmd` in the terminal, every one of them rebindable in `settings.json`. |

## Usage

```bash
ycode                      # opens with no project — the start page lists the keys
ycode ~/code/project       # opens that folder
ycode-gui ~/code/project   # the window
```

| Action | Terminal | Window |
| --- | --- | --- |
| Save / Save As… | `Ctrl+S` / `Ctrl+Shift+S` | `Cmd+S` / `Cmd+Shift+S` |
| Find in file / Search project | `Ctrl+F` / `Ctrl+Shift+F` | `Cmd+F` / `Cmd+Shift+F` |
| Git panel / Terminal panel | `Ctrl+Shift+G` / `Ctrl+J` | `Ctrl+Shift+G` / `Cmd+J` |
| Undo / Redo | `Ctrl+Z` / `Ctrl+Shift+Z` | `Cmd+Z` / `Cmd+Shift+Z` |
| Key bindings overlay | `F1` | `F1` |

The full table is in the [key bindings guide](https://vsdudakov.github.io/yara-code/guides/keys/).

## Documentation

| Page | What it covers |
| --- | --- |
| [Installation](https://vsdudakov.github.io/yara-code/getting-started/installation/) | Homebrew, binaries, building from source |
| [First run](https://vsdudakov.github.io/yara-code/getting-started/first-run/) | The start page, opening a folder, the panes |
| [Project folders](https://vsdudakov.github.io/yara-code/guides/project-folders/) | Several folders in one window |
| [Editing](https://vsdudakov.github.io/yara-code/guides/editing/) | Tabs, folding, smart indent, undo |
| [Search & replace](https://vsdudakov.github.io/yara-code/guides/search/) | Project search, find in file, excludes |
| [Git](https://vsdudakov.github.io/yara-code/guides/git/) | Status, diffs, gutter marks, blame |
| [Terminal](https://vsdudakov.github.io/yara-code/guides/terminal/) | The integrated shell and its tabs |
| [Themes & syntax](https://vsdudakov.github.io/yara-code/guides/themes/) | VS Code themes and grammars |
| [Key bindings](https://vsdudakov.github.io/yara-code/guides/keys/) | Every default, and how to rebind |
| [Settings](https://vsdudakov.github.io/yara-code/guides/settings/) | `settings.json`, field by field |
| [Architecture](https://vsdudakov.github.io/yara-code/architecture/) | How one core drives two frontends |

## Development

```bash
make lint      # cargo fmt --check + clippy -D warnings — the CI gate
make test      # cargo test --all-features
make coverage  # cargo llvm-cov, with the 90% gate on src/core
make build     # release build of both binaries
make run ARGS=~/code/project
```

**Coverage.** 251 tests, **81% of the crate**: 92% of `src/core` — buffers
and undo, project folders, search, find and replace, diffs, git against a real
repository, themes, settings and key chords, the updater, terminal sessions
over live PTYs — and the two frontends driven **end to end**. `tests/tui_e2e.rs`
runs the real terminal editor on ratatui's test backend and reads the frame
back; `tests/gui_e2e.rs` runs the real window on a bare egui context with
synthetic keys and clicks. Between them they cover 94% of the terminal's
drawing and 61% of the window's. CI gates `src/core` at 90%.

Pull requests must pass `make lint` and `make test`; CI runs them on Linux,
macOS and Windows, and separately builds the terminal frontend with no graphics
stack to keep it headless-safe. See
[Contributing](https://vsdudakov.github.io/yara-code/contributing/).

## License

MIT — see [LICENSE](LICENSE).

<div align="center">

# Yara Code

**The terminal editor for the agent loop: your coding agent on the left, the
diff of what it just did on the right.**

[![CI](https://github.com/vsdudakov/yara-code/actions/workflows/ci.yml/badge.svg)](https://github.com/vsdudakov/yara-code/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/vsdudakov/yara-code?sort=semver)](https://github.com/vsdudakov/yara-code/releases)
[![Docs](https://img.shields.io/badge/docs-vsdudakov.github.io%2Fyara--code-blue.svg)](https://vsdudakov.github.io/yara-code/)
[![Rust](https://img.shields.io/badge/rust-stable-orange.svg)](https://www.rust-lang.org/)
[![License: MIT](https://img.shields.io/badge/license-MIT-green.svg)](LICENSE)

📖 **[Documentation](https://vsdudakov.github.io/yara-code/)** ·
🚀 **[Install](#install)** ·
⌨️ **[Keys](https://vsdudakov.github.io/yara-code/guides/keys/)**

</div>

![Yara Code: the agent's session beside the diff of its last edit](docs/assets/shots/hero.svg)

```bash
brew install vsdudakov/tap/ycode
ycode ~/code/project
```

## The loop

You write code with an agent now — Claude Code, Codex, Cursor's CLI — and the
agent lives in a terminal. What you do all day is *watch it work*: read the
diff it just made, decide whether it is right, tell it what to do next. Yara
Code is one screen for exactly that.

- **AGENT** is the agent itself, on a real pseudo-terminal, in the left pane.
  Type at it as you would in any terminal; it keeps its own keys.
- **FOLLOW** watches the working tree. Every edit the agent makes lands on a
  timeline and the pane snaps to its diff. `←` `→` scrub back through the
  edits, `⏎` marks one reviewed, `f` snaps back to live, `v` shows the file
  instead of the diff.
- **CHANGES** (`F4`) is the other view of the same work: what the
  branch differs from `main` by, file by file, against the timeline of how it
  got there.
- **Workspaces** run agents in parallel: `F7` names one, gets a git
  worktree on a branch of that name, starts an agent in it, and gives it a tab.

Everything else is the small editor around that: a file tree, an editor with
undo and syntax colours for fixing a line, project search, a command palette,
and a `settings.json` that holds everything you might want different.

## What it looks like

| | |
| --- | --- |
| ![Scrubbed back to an earlier edit](docs/assets/shots/paused.svg) | ![The file as it stands, the added lines marked](docs/assets/shots/file-view.svg) |
| *Scrubbed back: the pane pauses on an earlier edit, reviewed ones are hollow ticks.* | *The same edit as the file, with a bar beside every added line.* |
| ![What differs from main](docs/assets/shots/changes.svg) | ![Two workspaces, two agents](docs/assets/shots/tabs.svg) |
| *CHANGES: the branch against main, one row a file.* | *A second workspace: its own worktree, its own agent, its own timeline.* |
| ![A file open for editing](docs/assets/shots/edit.svg) | ![Project search](docs/assets/shots/search.svg) |
| *A file from the tree, open where the follow pane was.* | *Search the project; Enter opens the hit on its line.* |
| ![The command palette](docs/assets/shots/palette.svg) | ![Every key binding](docs/assets/shots/keys.svg) |
| *The palette finds a command by a few letters.* | *F1: every command and its chord, always current.* |

## Install

```bash
brew install vsdudakov/tap/ycode            # macOS and Linux
```

```bash
# Debian, Ubuntu — the key and the source, once
curl -fsSL https://vsdudakov.github.io/packages/apt/ycode.gpg \
  | sudo tee /usr/share/keyrings/ycode.gpg > /dev/null
echo "deb [signed-by=/usr/share/keyrings/ycode.gpg] https://vsdudakov.github.io/packages/apt stable main" \
  | sudo tee /etc/apt/sources.list.d/ycode.list > /dev/null
sudo apt update && sudo apt install ycode

# Fedora, RHEL, openSUSE
sudo curl -fsSL -o /etc/yum.repos.d/ycode.repo https://vsdudakov.github.io/packages/yum/ycode.repo
sudo dnf install ycode

# Arch, from the release PKGBUILD
makepkg -si

# Anywhere with a Rust toolchain
cargo install --git https://github.com/vsdudakov/yara-code ycode
```

Plain binaries for macOS (Apple Silicon and Intel), Linux (x86_64 and arm64)
and Windows are on the
[releases page](https://github.com/vsdudakov/yara-code/releases/latest). The
command is `ycode` because `yara` belongs to VirusTotal's scanner in every
package manager.

## Usage

```bash
ycode                    # the start page: recent projects
ycode ~/code/project     # open a folder; the agent starts in it
```

The agent is whatever `"agent"` in `settings.json` says — `claude` by
default; `codex`, `cursor-agent`, or a path to any of them. `File → Settings`
opens the file, every key commented.

| | Key |
| --- | --- |
| Switch pane (agent → files → editor / follow) | `F6` |
| Scrub the timeline · go live · mark reviewed · diff/file | `←` `→` · `f` · `⏎` · `v` |
| Changes vs main · palette · search · go to file · files | `F4` · `F5` · `F3` · `Ctrl+P` · `Ctrl+B` |
| New workspace · rename · next / previous · close | `F7` · `F2` · `Ctrl+L` / `Ctrl+K` · `Ctrl+W` |
| Save · close the file · undo / redo | `Ctrl+S` · `Esc` · `Ctrl+Z` / `Ctrl+Y` |
| Keys · menus · recent · theme · agent usage · quit | `F1` · `F10` · `Ctrl+R` · `F9` · `F8` · `Ctrl+Q` |

With the agent focused, plain keys, `⏎`, `Esc`, `Tab` and the arrows are the
agent's; a function key or a bound `Ctrl` chord is the editor's, except the
ones `"agent_keys"` lists (`Ctrl+R`, `Ctrl+N`, `Ctrl+Z`, `Ctrl+O`, `Ctrl+W`,
`Ctrl+Y` by default), which agents use themselves. The defaults need nothing
a terminal cannot send: no `Ctrl+Shift`, no `Alt`.

## Development

```bash
make lint     # cargo fmt --check + clippy -D warnings
make test     # cargo test --workspace
make shots    # redraws docs/assets/shots/*.svg with the editor itself
make run ARGS=~/code/project
```

Two crates: `crates/yara-core` is everything the editor knows — the follow
loop, git, the agent's pty and keyboard protocol, settings, themes, search —
and `crates/ycode` is the terminal that shows it, with end-to-end tests that
drive the real app on ratatui's test backend and read the frame back.

## License

MIT — see [LICENSE](LICENSE).

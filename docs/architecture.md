---
description: How Yara Code is built — a core crate that knows everything and a terminal crate that draws it, the session, the watcher, the pty, and the tests.
---

# Architecture

Two crates in one task.

**`crates/yara-core`** is everything the editor knows and nothing it draws:

- `follow.rs` — the timeline: `EditEvent`s with their hunks, and `Follow`
  with its live/paused cursor and the reviewed flags.
- `git.rs` — the repository through the `git` CLI: the base branch, what
  differs from it, a `Watcher` that turns working-tree changes into edits
  by diffing each file against its last snapshot, worktrees, and pull
  request titles through `gh`.
- `pty.rs` and `keyboard.rs` — the agent on a pseudo-terminal, its screen
  parsed by `vt100`, and the kitty keyboard protocol it may ask for.
- `command.rs`, `settings.rs`, `theme.rs` — every action and its chord,
  `settings.json`, the theme and VS Code theme files.
- `buffer.rs`, `history.rs`, `syntax.rs`, `tree.rs`, `search.rs`, `glob.rs`,
  `fuzzy.rs` — the editor, the tree, search and the finders.
- `update.rs`, `usage.rs` — releases through `curl`, usage through the
  commands the settings name.

**`crates/ycode`** is the terminal:

- `app.rs` — `App` holds the settings, the theme and a `Vec<Session>`; a
  `Session` is one task — its folder, repository, agent, watcher,
  timeline, tree and open file. `App` derefs to the active session, so the
  drawing code reads `app.follow` and means the one in front. `handle_key`
  decides whose key it is; `execute` runs a `Command`; `handle_mouse`
  matches a click against the `Hits` the last frame recorded.
- `ui.rs` — every frame: header, panes, status bar, overlays.
- `main.rs` — raw mode, the kitty flags, mouse capture, the loop.

The tests drive the real `App` on ratatui's `TestBackend` and read the frame
back, with real repositories in temp folders and `cat` on a real pty for the
agent. `examples/screenshot.rs` does the same into SVG for these pages.

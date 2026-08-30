---
description: Yara Code's settings.json — the agent, the layout, the terminal, the follow loop, the folders nobody works in, the keys; every key commented, every one optional.
---

# Settings

`File → Settings` (++f12++) opens the file in the editor:
`~/.config/ycode/settings.json`, or `%APPDATA%\ycode\settings.json` on
Windows, or wherever `YARA_CONFIG_DIR` points. Every key is optional, and the
file is written afresh — with a comment over each key — whenever the editor
saves a setting. `//` comments are allowed.

| Key | Default | What it is |
| --- | --- | --- |
| `theme` | `"Dark Modern"` | The theme, built in or a VS Code JSON in `themes/` beside the file |
| `agent` | `"claude"` | The command that runs in the AGENT pane: `claude`, `codex`, `cursor-agent`, a path, with arguments |
| `agent_side` | `"left"` | Which side the agent sits on; "Move Panes to the Other Side" in the palette swaps it |
| `agent_width` | `42` | The agent pane's share of the width, in percent; dragging the seam between the panes sets it |
| `sidebar_width` | `30` | The FILES tree, in columns; its own seam drags it too |
| `show_sidebar` | `false` | Whether FILES is open at start |
| `narrow_width` | `80` | Under this many columns — a phone over SSH, a narrow split — the panes take turns: the one with the keyboard fills the body and ++f6++ brings up the next; `0` keeps them side by side at any width |
| `shell` | `""` | The shell ++ctrl+t++ runs under the agent; empty means `$SHELL` |
| `terminal_height` | `40` | That terminal's share of the agent pane's height, in percent |
| `timeline_ticks` | `12` | Edits on the strip before it windows around the current one |
| `cursor_blink_ms` | `500` | One step of the editor caret's blink; `0` keeps it on |
| `blame` | `true` | Whether the editor says, at the end of the line under the mouse, who last committed it and when |
| `refresh_ms` | `500` | How often the folders are looked at |
| `base_branch` | `""` | What CHANGES measures against; empty means the main working copy's branch |
| `worktrees_dir` | `""` | Where a task's worktrees go; empty means `<repo>-worktrees` beside each repository |
| `ignore_folders` | `[".", "node_modules", "target", "dist", "build", "vendor", "venv"]` | Folders nobody works in: left out of the tree, Go to File, search and the watching. `"."` stands for every hidden folder |
| `search_exclude` | `[]` | What Search Project leaves out on top of those, in VS Code's glob spelling |
| `usage_slash` | `{"claude": "/usage", "cursor-agent": "/usage", "codex": "/status"}` | What ++f8++ types at each agent, by program name |
| `usage_commands` | `{}` | A command per agent that prints its usage as JSON — see [Agent usage](usage.md) |
| `agent_keys` | `["Ctrl+R", "Ctrl+N", "Ctrl+Z", "Ctrl+W", "Ctrl+Y", "Ctrl+C", "Ctrl+V"]` | Bound chords the agent keeps anyway |
| `keys` | `{}` | Rebindings, command id to chord — see [Key bindings](keys.md) |
| `recent_workspaces` | `[]` | Workspaces opened before, each its folders. Kept by the editor |

A file that cannot be read is reported in the status bar and never written
over: the mistake is yours to fix, and the editor runs on the defaults until
you do. A chord that cannot be read keeps its default, and is written back as
it was — a save is not the moment to lose your line.

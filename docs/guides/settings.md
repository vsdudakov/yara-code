---
description: Yara Code's settings.json — the agent command, the layout, the follow loop, workspaces, keys, search, usage; every key commented, every one optional.
---

# Settings

`File → Settings` (++f12++) opens the file in the editor:
`~/.config/ycode/settings.json`, or `%APPDATA%\ycode\settings.json` on
Windows, or wherever `YARA_CONFIG_DIR` points. Every key is optional and the
file is written afresh, with a comment over each key, whenever the editor
saves a setting. `//` comments are allowed.

| Key | Default | What it is |
| --- | --- | --- |
| `theme` | `"Dark Modern"` | The theme, built in or a VS Code JSON in `themes/` beside the file |
| `agent` | `"claude"` | The command that runs in the AGENT pane: `claude`, `codex`, `cursor-agent`, a path, with arguments |
| `agent_side` | `"left"` | Which side the agent sits on; ++shift+f6++ swaps it |
| `cursor_blink_ms` | `500` | One step of the editor caret's blink; `0` keeps it on |
| `agent_width` | `42` | The agent pane's share of the width, in percent; dragging the seam between the panes sets it |
| `sidebar_width` | `30` | The FILES tree, in columns |
| `show_sidebar` | `false` | Whether FILES is open at start |
| `timeline_ticks` | `12` | Edits on the strip before it windows around the current one |
| `refresh_ms` | `500` | How often the working tree is looked at |
| `base_branch` | `""` | What CHANGES measures against; empty means the main working copy's branch |
| `worktrees_dir` | `""` | Where new workspaces' worktrees go; empty means `<repo>-worktrees` beside the repository |
| `usage_commands` | `{}` | A command per agent that prints its plan usage as JSON — see [Agent usage](usage.md) |
| `search_exclude` | `["target", "node_modules", ".*"]` | What Search Project leaves out |
| `agent_keys` | `["Ctrl+R", "Ctrl+N", "Ctrl+Z"]` | Bound chords the agent keeps anyway |
| `keys` | `{}` | Rebindings, command id to chord — see [Key bindings](keys.md) |
| `recent_projects` | `[]` | Kept by the editor for the start page and ++ctrl+r++ |

A file that cannot be read is reported in the status bar and never written
over: the mistake is yours to fix, and the editor runs on the defaults until
you do.

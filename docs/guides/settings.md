# Settings

Everything Yara reads lives in one JSON file, editable from inside the editor
(**File → Settings**, `Cmd+,` / `Ctrl+,`). Saving it applies the changes
immediately. It is written on first run to:

```
~/.config/yara/settings.json
```

```jsonc
{
  "theme": "Dark+",
  "indent": { "style": "spaces", "width": 4, "detect_from_file": true },
  "font_size": 13.5,
  "show_sidebar": true,
  "show_terminal": true,
  "goto_modifiers": { "gui": ["cmd"], "tui": ["ctrl", "alt"] },
  "keys": { "gui": { "save": "Cmd+S" }, "tui": { "save": "Ctrl+S" } },
  "recent_projects": ["/path/to/project"]
}
```

| Field | What it does |
| --- | --- |
| `theme` | Name of the active theme, as the picker shows it. |
| `indent.width` | Spaces per level; `style` picks `spaces` or `tabs`. |
| `indent.detect_from_file` | When on, a file that already uses another width wins and these values are only the fallback. |
| `font_size` | The window's code font size — what Zoom In and Zoom Out change. The terminal frontend uses your terminal's font. |
| `show_sidebar`, `show_terminal` | Which panels are open at startup. |
| `goto_modifiers` | Which modifier turns a click into go-to-definition. A list, because terminals differ in which ones they deliver — and none deliver Cmd, which is why the terminal default is Ctrl or Alt. |
| `keys` | Every binding, per frontend. See [Key bindings](keys.md). |
| `recent_projects` | Most recent first, capped at 15 — what **Open Recent** lists. |

## How the file is merged

Anything you leave out keeps its default, and **key bindings are laid over the
defaults** — rebinding one key does not drop the rest.

Only the bindings that *differ* from the defaults are written back, so a later
change to a default (or a newly added command) still reaches you.

A malformed file is reported in the status bar rather than silently ignored, and
so is a binding you gave to two commands at once:

```
settings.json: tui Ctrl+X is bound to both cut and file_menu
```

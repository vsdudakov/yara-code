---
description: Everything in settings.json: themes, indentation, font size, go-to-definition modifiers and key bindings.
---

# Settings

Everything Yara Code reads lives in one JSON file, editable from inside the editor
(**File → Settings**, `Cmd+,` / `Ctrl+,`) or with anything else. The editor
looks at the file once a second, so a change applies the moment it is saved —
from either frontend, a script, or a hand edit — without a restart. It is
written on first run to:

```
~/.config/yara-code/settings.json
```

The file explains itself: every key is written out with a comment over it, and
`//` comments survive the editor writing the file back.

```jsonc
// Yara Code settings. Every key is optional: leave one out and the built-in
// default applies.
{
  // Colour theme: "Dark+", "Light+", "Monokai". Also View → Theme.
  "theme": "Dark+",

  // Editor font size in points. Window frontend only: the terminal frontend
  // draws in whatever font the terminal itself uses.
  "font_size": 13.5,

  // How much further the wheel carries than the platform asks for. 1.0 is the
  // platform's own notch — three lines in a terminal.
  "scroll_speed": 1.5,

  "indent": {
    // "spaces" or "tabs". Also View → Indentation.
    "style": "spaces",
    // Spaces per level, and how wide a tab is drawn.
    "width": 4,
    // Follow the indentation a file already uses, falling back to the above.
    "detect_from_file": true
  },

  // Panels open at start.
  "show_sidebar": true,
  "show_terminal": true,

  // Modifier held while clicking an identifier to jump to its definition.
  "goto_modifiers": { "gui": ["cmd"], "tui": ["ctrl", "alt"] },

  // Modifier held while the pointer rests on a line to blame it.
  "blame_modifiers": { "gui": ["shift"], "tui": ["shift"] },

  // Key bindings per frontend; only what differs from the defaults is listed.
  "keys": { "gui": { "save": "Cmd+S" }, "tui": { "save": "Ctrl+S" } },

  // Folders offered by File → Open Recent, newest first.
  "recent_projects": ["/path/to/project"]
}
```

## Per-project settings

A project can carry its own `.ycode/settings.json` at its root. Whatever it
sets — indentation and theme are the usual reasons — is laid over the global
file while that project is open, and nothing else changes. The folder is hidden
from the navigator and from project search.

| Field | What it does |
| --- | --- |
| `theme` | Name of the active theme, as the picker shows it. |
| `indent.width` | Spaces per level; `style` picks `spaces` or `tabs`. |
| `indent.detect_from_file` | When on, a file that already uses another width wins and these values are only the fallback. |
| `font_size` | The window's code font size; it takes effect the moment the file is saved. The terminal frontend draws in your terminal's own font, so it does not apply there. |
| `scroll_speed` | How much further the wheel and the trackpad carry than the platform asks for, in both frontends. `1.0` is the platform's own notch — three lines in a terminal — and the default `1.5` moves four rows where a terminal would move three. Held between `0.25` and `8`. |
| `show_sidebar`, `show_terminal` | Which panels are open at startup. |
| `goto_modifiers` | Which modifier turns a click into go-to-definition. A list, because terminals differ in which ones they deliver — and none deliver Cmd, which is why the terminal default is Ctrl or Alt. |
| `blame_modifiers` | Which modifier held under the pointer blames the line it rests on, in the status bar. Shift in both frontends by default: the other three are spoken for by go-to-definition in one or the other. |
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

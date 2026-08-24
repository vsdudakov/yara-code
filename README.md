# Yara

A minimal code editor in Rust with Zed-like chrome and a VS Code Dark+ look,
shipped as **two frontends over one core**:

| Binary | Frontend | Runs where |
| --- | --- | --- |
| `yara` | GPU window (egui on wgpu — Metal/Vulkan/DX) | Local desktop session |
| `yara-tui` | Terminal UI (ratatui on crossterm) | Anywhere a shell runs, including SSH on a headless server |

Both share `src/core`: buffers, file operations, project search, go-to-definition,
smart indentation, syntax highlighting and themes. Only rendering and input differ.

## Build & run

```bash
cargo run --release --bin yara            # GPU window, current directory
cargo run --release --bin yara ~/project

cargo run --release --bin yara-tui            # terminal UI
cargo run --release --bin yara-tui ~/project
```

On a server, build the terminal frontend alone — it pulls no graphics stack at
all (4 dependencies, ~2 MB binary), so no X11, Wayland, or GPU drivers needed:

```bash
cargo build --release --no-default-features --features tui
```

That build pulls no graphics stack — only ratatui, crossterm, syntect, serde and
the PTY crates.

The GPU frontend is likewise buildable alone with `--features gui`.

## Features

- **Navigator** — file tree with new file / new folder / rename / delete, and
  moving entries (drag-and-drop in the GUI, `m` in the TUI).
- **Tabs** — multiple open buffers, unsaved-change markers.
- **Syntax highlighting** — 75 syntect grammars, plus bundled ones for
  TypeScript/TSX, TOML, Kotlin, Swift, Dart, Dockerfile, Protobuf and GraphQL,
  plus an alias table pointing the remaining common extensions at their closest
  relative (`.mjs` → JavaScript, `.ex` → Ruby, `.zig` → C, `.ini` → TOML…). Drop
  any `.sublime-syntax` into `~/.config/yara/syntaxes/` to add or replace a
  language; user grammars win. They load into their own set, so startup stays
  at ~40 ms instead of the ~1.2 s a full relink would cost.
- **Folding** — every indented block folds, in both frontends: click the marker
  in the gutter, or use Fold/Unfold, Fold All and Unfold All. Brace languages
  take their closing line with them; a collapsed block shows how many lines it
  swallowed. Folds are dropped when an edit moves their header.
- **Sticky scroll** — the headers of the blocks you are inside stay pinned at
  the top of the editor while you scroll, syntax highlighted, with their real
  line numbers, up to three deep.
- **Smart indent** — Enter continues the current indentation, opens a block
  after `:` (Python, YAML) or `{ ( [`, dedents after `return`/`pass`/`break`/
  `continue`/`raise`, and splits bracket pairs onto their own lines. The indent
  unit (tabs, 2/4/8 spaces) is inferred from the file being edited.
- **Project search** — live, case-insensitive, grouped by file, click/Enter to
  jump, with an **exclude box** above the query taking comma-separated globs in
  VS Code's spelling: `target, *.lock, **/node_modules, src/generated`. A bare
  name matches any path component, `*`/`?` match within one component, `**`
  spans directories, and naming a directory also excludes everything under it.
  Excluded directories are never walked. `.git`, `node_modules`, `target`,
  `dist`, `build`, `.venv`, binaries and files over 1 MB are always skipped.
- **Go to definition** — ⌘-click in the window, **Ctrl+click or Alt+click** in the
  terminal (the xterm mouse protocol carries only Shift/Alt/Ctrl, never Cmd), or
  `Ctrl+G` at the cursor. Holding the modifier underlines the identifier under
  the pointer in both frontends. Uses a keyword heuristic across Rust, Python,
  JS/TS, Go, Swift, Java/Kotlin, C#, and proto; falls back to listing
  references. `Ctrl+-` (GUI) / `Ctrl+O` (TUI) goes back.
- **Terminal** — a real login shell on a pseudo-terminal in **both** frontends,
  sharing one `core::pty` implementation: tab completion, Ctrl-C, and
  full-screen programs all behave as they do outside Yara, with 5000 lines of
  scrollback (mouse wheel). The frontends differ only in how they paint the
  grid. In the terminal frontend, keys go to the shell while the panel has
  focus — only Toggle Terminal and Quit stay reserved — so leave the panel with
  its toggle or by clicking another pane.

## Settings

Everything the app reads lives in one JSON file, editable from inside the editor
(**File → Settings**, `Cmd+,` / `Ctrl+P`). Saving it applies the changes
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
  "keys": { "gui": { "save": "Cmd+S", ... }, "tui": { "save": "Ctrl+S", ... } },
  "recent_projects": ["/path/to/project"]
}
```

- `indent.width` is the number of spaces per level; `style` picks spaces or
  tabs. With `detect_from_file` on, a file that already uses another width wins,
  and these values are the fallback.
- `goto_modifiers` chooses which modifier turns a click into go-to-definition.
  It is a list because terminals differ in which ones they deliver — and none of
  them deliver Cmd, which is why the terminal default is Ctrl or Alt.
- `keys` holds every shortcut, per frontend, as chords like `"Cmd+Shift+F"` or
  `"Ctrl+-"`. Modifier spellings are forgiving (`cmd`/`command`/`super`,
  `alt`/`option`, `ctrl`/`control`).

Anything you leave out keeps its default, and **key bindings are merged over the
defaults** — rebinding one key does not drop the rest. A malformed file is
reported in the status bar rather than silently ignored.

## Themes

Themes are data, not constants — see `src/core/theme.rs`. Built in: **Dark+**,
**Light+**, **Monokai**. Switch with `Cmd+Shift+T` (GUI) or `Ctrl+T` (TUI).

Any VS Code color theme works: drop its `.json` into

```
~/.config/yara/themes/
```

and it appears in the picker. The loader reads the `colors` map (chrome and the
16 ANSI terminal colors) and `tokenColors` (syntax); anything the file omits
falls back to the built-in Dark+ or Light+ value, per the theme's `type`.

## File menu

Both frontends carry the same **File** menu in a top bar, after the YARA
label — click it, or press `Ctrl+X` in the terminal. Entries show their current chord, read from settings:

New File… · Open File… · Open Folder… · Open Recent… · Save · Save As… ·
Save All · Settings · Close Editor · Quit

## Keys

Defaults; all of them are rebindable in `settings.json`.

| Action | GUI | TUI |
| --- | --- | --- |
| Save | `Cmd+S` | `Ctrl+S` |
| Save As… / Save All | `Cmd+Shift+S` / `Cmd+Alt+S` | `Ctrl+Shift+S` / `Ctrl+A` |
| New file / Open file / Open folder | `Cmd+N` / `Cmd+O` / `Cmd+Shift+O` | `Ctrl+N` / `Ctrl+O` / `Ctrl+Shift+O` |
| Open recent | `Cmd+Alt+O` | `Ctrl+R` |
| Settings | `Cmd+,` | `Ctrl+P` |
| Toggle sidebar / terminal | `Cmd+B` / `Cmd+J` | `Ctrl+B` / `Ctrl+J` |
| Search / Files sidebar | `Cmd+Shift+F` / `Cmd+Shift+E` | `Ctrl+F` / `Ctrl+E` |
| Switch query / exclude box | click the field | `Ctrl+F` again |
| Theme picker | `Cmd+Shift+T` | `Ctrl+T` |
| Close tab / Quit | `Cmd+W` / `Cmd+Q` | `Ctrl+W` / `Ctrl+Q` |
| Go to definition | `Cmd+click` | `Ctrl+G`, `Ctrl`/`Alt+click` |
| Fold / unfold | `Cmd+Alt+F` | `Alt+F` |
| Fold all / unfold all | `Cmd+Alt+0` / `Cmd+Alt+9` | `Alt+0` / `Alt+9` |
| Back | `Ctrl+-` | `Ctrl+Y` |
| Previous / next tab | `Alt+←` / `Alt+→` | `Alt+←` / `Alt+→` |
| File menu / context menu | click | `Ctrl+X` / `Ctrl+K` |
| Show bindings overlay | — | `Ctrl+H` |
| Cycle panes | — | `Tab` / `Shift+Tab` |

In the terminal navigator: arrows or `j`/`k` move, `Enter` opens or expands,
`a` new file, `A` new folder, `r` rename, `d` delete, `Ctrl+K` opens the context
menu (which also carries Move To…).

Both navigators look and behave the same: a `FILES` / `SEARCH` switch in the
header, `▸`/`▾` for folders and `▫` for files, the selected row filled with the
selection color, hover highlighting, the drop target filled with the accent
color, and the identical context menu — Open · New File · New Folder · Rename ·
Move To… · Delete.

Panes are resizable in both: drag the border between the sidebar and the editor,
or the one above the terminal. The border lights up under the pointer and while
being dragged.

The terminal frontend mirrors the window's layout throughout: a top bar with the
YARA label and the File menu separated by a rule, the sidebar down the left, the
terminal under the editor only, a centered hint in an empty editor, a `TERMINAL`
panel header that brightens when the panel has the keyboard, and a status bar
carrying just the file, cursor position, language and theme. Key bindings
live in an overlay (`Ctrl+H`), not along the bottom edge.

### Mouse (TUI)

The terminal frontend is fully mouse-driven, mirroring the GPU window:

- **Left click** — select and open a file, expand a folder, focus a pane, place
  the text cursor, switch tabs, pick a search result or a prompt entry.
- **Click the tab marker** — closes that tab. It shows a dot while the buffer has
  unsaved changes and turns into a cross when the pointer is over the tab.
- **Ctrl+click / Alt+click in the editor** — go to definition. Two modifiers
  because some terminals grab one of them: macOS Terminal turns Ctrl+click into
  a right click, so use Alt+click there.
- **Right click** — context menu on the row under the pointer: Open, New File,
  New Folder, Rename, Move To..., Delete. `Ctrl+K` opens the same menu from the
  keyboard; arrows and Enter drive it, Esc or a click outside dismisses it.
- **Drag and drop** — press a navigator row and drag it onto a folder to move it;
  the target folder highlights, and dropping on empty space moves to the project
  root.
- **Hover** — rows and tabs light up under the pointer.
- **Wheel** — scrolls whichever pane is under the pointer.

Icons use Unicode by default; set `YARA_ASCII=1` for terminals with sparse
font coverage. Since the app captures the mouse, use your terminal's usual
override (**Shift+drag** in iTerm2, GNOME Terminal, Windows Terminal) to select
text for copying.

## License

MIT — see [LICENSE](LICENSE).

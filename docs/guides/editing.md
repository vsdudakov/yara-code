# Editing

## Tabs

Open files sit in a strip over the editor, with a dot while they have unsaved
changes and a cross once the pointer is over them. Drag a tab onto another to
reorder; `Ctrl+PageUp` / `Ctrl+PageDown` walk the strip. Diffs open as tabs of
their own beside the files — see [Git](git.md).

## Undo and redo

`Cmd+Z` / `Ctrl+Z` and `Cmd+Shift+Z` / `Ctrl+Shift+Z`. Every change is
undoable, including **Replace All** and a replace inside the find bar.

A run of typing folds into a single step, so undo takes back a word rather than
a letter; moving the cursor closes the run; a bulk rewrite always stands alone.
Each buffer keeps 200 steps, and the cursor returns to where the step left it.

## Smart indent

Enter continues the current indentation, opens a block after `:` (Python, YAML)
or `{ ( [`, dedents after `return` / `pass` / `break` / `continue` / `raise`,
and splits bracket pairs onto their own lines. The indent unit — tabs, 2, 4 or
8 spaces — is inferred from the file being edited unless
`indent.detect_from_file` is off.

## Folding

Every indented block folds, in both frontends: click the marker in the gutter,
or use **Fold / Unfold** (`Cmd+Alt+F` / `Ctrl+Alt+F`), **Fold All**
(`Cmd+Alt+0`) and **Unfold All** (`Cmd+Alt+9`). Brace languages take their
closing line with them, and a collapsed block shows how many lines it swallowed.
Folds are dropped when an edit moves their header.

**Sticky scroll**: the headers of the blocks you are inside stay pinned at the
top of the editor while you scroll, syntax highlighted, with their real line
numbers, up to three deep.

## Go to definition

`F12` at the cursor, `Cmd+click` in the window, `Ctrl+click` or `Alt+click` in
the terminal (the xterm mouse protocol carries Shift, Alt and Ctrl, never Cmd).
Holding the modifier underlines the identifier under the pointer.

It is a keyword heuristic across Rust, Python, JS/TS, Go, Swift, Java/Kotlin,
C# and proto — not an LSP — and it falls back to listing references. `Ctrl+-`
(window) / `Alt+←` (terminal) goes back.

## The navigator

Arrows or `j`/`k` move, <kbd>Enter</kbd> opens or expands. New file, new folder,
rename, move and delete are on the context menu (right-click, or `Shift+F10`),
and each has a binding of its own — `F2` renames, `Cmd+Backspace` /
`Shift+Delete` deletes.

Drag a row onto a folder to move it; the target folder highlights, and dropping
on empty space moves to the project root.

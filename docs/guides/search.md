# Search & replace

Yara has two searches built from the same parts, so they look and behave alike:
**project search** in the sidebar, and **find in file** over the editor.

## Project search

`Cmd+Shift+F` / `Ctrl+Shift+F`. Live, case-insensitive by default, grouped by
file; click or press <kbd>Enter</kbd> on a hit to jump to it, and the file opens
with every match highlighted.

Three fields, each with a lit heading over it: `SEARCH`, `REPLACE`, `EXCLUDE`.
The toggles at the right of the heading row are `Aa` (match case), `ab` (whole
word) and `.*` (regular expression) — VS Code's own.

**Replace All** rewrites every match in the results and reloads any open buffer
that was not modified.

### The exclude box

Comma-separated globs in VS Code's spelling:

```
target, *.lock, **/node_modules, src/generated
```

- a bare name matches any path component;
- `*` and `?` match within one component;
- `**` spans directories;
- naming a directory excludes everything under it.

Excluded directories are never walked. `.git`, `node_modules`, `target`,
`dist`, `build`, `.venv`, binaries and files over 1 MB are always skipped.

## Find in file

`Cmd+F` / `Ctrl+F` opens the same form over the editor: a lit `FIND` /
`REPLACE` heading over each field, both always shown, `…` in an empty one, the
option toggles and the close mark in the top-right corner, and the match counter
along the bottom with `Replace` · `Replace All` · `<` · `>` at its right end —
the two replace actions appear once the replace field has something in it.

- <kbd>Tab</kbd> switches field.
- `F3` / `Shift+F3` (or `Cmd+G` / `Cmd+Shift+G` in the window) step through
  matches; `Ctrl+Alt+Enter` replaces them all.
- The bar **belongs to the file it was opened on**: switching tabs hides it, and
  coming back brings the query, the options and the counter with it. Closing
  that file closes it.
- Match highlighting follows the text as you edit.

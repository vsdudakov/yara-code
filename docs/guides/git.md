# Git

Yara talks to the `git` CLI — no libgit2, no extra dependency. Everything below
works in both frontends.

## The panel

`Cmd+Shift+G` / `Ctrl+Shift+G` opens the Git view in the sidebar:

- a **repository** picker (`Cmd+Alt+G` / `Ctrl+Alt+G`) listing every repository
  found under the project;
- a **worktree** picker (`Cmd+Alt+K` / `Ctrl+Alt+K`);
- the list of changed files, each with its porcelain letter — `M`, `A`, `D`,
  `R`, `U` — colored from the theme's terminal palette.

The status is re-polled on a timer while the view is on screen.

## Diffs

Clicking a changed file opens a **side-by-side diff** as a tab of its own,
beside the open files: what the file was on the left, what it is now on the
right, each side with its own line numbers.

- Replacements stand on one row — the old line facing the new one.
- Added lines leave the left half blank, removed lines the right half.
- Changed lines are washed in the theme's own red and green.
- Untracked files show as wholly added; deleted ones as wholly removed.

The header carries the path, `+N −M`, an **Open File** button and a close mark.
In the terminal frontend, <kbd>Esc</kbd> closes the diff and <kbd>Enter</kbd>
opens the file it is showing.

`git` itself does the diffing (`git diff -U1000000`), so what you see is what
`git diff` says.

## Decorations

- **Changed files are tinted in the navigator**, and so are the folders above
  them — a collapsed tree still shows where the changes are.
- **Changed lines carry a bar in the editor's gutter**: green for added, yellow
  for modified, red where lines were removed.
- **The status bar blames the line under the cursor** — commit, author, how long
  ago, the pull request its message names (`(#412)` or
  `Merge pull request #7`), and the commit summary. A line you have not
  committed yet says so.

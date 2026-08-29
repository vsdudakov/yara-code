---
description: Tasks in Yara Code — one agent per task in its own git worktree, each with a tab, a timeline and a name of your choosing.
---

# Tasks

A task is an agent and the folders it works in
— a git worktree per repository, most often — with one timeline and one
CHANGES over them all. The tabs in the header are the tasks.

![Naming a new task](../assets/shots/new-tab.svg)

++f7++ — or `[+]` in the header, or `File → New Task…` —
asks for a name. The name becomes the tab, and, with spaces and slashes made
dashes, the branch and the folder: `git worktree add -b logout-flow
<worktrees_dir>/logout-flow`. An agent starts there, and the keyboard goes to
it.

![Two tasks](../assets/shots/tabs.svg)

| | Key |
| --- | --- |
| New task | ++f7++ |
| Next / previous | ++ctrl+l++ / ++ctrl+k++, or click the tab |
| Rename | ++f2++, or a right click on the tab |
| Delete the worktree | a right click on the tab — the folder goes, the branch stays |
| Reorder | drag a tab along the strip |
| Close | ++ctrl+w++ — the agent stops; the worktree stays for git |

The first task, the one without a name of its own, is called by its
pull request when `gh` is installed and the branch has one (`#42 Fix the
login redirect`), else by its branch, else by its folder.

Where the worktrees go is `worktrees_dir` in [settings](settings.md):
empty means a `<repo>-worktrees` folder beside the repository.

## Several folders, one task

A feature that touches a backend and a frontend is one task with two
folders. `File → Add Folder to Task…` takes a path — a worktree of another
repository, or any folder at all — and from then on the task treats them as
one:

- the timeline carries the edits of both, each named by its folder —
  `frontend/src/app.js`;
- CHANGES heads each folder with its branch and its counts;
- the FILES tree heads each folder, and ++ctrl+p++ and project search reach
  across both;
- the status bar shows the main folder's branch and `+1 folders`.

A folder that is not a repository is watched all the same: its files are
read rather than asked of git, and their edits land on the timeline like any
other. CHANGES has nothing to say about it, and says so.

The first folder is the main one: the agent runs there, and it names the tab.

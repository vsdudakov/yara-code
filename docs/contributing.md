---
description: How to build, test and contribute to Yara Code — the CI gate, the house rules, and how the frontend is tested.
---

# Contributing

```bash
git clone https://github.com/vsdudakov/yara-code
cd yara-code
make lint     # cargo fmt --check + clippy -D warnings
make test     # cargo test --workspace
make run ARGS=~/code/project
```

Nothing but a Rust toolchain and `git` is needed. CI runs the gate on Linux,
macOS and Windows and gates `crates/yara-core` at 90% line coverage.

## House rules

- **Minimal.** Add what a screen uses; no new dependency without a reason
  the standard library or a crate already in the tree cannot meet.
- **Everything configurable lives in `settings.json`**, with a comment in
  `Settings::to_commented_json` and a test. Keys are never hardcoded: an
  action is a `Command` variant, a default chord in `settings.rs`, and an
  arm in `App::execute` — the settings tests then enforce that it is bound
  and that no two commands share a chord.
- Comments explain *why*, not *what*, and read as prose.
- Tests are named as sentences and assert behaviour, not implementation.
- The frontend is tested end to end: `crates/ycode/tests/frame.rs` runs
  the real `App` on ratatui's test backend, keys and mouse in, the frame's
  text out.

## Screenshots

`make shots` redraws every image on these pages with the editor itself,
from a scripted session in a temporary repository. Change a screen, run it,
commit the SVGs with the change.

## Releasing

Tag `vX.Y.Z`. The release workflow builds the binary for macOS, Linux and
Windows, the `.deb` and `.rpm`, attaches them with checksums, publishes the
apt and dnf repositories, and updates the Homebrew tap and the AUR
`PKGBUILD`.

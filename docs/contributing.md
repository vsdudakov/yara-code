# Contributing

Yara is one Rust crate with two frontends. Everything below is what CI enforces,
so a green local run is a green pull request.

## Getting set up

```bash
git clone https://github.com/vsdudakov/yara-code
cd yara-code
make build
```

On Debian/Ubuntu the window frontend needs `libgtk-3-dev libxkbcommon-dev
libwayland-dev`; the terminal frontend needs nothing beyond a Rust toolchain.

## The gate

```bash
make lint     # cargo fmt --check + clippy -D warnings
make test     # cargo test --all-features
```

CI runs both on Linux, macOS and Windows, and separately builds the terminal
frontend with `--no-default-features --features tui` — that configuration must
keep compiling with no graphics stack at all, because that is how Yara is used
over SSH.

## House rules

- **The two frontends mirror each other.** A feature that lands in one is
  expected in the other, with the same wording, the same menu entry and the same
  chord modulo `Cmd`→`Ctrl`. If a frontend genuinely cannot do it — the terminal
  has no font size of its own — say so in a comment.
- **Key bindings are never hardcoded.** Add a `Command` variant, a default chord
  in both keymaps in `core/settings.rs`, and an arm in each frontend's
  `execute`. The settings tests then enforce that every command is bound and
  that no two share a chord.
- **Logic goes in `core`.** The frontends paint and translate input; they do not
  decide what an action means.
- **Comments explain why, not what**, and read as prose. No comment restates the
  line under it.
- **Tests are named as sentences** — `a_run_of_typing_undoes_in_one_step` — and
  assert behaviour, not implementation.
- **No new dependency** without a reason the standard library or a crate already
  in the tree cannot meet.

## Testing the terminal frontend

A terminal UI cannot be driven by a normal test harness. To see a real frame,
run it under a pty: spawn `target/debug/ycode`, write the key bytes, strip the
escape sequences and print the grid. That is how the terminal changes in this
repository are checked — including the mouse, which is just another escape
sequence (`\x1b[<0;25;40M`).

## Releases

Tagging `vX.Y.Z` builds the binaries for macOS (Apple Silicon and Intel), Linux
x86_64 and Windows x64, attaches them to a GitHub release with their checksums,
and updates the Homebrew tap. The documentation site redeploys from `main`
whenever `docs/` or `mkdocs.yml` changes.

---
description: Install Yara Code with Homebrew, apt, dnf, the AUR, a prebuilt binary, or cargo. One command, ycode, on macOS, Linux and Windows.
---

# Installation

Yara Code is one binary, **`ycode`**. The command is `ycode` rather than `yara`
because `yara` belongs to VirusTotal's scanner in every package manager.

## Homebrew — macOS and Linux

```bash
brew install vsdudakov/tap/ycode
```

## Debian and Ubuntu

Add the repository once; `apt upgrade` follows releases from then on:

```bash
sudo install -d /usr/share/keyrings
curl -fsSL https://vsdudakov.github.io/packages/apt/ycode.gpg \
  | sudo tee /usr/share/keyrings/ycode.gpg > /dev/null
echo "deb [signed-by=/usr/share/keyrings/ycode.gpg] https://vsdudakov.github.io/packages/apt stable main" \
  | sudo tee /etc/apt/sources.list.d/ycode.list > /dev/null
sudo apt update
sudo apt install ycode
```

`amd64` and `arm64` are both served; the `Release` file is signed and
`signed-by` ties the key to this source alone.

## Fedora, RHEL and openSUSE

```bash
sudo curl -fsSL -o /etc/yum.repos.d/ycode.repo \
  https://vsdudakov.github.io/packages/yum/ycode.repo
sudo dnf install ycode
```

## Arch Linux

A `PKGBUILD` is attached to every release:

```bash
curl -LO https://github.com/vsdudakov/yara-code/releases/latest/download/PKGBUILD
makepkg -si
```

## A single package file

```bash
curl -LO https://github.com/vsdudakov/yara-code/releases/latest/download/ycode_X.Y.Z-1_amd64.deb
sudo apt install ./ycode_X.Y.Z-1_amd64.deb

curl -LO https://github.com/vsdudakov/yara-code/releases/latest/download/ycode-X.Y.Z-1.x86_64.rpm
sudo dnf install ./ycode-X.Y.Z-1.x86_64.rpm
```

## Prebuilt binaries

From the [latest release](https://github.com/vsdudakov/yara-code/releases/latest):

| Platform | Archive |
| --- | --- |
| macOS, Apple Silicon | `ycode-vX.Y.Z-aarch64-apple-darwin.tar.gz` |
| macOS, Intel | `ycode-vX.Y.Z-x86_64-apple-darwin.tar.gz` |
| Linux, x86_64 | `ycode-vX.Y.Z-x86_64-unknown-linux-gnu.tar.gz` |
| Linux, arm64 | `ycode-vX.Y.Z-aarch64-unknown-linux-gnu.tar.gz` |
| Windows, x64 | `ycode-vX.Y.Z-x86_64-pc-windows-msvc.zip` |

```bash
shasum -a 256 -c ycode-vX.Y.Z-x86_64-unknown-linux-gnu.tar.gz.sha256
tar xzf ycode-vX.Y.Z-x86_64-unknown-linux-gnu.tar.gz
sudo mv ycode-vX.Y.Z-x86_64-unknown-linux-gnu/ycode /usr/local/bin/
```

`Help → Check for Updates…` downloads a newer release over the binary in
place, when the folder it lives in is writable; otherwise it says which package
manager to ask.

## From source

```bash
cargo install --git https://github.com/vsdudakov/yara-code ycode
```

Nothing but a Rust toolchain and `git` on the `PATH` is needed: there is no
graphics stack, and git is driven through its own CLI.

## What it expects to find

- **git**, for everything the follow loop and CHANGES do.
- **An agent** — `claude` by default; `codex`, `cursor-agent` or any command
  at all, named by `agent` in [settings](../guides/settings.md).
- **`gh`**, optionally: a task is named by its pull request when it can be
  asked for one.
- **A clipboard tool**, optionally: `pbcopy`, `wl-copy`, `xclip` or `xsel`.
  Without one — over SSH, say — copying falls back on OSC 52 and the terminal
  does it.

Any terminal works: the default keys are function keys and plain `Ctrl`
chords, which every terminal sends. Where the kitty keyboard protocol is there
— Ghostty, kitty, WezTerm, foot, iTerm2 3.5 and later — the agent in the pane
gets it too.

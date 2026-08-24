---
description: Install Yara Code with Homebrew, a .deb or .rpm package, the AUR, prebuilt binaries, or from source. One install gives you both the ycode terminal editor and the ycode-gui window.
---

# Installation

Yara Code ships two binaries from one crate: **`ycode`** (the terminal frontend) and
**`ycode-gui`** (the window). Every install method below installs **both at
once** — one `brew install`, one `.deb`, one archive — because the point of the
two frontends is that you can move between them without thinking about it.

## Homebrew

```bash
brew install vsdudakov/tap/ycode
```

The tap is updated by the release workflow, so `brew upgrade` follows new
versions as they are tagged.

!!! note "Why `yara-code` and not `ycode`"

    `ycode` is already taken in Homebrew core, Debian, Fedora and the AUR by
    [VirusTotal's YARA](https://virustotal.github.io/yara/), the malware
    pattern-matching tool, and it owns `/usr/bin/yara`. The **package** is
    therefore called `yara-code` and declares a conflict with `ycode`; the
    **commands** it installs are still `ycode` and `ycode-gui`.

## Prebuilt binaries

Download the archive for your platform from the
[latest release](https://github.com/vsdudakov/yara-code/releases/latest):

| Platform | Archive |
| --- | --- |
| macOS, Apple Silicon | `ycode-vX.Y.Z-aarch64-apple-darwin.tar.gz` |
| macOS, Intel | `ycode-vX.Y.Z-x86_64-apple-darwin.tar.gz` |
| Linux, x86_64 | `ycode-vX.Y.Z-x86_64-unknown-linux-gnu.tar.gz` |
| Windows, x64 | `ycode-vX.Y.Z-x86_64-pc-windows-msvc.zip` |

Each archive carries both binaries, the README and the licence, and each has a
`.sha256` beside it:

```bash
shasum -a 256 -c ycode-vX.Y.Z-aarch64-apple-darwin.tar.gz.sha256
tar xzf ycode-vX.Y.Z-aarch64-apple-darwin.tar.gz
sudo mv ycode-vX.Y.Z-aarch64-apple-darwin/ycode* /usr/local/bin/
```

## Linux packages

Every release carries a `.deb` and an `.rpm` built from the same binaries:

```bash
# Debian, Ubuntu, Mint…
curl -LO https://github.com/vsdudakov/yara-code/releases/latest/download/ycode_X.Y.Z-1_amd64.deb
sudo apt install ./ycode_X.Y.Z-1_amd64.deb

# Fedora, RHEL, openSUSE…
curl -LO https://github.com/vsdudakov/yara-code/releases/latest/download/ycode-X.Y.Z-1.x86_64.rpm
sudo dnf install ./ycode-X.Y.Z-1.x86_64.rpm
```

Both install `ycode` and `ycode-gui` into `/usr/bin` and register a desktop entry
for the window frontend.

### Arch Linux

A `PKGBUILD` is attached to every release and tracks the published tarball:

```bash
curl -LO https://github.com/vsdudakov/yara-code/releases/latest/download/PKGBUILD
makepkg -si
```

!!! note "About apt and dnf repositories"

    Yara Code is not served from an APT or DNF repository yet — the packages above
    install straight from the release, which is why the commands name a file
    rather than `apt install ycode`. A signed repository is planned; until then
    `brew`, the `.deb`/`.rpm` files and the tarballs are the supported paths.

## From source

```bash
git clone https://github.com/vsdudakov/yara-code
cd yara-code
cargo build --release
```

The binaries land in `target/release/`.

### Terminal frontend only

The window needs a display server and a graphics stack; the terminal frontend
needs neither. On a server, build it alone:

```bash
cargo build --release --no-default-features --features tui
```

Nothing but a Rust toolchain is required — no GTK, no Wayland, no wgpu.

### Linux build dependencies

The window frontend needs the usual desktop development packages. On Debian and
Ubuntu:

```bash
sudo apt-get install libgtk-3-dev libxkbcommon-dev libwayland-dev
```

(GTK is there for the native Open/Save dialogs.)

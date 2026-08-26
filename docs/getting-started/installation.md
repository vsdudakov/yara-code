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

On **macOS** that is the whole install: **Yara Code.app** is linked into
`/Applications` with its own icon and opens from the Dock and from Spotlight.
Both commands live inside the bundle and are linked onto your `PATH` at the
same time, so `ycode` still works in a terminal.

On **Linux** the same command installs the two bare commands, which is all a
box without a Dock has use for.

The tap is updated by the release workflow, so `brew upgrade` follows new
versions as they are tagged.

!!! warning "The first open on macOS"

    The app is signed ad-hoc rather than with an Apple Developer ID, so macOS
    reports an unidentified developer the first time it is opened from Finder.
    Allow it once in **System Settings → Privacy & Security**; every open after
    that is ordinary. Running `ycode-gui` from a terminal never asks.

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
| Linux, arm64 | `ycode-vX.Y.Z-aarch64-unknown-linux-gnu.tar.gz` |
| Windows, x64 | `ycode-vX.Y.Z-x86_64-pc-windows-msvc.zip` |

Each archive carries the README, the licence and a `.sha256` beside it. On
Linux and Windows it carries the two binaries; on **macOS** it carries
`Yara Code.app`, which holds them both:

```bash
# Linux
shasum -a 256 -c ycode-vX.Y.Z-x86_64-unknown-linux-gnu.tar.gz.sha256
tar xzf ycode-vX.Y.Z-x86_64-unknown-linux-gnu.tar.gz
sudo mv ycode-vX.Y.Z-x86_64-unknown-linux-gnu/ycode* /usr/local/bin/

# macOS: drag the app to /Applications, and link the commands out of it
shasum -a 256 -c ycode-vX.Y.Z-aarch64-apple-darwin.tar.gz.sha256
tar xzf ycode-vX.Y.Z-aarch64-apple-darwin.tar.gz
mv "ycode-vX.Y.Z-aarch64-apple-darwin/Yara Code.app" /Applications/
sudo ln -sf "/Applications/Yara Code.app/Contents/MacOS/ycode" /usr/local/bin/ycode
sudo ln -sf "/Applications/Yara Code.app/Contents/MacOS/ycode-gui" /usr/local/bin/ycode-gui
```

## Debian and Ubuntu

Add the repository once, and `apt install ycode` works from then on — upgrades
included, with `apt upgrade` like anything else on the machine:

```bash
sudo install -d /usr/share/keyrings
curl -fsSL https://vsdudakov.github.io/packages/apt/ycode.gpg \
  | sudo tee /usr/share/keyrings/ycode.gpg > /dev/null
echo "deb [signed-by=/usr/share/keyrings/ycode.gpg] https://vsdudakov.github.io/packages/apt stable main" \
  | sudo tee /etc/apt/sources.list.d/ycode.list > /dev/null

sudo apt update
sudo apt install ycode
```

Both architectures are served: `amd64` and `arm64`. The repository's `Release`
file is signed, and `signed-by` is what ties the key to it, so nothing else on
the machine is trusted any further than it was.

## Fedora, RHEL and openSUSE

```bash
sudo curl -fsSL -o /etc/yum.repos.d/ycode.repo \
  https://vsdudakov.github.io/packages/yum/ycode.repo
sudo dnf install ycode
```

`x86_64` and `aarch64`, and the repository metadata is signed with the same
key, which `ycode.repo` points at.

## A single package file

Every release also carries the `.deb` and the `.rpm` on their own, for a
machine that should not grow a new repository:

```bash
curl -LO https://github.com/vsdudakov/yara-code/releases/latest/download/ycode_X.Y.Z-1_amd64.deb
sudo apt install ./ycode_X.Y.Z-1_amd64.deb

curl -LO https://github.com/vsdudakov/yara-code/releases/latest/download/ycode-X.Y.Z-1.x86_64.rpm
sudo dnf install ./ycode-X.Y.Z-1.x86_64.rpm
```

Every Linux path installs `ycode` and `ycode-gui` into `/usr/bin` and registers
a desktop entry for the window frontend. The repositories keep the ten most
recent versions; older ones stay on their GitHub release.

### Arch Linux

A `PKGBUILD` is attached to every release and tracks the published tarball:

```bash
curl -LO https://github.com/vsdudakov/yara-code/releases/latest/download/PKGBUILD
makepkg -si
```


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

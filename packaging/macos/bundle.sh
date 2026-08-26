#!/usr/bin/env bash
# Assembles "Yara Code.app" around binaries that are already built.
#
#   packaging/macos/bundle.sh <version> <dir-with-ycode-and-ycode-gui> [out-dir]
#
# Both commands go inside the bundle: the window is what the icon starts, and
# the terminal frontend rides along so one install gives both. A cask links
# them onto the PATH from in there, the way Visual Studio Code's does.
#
# Signing is ad-hoc (`codesign -s -`). Apple Silicon refuses to run an unsigned
# binary at all, so this is the floor, not a nicety; it is not a Developer ID,
# so a download still meets Gatekeeper's "unidentified developer" on first open.
set -euo pipefail

version="${1:?usage: bundle.sh <version> <bin-dir> [out-dir]}"
bin_dir="${2:?usage: bundle.sh <version> <bin-dir> [out-dir]}"
out_dir="${3:-dist}"

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
root="$(cd "$here/../.." && pwd)"
app="$out_dir/Yara Code.app"

rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"

sed "s/@VERSION@/$version/g" "$here/Info.plist.tmpl" > "$app/Contents/Info.plist"
# The four-byte type code, which macOS still reads before the plist.
printf 'APPL????' > "$app/Contents/PkgInfo"
cp "$root/assets/icon/yara.icns" "$app/Contents/Resources/yara.icns"

for command in ycode ycode-gui; do
    cp "$bin_dir/$command" "$app/Contents/MacOS/$command"
    chmod 755 "$app/Contents/MacOS/$command"
done

codesign --force --deep --sign - "$app"
codesign --verify --strict "$app"
echo "built $app"

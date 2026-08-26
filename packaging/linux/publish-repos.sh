#!/usr/bin/env bash
# Builds the APT and DNF repositories that `apt install ycode` and
# `dnf install ycode` read, from the packages a release has just produced.
#
#   packaging/linux/publish-repos.sh <packages-dir> <site-dir> [version]
#
# <packages-dir> holds the .deb and .rpm files; <site-dir> is a checkout of the
# repository GitHub Pages serves. Both trees are rebuilt from every package in
# the pool, not only the new ones, so older versions stay installable and
# `apt install ycode=0.5.8` keeps meaning something.
#
# Signing: $REPO_GPG_KEY is an ASCII-armoured private key without a passphrase.
# Without it the trees are still built and still usable, but a client has to be
# told to trust them unverified, which the docs would rather not say — so the
# absence is loud.
set -euo pipefail

packages="${1:?usage: publish-repos.sh <packages-dir> <site-dir> [version]}"
site="${2:?usage: publish-repos.sh <packages-dir> <site-dir> [version]}"
version="${3:-}"

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

# The key lives in a home of its own, thrown away with the job, so nothing is
# left behind on a runner that other work could read.
signing=""
if [ -n "${REPO_GPG_KEY:-}" ]; then
    GNUPGHOME="$(mktemp -d)"
    export GNUPGHOME
    chmod 700 "$GNUPGHOME"
    printf '%s' "$REPO_GPG_KEY" | gpg --batch --quiet --import
    signing="$(gpg --list-secret-keys --with-colons | awk -F: '/^fpr:/ { print $10; exit }')"
    trap 'rm -rf "$GNUPGHOME"' EXIT
fi
if [ -z "$signing" ]; then
    echo "REPO_GPG_KEY is not set: building unsigned repositories" >&2
fi

# GitHub Pages serves a gigabyte and no more, and every release adds four
# packages, so the pool keeps the newest few versions and lets the rest go. The
# releases themselves keep every version that ever shipped; this is only what
# `apt install ycode=` can still reach.
KEEP_VERSIONS="${KEEP_VERSIONS:-10}"

prune() {
    local dir="$1" pattern="$2"
    [ -d "$dir" ] || return 0
    # Versions newest first, and everything past the cut goes.
    local doomed
    # An empty pool is not a failure, and neither is a grep that matches
    # nothing — under pipefail either would end the run.
    doomed=$(ls "$dir" 2>/dev/null | grep -E "$pattern" |
        sed -E 's/.*[_-]([0-9]+\.[0-9]+\.[0-9]+).*/\1/' | sort -Vru |
        tail -n "+$((KEEP_VERSIONS + 1))" || true)
    for version in $doomed; do
        # A name with no version in it would match everything.
        [ -n "$version" ] || continue
        echo "dropping $version from $dir" >&2
        find "$dir" -maxdepth 1 -name "*$version*" -delete
    done
}

# ----- APT -----------------------------------------------------------------
# Debian names the two architectures amd64 and arm64, and the package files
# already carry those names, so the pool is flat and the index is per-arch.

apt="$site/apt"
mkdir -p "$apt/pool/main"
cp -f "$packages"/*.deb "$apt/pool/main/" 2>/dev/null || true
prune "$apt/pool/main" '\.deb$'

for arch in amd64 arm64; do
    dist="$apt/dists/stable/main/binary-$arch"
    mkdir -p "$dist"
    (
        cd "$apt"
        apt-ftparchive --arch "$arch" packages pool/main > "$dist/Packages"
    )
    gzip -9cn "$dist/Packages" > "$dist/Packages.gz"
done

(
    cd "$apt"
    apt-ftparchive -c "$here/apt-release.conf" release dists/stable > dists/stable/Release.tmp
    mv dists/stable/Release.tmp dists/stable/Release
    rm -f dists/stable/InRelease dists/stable/Release.gpg
    if [ -n "$signing" ]; then
        # Both forms: InRelease is what a current apt reads, Release.gpg what
        # an older one falls back to.
        gpg --batch --yes --local-user "$signing" \
            --clearsign -o dists/stable/InRelease dists/stable/Release
        gpg --batch --yes --local-user "$signing" \
            --detach-sign --armor -o dists/stable/Release.gpg dists/stable/Release
    fi
)

# ----- DNF and YUM ---------------------------------------------------------
# RPM names the same two architectures x86_64 and aarch64, and gives each its
# own directory because a repository's metadata covers one tree.

yum="$site/yum"
for arch in x86_64 aarch64; do
    mkdir -p "$yum/$arch"
    for rpm in "$packages"/*."$arch".rpm; do
        [ -e "$rpm" ] || continue
        cp -f "$rpm" "$yum/$arch/"
    done
    prune "$yum/$arch" '\.rpm$'
    # A full pass rather than an update: pruning takes packages out, and an
    # update would leave the metadata still naming them.
    createrepo_c --quiet "$yum/$arch"
    rm -f "$yum/$arch/repodata/repomd.xml.asc"
    if [ -n "$signing" ]; then
        gpg --batch --yes --local-user "$signing" --detach-sign --armor \
            "$yum/$arch/repodata/repomd.xml"
    fi
done

# ----- What a client needs to trust it --------------------------------------

if [ -n "$signing" ]; then
    # apt wants the key as bytes for signed-by; dnf wants it armoured.
    gpg --batch --yes --export "$signing" > "$apt/ycode.gpg"
    gpg --batch --yes --armor --export "$signing" > "$yum/ycode.asc"
fi

sed -e "s/@SIGNED@/$([ -n "$signing" ] && echo 1 || echo 0)/g" \
    "$here/ycode.repo.tmpl" > "$yum/ycode.repo"

echo "built the apt and dnf trees in $site${version:+ for $version}"

#!/usr/bin/env bash
set -euo pipefail
# Reproduce the capability of stock macOS Bash even on a newer Linux shell.
if type mapfile >/dev/null 2>&1; then
    enable -n mapfile
fi
source "$(dirname "${BASH_SOURCE[0]}")/release-archives.sh"
root="$(mktemp -d)"
trap 'rm -rf "$root"' EXIT
mkdir -p "$root/dist with spaces" "$root/empty"
dist="$root/dist with spaces"
touch "$dist/pelty-server-linux.tar.gz" "$dist/pelty-server-windows build.zip"
touch "$dist/manifest.json" "$dist/SHA256SUMS" "$dist/CHANGELOG.md" "$dist/unrelated.zip"
mkdir "$dist/pelty-server-not-an-archive.tar.gz"
collect_release_archives "$dist"
test "${#RELEASE_ARCHIVES[@]}" -eq 2
test "${RELEASE_ARCHIVES[0]}" = "$dist/pelty-server-linux.tar.gz"
test "${RELEASE_ARCHIVES[1]}" = "$dist/pelty-server-windows build.zip"
if collect_release_archives "$root/empty"; then
    echo 'Empty package directory must fail' >&2
    exit 1
fi
test -z "${RELEASE_ARCHIVES[0]-}"
printf 'Release archive discovery passed without mapfile (Bash %s)\n' "$BASH_VERSION"

#!/usr/bin/env bash
# Source this file from Linux, macOS Bash 3.2, or Windows Git Bash.
# Only collect finished platform archives; never pass metadata or directories
# to gh release upload. Preserve spaces and reject an empty package directory.
collect_release_archives() {
    local file found
    found=0
    RELEASE_ARCHIVES=()
    for file in "$1"/pelty-server-*.tar.gz "$1"/pelty-server-*.zip; do
        if [ -f "$file" ]; then
            RELEASE_ARCHIVES+=("$file")
            found=1
        fi
    done
    if [ "$found" -eq 0 ]; then
        printf 'No release archives found in %s\n' "$1" >&2
        return 1
    fi
}

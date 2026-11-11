#!/usr/bin/env bash
# Install apt packages on a CI runner. The security pocket is fetched from
# the archive mirror (same packages) instead of security.ubuntu.com, which
# stopped resolving on hosted runners and made apt hang for an hour.
set -euo pipefail
for f in /etc/apt/apt-mirrors.txt /etc/apt/sources.list /etc/apt/sources.list.d/*; do
    [ -f "$f" ] || continue
    if [ "$f" = /etc/apt/apt-mirrors.txt ]; then
        sudo sed -i '/security\.ubuntu\.com/d' "$f"
    else
        sudo sed -i 's#https\?://security\.ubuntu\.com/ubuntu#http://azure.archive.ubuntu.com/ubuntu#g' "$f"
    fi
done
opts=(-o Acquire::http::Timeout=30 -o Acquire::Retries=1)
sudo apt-get update -qq "${opts[@]}" || true
sudo apt-get install -y -qq "${opts[@]}" "$@"

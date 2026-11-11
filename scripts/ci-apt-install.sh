#!/usr/bin/env bash
# Install apt packages on a CI runner. Retries, then, if security.ubuntu.com
# cannot be reached (DNS outages on hosted runners), fetches the security
# pocket from the main archive mirror, which carries the same packages.
set -uo pipefail
for i in 1 2 3; do
    sudo apt-get update -qq -o Acquire::Retries=3 || true
    if sudo apt-get install -y -qq -o Acquire::Retries=3 "$@"; then
        exit 0
    fi
    echo "apt install failed (attempt $i), retrying" >&2
    sleep 15
done
echo "falling back to the archive mirror for the security pocket" >&2
for f in /etc/apt/apt-mirrors.txt /etc/apt/sources.list /etc/apt/sources.list.d/*; do
    [ -f "$f" ] || continue
    if [ "$f" = /etc/apt/apt-mirrors.txt ]; then
        sudo sed -i '/security\.ubuntu\.com/d' "$f"
    else
        sudo sed -i 's#https\?://security\.ubuntu\.com/ubuntu#http://azure.archive.ubuntu.com/ubuntu#g' "$f"
    fi
done
sudo apt-get update -qq -o Acquire::Retries=3
sudo apt-get install -y -qq -o Acquire::Retries=3 "$@"

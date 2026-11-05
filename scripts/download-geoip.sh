#!/usr/bin/env bash
# Download DB-IP City Lite (MMDB, CC BY 4.0). No account or license key.
# https://db-ip.com/db/download/ip-to-city-lite
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
DEST_DIR="${PELTY_GEOIP_DIR:-$ROOT/data/geoip}"
DEST="$DEST_DIR/dbip-city-lite.mmdb"
MONTH="${1:-$(date -u +%Y-%m)}"
URL="https://download.db-ip.com/free/dbip-city-lite-${MONTH}.mmdb.gz"

mkdir -p "$DEST_DIR"
TMP="$(mktemp "${TMPDIR:-/tmp}/dbip-XXXXXX.mmdb.gz")"
trap 'rm -f "$TMP"' EXIT

echo "Downloading $URL"
if ! curl -fsSL --retry 2 -A "PeltyCMS-geoip/1" -o "$TMP" "$URL"; then
  # New month can lag a few days; try previous calendar month.
  PREV="$(date -u -v-1m +%Y-%m 2>/dev/null || date -u -d '1 month ago' +%Y-%m)"
  URL="https://download.db-ip.com/free/dbip-city-lite-${PREV}.mmdb.gz"
  echo "Retrying $URL"
  curl -fsSL --retry 2 -A "PeltyCMS-geoip/1" -o "$TMP" "$URL"
fi

gzip -dc "$TMP" > "${DEST}.part"
mv "${DEST}.part" "$DEST"
echo "Installed $DEST"
echo "Attribution (required): https://db-ip.com. Set PELTY_GEOIP_DB=$DEST if not using the default path."

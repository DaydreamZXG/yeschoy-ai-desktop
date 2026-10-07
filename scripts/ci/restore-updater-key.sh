#!/usr/bin/env bash
# Write one edition's Tauri/minisign updater key where local-signing.mjs looks
# for it, plus the matching .pub taken from src-tauri/update-channels.json.
# Input: UPDATER_KEY = base64 of ~/.config/yeschoy-release/updater-v2/<variant>.key
# Usage: restore-updater-key.sh official|partner
set -euo pipefail
variant="${1:?usage: restore-updater-key.sh official|partner}"
case "$variant" in official|partner) ;; *) echo "bad variant" >&2; exit 64;; esac
: "${UPDATER_KEY:?UPDATER_KEY is empty}"
repo="$(cd "$(dirname "$0")/../.." && pwd)"
dir="$HOME/.config/yeschoy-release/updater-v2"
umask 077
mkdir -p "$dir"
chmod 700 "$dir"
key="$dir/$variant.key"
if ! printf '%s' "$UPDATER_KEY" | tr -d '\r\n ' | base64 --decode > "$key" 2>/dev/null || [ ! -s "$key" ]; then
  echo "::error::UPDATER_KEY_${variant^^} is not valid base64 of the key file" >&2
  exit 1
fi
chmod 600 "$key"
node -e '
const fs = require("fs");
const registry = JSON.parse(fs.readFileSync(process.argv[1], "utf8"));
const key = registry[process.argv[2]] && registry[process.argv[2]].publicKey;
if (!key) throw new Error("no public key for " + process.argv[2]);
fs.writeFileSync(process.argv[3], key + "\n", { mode: 0o600 });
' "$repo/src-tauri/update-channels.json" "$variant" "$key.pub"
echo "Restored $variant updater key (contents not shown)."

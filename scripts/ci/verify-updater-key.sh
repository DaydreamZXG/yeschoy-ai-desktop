#!/usr/bin/env bash
# Prove the restored private key belongs to the edition's compiled public key
# by signing a throwaway file and verifying it with minisign, before any
# expensive build. Usage: verify-updater-key.sh official|partner
set -euo pipefail
variant="${1:?}"
repo="$(cd "$(dirname "$0")/../.." && pwd)"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
echo "yeschoy key probe $variant $(date +%s)" > "$work/probe.txt"
node "$repo/deploy/self-update/local-signing.mjs" sign "$variant" "$work/probe.txt" > /dev/null
node -e '
const fs = require("fs");
const r = JSON.parse(fs.readFileSync(process.argv[1], "utf8"));
fs.writeFileSync(process.argv[3], Buffer.from(r[process.argv[2]].publicKey, "base64"));
' "$repo/src-tauri/update-channels.json" "$variant" "$work/key.pub"
base64 --decode "$work/probe.txt.sig" > "$work/probe.minisig"
if ! minisign -Vm "$work/probe.txt" -x "$work/probe.minisig" -p "$work/key.pub" > /dev/null; then
  echo "::error::UPDATER_KEY_${variant^^} does not match the $variant public key in src-tauri/update-channels.json" >&2
  exit 1
fi
echo "$variant updater key matches the compiled public key."

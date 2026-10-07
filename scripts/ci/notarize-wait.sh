#!/usr/bin/env bash
# Poll notarize-local.mjs status-<kind> until Apple returns a final answer.
# Usage: notarize-wait.sh <macos-output-root> official|partner app|dmg [timeout-seconds]
set -euo pipefail
root="$1" flavor="$2" kind="$3" timeout="${4:-5400}"
repo="$(cd "$(dirname "$0")/../.." && pwd)"
deadline=$(( $(date +%s) + timeout ))
while :; do
  node "$repo/deploy/self-update/notarize-local.mjs" "$root" "$flavor" "status-$kind" > /dev/null
  status="$(node -e 'console.log(JSON.parse(require("fs").readFileSync(process.argv[1])).status)' "$root/evidence/$flavor-$kind-notarization.json")"
  echo "$(date -u +%H:%M:%SZ) $flavor $kind notarization: $status"
  case "$status" in
    Accepted) exit 0 ;;
    "In Progress") ;;
    *)
      id="$(node -e 'console.log(JSON.parse(require("fs").readFileSync(process.argv[1])).id)' "$root/evidence/$flavor-$kind-submission.json")"
      xcrun notarytool log "$id" --keychain-profile yeschoy-notary || true
      echo "::error::$flavor $kind notarization ended with status $status"
      exit 1 ;;
  esac
  if [ "$(date +%s)" -ge "$deadline" ]; then
    echo "::error::$flavor $kind notarization did not finish within ${timeout}s"
    exit 1
  fi
  sleep 30
done

#!/usr/bin/env bash
# T8 security & regression: connect code replay/expiry/origin/http downgrade + attribution/updater invariants.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$ROOT"

fail() {
  echo "T8 regression FAILED: $*" >&2
  exit 1
}

echo "== Rust: station connect security tests =="
cargo test station_connect_exchange --manifest-path "$ROOT/src-tauri/Cargo.toml"
cargo test station_connect --manifest-path "$ROOT/src-tauri/Cargo.toml"
cargo test variants_use_disjoint_fixed_feeds_and_keys --manifest-path "$ROOT/src-tauri/Cargo.toml"
cargo test origins_redirects_and_authorization_inputs_are_closed --manifest-path "$ROOT/src-tauri/Cargo.toml"

echo "== Frontend: connect.js contract tests =="
pnpm exec vitest run connect/yeschoy-connect.test.ts

echo "== Invariants: CC Switch attribution =="
grep -q "Jason Young" "$ROOT/LICENSE" || fail "LICENSE missing CC Switch attribution"
grep -q "CC Switch" "$ROOT/README.md" || fail "README missing CC Switch attribution"
test -f "$ROOT/src-tauri/src/proxy/VENDOR.md" || fail "VENDOR.md missing"
grep -qi "cc-switch" "$ROOT/src-tauri/src/proxy/VENDOR.md" || fail "VENDOR.md missing upstream reference"

echo "== Invariants: official updater channels (ergou) unchanged =="
grep -q '"endpoint": "https://ergou.qzz.io/updates/official/stable.json"' \
  "$ROOT/src-tauri/update-channels.json" || fail "official ergou endpoint missing"
grep -q '"endpoint": "https://ergou.qzz.io/updates/partner/stable.json"' \
  "$ROOT/src-tauri/update-channels.json" || fail "partner ergou endpoint missing"

echo "== Invariants: third-party connect assets must not use ergou =="
if grep -q "ergou" "$ROOT/public/connect.js"; then
  fail "public/connect.js must not reference ergou update CDN"
fi
if grep -q "ergou.qzz.io" "$ROOT/docs/station-connect-embed.md" && \
  ! grep -q "Do not\|never\|禁止\|**not**" "$ROOT/docs/station-connect-embed.md"; then
  fail "embed doc mentions ergou without prohibition context"
fi

echo "T8 security regression checks passed."

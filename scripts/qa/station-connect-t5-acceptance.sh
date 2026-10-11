#!/usr/bin/env bash
# T5 scripted acceptance: station connect activation wiring (no TLS mock required).
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"

echo "== Rust: station_connect + account line mapping =="
cargo test station_connect --manifest-path "$ROOT/src-tauri/Cargo.toml"
cargo test station_connect_line_id_maps --manifest-path "$ROOT/src-tauri/Cargo.toml"

echo "== Frontend: station connect event decoder =="
cd "$ROOT"
pnpm test:unit -- src/account/stationConnect.test.ts

echo "T5 acceptance checks passed."

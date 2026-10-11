# Station connect — local mock & E2E harness (T4)

YesChoy production website backend is **not** in this repository. T4 delivers the
**issue** API contract, a local mock station, and a browser harness that mimics
the yeschoy.com「打开桌面助手」button path.

## Artifacts

| Item | Location |
|------|----------|
| OpenAPI (issue + exchange) | [`station-connect-api.openapi.yaml`](./station-connect-api.openapi.yaml) |
| Deep-link protocol | [`station-connect-protocol.md`](./station-connect-protocol.md) |
| Mock HTTP server + harness UI | [`../tools/station-connect-mock/`](../tools/station-connect-mock/) |

## Run the mock

```bash
cd tools/station-connect-mock
cargo run
# default: http://127.0.0.1:8787/
# port: STATION_CONNECT_MOCK_PORT=9090 cargo run
```

1. Open `http://127.0.0.1:8787/` in a browser.
2. Click **模拟登录** — sets `yeschoy_desktop_session` (HttpOnly, Lax).
3. Set **HTTPS origin** (default `https://connect.localtest.me`).
4. Click **打开桌面助手** — `POST /api/desktop/v2/connect-codes` then navigates to:

   `yeschoy://connect?origin=<canonical-https-origin>&code=<one-time-code>`

The harness uses `credentials: "include"` on the issue request, matching how a
real logged-in page would call the API same-origin.

### Mock semantics (aligned with product)

- Code TTL: **300 seconds** (≤ 5 minutes).
- **Single use** — second exchange → `409` / `connect_code_already_used`.
- **Origin bind** — issue stores canonical HTTPS `origin`; exchange compares
  `https://{Host}` to that value → mismatch → `403` / `connect_origin_mismatch`.
- Tokens appear only in the **exchange** JSON body, never in the deep link.

## Why HTTPS + a hostname (not raw `127.0.0.1`)

The desktop client enforces:

- Exchange over **HTTPS** only (except unit-test loopback transport).
- Deep-link `origin` must be canonical **HTTPS** and **not** a literal IP.

So a full desktop E2E needs a hostname with TLS terminating in front of the mock.

### Example: Caddy → mock (local)

```bash
# /etc/hosts (or dnsmasq): 127.0.0.1 connect.localtest.me

# Caddyfile snippet:
connect.localtest.me {
  reverse_proxy 127.0.0.1:8787
  tls internal
}
```

Then:

1. Run `cargo run` in `tools/station-connect-mock`.
2. Open `https://connect.localtest.me/` (trust Caddy's local CA if prompted).
3. Use origin `https://connect.localtest.me` in the harness.
4. With the desktop app installed and `YESCHOY_STATION_CONNECT_EXCHANGE` unset
   (default **on**), the deep link should exchange against
   `https://connect.localtest.me/api/desktop/v2/connect-codes/exchange`.

##「打开桌面助手」button behavior (production intent)

On yeschoy.com (reference station), the button should:

1. Require an authenticated browser session (cookie).
2. `POST /api/desktop/v2/connect-codes` with `{ "origin": "https://yeschoy.com" }`
   (or rely on same-origin `Origin` header when canonical).
3. On success, set `window.location` to the deep link (no tokens in the URL).
4. If the desktop app is not installed, **degrade** to the public download page
   (GitHub Release / yeschoy.com download) — **not** `ergou.qzz.io` update CDN.

Future hosted `connect.js` (T6) will wrap steps 2–4 for third-party sites.

## curl smoke tests (issue + exchange)

```bash
BASE=http://127.0.0.1:8787
ORIGIN=https://connect.localtest.me

# Session (browser uses cookie; curl uses login + cookie jar)
curl -s -c /tmp/mock.jar -X POST "$BASE/mock/login"

CODE=$(curl -s -b /tmp/mock.jar -H 'Content-Type: application/json' \
  -d "{\"origin\":\"$ORIGIN\"}" "$BASE/api/desktop/v2/connect-codes" \
  | jq -r '.data.code')

# Exchange: Host must match ORIGIN host (use Host header when hitting loopback)
curl -s -H "Host: connect.localtest.me" -H 'Content-Type: application/json' \
  -d "{\"code\":\"$CODE\",\"deviceId\":\"00000000-0000-4000-8000-000000000001\",\"platform\":\"linux\",\"clientVersion\":\"0.4.30\"}" \
  "$BASE/api/desktop/v2/connect-codes/exchange" | jq .
```

## Desktop-only checks (no mock TLS)

- Parse / policy: `cargo test station_connect --manifest-path src-tauri/Cargo.toml`
- Disable network exchange while testing deep-link delivery:
  `YESCHOY_STATION_CONNECT_EXCHANGE=0`

## Out of scope (T5+)

- Wiring exchange success into activation UI / tool preview (T5).
- Hosted `connect.js` and third-party embed docs (T6).
- Loopback OAuth fallback (T7).
- Production yeschoy.com API deployment (separate web repo).

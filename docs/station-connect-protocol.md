# Station deep-link connect protocol (draft)

This document describes the **client-facing** shape of one-click connect from a
logged-in station website into the YesChoy desktop assistant. It complements the
internal handoff; behavior here is what site owners and client implementers
should rely on.

## Deep link

Registered custom URL scheme (OS registration is a separate delivery task):

```text
yeschoy://connect?origin=<https-origin>&code=<one-time-code>
```

| Part | Requirement |
|------|-------------|
| Scheme | `yeschoy` |
| Host | `connect` (no alternate aliases for the primary path) |
| Query `origin` | Percent-encoded **canonical HTTPS origin** of the station |
| Query `code` | One-time connect code issued by that station |

Long-lived `access_token`, `refresh_token`, or API keys **must not** appear in
the URL.

### `origin` parameter

The decoded value must be a canonical HTTPS origin:

- Scheme `https` only (no `http` downgrade).
- No `userinfo` (`user:pass@`).
- No non-default port (no `:443` suffix).
- No path beyond an optional single `/`, no `?query`, no `#fragment`.
- Host must not be a literal IP address (IPv4/IPv6).
- Host labels are normalized to lowercase in the canonical form
  (`https://example.com`).

Third-party station origins are allowed when they satisfy these rules. That
policy is **independent** of the desktop build’s compile-time authorization-page
allowlist used for device-code / OAuth authorize pages.

### `code` parameter

- Length between 32 and 512 bytes (UTF-8).
- Characters: URL-safe — ASCII letters, digits, `-`, `_`.
- Issued server-side with short TTL and single use (exchange task; not enforced
  at parse time in the desktop client).

## Suggested station HTTP API (reference)

Naming may vary per station; YesChoy reference implementation targets:

| Action | Method | Path (example) |
|--------|--------|----------------|
| Issue code (browser session) | `POST` | `/api/desktop/v2/connect-codes` |
| Exchange code (desktop) | `POST` | `/api/desktop/v2/connect-codes/exchange` |
| Desktop bootstrap (session capabilities) | `GET` | `/api/desktop/v2/bootstrap` |
| Optional in-app announcements | `GET` | path from bootstrap `announcements_path` (e.g. `/api/desktop/v2/notices`) |

Machine-readable contract: [`station-connect-api.openapi.yaml`](./station-connect-api.openapi.yaml).
Announcements shape: [`station-announcements.md`](./station-announcements.md).
Local mock + E2E harness: [`station-connect-e2e-mock.md`](./station-connect-e2e-mock.md).

### Issue endpoint (desktop T4)

```http
POST {origin}/api/desktop/v2/connect-codes
Cookie: <station session>
Content-Type: application/json
```

Optional body: `{ "origin": "https://example.com" }` (canonical HTTPS; may match
`Origin` header when omitted).

Success `data` includes `code`, `origin`, `expiresAt`, `ttlSeconds` (TTL ≤ 300s).
Codes are single-use and bound to `origin` at issuance.

Exchange must use HTTPS to the same `origin` as in the deep link. The desktop
client must verify that request host matches the declared origin.

## Site embed

Official hosted script: **`https://yeschoy.com/connect.js`** — one `<script>` tag,
same-origin issue request, then navigate to the deep link. If the desktop app is
not installed, fall back to the public download page (GitHub Release /
yeschoy.com download), not third-party update CDNs.

Copy-paste guide for New API / third-party stations:
[`station-connect-embed.md`](./station-connect-embed.md). Source:
`connect/yeschoy-connect.ts` → `public/connect.js` (`pnpm build:connect`).

## Desktop implementation status

| Task | Scope |
|------|--------|
| **T1** | Parse + validate deep link (`station_connect` Rust module), unit tests, this doc |
| **T2** | OS `yeschoy` scheme registration, single-instance / deep-link delivery |
| **T3** | HTTPS `POST …/connect-codes/exchange` client, origin policy, session install |
| **T4** | Issue API contract (OpenAPI), local mock station + harness (`docs/station-connect-e2e-mock.md`) |
| **T5** | Exchange success → account inspect (signed-in UI) + tool activation line ids; progress events |
| **T6** | Hosted `connect.js`, third-party embed docs (`station-connect-embed.md`) |
| **T7** | Loopback OAuth fallback (`oauth_pkce` / `station_connect_loopback`), feature-flagged |
| **T8** | Security regression (replay, expiry, origin tamper, HTTPS downgrade), checklist (`station-connect-security-checklist.md`) |

### Exchange endpoint (desktop T3)

The desktop client posts to:

```http
POST {origin}/api/desktop/v2/connect-codes/exchange
Content-Type: application/json
```

Example JSON body (camelCase):

```json
{
  "code": "<one-time-code>",
  "deviceId": "<uuid>",
  "platform": "macos|windows|linux",
  "clientVersion": "<semver>"
}
```

Success envelope matches the device-code token shape (`accessToken`, `refreshToken`,
`sessionId`, expiry fields). Error `code` values are mapped to stable desktop
reasons such as `connect_code_expired`, `connect_code_already_used`, and
`connect_origin_mismatch`.

Set `YESCHOY_STATION_CONNECT_EXCHANGE=0` to validate deep links without calling
the network (for example before the station API is deployed).

### Local mock station (T4)

Run `cargo run` in `tools/station-connect-mock` for issue + exchange and a browser
harness that simulates「打开桌面助手」. See [`station-connect-e2e-mock.md`](./station-connect-e2e-mock.md).
Unit tests in `station_connect_exchange` still use an axum mock on `127.0.0.1` for
transport mapping; production exchange remains HTTPS-only on the declared origin.

## Security notes

- Treat deep-link URLs as sensitive while `code` is valid; do not log full URLs
  in production diagnostics.
- Reject malformed origins before any network call.
- Failed exchange must not leave partial tool configuration (activation
  rollback semantics apply once exchange is wired).

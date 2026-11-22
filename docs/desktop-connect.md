# desktop-connect: the station data contract (v1)

How a relay station lets a desktop client show an account, prices, keys, usage
and notices after the user has signed in. It continues
[`station-connect-protocol.md`](./station-connect-protocol.md), which covers the
sign-in itself (the `/desktop/connect` page and the one-time connect code).

Nothing here names a product. Any station can implement it; a client recognises
a station by what its bootstrap declares, never by its domain.

## Three depths of integration

| Depth | The station does | The user connects by | The user gets |
|---|---|---|---|
| 1. Nothing | — | Pasting the address and a key | Activation, models, whatever the station's key API exposes |
| 2. This contract | One page and the endpoints below | Pasting the address, approving in the browser | Plans and prices, keys made for each app, usage, notices, client-only prices |
| 3. A reference implementation | Upgrades or applies a patch | Same as 2 | Same as 2 |

## Conventions

- HTTPS only. Every path is on the station's own origin.
- Replies are JSON: `{"success": true, "data": …}` or
  `{"success": false, "code": "…"}` with a matching HTTP status.
- Signed-in endpoints take `Authorization: Bearer <access token>`.
- **Money is ready to show.** The station converts its internal units and says
  which `currency` (`CNY` or `USD`) the numbers are in. Prices are per million
  tokens.
- Ids are strings.

## Bootstrap

`GET /api/desktop/v2/bootstrap` (no sign-in)

```json
{
  "schema_version": 2,
  "service": "desktop-connect",
  "contract_id": "desktop-connect-v1",
  "minimum_client_version": "0.0.0",
  "account_read": true,
  "usage_read": true,
  "models_read": true,
  "pricing_read": true,
  "tool_keys_manage": true,
  "session_refresh_path": "/api/desktop/v2/sessions/refresh",
  "session_logout_path": "/api/desktop/v2/sessions/current",
  "account_path": "/api/desktop/v2/account",
  "plans_path": "/api/desktop/v2/plans",
  "keys_path": "/api/desktop/v2/keys",
  "usage_summary_path": "/api/desktop/v2/usage/summary",
  "usage_records_path": "/api/desktop/v2/usage/records",
  "announcements_path": "/api/desktop/v2/notices",
  "app_tokens_path": "/api/desktop/v2/app-tokens",
  "app_token_header": "X-Desktop-App-Token",
  "wallet_url": "https://station.example/topup"
}
```

The client refuses a bootstrap whose data paths are not same-origin relative
paths, whose `wallet_url` is on another origin, or whose capabilities are not
all `true`. `app_tokens_path` and `app_token_header` are optional: leave them
out if the station has no client-only prices. `announcements_path` is optional
too.

## Session

| Call | Body | Reply `data` |
|---|---|---|
| `POST /api/desktop/v2/connect-codes/exchange` | see the sign-in protocol | token bundle |
| `POST {session_refresh_path}` | `{"refresh_token", "session_id"}` | token bundle |
| `DELETE {session_logout_path}` | — | — |

A token bundle is `token_type` (`Bearer`), `access_token`, `refresh_token`,
`session_id`, `access_expires_at`, `refresh_expires_at` (epoch seconds).
Answer `401 session_expired` only when the session is really over; answer
`503 service_unavailable` for an outage, so the client keeps the user signed in
and retries.

## Account

`GET {account_path}`

```json
{"username": "…", "displayName": "…", "email": "…", "currency": "CNY", "balance": 12.5}
```

## Plans

`GET {plans_path}` — the plans this account can create a key on.

```json
{
  "currency": "CNY",
  "plans": [{
    "id": "7",
    "name": "Standard",
    "description": "…",
    "formats": ["anthropic", "openai-chat", "openai-responses", "gemini"],
    "subscription": false,
    "appOnly": false,
    "peak": [{"start": "09:00", "end": "18:00", "multiplier": 1.5, "timezone": "Asia/Shanghai", "weekdaysOnly": false}],
    "models": [{
      "id": "model-id",
      "price": {"input": 10, "output": 50, "cacheRead": 1, "cacheWrite": 2, "perRequest": 0.2},
      "official": {"input": 20, "output": 100},
      "longContext": [{"fromTokens": 200000, "toTokens": 272000, "price": {"input": 20, "output": 75}}]
    }]
  }]
}
```

- `price` is what this account pays on this plan. `official` is the upstream
  list price, when the station knows it; the client shows the discount from the
  two.
- Surcharges are stated apart (`peak`, `longContext`) rather than folded into a
  name, so a client can describe them in the user's language.
- `formats` is which request formats the plan accepts.
- `subscription` marks a plan paid for by a pass rather than by balance.
- `appOnly` marks a client-only price (below).

## Keys

One key belongs to one plan.

| Call | Body | Reply `data` |
|---|---|---|
| `GET {keys_path}` | — | `{"keys": [key…]}` |
| `POST {keys_path}` | `{"name", "planId"}` | key, plus `"key": "<secret>"` |
| `POST {keys_path}/{id}/reveal` | — | `{"key": "<secret>"}` |
| `DELETE {keys_path}/{id}` | — | — |

A key is `{"id", "name", "planId", "status": "active" | …, "createdAtEpochMs"}`.
Error codes: `plan_not_allowed` (403), `key_limit_reached` (409),
`rate_limited` (429), `key_not_found` (404).

## Usage

`GET {usage_summary_path}`

```json
{"currency": "CNY", "windowDays": 30, "consumed": 1.25, "requests": 8, "tokens": 900}
```

`GET {usage_records_path}?page=1&pageSize=100` (newest first, `pageSize` ≤ 100)

```json
{
  "currency": "CNY",
  "records": [{
    "id": "1", "atEpochMs": 1800000000000, "model": "model-id",
    "planId": "7", "keyId": "1", "keyName": "…",
    "inputTokens": 100, "outputTokens": 20, "cacheTokens": 5, "amount": 0.75
  }],
  "total": 1, "page": 1, "pageSize": 100
}
```

## Notices

`GET {announcements_path}` — see
[`station-announcements.md`](./station-announcements.md).

## Client-only prices

A station may charge less for requests that come through a desktop client.

1. After sign-in the client calls `POST {app_tokens_path}` with
   `{"deviceId", "platform", "clientVersion"}` and receives
   `{"appToken", "deviceId"}`. Issuing again for the same `deviceId` replaces
   the earlier credential.
2. The client keeps the credential in the OS credential store and sends it in
   `{app_token_header}` on the model requests it forwards to this station — and
   to no other origin.
3. The station serves a key on an `appOnly` plan at that plan's price when the
   header carries a live credential of the key's owner. Without it the station
   decides: another price, or a refusal.

The credential identifies a device, not a user: it grants nothing by itself and
must not be accepted in place of a key or a session. Store only its hash, and
do not pass the header on to upstream providers.

## Reference implementations

- sub2api: `backend/internal/handler/desktop_*.go`, `frontend` route
  `/desktop/connect`.
- new-api: the sign-in part is in [`patch/new-api-desktop-connect`](./patch/new-api-desktop-connect/README.md).
  Stations on the earlier contract (`service: "yeschoy-desktop"`,
  `contract_id: "desktop-integration-v2"`) keep working unchanged.

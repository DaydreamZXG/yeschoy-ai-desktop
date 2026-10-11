# Station-sourced desktop announcements

Desktop assistants that speak **desktop-integration-v2** can show in-app announcements
from the **currently connected station** (compiled relay line or station-connect
origin). The OSS client does not ship marketing copy or a default notices URL;
if the station does not advertise a path, the sidebar omits **Announcements**.

Chinese field-by-field reference for operators already on YesChoy-style stacks:
[`announcements-contract-zh.md`](./announcements-contract-zh.md).

## Bootstrap: optional `announcements_path`

After sign-in, the client loads:

```http
GET {station-origin}/api/desktop/v2/bootstrap
```

In the `data` object, add an optional string field:

```json
"announcements_path": "/api/desktop/v2/notices"
```

Rules enforced by the client:

- Omit the field or send `""` → no announcements UI (no error).
- Value must be a **same-origin relative path** (`/…`, ≤256 chars, `[a-zA-Z0-9/_.-]` only, no `..`).
- Absolute URLs or scheme-relative paths are ignored (treated as absent).

`station-origin` is the HTTPS API origin the session is bound to (for example
`https://yeschoy.com`, `https://api.yeschoy.com`, or `https://your-station.example`
after station connect).

## Notices API

```http
GET {station-origin}{announcements_path}
Authorization: Bearer <desktop session access token>
Accept: application/json
```

- Same bearer session as other desktop v2 reads.
- JSON body ≤ 2 MB; non-2xx → client keeps prior notices and shows retry; **does not** affect sign-in or tool activation.

### Response shape

```json
{
  "success": true,
  "data": {
    "notices": [
      {
        "id": "maint-2026-10",
        "title": "Scheduled maintenance",
        "body": "Short plain text. Use \\n for paragraphs. **bold** only.",
        "severity": "info",
        "publishedAtEpochMs": 1759276800000,
        "expiresAtEpochMs": 0,
        "banner": true,
        "action": { "kind": "wallet", "label": "Top up" }
      }
    ]
  }
}
```

| Field | Notes |
|-------|--------|
| `id` | Stable id for “read” state on device; duplicates dropped. |
| `title` | Required; empty titles dropped. |
| `body` / `content` | Optional body; `body` wins if both set. |
| `severity` | `info` (default) or `warning`. |
| `publishedAtEpochMs` / `expiresAtEpochMs` | Milliseconds; `0` expiry = never auto-hide. |
| `banner` | Home banner while unread (first eligible notice in list order). |
| `action` | Optional button: `wallet` (station wallet URL from bootstrap) or `link` with `url`. |

`link` URLs must be `https`, no credentials/port, ≤2048 chars, host either:

- Official YesChoy hosts (`yeschoy.com`, `*.yeschoy.com`, `ai.yeschoy.io`), or
- The **same host** as `{station-origin}` (for third-party stations).

Invalid `action` values are stripped; the notice still renders.

## Client refresh cadence

- Once after sign-in.
- Every 30 minutes while signed in.
- When the window becomes visible again if the last fetch was more than 5 minutes ago.

Read state is stored locally only (not synced across devices).

## Local E2E mock

[`tools/station-connect-mock/`](../tools/station-connect-mock/) serves bootstrap with
`announcements_path` and a sample `GET /api/desktop/v2/notices` when exercised
through TLS + hostname (see [`station-connect-e2e-mock.md`](./station-connect-e2e-mock.md)).

## Checklist for station owners

- [ ] `announcements_path` present on bootstrap when you want the entry shown.
- [ ] Notices endpoint returns 200 + JSON with bearer auth.
- [ ] `id` values are stable for a given campaign.
- [ ] `link` buttons use https on your station host or allowed official hosts.
- [ ] Desktop: sidebar **Announcements**, optional home banner, wallet button opens your `/wallet/` URL from bootstrap.

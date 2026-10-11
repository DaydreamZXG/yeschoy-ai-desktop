# Station connect embed (`connect.js`)

Third-party **New API** stations (and yeschoy.com) can let logged-in users open the
YesChoy desktop assistant with one click — no API key paste in the main flow.

Protocol details: [`station-connect-protocol.md`](./station-connect-protocol.md).  
HTTP contract: [`station-connect-api.openapi.yaml`](./station-connect-api.openapi.yaml).

## One-line script

Host the official bundle at **`https://yeschoy.com/connect.js`** (source and build
live in this repo under `public/connect.js`).

```html
<script src="https://yeschoy.com/connect.js" defer></script>
```

Optional: override the missing-app download target (must remain a **public** install
page — never an update CDN such as `ergou.qzz.io`):

```html
<script
  src="https://yeschoy.com/connect.js"
  defer
  data-download-url="https://yeschoy.com/download"
></script>
```

## Button markup

Recommended primary label (Chinese reference station):

| Locale | Copy |
|--------|------|
| zh-CN | **打开桌面助手** |
| en | **Open YesChoy desktop** |

Minimal HTML:

```html
<button type="button" data-yeschoy-connect>打开桌面助手</button>
```

The script auto-binds every `[data-yeschoy-connect]` element after load.

### Programmatic use

```javascript
await window.YeschoyConnect.open();
// optional: window.YeschoyConnect.open({ origin: "https://your-station.example" })
```

Listen for failures on the button:

```javascript
document.querySelector("[data-yeschoy-connect]").addEventListener(
  "yeschoy-connect-error",
  (event) => {
    const err = event.detail;
    // err.reasonCode e.g. session_required, connect_origin_not_https
  },
);
```

## What the script does

1. Reads the current page **HTTPS** origin (`window.location.origin`).
2. `POST /api/desktop/v2/connect-codes` on the **same origin** with `credentials:
   include` (session cookie).
3. Navigates to `yeschoy://connect?origin=…&code=…` (one-time code only — **no**
   long-lived tokens in the URL).
4. If the desktop app is probably not installed (page stays focused), redirects to
   [`https://yeschoy.com/download`](https://yeschoy.com/download) or
   [GitHub Releases](https://github.com/DaydreamZXG/yeschoy-ai-desktop/releases).

## Station backend requirements

Implement the v2 connect-code endpoints on **your** origin (see OpenAPI):

| Step | Who | Endpoint |
|------|-----|----------|
| Issue | Browser (cookie session) | `POST /api/desktop/v2/connect-codes` |
| Exchange | Desktop app (HTTPS) | `POST /api/desktop/v2/connect-codes/exchange` |

Codes must be single-use, short TTL (≤ 5 minutes), and bound to the canonical HTTPS
`origin` you return from the issue call.

## CORS and same-origin

- The hosted `connect.js` file may be loaded from `yeschoy.com`, but **issue** must
  be a **same-origin** request to the station that issued the user session.
- Do **not** call another site's issue API from the browser with a third-party
  cookie; host your own issue endpoint on the same host as your login session.
- You do **not** need CORS for issue when the script tag is on the same site as the
  API (typical New API dashboard). If you split admin UI and API hosts, keep issue
  on the host that holds the session cookie or proxy it same-origin.

## Missing desktop app

Users without the app installed should get a **public installer** link:

- Primary: [yeschoy.com/download](https://yeschoy.com/download)
- Alternative: [GitHub Releases](https://github.com/DaydreamZXG/yeschoy-ai-desktop/releases)

**Do not** send third-party station users to `ergou.qzz.io` or other official-only
update infrastructure. Self-built clients should document GitHub (or their own)
update channels separately; that is out of scope for this embed.

## Local development

- Mock station + harness: [`station-connect-e2e-mock.md`](./station-connect-e2e-mock.md)
- Rebuild the hosted bundle: `pnpm build:connect`

## Self-hosting the script (optional)

You may copy `public/connect.js` to your own static host **only if** you still call
**your** origin's issue API (same-origin). Prefer the official URL so sites pick up
protocol fixes without redeploying.

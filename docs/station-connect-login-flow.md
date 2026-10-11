# Desktop ↔ station login / connect flows (as implemented)

Reference for engineers debugging UX around「我的应用」、站点 `/desktop/connect`、与 `account_inspect`.
Derived from code and `docs/station-connect-protocol.md` — not a product spec.

## 1) Cold start, no keyring session (official build)

| Step | What happens |
|------|----------------|
| Boot | `App` mounts `useAccountSession(accountLineId)` (`src/App.tsx`). Default `accountLineId` is `mainland_optimized`. |
| Session hook | **`useAccountSession`** (`src/account/useAccountSession.ts`) runs a mount `useEffect` that calls `execute("account_inspect_v2")` unless skipped (see §5 / PR #46 follow-up). |
| Native inspect | **`account_inspect_v2`** → **`inspect_inner`** (`src-tauri/src/account_v2.rs`): `fetch_bootstrap(line_id)` (HTTPS), then **`refresh_access`**. With no keyring entry, **`load_stored_session`** is empty → **`SessionExpired`** → projection **`signed_out`** (or **`authorization_unavailable`** if bootstrap says device auth off). |
| UI | **`showStationConnectWelcomeStrip`** (`src/workbench/AppLibraryView.tsx`) is true for `signed_out` / missing status → welcome strip + **`StationConnectOriginForm`**. Parallel: **`scanActivationTargets`** runs for app cards (separate from account). |
| Official-only | **`isOfficialDistribution()`** (`src/distribution/identity.ts`) hides OSS soft-traffic CTA; form still prefills **`REFERENCE_STATION_ORIGIN`** (`src/distribution/softTraffic.ts` → `https://yeschoy.com`). |

**PR #46:** if **`account_session_snapshot_v2`** reports no keyring session, **skip** `account_inspect_v2` on startup; set **`idleSignedOutProjection`** and `loading: false` immediately. Manual **`refresh()`** still inspects.

**Stale keyring follow-up:** when a session exists in the keyring, startup still runs **`account_inspect_v2`** once in the background (`silent` + `startup` options in **`useAccountSession`**): `loading` stays false so「我的应用」welcome + **`StationConnectOriginForm`** stay clean; no **`stationConnectPage.accountChecking`** hint on the origin form. Startup inspect does **not** use the bounded retry schedule (manual **`refresh()`** still retries transient failures). Native requests remain capped at **`REQUEST_TIMEOUT`** (20s) per attempt in **`account_v2.rs`**.

## 2) User enters station origin on「我的应用」

| Step | Code |
|------|------|
| Form | **`StationConnectOriginForm`** (`src/workbench/StationConnectOriginForm.tsx`): validates with **`canonicalStationHttpsOrigin`** (`connect/stationConnectPage.ts`). |
| Open browser | **`openStationConnectPage`** → IPC **`station_connect_open_connect_page_v1`** opens `{origin}/desktop/connect?auto=1`. Page **`connect/entry.ts`** auto-issues + deep-links when the site session cookie is present; button stays for sign-in / retry. |
| Line switch | **`onLineChange(lineIdForStationConnectOrigin(origin))`** → `App` updates **`accountLineId`** (synthetic id `station_connect:https://…` or catalog line for canonical origins — see **`line_id_for_station_connect_origin`** in Rust). |

Draft origin is stored in `localStorage` key **`yeschoy.stationConnectOriginDraft`**.

## 3) Browser `/desktop/connect` → connect-codes → `yeschoy://` → exchange

| Step | Code |
|------|------|
| Page | **`public/desktop/connect.html`** loads **`https://yeschoy.com/connect.js`** (built from **`connect/yeschoy-connect.ts`** via `pnpm build:connect`). Button **`data-yeschoy-connect`** → **`createYeschoyConnect().bindAuto`**. |
| Issue code | **`issueConnectCode`**: `POST /api/desktop/v2/connect-codes` with `credentials: "include"`. Errors → **`YeschoyConnectError`** with **`reasonCode`** (normalized in PR #46). |
| Deep link | **`buildConnectDeepLink`** → `yeschoy://connect?origin=…&code=…`; **`navigateDeepLinkWithDownloadFallback`** opens scheme, else download URL. |
| Desktop receive | **`parse_station_connect_from_argv`** / deep-link plugin (`src-tauri/src/station_connect.rs`) → **`run_station_connect_exchange`**. |
| Exchange | **`exchange_connect_code`** (`station_connect_exchange.rs`) `POST …/connect-codes/exchange` on the station origin. |
| Install session | **`install_station_connect_session`** (`account_v2.rs`) writes keyring + runtime; **`account_project_line_after_connect`** runs inspect for UI. |
| Renderer feedback | Event **`yeschoy://station-connect-progress`** → **`useStationConnect`** (`src/account/useStationConnect.ts`) toast + **`accountSession.refresh()`**. |

## 4)「网页登录」/ device authorization (still present)

| Entry | Flow |
|-------|------|
| Account page | **`AccountView`**: primary **`beginAuthorization()`** → **`account_begin_authorization_v2`** (`account_v2.rs`): bootstrap, `POST` device authorization, stores **`runtime.pending`**, opens browser to `https://yeschoy.com/desktop-authorize?…`. |
| Poll | **`useAccountSession`** polls **`account_poll_authorization_v2`** while `projection.status === "authorization_pending"`. |
| Loopback OAuth | Optional **`beginStationConnectLoopbackOauth`** (`src/account/stationConnectLoopback.ts`) when capabilities allow — alternate browser login for station lines. |
| Catalog lines | `mainland_optimized` / `global_accelerated` use connectivity catalog origins; third-party stations use station-connect line ids (§2). |

Station connect (§3) and device authorization (§4) are **parallel** ways to obtain a keyring session; connect does not require a prior successful `account_inspect`.

## 5) Where `account_inspect` runs and what greyed the UI

| Trigger | Location |
|---------|----------|
| **App start** | No keyring session: skip inspect (§1). Keyring session present: one silent `account_inspect_v2` (no startup retries). |
| **Manual refresh** | `accountSession.refresh()` — library account banner, account page, station-connect failure retry, etc. |
| **After deep link** | **`useStationConnect`** on `signed_in` progress phase. |
| **Line change** | `execute` dependency on `lineId` re-runs mount effect. |

**Grey / blocked UI (before PR #46):**

- **`AppLibraryView`** passed **`disabled={accountSession.loading}`** to **`StationConnectOriginForm`** →「在浏览器中继续」disabled while inspect (+ retries, up to ~2.5s+) ran.
- **`AccountView`**: **`signInDisabled`** included **`loading`**; initial **`loading && !projection`** showed account skeleton instead of sign-in.

**After PR #46:** form not disabled by loading; sign-in not disabled by loading; **`AUTH_UNAUTHORIZED`** mapped to sign-in copy (§6).

**Welcome strip:** origin form no longer shows **`stationConnectPage.accountChecking`**; startup inspect with a stored session does not set **`accountSession.loading`**. Account page may still show the same copy under sign-in while a non-silent **`refresh()`** runs.

## 6) Why live `AUTH_UNAUTHORIZED` became「连接未完成」

Unauthenticated `POST /api/desktop/v2/connect-codes` returns `code: "AUTH_UNAUTHORIZED"` (not OpenAPI example `session_required`).

**Before fix:** **`issueConnectCode`** threw **`YeschoyConnectError("AUTH_UNAUTHORIZED")`**. **`connect.html`** message map only defined `session_required`, `issue_failed`, etc. → **`messages[code] || messages.generic`** → Chinese **`generic`**:「连接未完成，请重试。」— reads like a broken deep link, not「请先登录本站」.

**After PR #46:** **`normalizeConnectIssueReasonCode`** in **`yeschoy-connect.ts`**; **`connect.html`** **`uiReasonCode`** fallback; bundle **`public/connect.js`** rebuilt.

---

## Intended happy path (user language)

1. Open desktop →「我的应用」shows site URL field immediately (no wait for account probe when never logged in).
2. Sign in on the website if needed → open `/desktop/connect` →「打开桌面助手」.
3. Browser issues code → desktop opens via `yeschoy://` → exchange → signed in; apps can be connected.
4. Next launches: desktop briefly validates existing keyring session via `account_inspect` (then shows balance / library as signed in).

Alternative: Account →「网页登录」→ approve device code in browser (canonical YesChoy lines).

---

## PR #46 status

| Item | Status |
|------|--------|
| No keyring session ⇒ skip startup `account_inspect` | **`account_session_snapshot_v2`** + **`useAccountSession`** |
| Stale keyring ⇒ silent single startup inspect, no welcome checking chrome | **`useAccountSession`** `silent` / `startup` |
| Form / sign-in not blocked by `loading` | Done |
| Relay line preference (`localStorage` + stored session line) | **`accountLinePreference.ts`**, **`App.applyAccountLine`** |
| Connect `?auto=1` + **`AUTH_UNAUTHORIZED`** mapping | Done, **`connect.js`** rebuilt |

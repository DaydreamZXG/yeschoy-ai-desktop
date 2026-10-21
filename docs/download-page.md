# Desktop download page (`/download`) and site root (`/`)

Public install entry for YesChoy desktop. Host this repository’s static assets on
**`https://yeschoy.app`** (site root landing) and **`https://yeschoy.app/download`**
(minimal download card). Partner edition may still use
`https://ai.yeschoy.io/download?variant=partner`.

Visual language matches the desktop client (`src/theme/new-api.css`): lime `#c9fa10`
+ near-black `#111111`, not the legacy warm-paper / vermilion cards.

## Files in this repo

| Asset | Role |
| --- | --- |
| [`public/index.html`](../public/index.html) | One-page landing: brand, three steps, download, connect, open source, privacy |
| [`public/site/theme.css`](../public/site/theme.css) | Shared tokens aligned with client UI |
| [`public/download/index.html`](../public/download/index.html) | Minimal Chinese download card |
| [`public/desktop/connect/index.html (also connect.html)`](../public/desktop/connect/index.html (also connect.html)) | Station one-click connect page (`/desktop/connect`) |
| [`public/download.js`](../public/download.js) | Built from [`download/entry.ts`](../download/entry.ts) (`pnpm build:download`) |
| [`public/connect.js`](../public/connect.js) | Built from [`connect/entry.ts`](../connect/entry.ts) (`pnpm build:connect`) |
| [`download/yeschoyDownload.ts`](../download/yeschoyDownload.ts) | URL resolution logic (unit-tested) |

## Path map (yeschoy.app)

| Path | Content |
| --- | --- |
| `/` | Landing page |
| `/download/` | Minimal download page |
| `/desktop/connect` | One-click connect (same host as New API session cookie) |
| `/download.js` / `/connect.js` | Hosted bundles |
| `/site/theme.css` | Shared theme |

Default public download URL in code: `https://yeschoy.app/download`.

## Resolution order (safe for ergou updates)

1. **GitHub Releases** — latest release where `draft === false` and `published_at` is set.
   Installers must match `yeschoy-<version>-{official|partner}-windows-x86_64-installer.exe`
   and `…-macos-universal-installer.dmg`.
2. **ergou manifest** — `https://ergou.qzz.io/releases/yeschoy-official.json` (or
   `yeschoy-partner.json`).
3. **Permanent ergou aliases** — `https://ergou.qzz.io/releases/{variant}/yeschoy-…-installer.*`

The in-app Tauri updater **still uses** `src-tauri/update-channels.json` →
`ergou.qzz.io/updates/{official|partner}/stable.json`. This page does not change
that channel.

### GitHub `v0.5.1` draft

A **draft** GitHub release may exist without public installer URLs. Until it is
**published**, the download page correctly falls back to ergou (currently serving
0.5.1). Publishing the GitHub release is required before GitHub becomes the
primary source for end users.

## Operator checklist

1. Run `pnpm build:download` and `pnpm build:connect`; deploy the whole `public/` tree
   (including `index.html`, `site/`, `download/`, `desktop/`).
2. Keep `connect.js` / station pages pointing at `https://yeschoy.app/download` (or a
   same-origin `/download/`), not ergou update CDNs
   ([`station-connect-embed.md`](./station-connect-embed.md)).
3. After publishing a new GitHub release, smoke-test the page: primary button should
   show “GitHub” in the subtitle.
4. Partner edition: serve the same HTML on `ai.yeschoy.io` or pass `?variant=partner`.

## Local smoke test

```bash
pnpm build:download
pnpm build:connect
pnpm exec vite preview --outDir public --port 4173
# open http://127.0.0.1:4173/
# open http://127.0.0.1:4173/download/
# open http://127.0.0.1:4173/desktop/connect.html
```

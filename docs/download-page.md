# Desktop download page (`/download`)

Public install entry for YesChoy desktop. Host this repository’s static assets on
**`https://yeschoy.com/download`** (and optionally
`https://ai.yeschoy.io/download?variant=partner` for the partner edition).

## Files in this repo

| Asset | Role |
| --- | --- |
| [`public/download/index.html`](../public/download/index.html) | Minimal Chinese page: one primary button per detected OS |
| [`public/download.js`](../public/download.js) | Built from [`download/entry.ts`](../download/entry.ts) (`pnpm build:download`) |
| [`download/yeschoyDownload.ts`](../download/yeschoyDownload.ts) | URL resolution logic (unit-tested) |

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

### GitHub `v0.5.0` draft

A **draft** GitHub release may exist without public installer URLs. Until it is
**published**, the download page correctly falls back to ergou (currently serving
0.5.0). Publishing the GitHub release is required before GitHub becomes the
primary source for end users.

## Operator checklist

1. Run `pnpm build:download` and deploy `public/download/` + `public/download.js`.
2. Keep `connect.js` / station pages pointing at `https://yeschoy.com/download`, not
   ergou update CDNs ([`station-connect-embed.md`](./station-connect-embed.md)).
3. After publishing a new GitHub release, smoke-test the page: primary button should
   show “GitHub” in the subtitle.
4. Partner edition: serve the same HTML on `ai.yeschoy.io` or pass `?variant=partner`.

## Local smoke test

```bash
pnpm build:download
pnpm exec vite preview --outDir public --port 4173
# open http://127.0.0.1:4173/download/
```

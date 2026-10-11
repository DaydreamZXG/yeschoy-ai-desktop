# Station connect — security & regression checklist (T8)

Use this before merging deep-link / connect changes or cutting a release candidate.
Automated coverage: `scripts/qa/station-connect-t8-security-regression.sh`.

## Connect code & origin

- [ ] One-time codes cannot be exchanged twice (`connect_code_already_used` / `connect_code_reused`).
- [ ] Expired codes return `connect_code_expired` (mock TTL ≤ 5 minutes; desktop maps stable reason codes).
- [ ] Deep-link `origin` must be canonical HTTPS (no `http://`, path, query, userinfo, literal IP).
- [ ] Server rejects origin tamper (`connect_origin_mismatch`) when Host/origin does not match issuance.
- [ ] Desktop exchange uses HTTPS-only HTTP client in production (`https_only`); loopback mocks are test-only.
- [ ] Long-lived tokens never appear in `yeschoy://` query parameters or connect.js deep links.

## Regression (do not break)

- [ ] Device-code path (`account_v2` device-authorizations) still works; compile-time authorization origins unchanged unless explicitly scoped.
- [ ] Official updater feeds in `src-tauri/update-channels.json` still point at `ergou.qzz.io` **only** for `official` / `partner` variants.
- [ ] Third-party / self-built docs (`docs/station-connect-embed.md`, `public/connect.js`) do **not** send users to `ergou.qzz.io` for updates or downloads.
- [ ] `LICENSE` retains CC Switch (Jason Young) copyright line.
- [ ] `README.md` / `CONTRIBUTING.md` CC Switch attribution and `src-tauri/src/proxy/VENDOR.md` remain while vendored proxy files exist.

## Explicitly out of scope (need user sign-off)

- [ ] Publish GitHub Release `v0.5.0` (draft exists — download page stays on ergou until published; see [`download-page.md`](./download-page.md)).
- [ ] Production deploy of yeschoy.com connect-code issue/exchange APIs.
- [ ] Migrating official auto-update primary feed to GitHub (ergou remains official domestic fallback only).

## Local commands

```bash
# Security-focused unit tests (Rust + connect.js helpers)
./scripts/qa/station-connect-t8-security-regression.sh

# Full station-connect parse / line-id regression (T5)
./scripts/qa/station-connect-t5-acceptance.sh

# Mock station E2E (issue + exchange + harness UI)
# See docs/station-connect-e2e-mock.md
```

Simulate a deep link (desktop installed):

```text
yeschoy://connect?origin=https%3A%2F%2Fexample.com&code=<32+ char url-safe code>
```

Use `tools/station-connect-mock` for a local HTTPS-shaped origin policy test without production credentials.

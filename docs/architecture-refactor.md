# Modular architecture refactor

## Baseline (2026-10-29)

This is an incremental refactor of the working tree, not a rewrite. Existing uncommitted bridge, Cursor, health/feedback, settings and localization changes belong to the user and are retained. No real tool configuration, OS credentials or paid inference endpoints are used for verification.

- `cargo fmt --check` and `cargo check --locked`: passed.
- Native CI runner: **799 passed, 3 declared hardware-dependent tests ignored**.
- `pnpm typecheck` and `pnpm build:renderer`: passed.
- Original `pnpm test:unit`: 135 files, 1368 tests, 9 failures, all in an untracked nested checkout. The root checkout's tests passed. Vitest is now restricted to root `src`, `connect`, `download` sources, without deleting the user's nested checkout.
- Host toolchain: Rust 1.95.0, pnpm 10.12.3, Node 26.10.0 (the project recommends Node 22). Existing Browserslist/module chunk warnings are baseline warnings, not newly resolved issues.
- Trellis scripts exist in the parent workspace, but initialization lacks developer identity and a local workflow file. The user elected session todos instead of a new Trellis task.

## Observed dependencies before changes

```text
React ConfigurationPreviewView
  -> account/session, models/billing, installation, activation task, connections
  -> direct read-only connectivity IPC + discovery/configuration IPC decoders
Tauri lib.rs (composition, shutdown, command registration)
  -> tool_activation (IPC DTOs + admission/cancellation + use case)
     -> account_v2 (session, /api/user/models, /api/pricing, /api/token)
     -> each concrete tool adapter + desktop lifecycle + login/history takeover
     -> tool_credentials (keyring chunks/generation publication)
     -> connection_recovery (AES-GCM baseline + pending checkpoint + three-way merge)
Tool adapters -> common (file transaction + file safety/IO + process/runtime utilities)
connection_recovery -> tool adapters/common AND DSH format-specific restore helpers
local bridges -> tool credential routing + shared local runtime + protocol conversion
```

The issue is not module size alone. Token protocol changes, tool configuration changes and lifecycle rollback changes currently touch the same command. Recovery infrastructure also calls tool-specific DSH merge code, a real reverse dependency. Seven adapters duplicate config-error mappings and wrap the same byte transaction. The preview page mixes remote read state, submission snapshots, model selection, feedback effects and presentation, despite already having useful separate task/connection hooks.

## Security boundaries to retain

- Preview/discovery/diagnostics do not mutate configuration.
- Explicit user consent precedes application shutdown and Claude login displacement.
- Global async admission and OS-released process locks serialize configuration writes.
- A durable encrypted pending recovery record precedes credential/file publication.
- File version checks, atomic replacement, immediate readback and conflict-preserving rollback remain authoritative.
- Failed local restoration must retain tokens/checkpoints when live files may still use them.
- Remote APIs, keyring and files are compensated resources, not one database transaction.
- Scoped tokens are reused only for exact group/model/device ownership; superseded tokens retire only after local success.
- Credential identity validation must allow the same canonical third-party origins as station connection, without consulting a mutable trust store during recovery.
- IPC names, strict keys, schema versions and native-only paths remain compatible.

## Verified risks discovered in source

1. `tool_credentials::record_is_valid` accepts only two built-in origins whereas `station_trust` accepts approved third-party stations. This rejects otherwise valid third-party activations and recovery credentials.
2. Trust normalization takes `Url::origin` before checking userinfo/explicit port, losing evidence that the user's input was unsafe.
3. `log_token_rejection` logs free-form station response text. A server can place credentials in that text; logs should contain stable operation/status only.
4. Existing JSON5/JSONC parsers often reserialize entire JSON documents, retaining values but losing comments and layout. Exact baseline recovery does not make activation writes formatting-preserving.
5. Existing Codex activation probes explicitly generate up to 16 tokens. This contradicts the read-only/no-paid-verification requirement. Do not run real activation during tests; migration must preserve routing without silently issuing paid probes.

## Implemented boundaries

```text
domain/                 model binding + request envelope rules, closed errors, file-change intent,
                        canonical station identity (std + serde only)
application/ports/      ConfigFiles / ConfigCommit / ConfigurationPlan, StationProvider
application/activation/ request (DTO), tokens, capability, lifecycle, service (use case,
                        explicit ActivationContext), session, result (IPC DTO), error
adapters/stations/      NewApiStationProvider over the existing hardened account client
infrastructure/config/  bounded symlink-safe file IO + the one byte transaction
tool_activation.rs      IPC facade, connection inspection/restore, remaining orchestration
```

- **Activation split.** The 5.1k-line command is now a 101-line command facade plus focused modules:
  the use-case body lives in `service.rs` and receives its Tauri state explicitly through
  `ActivationContext` (no reliance on the facade's globals), then request validation, token
  enrollment/credential construction, model capability/Codex probes, lifecycle dispatcher and
  compensation helpers, admission/cancellation/progress, IPC result DTOs and failure
  projections. The relay's `{success, data}` envelope rules have one owner
  (`envelope.rs`) shared by capability and token enrollment. `application/connections/` (609 lines + tests) now owns connection inspection,
  the legacy-receipt conversion, restore and exit-time restore through an explicit
  `ConnectionContext`. The facade keeps only the three `#[tauri::command]` wrappers
  (`scan_activation_targets_v1`, `configure_desktop_tool_v2`, `manage_tool_connections_v1`)
  and re-exports the existing `tool_activation::*` names. Existing command names, request and
  response schemas, reason codes and error strings are unchanged.
- **Tokens and capability now reach the relay only through `StationProvider`.** The direct
  `native_account_json` calls for `/api/user/models`, `/api/pricing`, list/create/read-key/
  delete are gone from activation; the provider owns origin-bound `/api/` paths and the
  hardened client still owns TLS/redirect/proxy policy. Token ownership, device scope,
  reuse-only-the-exact-scope and retire-after-commit rules are unchanged and their tests moved
  with them.
- **Failure compensation reads as a sequence again.** `restore_files_and_credential` is a
  synchronous, testable file/credential step; `restart_previous_runtime` is the async
  helper-runtime restart; `recover_after_stop` still closes a launched desktop app before
  restoring anything. `DesktopReloadGuard`, login displacement and Codex history takeover keep
  their ordering and error codes.
- **One transaction implementation** serves every tool; the application ports describe the
  capability so fault-injection tests can drive write/readback failures without a real disk.
- **Station identity is unified.** `StationOrigin` is the single canonical HTTPS identity used
  by connect, trust and credential validation; trust input keeps its user-facing ASCII/IDNA
  normalization but rejects userinfo, explicit ports, paths, queries and fragments before
  normalization can erase them. A credential is valid because its stored identity is
  structurally valid, never because a mutable trust file happens to exist, so restore still
  works if trust data is lost. Legacy `https://api.yeschoy.com` credentials remain readable.
- **Frontend.** `ConfigurationPreviewView` dropped from 2890 to 1871 lines: model menu, network
  line selector, progress, first-activation celebration, and the selection/targets/speed-test
  hooks moved to `src/features/activation/`; pure rules moved to `previewRules.ts`; the only
  remaining direct IPC (read-only connectivity) goes through a strict decoder in
  `src/shared/api/`.

## Verification (after the refactor)

| Check | Result |
| --- | --- |
| `cargo fmt --check` | pass |
| `cargo check --locked` | pass |
| `cargo clippy --locked --all-targets -- -D warnings` | pass |
| `python3 scripts/ci/run-rust-tests.py` | **826 passed**, 0 failed, 3 declared hardware tests ignored |
| `pnpm typecheck` | pass |
| `pnpm test:unit` | **74 files / 731 tests passed** |
| `pnpm build:renderer` | pass |

Source guards were updated only where the guarded code genuinely moved
(`terminal_launch`'s activation/open handoff check now reads the lifecycle module;
`exit_restore`'s fixture check still hashes the real facade). No test was deleted or
ignored; moved tests live next to their new modules (`*_tests.rs` beside each).

## Remaining work and risks

- Tool adapters implement the shared `SharedCommit` (one commit order: transaction, read-back,
  adapter verification), the shared `SharedRollback` (one compensation order: undo the
  transaction newest-first, never clobbering files edited by someone else) and the
  `ConfigurationPlan` trait. Adapters that wrap the transaction before committing or
  rollback (WorkBuddy permission narrowing, DSH profile lock) do so inside their own
  implementations, which is intentional.
- The one native tool list (`tool_adapters::tool_ids`) is now the single source for request
  validation and compensation-side classification.
- Both use cases are now callable without Tauri `State` (`configure_desktop_tool(context, request)`
  and `manage_tool_connections(context, request)`); the remaining backend work is in the tool
  and station adapters, not the command layer.
- The full orchestration still needs a host `AppHandle` for progress events and desktop
  launch observation, so an end-to-end success/failure fixture is not yet possible without a
  test harness for those two capabilities.
- The unified `ConfigurationPlan` port exists but adapters still expose their own
  `Prepared::commit/rollback`; each adapter keeps its format-specific readback, which is
  intentional, but the error mapping is now shared via `From<ConfigFailure>`.
- Codex transport probing still sends minimal real generation requests (existing behaviour,
  kept deliberately per product decision; recorded as a product-invariant exception).
- JSONC/JSON5 activation writes still normalize formatting of the touched document; exact
  original bytes are preserved by the recovery baseline and restored on rollback, but comments
  inside a rewritten document are not preserved by the writer.
- Crash compensation is now evidence-backed end to end. Before any relay key is created,
  `infrastructure/secrets/issuance.rs` persists (owner-only, under `~/.yeschoy/`, names only,
  never key material) the exact token name plus a monotonic sequence allocated from the same
  file; the name carries that sequence after the device scope. The record is removed once this
  run owns the key, and a create without a record is refused (`issuance_record_unavailable`).
  After re-authentication each activation calls `release_abandoned_issuances`, which considers
  only ledger records older than 30 minutes, present in the relay listing by exact name, and
  carrying *this* device's scope — and only for tools that cannot adopt the key through the
  normal reuse path. Deleting keys for a still-working tool would revoke it with nothing to
  recover from, so that case is deliberately left alone.
- A nested untracked checkout `yeschoy-ai-desktop/` is excluded from Vitest by config rather
  than deleted; it still exists in the working tree.
- No real activation/restore was executed against this machine's tool configs or keychain.

## Adding a tool or a station

1. **New tool:** add `tool_adapters/<id>.rs` (discover, `prepare_*` returning `Prepared` with
   `changes/commit/rollback/validate_existing`), register it in `TARGETS` and the lifecycle
   `PreparedAdapter`, add its runtime to `HelperRuntimes` only if it owns a local bridge, and
   add the tool id to the frontend catalog. Nothing in other tools or station providers
   changes.
2. **New station:** implement `StationProvider` (account JSON only) under
   `adapters/stations/`, register how its origin is obtained, and use `StationOrigin` for
   identity. Tool adapters and their config formats are untouched.

# RU-056: unified fast activation and safe application reload

Approved scope: replace tool-specific, generated-response setup gates with one bounded local activation contract for Claude Code, Claude Desktop, Codex Desktop, Pi, DSH web, Hermes, and OpenClaw. No package, release, updater, server, database, billing, entitlement, DNS, or vendor-binary change is included.

## Fast discovery and completion

The application picker discovers CLI paths without starting third-party commands or waiting for `--version`; exact version diagnostics remain a separate action. An activation is ready after secure credential publication, atomic configuration commit/readback, required helper-owned local runtime startup, and the applicable open dispatch. It must not send or wait for a synthetic model prompt. The first real user request supplies upstream/model evidence to RecentRequest and does not retroactively roll back valid local configuration.

## Running desktop applications

Claude Desktop and Codex Desktop reload their settings only after restart. If the exact native-discovered installation is running, the first activation attempt returns `application_running` before account, key, or file mutation. The renderer asks the user to save work. Only an explicit confirmed retry may request a normal quit, wait at most ten seconds, perform the atomic transaction, and reopen that exact installation. macOS uses the exact bundle identifier and canonical bundle path. Windows enumerates exact executable image paths and posts `WM_CLOSE` to their top-level windows. Force termination, process-name-only matching, and arbitrary process control are forbidden. A refusal or timeout leaves settings untouched.

If the app was normally closed and a later activation step fails, an in-process guard reopens the exact installation after rollback. A successful configuration whose final open dispatch fails remains configured and reports an actionable launch failure rather than claiming rollback.

## CLI and helper-owned tools

Claude Code, Pi, Hermes, and OpenClaw never receive process-control requests. Existing terminal/editor sessions stay alive; the new configuration applies in a new session. DSH manages only the helper-owned runtime started by this application and may start or reuse it without probing a paid model.

## Verification boundary

Synthetic adapter probes remain isolated diagnostic contracts and are not reachable from activation. Native fixtures, renderer interaction tests, a production renderer build, strict Rust linting, Apple-Silicon compilation, and Windows x64 compilation are required. Tests use only fixtures and local compilers; they do not open the user's applications, touch real application settings or credentials, or call paid model routes.

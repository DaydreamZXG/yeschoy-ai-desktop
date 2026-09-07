# RU-054: retry self-healing and Claude model routes

Approved scope: repair stale activation retry and the current Claude Desktop / Claude Code model-catalog compatibility without changing NewAPI, account, billing, installation, packaging, or release behavior.

## Retry recovery

An explicit activation click may self-heal only an encrypted recovery record for the same selected tool whose state is `pending`. Under the existing cross-process operation lock, the native layer stops only that tool's helper-owned runtime, plans and applies the existing three-way file restoration, clears the tool credential from operating-system secure storage, removes the journal, and then continues the same click as a fresh activation. Completed or merely changed records are never auto-restored. A file, secure-storage, or receipt cleanup failure remains non-ready, retains recoverable state, and exposes the existing manual restore action.

## Claude Desktop routes

Claude Desktop's gateway-facing `inferenceModels[].name` and `/v1/models` IDs are deterministic, unique Anthropic-shaped local route aliases. The visible label is derived from the enrolled exact model ID. The loopback bridge accepts only an alias derived from the current credential's enrolled model set, resolves it to exactly one real model/group/key, and forwards the real model ID upstream. Unknown aliases fail closed. Alias generation contains no key, group, account, or upstream-provider metadata.

## Claude Code catalog

Claude Code keeps each actual selected model ID in `model`, `availableModels`, and each `modelPicker.options[].model`. Every picker row also includes the real ID as its description and a fixed known Claude `behavesAs` compatibility profile so current Claude Code can offer models that its bundled catalog does not know. `behavesAs` changes only client-side handling and never the ID forwarded, the billing group, or the credential route.

## Verification and limits

Synthetic recovery, config, catalog, routing, rejection, renderer-copy, full renderer, production build, macOS, Apple-Silicon, and Windows compilation evidence is required. Tests must not read or alter the user's real Claude configuration, credentials, account, applications, or paid routes. No package, version bump, signing, upload, deployment, server, database, token policy, or third-party binary patch is in scope. Installed Claude behavior remains version-sensitive; current evidence is bounded to the observed Claude Desktop validator and Claude Code 2.1.259 contract.

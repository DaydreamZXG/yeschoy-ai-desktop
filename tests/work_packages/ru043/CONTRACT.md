# RU-043: model reference profiles and native reasoning controls

Approved scope: exact-ID friendly naming and verified optional metadata across the existing seven adapters. This is not live upstream certification or a release.

## Authority and data

One bundled JSON reference catalog owns names, reasoning levels, optional input modalities/context and source URLs. Rust and TypeScript consume the same catalog. Exact IDs are keys, never substring aliases. No registry entry enrolls a model or changes its billing group. Unknown models retain their full ID and default access with no fabricated reasoning menu. Reference limits do not increase request output budgets.

## Native projections

Codex uses display_name, supported_reasoning_levels, nullable default_reasoning_level and verified input/context. Unknown effort is an empty list with null default. Claude Code and Desktop use friendly labels on exact IDs; no undocumented effort setting is injected into their configuration. Pi/OpenClaw use documented reasoning and thinkingLevelMap. DSH uses reasoningEfforts, not Pi's map. Hermes keeps its documented string-ID provider model list; unsupported optional UI metadata is omitted. Per-vendor source locators and executable assertions are recorded in acceptance output.

## Request behavior

Explicit low/medium/high/xhigh/max remain semantic values, not a universal max-to-xhigh alias. Only a documented target-specific alias may convert an effort. Adaptive thinking uses provider default, not maximum. DeepSeek none means thinking disabled, enabled low/high/max use its documented effort; medium/xhigh normalize to high as documented. Other conversion fields, exact model/group, local credential routing and in-flight snapshots remain unchanged. Existing native/provider user defaults are preserved, and no new global thinking default is written.

## Primary sources verified 2026-09-05

- https://developers.openai.com/api/docs/models/gpt-6-astra
- https://developers.openai.com/api/docs/models
- https://github.com/openai/codex/blob/main/codex-rs/protocol/src/openai_models.rs
- https://github.com/openai/codex/blob/main/codex-rs/models-manager/src/model_info.rs
- https://api-docs.deepseek.com/quick_start/agent_integrations/codex/
- https://api-docs.deepseek.com/quick_start/pricing/
- https://api-docs.deepseek.com/guides/thinking_mode/
- https://platform.claude.com/docs/en/build-with-claude/effort

## Verification and limits

Synthetic native conversion/catalog and HTTP fixtures, renderer tests, typecheck, frontend production build, macOS and Windows compilation. All inherited route/restore tests remain required. Reapplication is reversible through existing transactions. No real configs/keys/prompts, paid requests, binary app modifications, server changes, installs, package/sign/upload/publish. First native catalog refresh may need a restart; third-party UI may abbreviate names or expose fewer effort options, so successful compilation is not proof of native UI behavior.

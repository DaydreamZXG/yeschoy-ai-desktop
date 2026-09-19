//! Exact-ID reference metadata. Not an account allowlist or route capability probe.
use serde::Deserialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::fmt::Write as _;
use std::sync::{OnceLock, RwLock};

/// Safe fallback for Claude Code model IDs whose capabilities are not in the
/// bundled registry. The outbound model ID is always left unchanged.
pub(crate) const CLAUDE_BEHAVES_AS: &str = "claude-sonnet-4-6";

/// Return a Claude-native model family accepted by the desktop client's
/// fail-all validator. New point releases in an already supported native role
/// must not fall back to the opaque gateway alias: Claude Desktop rejects the
/// whole configured model set before it sends a request in that case.
fn native_claude_capability_family(id: &str) -> Option<&str> {
    let candidate = id.strip_prefix("anthropic/").unwrap_or(id);
    let tail = candidate.strip_prefix("claude-")?;
    let supported_role = ["sonnet-", "opus-", "haiku-", "fable-", "mythos-"]
        .iter()
        .any(|role| {
            tail.strip_prefix(role).is_some_and(|version| {
                !version.is_empty()
                    && version.bytes().all(|byte| {
                        byte.is_ascii_lowercase()
                            || byte.is_ascii_digit()
                            || matches!(byte, b'-' | b'_' | b'.')
                    })
            })
        });
    supported_role.then_some(candidate)
}

/// Whether the exact model is an Anthropic-native Claude model, as opposed to
/// a third-party model merely projected onto a Claude capability family.
/// Anthropic-only request features must be gated on this, never on the family.
pub(crate) fn is_native_claude(id: &str) -> bool {
    native_claude_capability_family(id).is_some()
}

/// Project a real gateway model onto the closest Claude client capability
/// family. This affects only native client controls (effort/thinking); the
/// loopback bridge still resolves and forwards the exact real model ID.
pub(crate) fn claude_capability_family(id: &str) -> Option<&str> {
    native_claude_capability_family(id)
        .or(match id {
            // Five effort levels, with thinking always enabled.
            "gpt-6-astra" => Some("claude-opus-5"),

            // Five effort levels plus an explicit off state.
            "gpt-5.6-sol" | "gpt-5.6-terra" | "gpt-5.6-luna" | "gpt-5.6" => Some("claude-sonnet-5"),

            // These models have low..xhigh but no max. Claude's UI may expose max
            // for this closest family; the bridge clamps it back to xhigh.
            "gpt-5.4-mini" | "gpt-5.3-codex" | "gpt-5.2" => Some("claude-opus-5"),

            // DeepSeek supports off/low/high/max. Sonnet 4.6 is the closest native
            // four-level UI; medium is translated to high by the bridge.
            "deepseek-v4-flash" | "deepseek-v4-pro" => Some("claude-sonnet-4-6"),
            _ => None,
        })
        .or_else(|| catalog_capability_family(id))
}

/// Catalog-driven fallback for models absent from the hardcoded table above
/// (new point releases like deepseek-v4.1-* or qwen3.8-*). The closest native
/// family is derived from the declared reasoning-level shape, so the desktop
/// client's effort control keeps working without a code change per release.
/// Models without a reasoning declaration stay unknown (PRD 6.5: never guess).
fn catalog_capability_family(id: &str) -> Option<&'static str> {
    let model = profile(id).or_else(|| id.strip_prefix("anthropic/").and_then(profile))?;
    if model.reasoning_levels.is_empty() {
        return None;
    }
    if model.reasoning_mode.as_deref() == Some("deepseek") {
        // off/low/high/max four-level UI; the bridge translates medium to high.
        return Some("claude-sonnet-4-6");
    }
    if model.reasoning_levels.iter().any(|level| level == "none") {
        // Five effort levels plus an explicit off state.
        return Some("claude-sonnet-5");
    }
    // Five effort levels, thinking always enabled; unsupported levels (e.g.
    // max for a low..xhigh model) are clamped back by the bridge.
    Some("claude-opus-5")
}

pub(crate) fn claude_code_behaves_as(id: &str) -> &str {
    claude_capability_family(id).unwrap_or(CLAUDE_BEHAVES_AS)
}

/// Claude Desktop currently accepts custom gateway models only when their
/// route names look like Anthropic models. Keep the real account model in the
/// visible label and resolve this deterministic, opaque local alias in the
/// loopback bridge before forwarding upstream.
pub(crate) fn claude_gateway_route_id(model_id: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"yeschoy:claude-desktop-route:v1\0");
    hasher.update(model_id.as_bytes());
    let digest = hasher.finalize();
    if let Some(family) = claude_capability_family(model_id) {
        // Claude Desktop removes a trailing numeric `-v...` suffix before its
        // capability lookup. A decimal SHA-256 suffix keeps routes distinct
        // without leaking the real upstream model ID, while the normalized
        // family unlocks the app's native effort control.
        let mut route = String::with_capacity(family.len() + 2 + digest.len() * 3);
        route.push_str(family);
        route.push_str("-v");
        for byte in digest {
            write!(&mut route, "{byte:03}").expect("writing to a String cannot fail");
        }
        route
    } else {
        // Do not invent capabilities for an unknown account model. It remains
        // usable through an opaque safe route, without a misleading selector.
        legacy_claude_gateway_route_id(model_id)
    }
}

pub(crate) fn legacy_claude_gateway_route_id(model_id: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"yeschoy:claude-desktop-route:v1\0");
    hasher.update(model_id.as_bytes());
    let digest = hasher.finalize();
    let mut route = String::with_capacity(32 + digest.len() * 2);
    route.push_str("anthropic/claude-router-");
    for byte in digest {
        write!(&mut route, "{byte:02x}").expect("writing to a String cannot fail");
    }
    route
}

/// Existing 0.4.8 profiles used an opaque alias. Accept it during the update
/// transition so installing a fixed assistant never breaks an active Claude
/// Desktop connection before the user next reapplies the managed profile.
/// The prefix carried by the opaque alias form only -- see
/// `is_claude_gateway_route`, which must also recognise the other one.
pub(crate) const CLAUDE_GATEWAY_ROUTE_PREFIX: &str = "anthropic/claude-router-";

/// Whether a requested id is one of our own Claude Desktop route aliases.
///
/// The alias is a hash we invent so Claude Desktop's profile can name a model
/// without carrying the real id. It means nothing upstream, so a request still
/// wearing one has failed to resolve here and must not be forwarded.
///
/// `claude_gateway_route_id` mints two shapes and this has to know both:
///
///   `<family>-v<96 decimal digits>`        when the model maps to a Claude
///                                          capability family (32 digest bytes
///                                          written as three digits each)
///   `anthropic/claude-router-<64 hex>`     the opaque fallback, when it does not
///
/// Matching on structure rather than a name list keeps this correct when the
/// family table grows. Neither shape can collide with a real upstream id: no
/// model carries a 96-digit tail.
pub(crate) fn is_claude_gateway_route(requested: &str) -> bool {
    let (requested, _) = split_one_m_context_marker(requested);
    if let Some(hex) = requested.strip_prefix(CLAUDE_GATEWAY_ROUTE_PREFIX) {
        return hex.len() == 64 && hex.bytes().all(|byte| byte.is_ascii_hexdigit());
    }
    requested.rsplit_once("-v").is_some_and(|(_, digits)| {
        digits.len() == 96 && digits.bytes().all(|byte| byte.is_ascii_digit())
    })
}

pub(crate) fn claude_gateway_route_matches(model_id: &str, requested: &str) -> bool {
    // Claude's picker may append a context marker. It is not part of the
    // upstream ID; normalize only for an exact, already-enrolled route lookup.
    let (requested, _) = split_one_m_context_marker(requested);
    claude_gateway_route_id(model_id) == requested
        || legacy_claude_gateway_route_id(model_id) == requested
}

pub(crate) fn split_one_m_context_marker(model: &str) -> (&str, bool) {
    let trimmed = model.trim();
    let marker = b"[1m]";
    let bytes = trimmed.as_bytes();
    if bytes.len() >= marker.len()
        && bytes[bytes.len() - marker.len()..].eq_ignore_ascii_case(marker)
    {
        return (trimmed[..trimmed.len() - marker.len()].trim_end(), true);
    }
    (trimmed, false)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ModelProfile {
    pub id: String,
    pub display_name: String,
    #[serde(default)]
    pub reasoning_levels: Vec<String>,
    pub default_reasoning: Option<String>,
    pub reasoning_mode: Option<String>,
    pub input: Option<Vec<String>>,
    pub context_window: Option<u64>,
    pub max_output_tokens: Option<u64>,
    /// 工具调用（function calling）声明。None = 目录未声明（不猜，PRD 6.5）；
    /// 路由别名（如 ark-code-latest）能力随背后模型变化，保持不填。
    pub tool_use: Option<bool>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Catalog {
    pub schema_version: u32,
    pub verified_at: String,
    pub models: Vec<ModelProfile>,
}

/// The reviewed catalog compiled into the binary. Always valid: malformed
/// bundled metadata is a build/test defect, never user input.
pub(crate) fn bundled() -> &'static Catalog {
    static BUNDLED: OnceLock<Catalog> = OnceLock::new();
    BUNDLED.get_or_init(|| {
        serde_json::from_str(include_str!("../../src/model-profiles/catalog.json"))
            .expect("validated bundled model profiles")
    })
}

/// The catalog every lookup reads.
///
/// This used to be the bundled copy and nothing else, which quietly decided a
/// lot: `auto_compact_window` asks this table for a model's context window, and
/// a model it has never heard of gets no `autoCompactWindow` at all — so Claude
/// Code never compacts and the conversation grows until the relay rejects it.
/// A model added after the release could therefore only be fixed by another
/// release, because the renderer's remote-catalog refresh swapped a map that
/// lives on the other side of the IPC boundary and never reached here.
///
/// So the active catalog is swappable. A refresh that passed integrity, schema
/// and freshness checks is leaked once and installed; at most one leak per
/// refresh, and a refresh happens once per launch.
static ACTIVE: OnceLock<RwLock<&'static Catalog>> = OnceLock::new();

fn active() -> &'static RwLock<&'static Catalog> {
    ACTIVE.get_or_init(|| RwLock::new(crate::model_catalog::initial()))
}

/// Adopt a catalog that has already been verified. Callers must not pass
/// unvalidated remote data — `model_catalog` owns that check.
pub(crate) fn install(catalog: Catalog) {
    let leaked: &'static Catalog = Box::leak(Box::new(catalog));
    if let Ok(mut guard) = active().write() {
        *guard = leaked;
    }
}

/// The `verifiedAt` of whatever is currently active, which may already be
/// ahead of the bundled copy when a cached revision was adopted at startup.
pub(crate) fn profile_catalog_verified_at() -> String {
    active()
        .read()
        .map(|catalog| catalog.verified_at.clone())
        .unwrap_or_else(|_| bundled().verified_at.clone())
}

pub(crate) fn profile(id: &str) -> Option<&'static ModelProfile> {
    // Copy the &'static out before releasing the guard so the search itself
    // holds no lock.
    let catalog: &'static Catalog = *active().read().ok()?;
    catalog.models.iter().find(|p| p.id == id)
}

pub(crate) fn display_name(id: &str) -> &str {
    profile(id).map_or(id, |p| p.display_name.as_str())
}

/// Advertise long context from the real model's exact metadata, never the
/// Claude family used to expose effort controls. Unknown models stay unknown.
/// An explicit Anthropic namespace is the same native model, not a new family.
pub(crate) fn supports_one_m_context(id: &str) -> bool {
    profile(id)
        .or_else(|| {
            id.strip_prefix("anthropic/")
                .filter(|native| native.starts_with("claude-"))
                .and_then(profile)
        })
        .and_then(|model| model.context_window)
        .is_some_and(|tokens| tokens >= 1_000_000)
}

pub(crate) fn is_deepseek(id: &str) -> bool {
    profile(id).is_some_and(|p| p.reasoning_mode.as_deref() == Some("deepseek"))
}

#[allow(dead_code)] // 供 vendored 转换器使用；桌面直连路径不再需要。
pub(crate) fn supports_reasoning_level(id: &str, effort: &str) -> bool {
    profile(id).is_some_and(|model| model.reasoning_levels.iter().any(|level| level == effort))
}

fn effort_rank(effort: &str) -> Option<usize> {
    [
        "none", "minimal", "low", "medium", "high", "xhigh", "max", "ultra",
    ]
    .iter()
    .position(|candidate| *candidate == effort)
}

/// Convert a client-visible effort to one that the exact real model supports.
/// Unknown models remain untouched because the account metadata is the only
/// authority for them; known models are never sent an unsupported level.
pub(crate) fn normalize_supported_effort(id: &str, effort: &str) -> Option<String> {
    let Some(model) = profile(id) else {
        return Some(effort.to_owned());
    };
    if model.reasoning_levels.iter().any(|level| level == effort) {
        return Some(effort.to_owned());
    }
    if model.reasoning_levels.is_empty() || effort == "none" {
        return None;
    }
    if is_deepseek(id) {
        return match effort {
            "minimal" => Some("low".into()),
            "medium" | "xhigh" => Some("high".into()),
            "ultra" => Some("max".into()),
            _ => None,
        };
    }
    let requested = effort_rank(effort)?;
    model
        .reasoning_levels
        .iter()
        .filter_map(|candidate| effort_rank(candidate).map(|rank| (rank, candidate)))
        .filter(|(rank, _)| *rank <= requested)
        .max_by_key(|(rank, _)| *rank)
        .map(|(_, candidate)| candidate.clone())
        .or_else(|| {
            model
                .reasoning_levels
                .iter()
                .filter_map(|candidate| effort_rank(candidate).map(|rank| (rank, candidate)))
                .min_by_key(|(rank, _)| *rank)
                .map(|(_, candidate)| candidate.clone())
        })
}

/// Explicit semantic effort is not a request to choose the strongest setting.
pub(crate) fn apply_chat_effort(body: &mut Value, id: &str, effort: &str) {
    let Some(effort) = normalize_supported_effort(id, effort) else {
        if let Some(object) = body.as_object_mut() {
            object.remove("reasoning_effort");
        }
        return;
    };
    if is_deepseek(id) {
        if effort == "none" {
            body["thinking"] = json!({"type":"disabled"});
            if let Some(object) = body.as_object_mut() {
                object.remove("reasoning_effort");
            }
        } else {
            body["thinking"] = json!({"type":"enabled"});
            body["reasoning_effort"] = json!(effort);
        }
    } else {
        body["reasoning_effort"] = json!(effort);
    }
}

/// Adapt documented nested custom-provider efforts and DeepSeek's
/// off toggle. Unknown models and unrelated payload fields remain untouched.
#[allow(dead_code)] // Used by the vendored converter set, not by the desktop paths.
pub(crate) fn normalize_chat_reasoning(body: &mut Value) -> bool {
    let Some(id) = body["model"].as_str().map(str::to_owned) else {
        return false;
    };
    if profile(&id).is_none() {
        return false;
    }
    let nested = body.get("reasoning").and_then(Value::as_object);
    let nested_effort = nested.and_then(|r| {
        if r.get("enabled").and_then(Value::as_bool) == Some(false) {
            Some("none")
        } else {
            r.get("effort").and_then(Value::as_str)
        }
    });
    // A stale effort can remain after a native UI turns thinking off. The
    // explicit independent toggle wins, just as it does on the vendor API.
    let disabled = (is_deepseek(&id)
        && body.pointer("/thinking/type").and_then(Value::as_str) == Some("disabled"))
        || nested
            .and_then(|r| r.get("enabled"))
            .and_then(Value::as_bool)
            == Some(false);
    let effort = disabled
        .then_some("none")
        .or(body["reasoning_effort"].as_str().or(nested_effort))
        .map(str::to_owned);
    let Some(effort) = effort else {
        return false;
    };
    if nested_effort.is_some() {
        if let Some(reasoning) = body["reasoning"].as_object_mut() {
            reasoning.remove("enabled");
            reasoning.remove("effort");
            if reasoning.is_empty() {
                body.as_object_mut().unwrap().remove("reasoning");
            }
        }
    }
    apply_chat_effort(body, &id, &effort);
    true
}

#[derive(Clone, Copy)]
pub(crate) enum ModelConsumer {
    Pi,
    Dsh,
}

pub(crate) fn native_chat_model(id: &str, consumer: ModelConsumer) -> Value {
    let mut model = json!({"id":id, "name":display_name(id)});
    let Some(p) = profile(id) else {
        return model;
    };
    if let Some(input) = &p.input {
        model["input"] = json!(input);
    }
    if let Some(context) = p.context_window {
        model["contextWindow"] = json!(context);
    }
    // maxTokens is also the per-request budget in Pi and DSH, so it is never
    // written from the catalog: doing so would change what the user's next
    // request is allowed to spend. `native_chat_catalog` carries the user's own
    // value across instead, with one narrow floor — see MIN_OUTPUT_BUDGET.
    if p.reasoning_levels.is_empty() {
        return model;
    }
    let dsh = matches!(consumer, ModelConsumer::Dsh);
    let mut levels = serde_json::Map::new();
    for (level, wire) in [
        ("off", "none"),
        ("minimal", "minimal"),
        ("low", "low"),
        ("medium", "medium"),
        ("high", "high"),
        ("xhigh", "xhigh"),
        ("max", "max"),
    ] {
        let supported = p.reasoning_levels.iter().any(|v| v == wire);
        if supported {
            levels.insert(level.into(), json!(wire));
        } else if !dsh {
            levels.insert(level.into(), Value::Null);
        }
    }
    if dsh {
        model["reasoningEfforts"] = Value::Object(levels);
    } else {
        model["reasoning"] = json!(true);
        model["thinkingLevelMap"] = Value::Object(levels);
        model["compat"] = json!({"supportsReasoningEffort":true});
    }
    model
}

/// A budget too small for a tool call to finish.
///
/// Pi documents `maxTokens` as a real output ceiling, not display metadata, and
/// DSH uses the same key. Under a few hundred tokens a function call cannot emit
/// its arguments JSON, so the model is cut off mid-object, the app rejects the
/// malformed call, and the user is billed for the truncated attempt regardless.
/// That is not a budget, it is a broken config — almost always a small value
/// carried in from whichever provider the user had set up before us.
///
/// 1024 is chosen to be large enough for an arguments payload plus a short
/// answer, and small enough that it cannot meaningfully change anyone's spend:
/// `maxTokens` is a cap, not a target, so a model that was already finishing
/// well inside its old ceiling generates exactly as much as before.
const MIN_OUTPUT_BUDGET: u64 = 1024;

/// Regenerating a catalog must not erase a user's per-model token caps or
/// compatibility settings and thereby raise their next request's budget.
///
/// The one exception is [`MIN_OUTPUT_BUDGET`]. Preserving a cap is protecting
/// the user's money; preserving a cap that truncates every tool call is just
/// carrying someone else's mistake forward, and the user has no way to know
/// that is why their agent keeps failing.
pub(crate) fn native_chat_catalog(
    existing: Option<&Value>,
    ids: &[String],
    consumer: ModelConsumer,
) -> Value {
    let models = ids
        .iter()
        .map(|id| {
            let mut previous = existing
                .and_then(Value::as_array)
                .and_then(|items| items.iter().find(|entry| entry["id"].as_str() == Some(id)))
                .and_then(Value::as_object)
                .cloned()
                .unwrap_or_default();
            // These model-level fields override the provider's loopback route.
            // They belong to the connection being replaced, not user preferences.
            if matches!(consumer, ModelConsumer::Pi) {
                for key in ["api", "baseUrl", "headers"] {
                    previous.remove(key);
                }
            }
            if matches!(consumer, ModelConsumer::Pi) {
                if let Some(params) = previous
                    .get_mut("samplingParams")
                    .and_then(Value::as_object_mut)
                {
                    // Pi merges this bag after its native request fields. Keep
                    // sampling/budget/reasoning defaults, not transport overrides.
                    for key in ["model", "stream"] {
                        params.remove(key);
                    }
                }
            }
            let generated = native_chat_model(id, consumer);
            for (key, value) in generated.as_object().unwrap() {
                if key == "contextWindow" && previous.contains_key(key) {
                    continue;
                }
                if key == "compat" {
                    let mut compat = value.as_object().cloned().unwrap_or_default();
                    if let Some(old) = previous.get(key).and_then(Value::as_object) {
                        compat.extend(old.clone());
                    }
                    previous.insert(key.clone(), Value::Object(compat));
                } else {
                    previous.insert(key.clone(), value.clone());
                }
            }
            // Only an existing, numeric, sub-floor cap is touched. An absent
            // key stays absent so the app applies its own default (Pi's is
            // 16384), and a value we cannot parse is left exactly as the user
            // wrote it.
            if previous
                .get("maxTokens")
                .and_then(Value::as_u64)
                .is_some_and(|budget| budget < MIN_OUTPUT_BUDGET)
            {
                previous.insert("maxTokens".into(), json!(MIN_OUTPUT_BUDGET));
            }
            Value::Object(previous)
        })
        .collect();
    Value::Array(models)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_budget_too_small_for_a_tool_call_is_lifted_to_the_floor() {
        // A stale cap carried in from whatever provider the user had before us.
        // Pi and DSH both treat maxTokens as a real output ceiling, so at 8 every
        // function call is cut off inside its arguments JSON — and billed anyway.
        for consumer in [ModelConsumer::Pi, ModelConsumer::Dsh] {
            let stale = json!([{"id":"deepseek-v4.1-flash","maxTokens":8}]);
            let merged =
                native_chat_catalog(Some(&stale), &["deepseek-v4.1-flash".into()], consumer);
            assert_eq!(merged[0]["maxTokens"], MIN_OUTPUT_BUDGET);
        }
    }

    #[test]
    fn a_workable_budget_is_still_never_raised() {
        // The rule this floor is an exception to: maxTokens is the per-request
        // budget, so carrying the user's own value across is what keeps a catalog
        // rewrite from spending their money.
        for consumer in [ModelConsumer::Pi, ModelConsumer::Dsh] {
            for budget in [MIN_OUTPUT_BUDGET, 4096, 128_000] {
                let existing = json!([{"id":"deepseek-v4.1-flash","maxTokens":budget}]);
                let merged =
                    native_chat_catalog(Some(&existing), &["deepseek-v4.1-flash".into()], consumer);
                assert_eq!(merged[0]["maxTokens"], budget, "{budget}");
            }
            // Absent stays absent: the app's own default applies (Pi's is 16384),
            // and inventing a number here would be guessing at the user's budget.
            let merged = native_chat_catalog(None, &["deepseek-v4.1-flash".into()], consumer);
            assert!(merged[0].get("maxTokens").is_none());
            // Unparsable is left exactly as the user wrote it.
            let odd = json!([{"id":"deepseek-v4.1-flash","maxTokens":"8"}]);
            let merged = native_chat_catalog(Some(&odd), &["deepseek-v4.1-flash".into()], consumer);
            assert_eq!(merged[0]["maxTokens"], "8");
        }
    }

    #[test]
    fn every_catalog_model_declares_a_usable_output_budget() {
        // WorkBuddy is the one adapter that writes this value straight from the
        // catalog (`maxOutputTokens`), so the catalog itself is where a too-small
        // budget would have to be caught. Guarding it here covers that adapter
        // and any future one without either needing its own test.
        #[derive(Deserialize)]
        struct Catalog {
            models: Vec<ModelProfile>,
        }
        let catalog: Catalog =
            serde_json::from_str(include_str!("../../src/model-profiles/catalog.json")).unwrap();
        assert!(!catalog.models.is_empty());
        for model in &catalog.models {
            let declared = model.max_output_tokens.unwrap_or_else(|| {
                panic!("{} declares no maxOutputTokens", model.id);
            });
            assert!(
                declared >= MIN_OUTPUT_BUDGET,
                "{} declares {declared}, below the tool-call floor",
                model.id
            );
        }
    }

    #[test]
    fn context_regression_capabilities_come_from_real_model_metadata() {
        for id in [
            "claude-sonnet-4-6",
            "claude-opus-4-6",
            "claude-opus-4-7",
            "claude-opus-4-8",
            "claude-opus-5",
            "claude-sonnet-5",
            "claude-fable-5",
            "claude-fable-5-1",
            "claude-mythos-5",
            "claude-mythos-5-1",
        ] {
            assert_eq!(profile(id).unwrap().context_window, Some(1_000_000), "{id}");
            assert!(supports_one_m_context(id), "{id}");
            assert!(supports_one_m_context(&format!("anthropic/{id}")), "{id}");
        }
        for id in [
            "gpt-6-astra",
            "gpt-5.6",
            "gpt-5.6-sol",
            "gpt-5.6-terra",
            "gpt-5.6-luna",
            "deepseek-v4-flash",
            "deepseek-v4-pro",
        ] {
            assert!(supports_one_m_context(id), "{id}");
        }
        // A known effort family does not establish the real context capacity.
        assert_eq!(
            claude_capability_family("gpt-5.4-mini"),
            Some("claude-opus-5")
        );
        for id in [
            "gpt-5.4-mini",
            "gpt-5.3-codex",
            "claude-haiku-4-5",
            "claude-sonnet-4-5",
            "claude-opus-future",
            "future-model",
            "claude-fable-5[1m]",
            "anthropic/gpt-6-astra",
            "other/claude-fable-5",
            "CLAUDE-FABLE-5",
        ] {
            assert!(!supports_one_m_context(id), "{id}");
        }
    }

    #[test]
    fn context_regression_native_consumers_share_capacity_without_raising_user_caps() {
        for consumer in [ModelConsumer::Pi, ModelConsumer::Dsh] {
            let ids = vec![
                "claude-fable-5".into(),
                "deepseek-v4-flash".into(),
                "future-model".into(),
            ];
            let generated = native_chat_catalog(None, &ids, consumer);
            assert_eq!(generated[0]["contextWindow"], 1_000_000);
            assert_eq!(generated[1]["contextWindow"], 1_048_576);
            assert!(generated[2].get("contextWindow").is_none());
            assert!(generated[0].get("maxTokens").is_none());
            let existing = json!([{
                "id":"claude-fable-5", "contextWindow":200000, "maxTokens":4096,
                "customUserOption":"keep"
            }]);
            let merged = native_chat_catalog(Some(&existing), &ids, consumer);
            assert_eq!(merged[0]["contextWindow"], 200000);
            assert_eq!(merged[0]["maxTokens"], 4096);
            assert_eq!(merged[0]["customUserOption"], "keep");
        }
    }

    #[test]
    fn context_regression_marker_parsing_is_terminal_and_exact_route_only() {
        let id = "claude-fable-5";
        let route = claude_gateway_route_id(id);
        for suffix in ["[1m]", " [1M] ", "\t[1m]\n"] {
            assert!(claude_gateway_route_matches(
                id,
                &format!("{route}{suffix}")
            ));
        }
        for requested in [
            format!("{route}[1m][1m]"),
            format!("{route}[1m]/extra"),
            format!("{route}[2m]"),
            format!("{route}[1m]suffix"),
            format!("{route}x[1m]"),
            "unknown[1m]".into(),
            "模型[1m]".into(),
            "[1m]".into(),
            "💭".into(),
            String::new(),
        ] {
            assert!(!claude_gateway_route_matches(id, &requested), "{requested}");
        }
        assert!(!claude_gateway_route_matches(
            "gpt-6-astra",
            &format!("{route}[1m]")
        ));
    }

    fn desktop_normalized_family(route: &str) -> &str {
        let route = route
            .strip_prefix("anthropic/")
            .unwrap_or(route)
            .split('[')
            .next()
            .unwrap_or(route);
        let Some((family, version)) = route.rsplit_once("-v") else {
            return route;
        };
        if !version.is_empty() && version.bytes().all(|byte| byte.is_ascii_digit()) {
            family
        } else {
            route
        }
    }

    #[test]
    fn ru056_claude_clients_receive_per_model_capability_families() {
        for (model, family) in [
            ("gpt-6-astra", "claude-opus-5"),
            ("gpt-5.6-sol", "claude-sonnet-5"),
            ("gpt-5.4-mini", "claude-opus-5"),
            ("deepseek-v4-flash", "claude-sonnet-4-6"),
            ("claude-opus-4-8", "claude-opus-4-8"),
            ("claude-fable-5", "claude-fable-5"),
            ("claude-fable-5-1", "claude-fable-5-1"),
            ("claude-mythos-5", "claude-mythos-5"),
            ("anthropic/claude-sonnet-5", "claude-sonnet-5"),
        ] {
            assert_eq!(claude_capability_family(model), Some(family));
            assert_eq!(claude_code_behaves_as(model), family);
            let route = claude_gateway_route_id(model);
            assert_eq!(desktop_normalized_family(&route), family);
            assert!(route
                .rsplit_once("-v")
                .is_some_and(|(_, suffix)| !suffix.is_empty()
                    && suffix.bytes().all(|byte| byte.is_ascii_digit())));
            if !model.starts_with("claude-") {
                assert!(!route.contains(model));
            }
        }
        for invalid in [
            "claude-router-deadbeef",
            "claude-fable-",
            "claude-fable-5[1m]",
            "claude-fable-5/other",
            "CLAUDE-FABLE-5",
        ] {
            assert_eq!(claude_capability_family(invalid), None, "{invalid}");
        }
        assert_eq!(claude_capability_family("future-model"), None);
        assert_eq!(claude_code_behaves_as("future-model"), CLAUDE_BEHAVES_AS);
        assert!(claude_gateway_route_id("future-model").starts_with("anthropic/claude-router-"));
        assert_ne!(
            claude_gateway_route_id("gpt-6-astra"),
            claude_gateway_route_id("gpt-5.6-sol")
        );
        let legacy = legacy_claude_gateway_route_id("deepseek-v4-flash");
        assert!(claude_gateway_route_matches("deepseek-v4-flash", &legacy));
        assert!(claude_gateway_route_matches(
            "deepseek-v4-flash",
            &claude_gateway_route_id("deepseek-v4-flash")
        ));
        assert!(!claude_gateway_route_matches(
            "deepseek-v4-pro",
            &claude_gateway_route_id("deepseek-v4-flash")
        ));
        let fable_route = claude_gateway_route_id("claude-fable-5");
        assert!(fable_route.starts_with("claude-fable-5-v"));
        assert!(!fable_route.starts_with("anthropic/claude-router-"));
        assert!(claude_gateway_route_matches("claude-fable-5", &fable_route));
        assert!(claude_gateway_route_matches(
            "claude-fable-5",
            &legacy_claude_gateway_route_id("claude-fable-5")
        ));
    }

    #[test]
    fn ru056_effort_projection_never_forwards_an_unsupported_known_level() {
        assert_eq!(
            normalize_supported_effort("gpt-5.4-mini", "max").as_deref(),
            Some("xhigh")
        );
        assert_eq!(
            normalize_supported_effort("deepseek-v4-flash", "medium").as_deref(),
            Some("high")
        );
        assert_eq!(
            normalize_supported_effort("deepseek-v4-flash", "xhigh").as_deref(),
            Some("high")
        );
        assert_eq!(
            normalize_supported_effort("deepseek-v4-flash", "minimal").as_deref(),
            Some("low")
        );
        assert_eq!(normalize_supported_effort("gpt-6-astra", "none"), None);
        assert_eq!(
            normalize_supported_effort("future-model", "ultra").as_deref(),
            Some("ultra")
        );

        let mut body = json!({"reasoning_effort":"low"});
        apply_chat_effort(&mut body, "gpt-5.4-mini", "max");
        assert_eq!(body["reasoning_effort"], "xhigh");
        apply_chat_effort(&mut body, "gpt-6-astra", "none");
        assert!(body.get("reasoning_effort").is_none());
    }

    #[test]
    fn ru043_reapply_clears_model_route_overrides_without_losing_budgets() {
        for consumer in [ModelConsumer::Pi] {
            for id in ["gpt-6-astra", "deepseek-v4-flash", "future-model"] {
                let old = json!([{
                    "id":id, "api":"old-protocol", "baseUrl":"https://old.invalid/v1",
                    "headers":{"Authorization":"synthetic-old-key"},
                    "maxTokens":2048, "contextWindow":65536,
                    "compat":{"supportsDeveloperRole":false},
                    "samplingParams":{"model":"stale-model","stream":false,
                        "reasoning_effort":"high","thinking":{"type":"disabled"},
                        "reasoning":{"effort":"high"},"max_tokens":1024,"temperature":0.2}
                }]);
                let merged = native_chat_catalog(Some(&old), &[id.into()], consumer);
                let model = &merged[0];
                for key in ["api", "baseUrl", "headers"] {
                    assert!(
                        model.get(key).is_none(),
                        "{key} must inherit the selected provider"
                    );
                }
                assert_eq!(model["maxTokens"], 2048);
                assert_eq!(model["contextWindow"], 65536);
                assert_eq!(model["compat"]["supportsDeveloperRole"], false);
                if matches!(consumer, ModelConsumer::Pi) {
                    let params = &model["samplingParams"];
                    assert!(params.get("model").is_none());
                    assert!(params.get("stream").is_none());
                    assert_eq!(params["max_tokens"], 1024);
                    assert_eq!(params["temperature"], 0.2);
                    for key in ["reasoning_effort", "thinking", "reasoning"] {
                        assert_eq!(params[key], old[0]["samplingParams"][key]);
                    }
                }
                assert_eq!(
                    native_chat_catalog(Some(&merged), &[id.into()], consumer),
                    merged
                );
            }
        }
    }

    #[test]
    fn ru043_unknown_chat_fields_and_reasoning_defaults_are_untouched() {
        for mut body in [
            json!({"model":"new-model","reasoning":{"effort":"ultra","future":true}}),
            json!({"model":"gpt-6-astra","messages":[],"max_tokens":1024}),
        ] {
            let before = body.clone();
            assert!(!normalize_chat_reasoning(&mut body));
            assert_eq!(body, before);
        }
        let mut nested = json!({"model":"gpt-5.6-sol","reasoning":{"enabled":false,"future":true},"reasoning_effort":"high","messages":[]});
        assert!(normalize_chat_reasoning(&mut nested));
        assert_eq!(nested["reasoning_effort"], "none");
        assert_eq!(nested["reasoning"], json!({"future":true}));
        let mut disabled = json!({"model":"deepseek-v4-flash","thinking":{"type":"disabled"},"reasoning_effort":"high","messages":[]});
        assert!(normalize_chat_reasoning(&mut disabled));
        assert_eq!(disabled["thinking"]["type"], "disabled");
        assert!(disabled.get("reasoning_effort").is_none());
    }
    #[test]
    fn ru043_registry_exact_ids_and_capabilities() {
        let p = profile("gpt-6-astra").unwrap();
        assert_eq!(p.display_name, "GPT-6 Astra");
        assert_eq!(
            p.reasoning_levels,
            ["low", "medium", "high", "xhigh", "max"]
        );
        // 工具调用声明：核实过的模型为 Some(true)，路由别名不猜保持 None。
        assert_eq!(p.tool_use, Some(true));
        assert!(profile("ark-code-latest").unwrap().tool_use.is_none());
        assert!(profile("org/gpt-6-astra").is_none());
        assert!(profile("gpt-6-astra-custom").is_none());
        assert_eq!(display_name("unknown/model"), "unknown/model");
        let catalog: Value =
            serde_json::from_str(include_str!("../../src/model-profiles/catalog.json")).unwrap();
        let mut ids = std::collections::HashSet::new();
        for model in catalog["models"].as_array().unwrap() {
            let id = model["id"].as_str().unwrap();
            assert!(ids.insert(id));
            assert!(!model["sources"].as_array().unwrap().is_empty());
            if let Some(tool_use) = model["toolUse"].as_bool() {
                assert!(tool_use, "{id}: toolUse 只允许 true（核实）或缺失（不猜）");
            }
            let p = profile(id).unwrap();
            if let Some(default) = &p.default_reasoning {
                assert!(p.reasoning_levels.contains(default));
            }
        }
    }

    #[test]
    fn ru044_catalog_driven_capability_family_covers_new_releases() {
        // 硬编码表未收录的新点版本，由目录声明的 levels 形状推导最近原生族。
        assert_eq!(
            claude_capability_family("deepseek-v4.1-flash"),
            Some("claude-sonnet-4-6")
        );
        // 未收录进目录的点版本不猜能力（PRD 6.5），保持未知。
        assert_eq!(claude_capability_family("deepseek-v4.1-pro"), None);
        assert_eq!(
            claude_capability_family("qwen3.8-max"),
            Some("claude-opus-5")
        );
        // 未声明思考能力的模型仍保持未知（不猜，PRD 6.5）。
        assert_eq!(claude_capability_family("ark-code-latest"), None);
        // 硬编码表优先于目录回退。
        assert_eq!(
            claude_capability_family("deepseek-v4-flash"),
            Some("claude-sonnet-4-6")
        );
    }

    #[test]
    fn ru043_native_model_metadata_uses_vendor_contracts() {
        let pi = native_chat_model("gpt-6-astra", ModelConsumer::Pi);
        assert_eq!(pi["thinkingLevelMap"]["max"], "max");
        assert!(pi["thinkingLevelMap"]["off"].is_null());
        assert_eq!(pi["input"], json!(["text", "image"]));
        assert!(pi.get("maxTokens").is_none());
        let dsh = native_chat_model("deepseek-v4-flash", ModelConsumer::Dsh);
        assert_eq!(
            dsh["reasoningEfforts"],
            json!({"off":"none","low":"low","high":"high","max":"max"})
        );
        assert!(dsh.get("thinkingLevelMap").is_none());
        assert_eq!(
            native_chat_model("future-model", ModelConsumer::Pi),
            json!({"id":"future-model","name":"future-model"})
        );
        for consumer in [ModelConsumer::Pi, ModelConsumer::Dsh] {
            let previous = json!([{"id":"gpt-6-astra","name":"old","maxTokens":4096,"contextWindow":65536,"compat":{"custom":true},"futureField":42}]);
            let merged = native_chat_catalog(Some(&previous), &["gpt-6-astra".into()], consumer);
            assert_eq!(merged[0]["name"], "GPT-6 Astra");
            assert_eq!(merged[0]["maxTokens"], 4096);
            assert_eq!(merged[0]["contextWindow"], 65536);
            assert_eq!(merged[0]["compat"]["custom"], true);
            assert_eq!(merged[0]["futureField"], 42);
            assert_eq!(
                native_chat_catalog(Some(&merged), &["gpt-6-astra".into()], consumer),
                merged
            );
        }
    }
}

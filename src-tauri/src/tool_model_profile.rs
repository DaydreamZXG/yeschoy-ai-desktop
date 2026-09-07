//! Exact-ID reference metadata. Not an account allowlist or route capability probe.
use serde::Deserialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::fmt::Write as _;
use std::sync::OnceLock;

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

/// Project a real gateway model onto the closest Claude client capability
/// family. This affects only native client controls (effort/thinking); the
/// loopback bridge still resolves and forwards the exact real model ID.
pub(crate) fn claude_capability_family(id: &str) -> Option<&str> {
    native_claude_capability_family(id).or(match id {
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
pub(crate) fn claude_gateway_route_matches(model_id: &str, requested: &str) -> bool {
    claude_gateway_route_id(model_id) == requested
        || legacy_claude_gateway_route_id(model_id) == requested
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
}

pub(crate) fn profile(id: &str) -> Option<&'static ModelProfile> {
    #[derive(Deserialize)]
    struct Catalog {
        models: Vec<ModelProfile>,
    }
    static CATALOG: OnceLock<Catalog> = OnceLock::new();
    // Malformed bundled metadata is a build/test defect, never user input.
    CATALOG
        .get_or_init(|| {
            serde_json::from_str(include_str!("../../src/model-profiles/catalog.json"))
                .expect("validated bundled model profiles")
        })
        .models
        .iter()
        .find(|p| p.id == id)
}

pub(crate) fn display_name(id: &str) -> &str {
    profile(id).map_or(id, |p| p.display_name.as_str())
}

pub(crate) fn is_deepseek(id: &str) -> bool {
    profile(id).is_some_and(|p| p.reasoning_mode.as_deref() == Some("deepseek"))
}

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

/// Adapt documented nested custom-provider efforts (Hermes) and DeepSeek's
/// off toggle. Unknown models and unrelated payload fields remain untouched.
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
    OpenClaw,
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
    // maxTokens is also the default request budget in several apps: never raise it here.
    if p.reasoning_levels.is_empty() {
        return model;
    }
    let dsh = matches!(consumer, ModelConsumer::Dsh);
    let claw = matches!(consumer, ModelConsumer::OpenClaw);
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
        // OpenClaw's SimpleStream aliases max to xhigh before the map runs.
        // Its full builder does apply off's map; only max must remain hidden.
        let supported = p.reasoning_levels.iter().any(|v| v == wire) && !(claw && level == "max");
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

/// Regenerating a catalog must not erase a user's per-model token caps or
/// compatibility settings and thereby raise their next request's budget.
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
            if matches!(consumer, ModelConsumer::Pi | ModelConsumer::OpenClaw) {
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
            Value::Object(previous)
        })
        .collect();
    Value::Array(models)
}

#[cfg(test)]
mod tests {
    use super::*;

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
        for consumer in [ModelConsumer::Pi, ModelConsumer::OpenClaw] {
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
            let p = profile(id).unwrap();
            if let Some(default) = &p.default_reasoning {
                assert!(p.reasoning_levels.contains(default));
            }
        }
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
        let claw = native_chat_model("gpt-6-astra", ModelConsumer::OpenClaw);
        assert!(claw["thinkingLevelMap"]["max"].is_null());
        assert_eq!(claw["thinkingLevelMap"]["xhigh"], "xhigh");
        assert_eq!(
            native_chat_model("future-model", ModelConsumer::Pi),
            json!({"id":"future-model","name":"future-model"})
        );
        for consumer in [
            ModelConsumer::Pi,
            ModelConsumer::Dsh,
            ModelConsumer::OpenClaw,
        ] {
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

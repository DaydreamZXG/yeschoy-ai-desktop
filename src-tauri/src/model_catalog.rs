//! Model reference catalog: fetch, verify, cache, and hand to both sides.
//!
//! The catalog carries display names and capabilities — context window,
//! reasoning levels, tool use. Two things read it, and until now they read
//! different copies:
//!
//!   * the renderer's picker, through `src/model-profiles/profile.ts`
//!   * `tool_model_profile`, which `auto_compact_window` asks for a model's
//!     context window when writing Claude Code's `autoCompactWindow`
//!
//! The refresh lived entirely in the renderer and fetched with the webview's
//! own `fetch`. The app's CSP is `connect-src 'self' ipc: http://ipc.localhost`,
//! which does not include the download host, so that request could never leave
//! the process — `refreshModelCatalog()` always returned `fetch_failed`. Fixing
//! the URL (an earlier round did) changed nothing, because the URL was not the
//! only thing in the way.
//!
//! Even had it worked, `applyModelCatalogOverride` swaps a map on the renderer
//! side; it never crossed the IPC boundary, so auto-compact would still have
//! been stuck on whatever shipped in the binary.
//!
//! So the fetch happens here, next to the installer's mirror fetch which uses
//! the same host and already works. Everything else about the contract is
//! unchanged and deliberately strict: sha256, schema, and strictly-newer
//! `verifiedAt` must all pass, and any failure keeps the bundled catalog.
use crate::tool_model_profile::{self, Catalog};
use reqwest::Client;
use serde::Serialize;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::path::PathBuf;

const CATALOG_URL: &str = "https://ergou.qzz.io/apps/model-catalog.json";
const SHA256_URL: &str = "https://ergou.qzz.io/apps/model-catalog.json.sha256";
const FETCH_TIMEOUT_SECS: u64 = 10;
const MAX_CATALOG_BYTES: usize = 2 * 1024 * 1024;
const MAX_SHA_BYTES: usize = 256;

/// Accepted input modalities, mirroring the renderer's validator.
const INPUT_MODALITIES: [&str; 2] = ["text", "image"];

fn cache_path() -> Option<PathBuf> {
    Some(
        crate::tool_adapters::user_home()?
            .join(".yeschoy")
            .join("model-catalog.json"),
    )
}

/// Validate an untrusted payload against the catalog schema.
///
/// Mirrors `parseRemoteCatalog` in `remoteCatalog.ts` field for field. Anything
/// off returns `None` and the caller keeps what it already had — unknown or
/// malformed metadata never reaches a lookup.
pub(crate) fn validate(payload: &Value, bundled_schema_version: u32) -> Option<()> {
    let object = payload.as_object()?;
    if object.get("schemaVersion")?.as_u64()? != u64::from(bundled_schema_version) {
        return None;
    }
    let verified_at = object.get("verifiedAt")?.as_str()?;
    if !is_iso_date(verified_at) {
        return None;
    }
    let models = object.get("models")?.as_array()?;
    if models.is_empty() {
        return None;
    }
    let mut seen = std::collections::BTreeSet::new();
    for model in models {
        let entry = model.as_object()?;
        let id = entry.get("id")?.as_str()?;
        if id.is_empty() || !seen.insert(id) {
            return None;
        }
        if entry.get("displayName")?.as_str()?.is_empty() {
            return None;
        }
        for key in ["contextWindow", "maxOutputTokens"] {
            if let Some(value) = entry.get(key) {
                if !value.as_u64().is_some_and(|n| n > 0) {
                    return None;
                }
            }
        }
        if let Some(input) = entry.get("input") {
            let items = input.as_array()?;
            if items.is_empty()
                || !items.iter().all(|item| {
                    item.as_str()
                        .is_some_and(|text| INPUT_MODALITIES.contains(&text))
                })
                || !items.iter().any(|item| item.as_str() == Some("text"))
            {
                return None;
            }
        }
        if let Some(levels) = entry.get("reasoningLevels") {
            if !levels
                .as_array()?
                .iter()
                .all(|level| level.as_str().is_some_and(|text| !text.is_empty()))
            {
                return None;
            }
        }
        for key in ["defaultReasoning", "reasoningMode"] {
            if let Some(value) = entry.get(key) {
                if !value.as_str().is_some_and(|text| !text.is_empty()) {
                    return None;
                }
            }
        }
        if let Some(tool_use) = entry.get("toolUse") {
            if !tool_use.is_boolean() {
                return None;
            }
        }
        // Every capability claim has to cite where it came from.
        let sources = entry.get("sources")?.as_array()?;
        if sources.is_empty() || !sources.iter().all(is_http_url) {
            return None;
        }
    }
    Some(())
}

fn is_iso_date(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() == 10
        && bytes[4] == b'-'
        && bytes[7] == b'-'
        && [0, 1, 2, 3, 5, 6, 8, 9]
            .iter()
            .all(|i| bytes[*i].is_ascii_digit())
}

fn is_http_url(value: &Value) -> bool {
    value
        .as_str()
        .is_some_and(|text| text.starts_with("https://") || text.starts_with("http://"))
}

/// A verified payload is newer only when its date strictly exceeds the one we
/// already trust. Equal dates are not adopted: re-adopting the same revision
/// would churn the cache for nothing, and a rolled-back date must never win.
fn is_newer(candidate: &str, current: &str) -> bool {
    candidate > current
}

/// Parse and fully verify a payload, without touching the network.
fn accept(text: &str, current_verified_at: &str) -> Option<Catalog> {
    let value: Value = serde_json::from_str(text).ok()?;
    let bundled = tool_model_profile::bundled();
    validate(&value, bundled.schema_version)?;
    let verified_at = value.get("verifiedAt")?.as_str()?;
    if !is_newer(verified_at, current_verified_at) {
        return None;
    }
    serde_json::from_value(value).ok()
}

/// The catalog to start the process with: a cached revision when it still
/// verifies and still beats the bundled one, otherwise the bundled copy.
///
/// The cache is re-validated on every load rather than trusted because it sat
/// on disk, where anything could have edited it.
pub(crate) fn initial() -> &'static Catalog {
    let bundled = tool_model_profile::bundled();
    let Some(path) = cache_path() else {
        return bundled;
    };
    let Ok(text) = std::fs::read_to_string(&path) else {
        return bundled;
    };
    if text.len() > MAX_CATALOG_BYTES {
        return bundled;
    }
    match accept(&text, &bundled.verified_at) {
        Some(catalog) => Box::leak(Box::new(catalog)),
        None => bundled,
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogProjection {
    request_id: String,
    schema_version: u8,
    status: &'static str,
    reason_code: &'static str,
    verified_at: String,
    model_count: usize,
    /// The verified payload, so the renderer adopts exactly what the native
    /// side adopted instead of fetching a second time and possibly a second
    /// revision.
    catalog: Option<Value>,
}

fn bundled_projection(request_id: String, reason_code: &'static str) -> CatalogProjection {
    CatalogProjection {
        request_id,
        schema_version: 1,
        status: "bundled",
        reason_code,
        verified_at: tool_model_profile::bundled().verified_at.clone(),
        model_count: tool_model_profile::bundled().models.len(),
        catalog: None,
    }
}

async fn text(client: &Client, url: &str, limit: usize) -> Option<String> {
    let response = client
        .get(url)
        .header("accept", "application/json, text/plain")
        .header("accept-encoding", "identity")
        .timeout(std::time::Duration::from_secs(FETCH_TIMEOUT_SECS))
        .send()
        .await
        .ok()?;
    if response.status() != reqwest::StatusCode::OK
        || response.content_length().is_some_and(|n| n > limit as u64)
    {
        return None;
    }
    let body = response.text().await.ok()?;
    (!body.is_empty() && body.len() <= limit).then_some(body)
}

#[tauri::command]
pub async fn refresh_model_catalog_v1(request_id: String) -> Result<CatalogProjection, String> {
    if request_id.is_empty()
        || request_id.len() > 100
        || !request_id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_'))
    {
        return Err("invalid_request".into());
    }
    let Ok(client) = Client::builder().build() else {
        return Ok(bundled_projection(request_id, "fetch_failed"));
    };
    let Some(body) = text(&client, CATALOG_URL, MAX_CATALOG_BYTES).await else {
        return Ok(bundled_projection(request_id, "fetch_failed"));
    };
    let Some(expected) = text(&client, SHA256_URL, MAX_SHA_BYTES).await else {
        return Ok(bundled_projection(request_id, "hash_unavailable"));
    };
    let digest = format!("{:x}", Sha256::digest(body.as_bytes()));
    if !expected.trim().to_ascii_lowercase().starts_with(&digest) {
        return Ok(bundled_projection(request_id, "hash_mismatch"));
    }
    // Compare against what is active, not against the bundled copy: a cached
    // revision may already be ahead of the binary.
    let current = tool_model_profile::profile_catalog_verified_at();
    let Some(catalog) = accept(&body, &current) else {
        // Distinguish "we understood it and it was not newer" from "we could
        // not understand it" — they need different next steps from an operator.
        let readable = serde_json::from_str::<Value>(&body).ok().filter(|value| {
            validate(value, tool_model_profile::bundled().schema_version).is_some()
        });
        return Ok(bundled_projection(
            request_id,
            if readable.is_some() {
                "not_newer"
            } else {
                "invalid_payload"
            },
        ));
    };
    let verified_at = catalog.verified_at.clone();
    let model_count = catalog.models.len();
    // Cache before installing: a write failure must not leave this run using a
    // revision the next run cannot reproduce.
    if let Some(path) = cache_path() {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let temporary = path.with_extension("json.tmp");
        if std::fs::write(&temporary, body.as_bytes()).is_ok() {
            let _ = std::fs::rename(&temporary, &path);
        }
    }
    let payload = serde_json::from_str::<Value>(&body).ok();
    tool_model_profile::install(catalog);
    Ok(CatalogProjection {
        request_id,
        schema_version: 1,
        status: "updated",
        reason_code: "updated",
        verified_at,
        model_count,
        catalog: payload,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn revision(verified_at: &str) -> Value {
        json!({
            "schemaVersion": tool_model_profile::bundled().schema_version,
            "verifiedAt": verified_at,
            "models": [{
                "id": "brand-new-model",
                "displayName": "Brand New Model",
                "reasoningLevels": ["low", "high"],
                "input": ["text", "image"],
                "contextWindow": 1_048_576u64,
                "maxOutputTokens": 128_000u64,
                "toolUse": true,
                "sources": ["https://example.com/brand-new-model"]
            }]
        })
    }

    fn schema_version() -> u32 {
        tool_model_profile::bundled().schema_version
    }

    #[test]
    fn a_well_formed_revision_validates() {
        assert!(validate(&revision("2099-01-01"), schema_version()).is_some());
    }

    /// Each of these is a way an untrusted payload could put something the
    /// picker would render, or the context window a compaction decision rests
    /// on, beyond what the reviewed schema allows.
    #[test]
    fn malformed_revisions_are_refused() {
        let cases: Vec<(&str, Value)> = vec![
            ("wrong schema version", {
                let mut v = revision("2099-01-01");
                v["schemaVersion"] = json!(schema_version() + 1);
                v
            }),
            ("date that is not a date", {
                let mut v = revision("2099-01-01");
                v["verifiedAt"] = json!("not-a-date");
                v
            }),
            ("no models at all", {
                let mut v = revision("2099-01-01");
                v["models"] = json!([]);
                v
            }),
            ("duplicate ids", {
                let mut v = revision("2099-01-01");
                let entry = v["models"][0].clone();
                v["models"] = json!([entry.clone(), entry]);
                v
            }),
            ("empty display name", {
                let mut v = revision("2099-01-01");
                v["models"][0]["displayName"] = json!("");
                v
            }),
            // A zero window would be read as a real number and produce a
            // nonsensical compaction threshold rather than "unknown".
            ("zero context window", {
                let mut v = revision("2099-01-01");
                v["models"][0]["contextWindow"] = json!(0);
                v
            }),
            ("unknown input modality", {
                let mut v = revision("2099-01-01");
                v["models"][0]["input"] = json!(["text", "haptic"]);
                v
            }),
            ("input without text", {
                let mut v = revision("2099-01-01");
                v["models"][0]["input"] = json!(["image"]);
                v
            }),
            ("tool use that is not a boolean", {
                let mut v = revision("2099-01-01");
                v["models"][0]["toolUse"] = json!("yes");
                v
            }),
            ("no sources", {
                let mut v = revision("2099-01-01");
                v["models"][0]["sources"] = json!([]);
                v
            }),
            ("a source that is not a URL", {
                let mut v = revision("2099-01-01");
                v["models"][0]["sources"] = json!(["ask-me"]);
                v
            }),
        ];
        for (name, payload) in cases {
            assert!(
                validate(&payload, schema_version()).is_none(),
                "should have been refused: {name}"
            );
        }
    }

    #[test]
    fn only_a_strictly_newer_revision_is_adopted() {
        assert!(is_newer("2026-09-20", "2026-09-19"));
        // Re-adopting the same revision would churn the cache for nothing.
        assert!(!is_newer("2026-09-19", "2026-09-19"));
        // A rolled-back date must never win.
        assert!(!is_newer("2026-09-18", "2026-09-19"));
    }

    #[test]
    fn accept_refuses_stale_and_unreadable_payloads() {
        let newer = revision("2099-01-01").to_string();
        assert!(accept(&newer, "2026-01-01").is_some());
        assert!(accept(&newer, "2099-01-01").is_none());
        assert!(accept(&newer, "2100-01-01").is_none());
        assert!(accept("not json at all", "2026-01-01").is_none());
        assert!(accept("{}", "2026-01-01").is_none());
    }

    /// The reason this module exists.
    ///
    /// `auto_compact_window` looks a model up in this catalog and writes
    /// Claude Code's `autoCompactWindow` from its context window. A model the
    /// catalog has never heard of gets no window, so Claude Code never
    /// compacts and the conversation grows until the relay rejects it — and
    /// the catalog could only change by shipping a new binary. An accepted
    /// revision has to carry that number for a model the build never knew.
    #[test]
    fn an_accepted_revision_supplies_a_window_for_a_model_the_build_never_knew() {
        assert!(
            tool_model_profile::bundled()
                .models
                .iter()
                .all(|m| m.id != "brand-new-model"),
            "fixture must name a model absent from the bundled catalog"
        );
        let adopted = accept(&revision("2099-01-01").to_string(), "2026-01-01")
            .expect("a newer, well-formed revision is adopted");
        let model = adopted
            .models
            .iter()
            .find(|m| m.id == "brand-new-model")
            .expect("the new model survives deserialization");
        assert_eq!(model.context_window, Some(1_048_576));
        assert_eq!(model.display_name, "Brand New Model");
        assert_eq!(model.tool_use, Some(true));
    }
}

use std::collections::HashSet;

use serde::Serialize;
use serde_json::Value;

pub const CAPABILITIES: [&str; 6] = [
    "device_authorization",
    "account_read",
    "usage_read",
    "models_read",
    "pricing_read",
    "tool_keys_manage",
];

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CatalogError {
    None,
    NetworkError,
    TimedOut,
    HttpError,
    InvalidResponse,
    ResponseTooLarge,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PublicGroup {
    pub id: String,
    pub description: String,
    pub multiplier: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PublicModel {
    pub id: String,
    pub groups: Vec<String>,
    pub endpoints: Vec<String>,
    pub billing_mode: &'static str,
}

#[derive(Debug)]
pub struct PublicCatalog {
    pub service_version: String,
    pub backend_display_exchange_rate: String,
    pub groups: Vec<PublicGroup>,
    pub models: Vec<PublicModel>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DesktopBackend {
    pub status: &'static str,
    pub error: CatalogError,
    pub declared_capabilities: Vec<&'static str>,
}

pub fn safe_text(value: &str, max: usize) -> bool {
    value.chars().count() <= max
        && !value.chars().any(|c| {
            c.is_control() || matches!(c, '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
        })
}

fn identifier(value: &Value, max: usize) -> Result<String, CatalogError> {
    let text = value.as_str().ok_or(CatalogError::InvalidResponse)?;
    if text.trim() != text || text.is_empty() || !safe_text(text, max) {
        return Err(CatalogError::InvalidResponse);
    }
    Ok(text.to_owned())
}

fn number_text(value: &Value) -> Result<String, CatalogError> {
    let number = value.as_f64().ok_or(CatalogError::InvalidResponse)?;
    let text = number.to_string();
    if !number.is_finite() || number <= 0.0 || text.len() > 40 {
        return Err(CatalogError::InvalidResponse);
    }
    Ok(text)
}

fn identifiers(
    value: &Value,
    max_items: usize,
    max_len: usize,
) -> Result<Vec<String>, CatalogError> {
    let items = value.as_array().ok_or(CatalogError::InvalidResponse)?;
    if items.len() > max_items {
        return Err(CatalogError::ResponseTooLarge);
    }
    let mut seen = HashSet::new();
    let mut output = Vec::new();
    for item in items {
        let id = identifier(item, max_len)?;
        if !seen.insert(id.clone()) {
            return Err(CatalogError::InvalidResponse);
        }
        output.push(id);
    }
    Ok(output)
}

/// Only a public projection is produced. Expressions, messages, keys and URLs
/// in the original response never cross the native/renderer boundary.
pub fn parse_public_catalog(
    status: &Value,
    pricing: &Value,
) -> Result<PublicCatalog, CatalogError> {
    if status["success"] != true || pricing["success"] != true {
        return Err(CatalogError::InvalidResponse);
    }
    if status["data"]["system_name"] != "野菜API" {
        return Err(CatalogError::InvalidResponse);
    }
    let service_version = identifier(&status["data"]["version"], 100)?;
    let backend_display_exchange_rate = match &status["data"]["usd_exchange_rate"] {
        Value::Null => String::new(),
        value => number_text(value)?,
    };
    let usable = pricing["usable_group"]
        .as_object()
        .ok_or(CatalogError::InvalidResponse)?;
    let ratios = pricing["group_ratio"]
        .as_object()
        .ok_or(CatalogError::InvalidResponse)?;
    let raw_models = pricing["data"]
        .as_array()
        .ok_or(CatalogError::InvalidResponse)?;
    if usable.len() > 128 || raw_models.len() > 2048 {
        return Err(CatalogError::ResponseTooLarge);
    }
    let mut groups = Vec::new();
    for (id, description) in usable {
        let id = identifier(&Value::String(id.clone()), 128)?;
        let description = description.as_str().ok_or(CatalogError::InvalidResponse)?;
        if !safe_text(description, 1000) {
            return Err(CatalogError::InvalidResponse);
        }
        let multiplier = number_text(ratios.get(&id).ok_or(CatalogError::InvalidResponse)?)?;
        groups.push(PublicGroup {
            id,
            description: description.to_owned(),
            multiplier,
        });
    }
    let group_ids: HashSet<_> = groups.iter().map(|group| group.id.as_str()).collect();
    let mut model_ids = HashSet::new();
    let mut models = Vec::new();
    for raw in raw_models {
        let id = identifier(&raw["model_name"], 200)?;
        if !model_ids.insert(id.clone()) {
            return Err(CatalogError::InvalidResponse);
        }
        let declared_groups = identifiers(&raw["enable_groups"], 128, 128)?;
        let model_groups = if declared_groups.iter().any(|id| id == "all") {
            groups.iter().map(|group| group.id.clone()).collect()
        } else {
            declared_groups
                .into_iter()
                .filter(|id| group_ids.contains(id.as_str()))
                .collect::<Vec<_>>()
        };
        let endpoints = match &raw["supported_endpoint_types"] {
            Value::Null => Vec::new(),
            value => identifiers(value, 32, 80)?,
        };
        let billing_mode = match raw["billing_mode"].as_str() {
            Some("tiered_expr") => "tiered_expr",
            Some("") | None => match raw["quota_type"].as_u64() {
                Some(0) => "ratio",
                Some(1) => "per_request",
                _ => "unknown",
            },
            _ => "unknown",
        };
        if !model_groups.is_empty() {
            models.push(PublicModel {
                id,
                groups: model_groups,
                endpoints,
                billing_mode,
            });
        }
    }
    Ok(PublicCatalog {
        service_version,
        backend_display_exchange_rate,
        groups,
        models,
    })
}

pub fn unavailable_backend(status: &'static str, error: CatalogError) -> DesktopBackend {
    DesktopBackend {
        status,
        error,
        declared_capabilities: Vec::new(),
    }
}

fn exact_keys(value: &Value, keys: &[&str]) -> bool {
    value.as_object().is_some_and(|object| {
        object.len() == keys.len() && keys.iter().all(|key| object.contains_key(*key))
    })
}

/// This is a future server handoff contract, not the existing website session.
/// Recognizing it must never mint a session or enable configuration writes.
pub fn parse_desktop_bootstrap(value: &Value) -> DesktopBackend {
    let data = &value["data"];
    let valid = exact_keys(value, &["success", "data"])
        && value["success"] == true
        && exact_keys(
            data,
            &[
                "schema_version",
                "service",
                "contract_id",
                "minimum_client_version",
                "capabilities",
            ],
        )
        && data["schema_version"].as_f64() == Some(1.0)
        && data["service"] == "yeschoy-desktop"
        && data["contract_id"] == "desktop-bootstrap-v1"
        && exact_keys(&data["capabilities"], &CAPABILITIES)
        && CAPABILITIES
            .iter()
            .all(|name| data["capabilities"][name].is_boolean());
    let version_ok = data["minimum_client_version"]
        .as_str()
        .filter(|version| {
            version.len() <= 100
                && version
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || byte == b'.')
        })
        .and_then(|version| semver::Version::parse(version).ok())
        .is_some_and(|minimum| {
            minimum
                <= semver::Version::parse(env!("CARGO_PKG_VERSION"))
                    .expect("Cargo version is valid SemVer")
        });
    if !valid || !version_ok {
        return unavailable_backend("incompatible", CatalogError::InvalidResponse);
    }
    DesktopBackend {
        status: "contract_recognized",
        error: CatalogError::None,
        declared_capabilities: CAPABILITIES
            .into_iter()
            .filter(|name| data["capabilities"][name] == true)
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn status() -> Value {
        json!({"success":true,"data":{"system_name":"野菜API","version":"v1.0.0-rc.27","usd_exchange_rate":1}})
    }

    fn pricing() -> Value {
        json!({"success":true,"usable_group":{"public":"Chat 流式"},"group_ratio":{"public":0.25},"pricing_version":"fixed-not-a-data-revision","data":[{"model_name":"vendor/model-A","enable_groups":["public"],"supported_endpoint_types":["openai"],"quota_type":0,"billing_mode":"tiered_expr","billing_expr":"secret-expression","key":"should-never-cross-ipc"}]})
    }

    #[test]
    fn projects_public_metadata_without_secrets_expressions_or_price_versions() {
        let result = parse_public_catalog(&status(), &pricing()).unwrap();
        assert_eq!(result.models[0].id, "vendor/model-A");
        assert_eq!(result.models[0].billing_mode, "tiered_expr");
        assert_eq!(result.groups[0].multiplier, "0.25");
        assert_eq!(result.backend_display_exchange_rate, "1");
        let serialized = serde_json::to_string(&result.models).unwrap();
        for forbidden in [
            "secret-expression",
            "should-never-cross-ipc",
            "pricing_version",
        ] {
            assert!(!serialized.contains(forbidden));
        }
    }

    #[test]
    fn preserves_empty_catalog_and_does_not_invent_default_ratios() {
        let empty = json!({"success":true,"usable_group":{},"group_ratio":{},"data":[]});
        assert!(parse_public_catalog(&status(), &empty)
            .unwrap()
            .models
            .is_empty());
        let mut missing = pricing();
        missing["group_ratio"] = json!({});
        assert_eq!(
            parse_public_catalog(&status(), &missing).unwrap_err(),
            CatalogError::InvalidResponse
        );
    }

    #[test]
    fn rejects_malformed_duplicate_and_oversize_metadata() {
        let mut duplicate = pricing();
        let model = duplicate["data"][0].clone();
        duplicate["data"].as_array_mut().unwrap().push(model);
        assert_eq!(
            parse_public_catalog(&status(), &duplicate).unwrap_err(),
            CatalogError::InvalidResponse
        );
        for invalid in [
            json!(null),
            json!({"success":false,"message":"private-server-error"}),
        ] {
            assert_eq!(
                parse_public_catalog(&status(), &invalid).unwrap_err(),
                CatalogError::InvalidResponse
            );
        }
        let mut huge = pricing();
        huge["data"] = Value::Array(vec![huge["data"][0].clone(); 2049]);
        assert_eq!(
            parse_public_catalog(&status(), &huge).unwrap_err(),
            CatalogError::ResponseTooLarge
        );
        assert!(!safe_text("model\nkey", 200));
        assert!(!safe_text("model\u{202e}", 200));
    }

    #[test]
    fn filters_groups_without_guessing_stream_restrictions_from_prose() {
        let mut source = pricing();
        source["data"][0]["enable_groups"] = json!(["private"]);
        assert!(parse_public_catalog(&status(), &source)
            .unwrap()
            .models
            .is_empty());
        source["data"][0]["enable_groups"] = json!(["all"]);
        source["data"][0]["supported_endpoint_types"] = Value::Null;
        let catalog = parse_public_catalog(&status(), &source).unwrap();
        assert_eq!(catalog.models[0].groups, ["public"]);
        assert!(catalog.models[0].endpoints.is_empty());
    }

    #[test]
    fn bootstrap_recognition_never_implies_login_and_rejects_drift() {
        let valid: Value = serde_json::from_str(include_str!(
            "../../contracts/fixtures/desktop-bootstrap/recognized.json"
        ))
        .unwrap();
        let result = parse_desktop_bootstrap(&valid);
        assert_eq!(result.status, "contract_recognized");
        assert!(result.declared_capabilities.is_empty());
        for (field, value) in [
            ("schema_version", json!(2)),
            ("minimum_client_version", json!("99.0.0")),
            ("minimum_client_version", json!("bad")),
            ("service", json!("other-service")),
        ] {
            let mut invalid = valid.clone();
            invalid["data"][field] = value;
            assert_eq!(parse_desktop_bootstrap(&invalid).status, "incompatible");
        }
        let mut unknown = valid.clone();
        unknown["data"]["capabilities"]["admin"] = json!(true);
        assert_eq!(parse_desktop_bootstrap(&unknown).status, "incompatible");
    }
}

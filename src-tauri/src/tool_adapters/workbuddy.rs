use std::path::{Path, PathBuf};

use serde_json::{Map, Value};

use crate::{
    tool_adapters::{
        common::{self, ConfigFailure, FileTransaction},
        AdapterFailure,
    },
    tool_credentials::ToolCredential,
    tool_model_profile,
};

const VENDOR: &str = "野菜API";
/// Shown in WorkBuddy's model picker ahead of the model id it appends itself.
const VENDOR_MARK: &str = "野菜";

pub(crate) struct Prepared {
    transaction: FileTransaction,
    path: PathBuf,
    endpoint: String,
    expected: Vec<(String, String)>,
}

fn config_error(error: ConfigFailure) -> AdapterFailure {
    match error {
        ConfigFailure::ExternalChange => AdapterFailure::ExternalOverride,
        ConfigFailure::Read => AdapterFailure::ConfigurationFailed("configuration_read_failed"),
        ConfigFailure::Parse => AdapterFailure::ConfigurationFailed("configuration_parse_failed"),
        ConfigFailure::Write => AdapterFailure::ConfigurationFailed("configuration_write_failed"),
        ConfigFailure::Readback => {
            AdapterFailure::ConfigurationFailed("configuration_readback_failed")
        }
        ConfigFailure::Rollback => {
            AdapterFailure::ConfigurationFailed("configuration_rollback_failed")
        }
    }
}

fn endpoint(origin: &str) -> String {
    format!("{}/v1/chat/completions", origin.trim_end_matches('/'))
}

fn parse(existing: Option<&[u8]>) -> Result<Value, ()> {
    match existing {
        None | Some([]) => Ok(Value::Array(Vec::new())),
        Some(bytes) => serde_json::from_slice(bytes).map_err(|_| ()),
    }
}

fn configured_model(id: &str, key: &str, endpoint: &str, previous: Option<&Value>) -> Value {
    let mut model = previous
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_else(Map::new);
    model.insert("id".into(), id.into());
    // Just the vendor mark. WorkBuddy renders its picker as `<name>:<id>`, so
    // carrying a display name here said the same thing twice and truncated:
    // "野菜 DeepSeek V4.1 Flash:deepseek-v4.1-flash". As "野菜" it reads
    // "野菜:deepseek-v4.1-flash" -- whose model it is, and which one, with no
    // room wasted on repeating it.
    model.insert("name".into(), VENDOR_MARK.into());
    model.insert("vendor".into(), VENDOR.into());
    model.insert("url".into(), endpoint.into());
    model.insert("apiKey".into(), key.into());
    // Full /v1/chat/completions URL. WorkBuddy otherwise appends that path again.
    model.insert("useCustomProtocol".into(), true.into());

    for owned in [
        "supportsToolCall",
        "supportsImages",
        "supportsReasoning",
        "maxInputTokens",
        "maxOutputTokens",
    ] {
        model.remove(owned);
    }
    if let Some(profile) = tool_model_profile::profile(id) {
        if let Some(value) = profile.tool_use {
            model.insert("supportsToolCall".into(), value.into());
        }
        model.insert(
            "supportsImages".into(),
            profile
                .input
                .as_ref()
                .is_some_and(|items| items.iter().any(|item| item == "image"))
                .into(),
        );
        model.insert(
            "supportsReasoning".into(),
            (!profile.reasoning_levels.is_empty()).into(),
        );
        if let Some(value) = profile.context_window {
            model.insert("maxInputTokens".into(), value.into());
        }
        if let Some(value) = profile.max_output_tokens {
            model.insert("maxOutputTokens".into(), value.into());
        }
    }
    Value::Object(model)
}

fn update_models(
    models: &mut Vec<Value>,
    endpoint: &str,
    credential: &ToolCredential,
) -> Result<Vec<(String, String)>, ()> {
    let ids = credential.model_ids();
    let selected = ids.iter().collect::<std::collections::HashSet<_>>();
    let mut previous = std::collections::HashMap::<String, Value>::new();
    let mut retained = Vec::with_capacity(models.len() + ids.len());
    for value in std::mem::take(models) {
        let id = value.get("id").and_then(Value::as_str).map(str::to_owned);
        if let Some(id) = id.filter(|id| selected.contains(id)) {
            previous.entry(id).or_insert(value);
        } else {
            retained.push(value);
        }
    }
    let mut expected = Vec::with_capacity(ids.len());
    for id in ids {
        let route = credential.resolve_model(&id).map_err(|_| ())?;
        retained.push(configured_model(
            &id,
            route.upstream_key(),
            endpoint,
            previous.get(&id),
        ));
        expected.push((id, route.upstream_key().to_owned()));
    }
    *models = retained;
    Ok(expected)
}

/// Rendered catalog bytes, plus the (model id, display name) pairs written
/// into it so the caller can read them back without re-parsing.
type RenderedCatalog = (Vec<u8>, Vec<(String, String)>);

fn render(
    existing: Option<&[u8]>,
    endpoint: &str,
    credential: &ToolCredential,
) -> Result<RenderedCatalog, ()> {
    let mut root = parse(existing)?;
    let expected = match &mut root {
        Value::Array(models) => update_models(models, endpoint, credential)?,
        Value::Object(object) => {
            let models = object
                .entry("models")
                .or_insert_with(|| Value::Array(Vec::new()))
                .as_array_mut()
                .ok_or(())?;
            let expected = update_models(models, endpoint, credential)?;
            if let Some(available) = object
                .get_mut("availableModels")
                .and_then(Value::as_array_mut)
            {
                for (id, _) in &expected {
                    if !available.iter().any(|value| value.as_str() == Some(id)) {
                        available.push(id.clone().into());
                    }
                }
            }
            expected
        }
        _ => return Err(()),
    };
    let mut bytes = serde_json::to_vec_pretty(&root).map_err(|_| ())?;
    bytes.push(b'\n');
    Ok((bytes, expected))
}

fn model_array(root: &Value) -> Option<&Vec<Value>> {
    match root {
        Value::Array(models) => Some(models),
        Value::Object(object) => object.get("models")?.as_array(),
        _ => None,
    }
}

pub(crate) fn prepare_catalog(
    home: &Path,
    credential: &ToolCredential,
) -> Result<Prepared, AdapterFailure> {
    let path = home.join(".workbuddy").join("models.json");
    let before = common::snapshot(&path)
        .map_err(|_| AdapterFailure::ConfigurationFailed("configuration_read_failed"))?;
    let endpoint = endpoint(&credential.origin);
    if endpoint.starts_with("http://127.") || endpoint.starts_with("http://localhost") {
        return Err(AdapterFailure::ConfigurationFailed("invalid_request"));
    }
    let (after, expected) = render(before.as_deref(), &endpoint, credential)
        .map_err(|_| AdapterFailure::ConfigurationFailed("configuration_parse_failed"))?;
    let transaction =
        FileTransaction::stage_with_snapshot(path.clone(), before, after).map_err(config_error)?;
    Ok(Prepared {
        transaction,
        path,
        endpoint,
        expected,
    })
}

impl Prepared {
    pub(crate) fn changes(&self) -> &[common::FileChange] {
        self.transaction.changes()
    }

    pub(crate) fn commit(&mut self) -> Result<(), AdapterFailure> {
        // WorkBuddy has no credential-helper indirection, so unlike every other
        // adapter this file holds the live relay key in clear text.
        //
        // Narrowed before the write, because `atomic_write` copies the target's
        // mode onto the replacement — tightening here covers the new content
        // too. See `narrow_third_party_file` for why this deliberately stops at
        // the POSIX mode and does not touch a Windows DACL.
        //
        // Best effort, unlike the break-glass copy. That file is optional and
        // fails closed; this one *is* the activation — refusing to write it
        // would just mean WorkBuddy cannot be connected at all. The user is told
        // in `readyWorkBuddy` that this app keeps its key on disk.
        let _ = common::narrow_third_party_file(&self.path);
        self.transaction.commit().map_err(config_error)?;
        self.validate_existing()
    }

    pub(crate) fn validate_existing(&self) -> Result<(), AdapterFailure> {
        let bytes = common::snapshot(&self.path)
            .map_err(|_| AdapterFailure::ConfigurationFailed("configuration_readback_failed"))?
            .ok_or(AdapterFailure::ConfigurationFailed(
                "configuration_readback_failed",
            ))?;
        let root: Value = serde_json::from_slice(&bytes)
            .map_err(|_| AdapterFailure::ConfigurationFailed("configuration_readback_failed"))?;
        let models = model_array(&root).ok_or(AdapterFailure::ConfigurationFailed(
            "configuration_readback_failed",
        ))?;
        let valid = self.expected.iter().all(|(id, key)| {
            let matching = models
                .iter()
                .filter(|model| model.get("id").and_then(Value::as_str) == Some(id))
                .collect::<Vec<_>>();
            matching.len() == 1
                && matching[0].get("vendor").and_then(Value::as_str) == Some(VENDOR)
                && matching[0].get("url").and_then(Value::as_str) == Some(self.endpoint.as_str())
                && matching[0].get("apiKey").and_then(Value::as_str) == Some(key)
                && matching[0]
                    .get("useCustomProtocol")
                    .and_then(Value::as_bool)
                    == Some(true)
        });
        valid
            .then_some(())
            .ok_or(AdapterFailure::ConfigurationFailed(
                "configuration_readback_failed",
            ))
    }

    pub(crate) fn rollback(&mut self) -> Result<(), AdapterFailure> {
        self.transaction.rollback().map_err(config_error)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tool_credentials::ToolModelRoute;
    use serde_json::json;

    fn credential() -> ToolCredential {
        let routes = ["gpt-6-astra", "deepseek-v4.1-flash"]
            .into_iter()
            .map(|id| ToolModelRoute {
                model_id: id.into(),
                billing_group: format!("group-{id}"),
                api_key: format!("synthetic-workbuddy-key-{id}"),
                origin: "https://yeschoy.com".into(),
                claude_transport: None,
                codex_transport: None,
            })
            .collect::<Vec<_>>();
        ToolCredential {
            api_key: routes[0].api_key.clone(),
            origin: "https://yeschoy.com".into(),
            model_id: routes[0].model_id.clone(),
            local_gateway_token: Some(format!("ycg-{}", "a".repeat(64))),
            claude_transport: None,
            codex_transport: None,
            models: routes,
        }
    }

    #[test]
    fn direct_array_preserves_unrelated_models_and_preferences_then_rolls_back() {
        let home = common::temporary_working_directory("workbuddy-array").unwrap();
        let path = home.join(".workbuddy/models.json");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let original = br#"[{"id":"other","vendor":"Other","apiKey":"keep"},{"id":"gpt-6-astra","temperature":0.2,"apiKey":"old"}]"#;
        std::fs::write(&path, original).unwrap();
        let mut prepared = prepare_catalog(&home, &credential()).unwrap();
        prepared.commit().unwrap();
        let root: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        let models = root.as_array().unwrap();
        assert_eq!(
            models.len(),
            3,
            "selected models must be replaced, not duplicated"
        );
        assert_eq!(
            models
                .iter()
                .filter(|model| model["id"] == "gpt-6-astra")
                .count(),
            1
        );
        assert_eq!(models[0]["apiKey"], "keep");
        let astra = models.iter().find(|m| m["id"] == "gpt-6-astra").unwrap();
        let deepseek = models
            .iter()
            .find(|m| m["id"] == "deepseek-v4.1-flash")
            .unwrap();
        assert_eq!(astra["temperature"], 0.2);
        assert_eq!(astra["apiKey"], "synthetic-workbuddy-key-gpt-6-astra");
        assert_eq!(
            deepseek["apiKey"],
            "synthetic-workbuddy-key-deepseek-v4.1-flash"
        );
        assert_eq!(astra["url"], "https://yeschoy.com/v1/chat/completions");
        assert_eq!(astra["useCustomProtocol"], true);
        // WorkBuddy appends `:<id>` itself, so the name carries only the mark.
        assert_eq!(astra["name"], "野菜");
        assert_eq!(deepseek["name"], "野菜");
        // The id is what tells them apart, and it must stay the exact wire id.
        assert_eq!(astra["id"], "gpt-6-astra");
        assert_eq!(deepseek["id"], "deepseek-v4.1-flash");
        assert_ne!(astra["apiKey"], deepseek["apiKey"]);
        assert!(!String::from_utf8_lossy(&std::fs::read(&path).unwrap()).contains("127.0.0.1"));
        prepared.rollback().unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), original);
        std::fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn documented_wrapper_and_available_models_are_preserved() {
        let source = serde_json::to_vec(&json!({
            "models": [{"id":"other","custom":true}],
            "availableModels": ["other"],
            "future": {"keep": true}
        }))
        .unwrap();
        let (bytes, _) = render(
            Some(&source),
            "https://api.yeschoy.com/v1/chat/completions",
            &credential(),
        )
        .unwrap();
        let value: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(value["future"]["keep"], true);
        let available = value["availableModels"].as_array().unwrap();
        assert_eq!(
            available.iter().filter(|id| *id == "gpt-6-astra").count(),
            1
        );
        assert_eq!(value["models"][0]["custom"], true);
    }

    #[test]
    fn malformed_or_tampered_catalog_fails_closed() {
        assert!(render(
            Some(br#"{"models":"wrong"}"#),
            "https://yeschoy.com/v1/chat/completions",
            &credential()
        )
        .is_err());
        let home = common::temporary_working_directory("workbuddy-tamper").unwrap();
        let mut prepared = prepare_catalog(&home, &credential()).unwrap();
        prepared.commit().unwrap();
        let path = home.join(".workbuddy/models.json");
        let mut value: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        value[0]["url"] = "https://other.invalid/v1/chat/completions".into();
        std::fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();
        assert!(prepared.validate_existing().is_err());
        std::fs::remove_dir_all(home).unwrap();
    }
}

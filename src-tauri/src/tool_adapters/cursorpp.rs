//! Cursor++ (`@cometix/ccursor`): only a copy the user installed themselves.
//!
//! YesChoy never installs it or patches Cursor. It only adds its own
//! providers to `~/.ccursor/providers.json`, which Cursor++ re-reads on its
//! own every two seconds, so nothing has to be restarted.
use std::path::{Path, PathBuf};

use serde_json::{json, Map, Value};

use crate::{
    tool_adapters::{
        common::{self, ConfigFailure, FileTransaction},
        AdapterFailure,
    },
    tool_credentials::ToolCredential,
    tool_model_profile,
};

pub(crate) const VENDOR: &str = "野菜API";
/// Ids of the providers we own start with this; `yeschoy`, `yeschoy-2`, ...
pub(crate) const PROVIDER_PREFIX: &str = "yeschoy";

pub(crate) fn config_dir(home: &Path) -> PathBuf {
    home.join(".ccursor")
}

pub(crate) fn providers_path(home: &Path) -> PathBuf {
    config_dir(home).join("providers.json")
}

pub(crate) fn is_owned_provider(provider: &Value) -> bool {
    provider.get("name").and_then(Value::as_str) == Some(VENDOR)
        && provider.get("type").and_then(Value::as_str) == Some("openai-chat")
        && provider
            .get("id")
            .and_then(Value::as_str)
            .is_some_and(|id| {
                id == PROVIDER_PREFIX
                    || id
                        .strip_prefix(PROVIDER_PREFIX)
                        .and_then(|rest| rest.strip_prefix('-'))
                        .is_some_and(|n| n.parse::<u32>().is_ok())
            })
}

pub(crate) struct Prepared {
    transaction: FileTransaction,
    path: PathBuf,
    expected: Vec<Value>,
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

fn model_entry(id: &str) -> Value {
    let mut model = Map::new();
    model.insert("id".into(), id.into());
    model.insert("apiModel".into(), id.into());
    let profile = tool_model_profile::profile(id);
    let name = profile
        .as_ref()
        .map(|p| p.display_name.clone())
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| id.to_owned());
    model.insert("displayName".into(), name.into());
    model.insert("defaultOn".into(), true.into());
    if let Some(profile) = &profile {
        if let Some(value) = profile.context_window {
            model.insert("contextTokenLimit".into(), value.into());
        }
        if let Some(value) = profile.max_output_tokens {
            model.insert("maxOutputTokens".into(), value.into());
        }
    }
    Value::Object(model)
}

/// One provider per scoped key: each billing group has its own key.
fn owned_providers(base_url: &str, credential: &ToolCredential) -> Result<Vec<Value>, ()> {
    let mut groups: Vec<(String, Vec<Value>)> = Vec::new();
    for id in credential.model_ids() {
        let route = credential.resolve_model(&id).map_err(|_| ())?;
        let key = route.upstream_key().to_owned();
        match groups.iter_mut().find(|(k, _)| *k == key) {
            Some((_, models)) => models.push(model_entry(&id)),
            None => groups.push((key, vec![model_entry(&id)])),
        }
    }
    Ok(groups
        .into_iter()
        .map(|(key, models)| {
            json!({
                "id": PROVIDER_PREFIX,
                "name": VENDOR,
                "type": "openai-chat",
                "baseUrl": base_url,
                "auth": {"kind": "apiKey", "value": key},
                "models": models,
            })
        })
        .collect())
}

/// Keeps the ids our providers already have, then takes `yeschoy`,
/// `yeschoy-2`, ... skipping any id one of the user's own providers has:
/// Cursor++ and the restore both key providers by id, and a stable id lets a
/// later readback recognise what was written.
fn assign_ids(owned: &mut [Value], kept: Vec<String>, providers: &[Value]) {
    let taken: Vec<&str> = providers
        .iter()
        .filter_map(|provider| provider.get("id").and_then(Value::as_str))
        .collect();
    let fresh = (1u32..).map(|n| {
        if n == 1 {
            PROVIDER_PREFIX.to_owned()
        } else {
            format!("{PROVIDER_PREFIX}-{n}")
        }
    });
    let mut used: Vec<String> = Vec::new();
    let mut candidates = kept.into_iter().chain(fresh);
    for provider in owned {
        let id = candidates
            .by_ref()
            .find(|id| !taken.contains(&id.as_str()) && !used.contains(id))
            .unwrap_or_default();
        used.push(id.clone());
        provider["id"] = id.into();
    }
}

fn render(existing: Option<&[u8]>, owned: &mut [Value]) -> Result<Vec<u8>, ()> {
    let mut root = match existing {
        None | Some([]) => json!({"$schemaVersion": 1, "providers": []}),
        Some(bytes) => serde_json::from_slice::<Value>(bytes).map_err(|_| ())?,
    };
    let object = root.as_object_mut().ok_or(())?;
    let providers = object
        .entry("providers")
        .or_insert_with(|| Value::Array(Vec::new()))
        .as_array_mut()
        .ok_or(())?;
    let kept = providers
        .iter()
        .filter(|provider| is_owned_provider(provider))
        .filter_map(|provider| provider.get("id").and_then(Value::as_str))
        .map(str::to_owned)
        .collect();
    providers.retain(|provider| !is_owned_provider(provider));
    assign_ids(owned, kept, providers);
    // Cursor++ takes the first provider that lists a model id: ours go first,
    // so a model the user picked here is served by the key just issued.
    for (index, provider) in owned.iter().enumerate() {
        providers.insert(index, provider.clone());
    }
    let mut bytes = serde_json::to_vec_pretty(&root).map_err(|_| ())?;
    bytes.push(b'\n');
    Ok(bytes)
}

pub(crate) fn prepare(
    home: &Path,
    credential: &ToolCredential,
) -> Result<Prepared, AdapterFailure> {
    if !config_dir(home).is_dir() {
        return Err(AdapterFailure::ConfigurationFailed(
            "configuration_read_failed",
        ));
    }
    let path = providers_path(home);
    let before = common::snapshot(&path)
        .map_err(|_| AdapterFailure::ConfigurationFailed("configuration_read_failed"))?;
    let base_url = crate::provider_presets::openai_base(&credential.origin);
    let mut expected = owned_providers(&base_url, credential)
        .map_err(|_| AdapterFailure::ConfigurationFailed("invalid_request"))?;
    let after = render(before.as_deref(), &mut expected)
        .map_err(|_| AdapterFailure::ConfigurationFailed("configuration_parse_failed"))?;
    let transaction =
        FileTransaction::stage_with_snapshot(path.clone(), before, after).map_err(config_error)?;
    Ok(Prepared {
        transaction,
        path,
        expected,
    })
}

impl Prepared {
    pub(crate) fn changes(&self) -> &[common::FileChange] {
        self.transaction.changes()
    }

    pub(crate) fn commit(&mut self) -> Result<(), AdapterFailure> {
        // The file holds the scoped key in clear text, like WorkBuddy's.
        let _ = common::narrow_third_party_file(&self.path);
        self.transaction.commit().map_err(config_error)?;
        self.validate_existing()
    }

    pub(crate) fn validate_existing(&self) -> Result<(), AdapterFailure> {
        let failed = AdapterFailure::ConfigurationFailed("configuration_readback_failed");
        let bytes = common::snapshot(&self.path)
            .map_err(|_| failed)?
            .ok_or(failed)?;
        let root: Value = serde_json::from_slice(&bytes).map_err(|_| failed)?;
        let providers = root
            .get("providers")
            .and_then(Value::as_array)
            .ok_or(failed)?;
        let present = self.expected.iter().all(|want| {
            providers.iter().any(|have| {
                have["id"] == want["id"]
                    && have["type"] == want["type"]
                    && have["baseUrl"] == want["baseUrl"]
                    && have["auth"] == want["auth"]
                    && have["models"]
                        .as_array()
                        .zip(want["models"].as_array())
                        .is_some_and(|(h, w)| {
                            w.iter().all(|m| h.iter().any(|x| x["id"] == m["id"]))
                        })
            })
        });
        present.then_some(()).ok_or(failed)
    }

    pub(crate) fn rollback(&mut self) -> Result<(), AdapterFailure> {
        self.transaction.rollback().map_err(config_error)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tool_credentials::ToolModelRoute;

    fn credential() -> ToolCredential {
        let routes = [
            ("glm-5.3-flash", "a"),
            ("kimi-k3", "a"),
            ("claude-opus-5-5", "b"),
        ]
        .into_iter()
        .map(|(id, group)| ToolModelRoute {
            model_id: id.into(),
            billing_group: format!("group-{group}"),
            api_key: format!("synthetic-cursorpp-key-{group}"),
            origin: "https://yeschoy.com".into(),
            claude_transport: None,
            codex_transport: None,
        })
        .collect::<Vec<_>>();
        ToolCredential {
            pasted_key: false,
            api_key: routes[0].api_key.clone(),
            origin: "https://yeschoy.com".into(),
            model_id: routes[0].model_id.clone(),
            local_gateway_token: None,
            claude_transport: None,
            codex_transport: None,
            models: routes,
        }
    }

    #[test]
    fn needs_an_existing_cursorpp_install() {
        let home = common::temporary_working_directory("cursorpp-missing").unwrap();
        assert!(prepare(&home, &credential()).is_err());
        assert!(!providers_path(&home).exists());
        std::fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn adds_one_provider_per_key_first_and_keeps_the_users_own() {
        let home = common::temporary_working_directory("cursorpp-merge").unwrap();
        std::fs::create_dir_all(config_dir(&home)).unwrap();
        let original = br#"{"$schemaVersion":1,"providers":[{"id":"mine","name":"Mine","type":"anthropic","baseUrl":"https://api.anthropic.com","auth":{"kind":"apiKey","value":"keep"},"models":[{"id":"kimi-k3"}]}],"future":true}"#;
        std::fs::write(providers_path(&home), original).unwrap();
        let mut prepared = prepare(&home, &credential()).unwrap();
        prepared.commit().unwrap();
        let root: Value =
            serde_json::from_slice(&std::fs::read(providers_path(&home)).unwrap()).unwrap();
        let providers = root["providers"].as_array().unwrap();
        assert_eq!(providers.len(), 3);
        assert_eq!(providers[0]["id"], "yeschoy");
        assert_eq!(providers[0]["type"], "openai-chat");
        assert_eq!(providers[0]["baseUrl"], "https://yeschoy.com/v1");
        assert_eq!(providers[0]["auth"]["value"], "synthetic-cursorpp-key-a");
        assert_eq!(providers[0]["models"].as_array().unwrap().len(), 2);
        assert_eq!(providers[0]["models"][0]["apiModel"], "glm-5.3-flash");
        assert_eq!(providers[1]["id"], "yeschoy-2");
        assert_eq!(providers[1]["auth"]["value"], "synthetic-cursorpp-key-b");
        assert_eq!(providers[2]["auth"]["value"], "keep");
        assert_eq!(root["future"], true);
        assert!(is_owned_provider(&providers[1]));
        assert!(!is_owned_provider(&providers[2]));
        prepared.rollback().unwrap();
        assert_eq!(std::fs::read(providers_path(&home)).unwrap(), original);
        std::fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn reconnecting_replaces_ours_instead_of_stacking() {
        let owned = || owned_providers("https://yeschoy.com/v1", &credential()).unwrap();
        let first = render(None, &mut owned()).unwrap();
        let again = render(Some(&first), &mut owned()).unwrap();
        assert_eq!(first, again);
    }

    #[test]
    fn never_takes_an_id_one_of_the_users_providers_has() {
        for name in ["My relay", VENDOR] {
            let existing = serde_json::to_vec(&json!({"providers": [
                {"id": "yeschoy", "name": name, "type": "anthropic", "models": []},
                {"id": "yeschoy-3", "name": "Other", "type": "gemini", "models": []}
            ]}))
            .unwrap();
            let mut owned = owned_providers("https://yeschoy.com/v1", &credential()).unwrap();
            let root: Value =
                serde_json::from_slice(&render(Some(&existing), &mut owned).unwrap()).unwrap();
            let ids: Vec<&str> = root["providers"]
                .as_array()
                .unwrap()
                .iter()
                .map(|p| p["id"].as_str().unwrap())
                .collect();
            assert_eq!(
                ids,
                ["yeschoy-2", "yeschoy-4", "yeschoy", "yeschoy-3"],
                "{name}"
            );
            assert_eq!(root["providers"][2]["name"], name);
        }
    }

    #[test]
    fn keeps_our_id_after_the_user_frees_a_lower_one() {
        let existing = serde_json::to_vec(&json!({"providers": [
            {"id": "yeschoy", "name": "My relay", "type": "anthropic", "models": []}
        ]}))
        .unwrap();
        let owned = || owned_providers("https://yeschoy.com/v1", &credential()).unwrap();
        let first: Value =
            serde_json::from_slice(&render(Some(&existing), &mut owned()).unwrap()).unwrap();
        let ours = first["providers"][0].clone();
        assert_eq!(ours["id"], "yeschoy-2");
        let freed = serde_json::to_vec(&json!({"providers": [ours]})).unwrap();
        let mut again = owned();
        render(Some(&freed), &mut again).unwrap();
        assert_eq!(again[0]["id"], "yeschoy-2");
    }

    #[test]
    fn unreadable_file_is_left_alone() {
        assert!(render(Some(b"{not json"), &mut []).is_err());
        assert!(render(Some(br#"{"providers":{}}"#), &mut []).is_err());
        assert!(render(Some(b"[]"), &mut []).is_err());
    }
}

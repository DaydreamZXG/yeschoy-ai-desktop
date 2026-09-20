use std::path::{Path, PathBuf};

use serde_json::{json, Map, Value};

use crate::{
    claude_bridge,
    local_bridge::LocalBridgeRuntime,
    tool_adapters::{
        common::{self, ConfigFailure, FileTransaction},
        AdapterFailure,
    },
    tool_credentials::{self, ToolCredential},
};

const PROFILE_ID: &str = "00000000-0000-4000-8000-000000157220";
const PROFILE_NAME: &str = "野菜API";
const PROXY_ADDRESS: &str = "127.0.0.1:15729";
const PROXY_BASE: &str = "http://127.0.0.1:15729/claude-desktop";
const SAFE_ROUTE_MODEL: &str = "claude-sonnet-4-6";

#[derive(Clone)]
pub(crate) struct ClaudeDesktopRuntimeState {
    runtime: LocalBridgeRuntime,
}

impl Default for ClaudeDesktopRuntimeState {
    fn default() -> Self {
        Self {
            runtime: claude_bridge::claude_runtime(PROXY_ADDRESS, "/claude-desktop"),
        }
    }
}

pub(crate) struct Prepared {
    transaction: FileTransaction,
    normal_config_path: PathBuf,
    threep_config_path: PathBuf,
    profile_path: PathBuf,
    meta_path: PathBuf,
    model: String,
    model_ids: Vec<String>,
    modern: bool,
    local_token: String,
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

fn json_document(existing: Option<&[u8]>) -> Result<Value, ()> {
    match existing {
        Some(bytes) if !bytes.is_empty() => {
            let value: Value = serde_json::from_slice(bytes).map_err(|_| ())?;
            value.is_object().then_some(value).ok_or(())
        }
        _ => Ok(Value::Object(Map::new())),
    }
}

fn pretty(value: &Value) -> Result<Vec<u8>, ()> {
    let mut bytes = serde_json::to_vec_pretty(value).map_err(|_| ())?;
    bytes.push(b'\n');
    Ok(bytes)
}

fn deployment_config(existing: Option<&[u8]>) -> Result<Vec<u8>, ()> {
    let mut value = json_document(existing)?;
    value
        .as_object_mut()
        .ok_or(())?
        .insert("deploymentMode".into(), "3p".into());
    pretty(&value)
}

fn profile_config(model: &str, local_token: &str) -> Result<Vec<u8>, ()> {
    pretty(&json!({
        "coworkEgressAllowedHosts": ["*"],
        "disableDeploymentModeChooser": true,
        "inferenceGatewayApiKey": local_token,
        "inferenceGatewayAuthScheme": "bearer",
        "inferenceGatewayBaseUrl": PROXY_BASE,
        "inferenceProvider": "gateway",
        "inferenceModels": [{
            "name": SAFE_ROUTE_MODEL,
            "labelOverride": model,
            "supports1m": crate::tool_model_profile::supports_one_m_context(model)
        }]
    }))
}

fn profile_catalog(model: &str, local_token: &str, model_ids: &[String]) -> Result<Vec<u8>, ()> {
    let mut profile: Value =
        serde_json::from_slice(&profile_config(model, local_token)?).map_err(|_| ())?;
    // The configured default leads the vendor list. Claude Desktop validates
    // the gateway route shape, while labelOverride preserves the real account
    // model identity for the user.
    let ordered = std::iter::once(model).chain(
        model_ids
            .iter()
            .map(String::as_str)
            .filter(|id| *id != model),
    );
    profile["inferenceModels"] = json!(ordered
        .map(|id| json!({
            "name": crate::tool_model_profile::claude_gateway_route_id(id),
            "labelOverride": crate::tool_model_profile::display_name(id),
            "supports1m": crate::tool_model_profile::supports_one_m_context(id)
        }))
        .collect::<Vec<_>>());
    pretty(&profile)
}

fn meta_config(existing: Option<&[u8]>) -> Result<Vec<u8>, ()> {
    let mut value = json_document(existing)?;
    let object = value.as_object_mut().ok_or(())?;
    let mut entries = object
        .get("entries")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    entries.retain(|entry| entry.get("id").and_then(Value::as_str) != Some(PROFILE_ID));
    entries.push(json!({"id": PROFILE_ID, "name": PROFILE_NAME}));
    object.insert("entries".into(), Value::Array(entries));
    object.insert("appliedId".into(), PROFILE_ID.into());
    pretty(&value)
}

pub(crate) fn current_paths(
    home: &Path,
) -> Result<(PathBuf, PathBuf, PathBuf, PathBuf), AdapterFailure> {
    #[cfg(target_os = "macos")]
    let (normal, threep) = {
        let support = home.join("Library").join("Application Support");
        (support.join("Claude"), support.join("Claude-3p"))
    };
    #[cfg(target_os = "windows")]
    let (normal, threep) = {
        let local = std::env::var_os("LOCALAPPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join("AppData").join("Local"));
        (local.join("Claude"), local.join("Claude-3p"))
    };
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    return Err(AdapterFailure::UnsupportedProfile);

    #[cfg(any(target_os = "macos", target_os = "windows"))]
    {
        let library = threep.join("configLibrary");
        Ok((
            normal.join("claude_desktop_config.json"),
            threep.join("claude_desktop_config.json"),
            library.join(format!("{PROFILE_ID}.json")),
            library.join("_meta.json"),
        ))
    }
}

fn new_local_token() -> Result<String, AdapterFailure> {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).map_err(|_| AdapterFailure::SecureStorageUnavailable)?;
    let mut value = String::with_capacity(68);
    value.push_str("ycg-");
    for byte in bytes {
        use std::fmt::Write as _;
        write!(&mut value, "{byte:02x}").map_err(|_| AdapterFailure::SecureStorageUnavailable)?;
    }
    Ok(value)
}

pub(crate) fn prepare(
    home: &Path,
    model: &str,
    existing_local_token: Option<&str>,
) -> Result<Prepared, AdapterFailure> {
    prepare_inner(
        home,
        model,
        existing_local_token,
        &[model.to_owned()],
        false,
    )
}

pub(crate) fn prepare_catalog(
    home: &Path,
    model: &str,
    existing_local_token: Option<&str>,
    model_ids: &[String],
) -> Result<Prepared, AdapterFailure> {
    crate::tool_adapters::common::validate_catalog(model, model_ids)?;
    prepare_inner(home, model, existing_local_token, model_ids, true)
}

fn prepare_inner(
    home: &Path,
    model: &str,
    existing_local_token: Option<&str>,
    model_ids: &[String],
    modern: bool,
) -> Result<Prepared, AdapterFailure> {
    let (normal_config_path, threep_config_path, profile_path, meta_path) = current_paths(home)?;
    let normal_before = common::snapshot(&normal_config_path)
        .map_err(|_| AdapterFailure::ConfigurationFailed("configuration_read_failed"))?;
    let threep_before = common::snapshot(&threep_config_path)
        .map_err(|_| AdapterFailure::ConfigurationFailed("configuration_read_failed"))?;
    let profile_before = common::snapshot(&profile_path)
        .map_err(|_| AdapterFailure::ConfigurationFailed("configuration_read_failed"))?;
    let meta_before = common::snapshot(&meta_path)
        .map_err(|_| AdapterFailure::ConfigurationFailed("configuration_read_failed"))?;
    let local_token = existing_local_token
        .filter(|value| value.starts_with("ycg-") && value.len() == 68)
        .map(str::to_owned)
        .map(Ok)
        .unwrap_or_else(new_local_token)?;
    let mut transaction = FileTransaction::stage_with_snapshot(
        normal_config_path.clone(),
        normal_before.clone(),
        deployment_config(normal_before.as_deref())
            .map_err(|_| AdapterFailure::ConfigurationFailed("configuration_parse_failed"))?,
    )
    .map_err(config_error)?;
    transaction
        .push_with_snapshot(
            threep_config_path.clone(),
            threep_before.clone(),
            deployment_config(threep_before.as_deref())
                .map_err(|_| AdapterFailure::ConfigurationFailed("configuration_parse_failed"))?,
        )
        .map_err(config_error)?;
    transaction
        .push_with_snapshot(
            profile_path.clone(),
            profile_before,
            if modern {
                profile_catalog(model, &local_token, model_ids)
            } else {
                profile_config(model, &local_token)
            }
            .map_err(|_| AdapterFailure::ConfigurationFailed("configuration_parse_failed"))?,
        )
        .map_err(config_error)?;
    transaction
        .push_with_snapshot(
            meta_path.clone(),
            meta_before.clone(),
            meta_config(meta_before.as_deref())
                .map_err(|_| AdapterFailure::ConfigurationFailed("configuration_parse_failed"))?,
        )
        .map_err(config_error)?;
    Ok(Prepared {
        transaction,
        normal_config_path,
        threep_config_path,
        profile_path,
        meta_path,
        model: model.to_owned(),
        model_ids: model_ids.to_vec(),
        modern,
        local_token,
    })
}

impl Prepared {
    pub(crate) fn changes(&self) -> &[common::FileChange] {
        self.transaction.changes()
    }

    pub(crate) fn commit(&mut self) -> Result<(), AdapterFailure> {
        self.transaction.commit().map_err(config_error)?;
        self.validate_readback(true)
    }

    pub(crate) fn validate_existing(&self) -> Result<(), AdapterFailure> {
        self.validate_readback(false)
    }

    fn validate_readback(&self, strict_default: bool) -> Result<(), AdapterFailure> {
        let normal: Value = serde_json::from_slice(
            &common::snapshot(&self.normal_config_path)
                .map_err(|_| AdapterFailure::ConfigurationFailed("configuration_readback_failed"))?
                .ok_or(AdapterFailure::ConfigurationFailed(
                    "configuration_readback_failed",
                ))?,
        )
        .map_err(|_| AdapterFailure::ConfigurationFailed("configuration_readback_failed"))?;
        let threep: Value = serde_json::from_slice(
            &common::snapshot(&self.threep_config_path)
                .map_err(|_| AdapterFailure::ConfigurationFailed("configuration_readback_failed"))?
                .ok_or(AdapterFailure::ConfigurationFailed(
                    "configuration_readback_failed",
                ))?,
        )
        .map_err(|_| AdapterFailure::ConfigurationFailed("configuration_readback_failed"))?;
        let profile: Value = serde_json::from_slice(
            &common::snapshot(&self.profile_path)
                .map_err(|_| AdapterFailure::ConfigurationFailed("configuration_readback_failed"))?
                .ok_or(AdapterFailure::ConfigurationFailed(
                    "configuration_readback_failed",
                ))?,
        )
        .map_err(|_| AdapterFailure::ConfigurationFailed("configuration_readback_failed"))?;
        let meta: Value = serde_json::from_slice(
            &common::snapshot(&self.meta_path)
                .map_err(|_| AdapterFailure::ConfigurationFailed("configuration_readback_failed"))?
                .ok_or(AdapterFailure::ConfigurationFailed(
                    "configuration_readback_failed",
                ))?,
        )
        .map_err(|_| AdapterFailure::ConfigurationFailed("configuration_readback_failed"))?;
        let correct = normal["deploymentMode"].as_str() == Some("3p")
            && threep["deploymentMode"].as_str() == Some("3p")
            && profile["inferenceGatewayBaseUrl"].as_str() == Some(PROXY_BASE)
            && profile["inferenceGatewayApiKey"].as_str() == Some(&self.local_token)
            && (if self.modern {
                let route_ids = self
                    .model_ids
                    .iter()
                    .map(|id| crate::tool_model_profile::claude_gateway_route_id(id))
                    .collect::<Vec<_>>();
                crate::tool_adapters::common::catalog_matches(
                    &profile["inferenceModels"],
                    Some("name"),
                    &route_ids,
                ) && profile["inferenceModels"].as_array().is_some_and(|models| {
                    models.iter().all(|model| {
                        model["name"].as_str().is_some_and(|route| {
                            self.model_ids
                                .iter()
                                .find(|id| {
                                    crate::tool_model_profile::claude_gateway_route_id(id) == route
                                })
                                .is_some_and(|id| {
                                    model["labelOverride"].as_str()
                                        == Some(crate::tool_model_profile::display_name(id))
                                        && (!strict_default
                                            || model["supports1m"].as_bool()
                                                == Some(crate::tool_model_profile::supports_one_m_context(id)))
                                })
                        })
                    })
                }) && crate::tool_adapters::common::default_matches(
                    profile["inferenceModels"][0]["name"].as_str(),
                    &crate::tool_model_profile::claude_gateway_route_id(&self.model),
                    &route_ids,
                    strict_default,
                )
            } else {
                profile["inferenceModels"][0]["name"].as_str() == Some(SAFE_ROUTE_MODEL)
                    && profile["inferenceModels"][0]["labelOverride"].as_str() == Some(&self.model)
                    && (!strict_default
                        || profile["inferenceModels"][0]["supports1m"].as_bool()
                            == Some(crate::tool_model_profile::supports_one_m_context(
                                &self.model,
                            )))
            })
            && meta["appliedId"].as_str() == Some(PROFILE_ID);
        if correct {
            Ok(())
        } else {
            Err(AdapterFailure::ConfigurationFailed(
                "configuration_readback_failed",
            ))
        }
    }

    pub(crate) fn rollback(&mut self) -> Result<(), AdapterFailure> {
        self.transaction.rollback().map_err(config_error)
    }
}

impl ClaudeDesktopRuntimeState {
    pub(crate) async fn start(
        &self,
        credential: ToolCredential,
    ) -> Result<
        tokio::sync::broadcast::Receiver<crate::claude_bridge::VerificationEvent>,
        AdapterFailure,
    > {
        self.runtime.start(credential).await
    }

    pub(crate) async fn stop(&self) {
        self.runtime.stop().await;
    }
}

/// Refresh only the model catalog inside the profile owned by this adapter.
/// This migrates aliases rejected by newer Claude Desktop validators without
/// requiring users to restore and reconnect after updating the assistant.
/// It also refreshes per-model context capabilities from the shared catalog.
fn refreshed_managed_profile(
    profile: &Value,
    credential: &ToolCredential,
    local_token: &str,
) -> Result<Option<Vec<u8>>, AdapterFailure> {
    let active = profile["inferenceGatewayBaseUrl"].as_str() == Some(PROXY_BASE)
        && profile["inferenceGatewayApiKey"].as_str() == Some(local_token)
        && profile["inferenceProvider"].as_str() == Some("gateway");
    if !active {
        return Ok(None);
    }
    let model_ids = credential.model_ids();
    let expected: Value = serde_json::from_slice(
        &profile_catalog(&credential.model_id, local_token, &model_ids)
            .map_err(|_| AdapterFailure::ConfigurationFailed("configuration_parse_failed"))?,
    )
    .map_err(|_| AdapterFailure::ConfigurationFailed("configuration_parse_failed"))?;
    let expected_models =
        expected["inferenceModels"]
            .as_array()
            .ok_or(AdapterFailure::ConfigurationFailed(
                "configuration_parse_failed",
            ))?;
    // Updating capabilities is not reapplying the activation default. Preserve
    // a valid model order and vendor/user picker options, including old aliases.
    // Invalid or incomplete catalogs still use the existing canonical repair.
    let models = profile["inferenceModels"]
        .as_array()
        .filter(|models| models.len() == expected_models.len())
        .and_then(|models| {
            let mut seen = std::collections::HashSet::new();
            models
                .iter()
                .map(|model| {
                    let route = model["name"].as_str()?;
                    let id = model_ids.iter().find(|id| {
                        crate::tool_model_profile::claude_gateway_route_matches(id, route)
                    })?;
                    if !seen.insert(id) {
                        return None;
                    }
                    let canonical_route = crate::tool_model_profile::claude_gateway_route_id(id);
                    let canonical = expected_models
                        .iter()
                        .find(|entry| entry["name"] == canonical_route)?;
                    let mut row = model.as_object()?.clone();
                    for key in ["name", "labelOverride", "supports1m"] {
                        row.insert(key.into(), canonical[key].clone());
                    }
                    Some(Value::Object(row))
                })
                .collect::<Option<Vec<_>>>()
        })
        .map(Value::Array)
        .unwrap_or_else(|| expected["inferenceModels"].clone());
    if profile["inferenceModels"] == models {
        return Ok(None);
    }
    let mut refreshed = profile.clone();
    refreshed["inferenceModels"] = models;
    pretty(&refreshed)
        .map(Some)
        .map_err(|_| AdapterFailure::ConfigurationFailed("configuration_parse_failed"))
}

pub(crate) async fn resume_if_configured(state: ClaudeDesktopRuntimeState) {
    let Ok(credential) = tool_credentials::load("claude_desktop") else {
        return;
    };
    let Some(local_token) = credential.local_gateway_token.as_deref() else {
        return;
    };
    let Some(home) = super::user_home() else {
        return;
    };
    let Ok((_, _, profile_path, _)) = current_paths(&home) else {
        return;
    };
    let Ok(Some(profile_before)) = common::snapshot(&profile_path) else {
        return;
    };
    let profile = serde_json::from_slice::<Value>(&profile_before).ok();
    let active = profile.as_ref().is_some_and(|value| {
        value["inferenceGatewayBaseUrl"].as_str() == Some(PROXY_BASE)
            && value["inferenceGatewayApiKey"].as_str() == Some(local_token)
    });
    if active {
        if let Some(after) = profile
            .as_ref()
            .and_then(|value| refreshed_managed_profile(value, &credential, local_token).ok())
            .flatten()
        {
            match FileTransaction::stage_with_snapshot(profile_path, Some(profile_before), after) {
                Ok(mut transaction) => {
                    if transaction.commit().is_ok() {
                        log::info!("claude_desktop_profile migration=model_catalog_refreshed");
                    } else {
                        log::warn!("claude_desktop_profile migration=commit_failed");
                    }
                }
                Err(_) => log::warn!("claude_desktop_profile migration=stage_failed"),
            }
        }
        let _ = state.start(credential).await;
    }
}

pub(crate) fn launch(path: &Path) -> Result<(), AdapterFailure> {
    super::desktop_launch::launch("claude_desktop", path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tool_credentials::ToolModelRoute;

    fn fable_credential(token: &str) -> ToolCredential {
        let route = ToolModelRoute {
            model_id: "claude-fable-5".into(),
            billing_group: "default".into(),
            api_key: "synthetic-upstream-key".into(),
            origin: "https://yeschoy.com".into(),
            claude_transport: Some("direct_anthropic".into()),
            codex_transport: None,
        };
        ToolCredential {
            api_key: route.api_key.clone(),
            origin: route.origin.clone(),
            model_id: route.model_id.clone(),
            local_gateway_token: Some(token.into()),
            codex_transport: None,
            claude_transport: route.claude_transport.clone(),
            models: vec![route],
        }
    }

    #[test]
    fn context_regression_desktop_catalog_declares_each_real_models_capacity() {
        let ids = vec![
            "claude-fable-5".into(),
            "gpt-6-astra".into(),
            "deepseek-v4-flash".into(),
            "claude-haiku-4-5".into(),
            "future-model".into(),
        ];
        let bytes = profile_catalog("claude-fable-5", "synthetic-token", &ids).unwrap();
        let value: Value = serde_json::from_slice(&bytes).unwrap();
        for (model, expected) in ids.iter().zip([true, true, true, false, false]) {
            let route = crate::tool_model_profile::claude_gateway_route_id(model);
            let entry = value["inferenceModels"]
                .as_array()
                .unwrap()
                .iter()
                .find(|row| row["name"] == route)
                .unwrap();
            assert_eq!(entry["supports1m"], expected, "{model}");
        }
    }

    #[test]
    fn context_regression_single_model_profiles_use_the_same_capability_source() {
        for (id, expected) in [
            ("claude-fable-5", true),
            ("anthropic/claude-sonnet-5", true),
            ("deepseek-v4-flash", true),
            ("gpt-5.4-mini", false),
            ("future-model", false),
        ] {
            let value: Value =
                serde_json::from_slice(&profile_config(id, "synthetic-token").unwrap()).unwrap();
            assert_eq!(value["inferenceModels"][0]["supports1m"], expected, "{id}");
            assert_eq!(value["inferenceModels"][0]["labelOverride"], id);
        }
    }

    #[test]
    fn context_regression_new_readback_checks_context_but_old_connections_stay_readable() {
        // Construct every path ourselves: never consult real desktop config directories.
        for modern in [false, true] {
            let home = common::temporary_working_directory("claude-context-readback").unwrap();
            let normal_config_path = home.join("normal.json");
            let threep_config_path = home.join("threep.json");
            let profile_path = home.join("profile.json");
            let meta_path = home.join("meta.json");
            let model = "claude-fable-5";
            let ids = vec![model.into()];
            let token = "synthetic-token";
            let mut transaction = FileTransaction::stage_with_snapshot(
                normal_config_path.clone(),
                None,
                deployment_config(None).unwrap(),
            )
            .unwrap();
            transaction
                .push(threep_config_path.clone(), deployment_config(None).unwrap())
                .unwrap();
            transaction
                .push(
                    profile_path.clone(),
                    if modern {
                        profile_catalog(model, token, &ids).unwrap()
                    } else {
                        profile_config(model, token).unwrap()
                    },
                )
                .unwrap();
            transaction
                .push(meta_path.clone(), meta_config(None).unwrap())
                .unwrap();
            let mut prepared = Prepared {
                transaction,
                normal_config_path,
                threep_config_path,
                profile_path: profile_path.clone(),
                meta_path,
                model: model.into(),
                model_ids: ids,
                modern,
                local_token: token.into(),
            };
            prepared.commit().unwrap();
            let mut value: Value =
                serde_json::from_slice(&std::fs::read(&profile_path).unwrap()).unwrap();
            assert_eq!(value["inferenceModels"][0]["supports1m"], true);
            value["inferenceModels"][0]["supports1m"] = false.into();
            let old_bytes = pretty(&value).unwrap();
            std::fs::write(&profile_path, &old_bytes).unwrap();
            assert!(prepared.validate_readback(true).is_err());
            assert!(prepared.validate_existing().is_ok());
            assert_eq!(
                std::fs::read(&profile_path).unwrap(),
                old_bytes,
                "inspection must be read-only"
            );
            value["inferenceModels"][0]
                .as_object_mut()
                .unwrap()
                .remove("supports1m");
            std::fs::write(&profile_path, pretty(&value).unwrap()).unwrap();
            assert!(prepared.validate_readback(true).is_err());
            std::fs::remove_dir_all(home).unwrap();
        }
    }

    #[test]
    fn context_regression_migration_is_owned_idempotent_and_fully_reversible() {
        let token = "synthetic-token";
        let mut credential = fable_credential(token);
        let mut small_model = credential.models[0].clone();
        small_model.model_id = "claude-haiku-4-5".into();
        credential.models.push(small_model);
        let mut expected: Value = serde_json::from_slice(
            &profile_catalog(&credential.model_id, token, &credential.model_ids()).unwrap(),
        )
        .unwrap();
        expected["futureOwnedField"] = json!({"keep":true});
        // Claude may put the user's newly selected model first. Refreshing
        // context metadata must not reset it to the original activation default.
        expected["inferenceModels"]
            .as_array_mut()
            .unwrap()
            .swap(0, 1);
        expected["inferenceModels"][0]["futurePickerOption"] = "keep".into();
        let mut old = expected.clone();
        old["inferenceModels"][1]["supports1m"] = false.into();
        let before = pretty(&old).unwrap();
        let after = refreshed_managed_profile(&old, &credential, token)
            .unwrap()
            .unwrap();
        let refreshed: Value = serde_json::from_slice(&after).unwrap();
        assert_eq!(
            refreshed, expected,
            "only the stale context flag should change"
        );
        assert_eq!(refreshed["inferenceModels"][0]["supports1m"], false);
        assert_eq!(refreshed["inferenceModels"][1]["supports1m"], true);
        assert!(refreshed_managed_profile(&refreshed, &credential, token)
            .unwrap()
            .is_none());
        for (field, value) in [
            ("inferenceGatewayApiKey", "other-token"),
            ("inferenceGatewayBaseUrl", "https://other.invalid"),
            ("inferenceProvider", "other-provider"),
        ] {
            let mut unowned = old.clone();
            unowned[field] = value.into();
            assert!(refreshed_managed_profile(&unowned, &credential, token)
                .unwrap()
                .is_none());
        }
        let home = common::temporary_working_directory("claude-context-migration").unwrap();
        let path = home.join("profile.json");
        std::fs::write(&path, &before).unwrap();
        let mut transaction =
            FileTransaction::stage_with_snapshot(path.clone(), Some(before.clone()), after.clone())
                .unwrap();
        transaction.commit().unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), after);
        transaction.rollback().unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), before);
        std::fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn context_regression_invalid_catalog_cannot_preserve_unregistered_rows() {
        let token = "synthetic-token";
        let mut credential = fable_credential(token);
        let mut second = credential.models[0].clone();
        second.model_id = "deepseek-v4-flash".into();
        credential.models.push(second);
        let expected: Value = serde_json::from_slice(
            &profile_catalog(&credential.model_id, token, &credential.model_ids()).unwrap(),
        )
        .unwrap();
        for rows in [
            json!([
                expected["inferenceModels"][0],
                expected["inferenceModels"][0]
            ]),
            json!([expected["inferenceModels"][0], {"name":"not-enrolled", "supports1m":true}]),
            json!([expected["inferenceModels"][0]]),
            json!([null, 17]),
        ] {
            let mut old = expected.clone();
            old["inferenceModels"] = rows;
            let bytes = refreshed_managed_profile(&old, &credential, token)
                .unwrap()
                .unwrap();
            let refreshed: Value = serde_json::from_slice(&bytes).unwrap();
            assert_eq!(refreshed, expected);
        }
    }

    #[test]
    fn ru042_claude_desktop_profile_real_identity_and_default_order_are_verified() {
        let home = common::temporary_working_directory("claude-desktop-model-set").unwrap();
        let normal_config_path = home.join("normal.json");
        let threep_config_path = home.join("threep.json");
        let profile_path = home.join("profile.json");
        let meta_path = home.join("meta.json");
        let ids = vec!["model-b".into(), "model-a".into()];
        let token = "synthetic-local-token";
        let mut transaction = FileTransaction::stage_with_snapshot(
            normal_config_path.clone(),
            None,
            deployment_config(None).unwrap(),
        )
        .unwrap();
        transaction
            .push(threep_config_path.clone(), deployment_config(None).unwrap())
            .unwrap();
        transaction
            .push(
                profile_path.clone(),
                profile_catalog("model-a", token, &ids).unwrap(),
            )
            .unwrap();
        transaction
            .push(meta_path.clone(), meta_config(None).unwrap())
            .unwrap();
        let mut prepared = Prepared {
            transaction,
            normal_config_path,
            threep_config_path,
            profile_path: profile_path.clone(),
            meta_path,
            model: "model-a".into(),
            model_ids: ids,
            modern: true,
            local_token: token.into(),
        };
        prepared.commit().unwrap();
        let mut profile: Value =
            serde_json::from_slice(&std::fs::read(&profile_path).unwrap()).unwrap();
        let first_route = crate::tool_model_profile::claude_gateway_route_id("model-a");
        assert_eq!(profile["inferenceModels"][0]["name"], first_route);
        assert_eq!(profile["inferenceModels"][0]["labelOverride"], "model-a");
        for row in profile["inferenceModels"].as_array().unwrap() {
            let route = row["name"].as_str().unwrap();
            // 按**结构**判定是不是我们铸的路由，而不是钉死某个前缀 ——
            // 铸名形状会随 Claude Desktop 的命名规则变，断言的意图始终是
            // 「这是不透明路由，没泄露真实模型名」。
            assert!(
                crate::tool_model_profile::is_claude_gateway_route(route),
                "{route}"
            );
            assert!(!route.contains("model-"));
            assert_ne!(route, SAFE_ROUTE_MODEL);
        }
        profile["inferenceModels"]
            .as_array_mut()
            .unwrap()
            .swap(0, 1);
        std::fs::write(&profile_path, serde_json::to_vec(&profile).unwrap()).unwrap();
        assert!(prepared.validate_existing().is_ok());
        assert!(prepared.validate_readback(true).is_err());
        profile["inferenceModels"][0]["name"] = "anthropic/claude-router-not-enrolled".into();
        std::fs::write(&profile_path, serde_json::to_vec(&profile).unwrap()).unwrap();
        assert!(prepared.validate_existing().is_err());
        std::fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn ru054_claude_desktop_catalog_uses_safe_routes_with_real_labels() {
        ru042_claude_desktop_profile_real_identity_and_default_order_are_verified();
    }

    #[test]
    fn ru056_claude_desktop_known_models_unlock_native_effort_without_fake_labels() {
        let ids = vec![
            "deepseek-v4-flash".into(),
            "gpt-6-astra".into(),
            "claude-fable-5".into(),
        ];
        let bytes = profile_catalog("deepseek-v4-flash", "synthetic-token", &ids).unwrap();
        let profile: Value = serde_json::from_slice(&bytes).unwrap();
        let deepseek = &profile["inferenceModels"][0];
        let astra = &profile["inferenceModels"][1];
        let fable = &profile["inferenceModels"][2];
        assert!(deepseek["name"]
            .as_str()
            .unwrap()
            .starts_with("claude-sonnet-4-6-v"));
        assert_eq!(deepseek["labelOverride"], "DeepSeek V4 Flash");
        assert!(astra["name"]
            .as_str()
            .unwrap()
            .starts_with("claude-opus-5-v"));
        assert_eq!(astra["labelOverride"], "GPT-6 Astra");
        assert!(fable["name"]
            .as_str()
            .unwrap()
            .starts_with("claude-fable-5-v"));
        assert_eq!(fable["labelOverride"], "Claude Fable 5");
        assert_ne!(deepseek["name"], astra["name"]);
        assert_ne!(astra["name"], fable["name"]);
    }

    #[test]
    fn ru074_startup_migrates_owned_fable_alias_without_rewriting_other_fields() {
        let token = format!("ycg-{}", "a".repeat(64));
        let credential = fable_credential(&token);
        let legacy = json!({
            "coworkEgressAllowedHosts":["*"],
            "futureOwnedField":{"keep":true},
            "inferenceGatewayApiKey":token,
            "inferenceGatewayBaseUrl":PROXY_BASE,
            "inferenceProvider":"gateway",
            "inferenceModels":[{
                "name":crate::tool_model_profile::legacy_claude_gateway_route_id("claude-fable-5"),
                "labelOverride":"claude-fable-5",
                "supports1m":false
            }]
        });
        let bytes = refreshed_managed_profile(&legacy, &credential, &token)
            .unwrap()
            .expect("legacy route should be refreshed");
        let refreshed: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(refreshed["futureOwnedField"], json!({"keep":true}));
        assert!(refreshed["inferenceModels"][0]["name"]
            .as_str()
            .unwrap()
            .starts_with("claude-fable-5-v"));
        assert_eq!(
            refreshed["inferenceModels"][0]["labelOverride"],
            "Claude Fable 5"
        );
        assert!(refreshed_managed_profile(&refreshed, &credential, &token)
            .unwrap()
            .is_none());

        let mut unowned = legacy;
        unowned["inferenceGatewayApiKey"] = "somebody-elses-token".into();
        assert!(refreshed_managed_profile(&unowned, &credential, &token)
            .unwrap()
            .is_none());
    }

    #[test]
    fn profile_uses_local_token_and_safe_route_not_upstream_key() {
        let bytes =
            profile_config("deepseek-v4-flash", &format!("ycg-{}", "a".repeat(64))).unwrap();
        let value: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(value["inferenceGatewayBaseUrl"], PROXY_BASE);
        assert_eq!(value["inferenceModels"][0]["name"], SAFE_ROUTE_MODEL);
        assert_eq!(
            value["inferenceModels"][0]["labelOverride"],
            "deepseek-v4-flash"
        );
        assert!(!String::from_utf8(bytes).unwrap().contains("sk-secret"));
    }

    #[test]
    fn meta_merge_keeps_other_profiles() {
        let before = br#"{"entries":[{"id":"other","name":"Other"}],"future":true}"#;
        let bytes = meta_config(Some(before)).unwrap();
        let value: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(value["future"], true);
        assert!(value["entries"]
            .as_array()
            .unwrap()
            .iter()
            .any(|entry| entry["id"] == "other"));
        assert_eq!(value["appliedId"], PROFILE_ID);
    }
}

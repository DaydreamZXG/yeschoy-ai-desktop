use std::{
    path::{Path, PathBuf},
    time::Duration,
};

use serde_json::{json, Map, Value};
use tokio::time::timeout;

use crate::{
    claude_bridge::ClaudeBridgeRuntime,
    tool_adapters::{
        common::{self, ConfigFailure, FileTransaction},
        AdapterFailure, ResolvedInstallation,
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
    runtime: ClaudeBridgeRuntime,
}

impl Default for ClaudeDesktopRuntimeState {
    fn default() -> Self {
        Self {
            runtime: ClaudeBridgeRuntime::new(PROXY_ADDRESS, "/claude-desktop"),
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
            "supports1m": false
        }]
    }))
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
            profile_config(model, &local_token)
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
        local_token,
    })
}

impl Prepared {
    pub(crate) fn changes(&self) -> &[common::FileChange] {
        self.transaction.changes()
    }

    pub(crate) fn local_token(&self) -> &str {
        &self.local_token
    }

    pub(crate) fn commit(&mut self) -> Result<(), AdapterFailure> {
        self.transaction.commit().map_err(config_error)?;
        self.validate_existing()
    }

    pub(crate) fn validate_existing(&self) -> Result<(), AdapterFailure> {
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
            && profile["inferenceModels"][0]["name"].as_str() == Some(SAFE_ROUTE_MODEL)
            && profile["inferenceModels"][0]["labelOverride"].as_str() == Some(&self.model)
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

pub(crate) async fn verify_and_launch(
    state: &ClaudeDesktopRuntimeState,
    installation: &ResolvedInstallation,
    credential: ToolCredential,
) -> Result<(), AdapterFailure> {
    let expected_model = credential.model_id.clone();
    let mut events = state.start(credential).await?;
    launch(&installation.path)?;
    let observed = timeout(Duration::from_secs(90), async {
        loop {
            match events.recv().await {
                Ok(event) if event.model == expected_model => return Ok(()),
                Ok(_) | Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                Err(_) => return Err(()),
            }
        }
    })
    .await;
    match observed {
        Ok(Ok(())) => Ok(()),
        _ => Err(AdapterFailure::VerificationFailed(
            "waiting_for_desktop_request",
        )),
    }
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
    let profile = common::snapshot(&profile_path)
        .ok()
        .flatten()
        .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok());
    let active = profile.as_ref().is_some_and(|value| {
        value["inferenceGatewayBaseUrl"].as_str() == Some(PROXY_BASE)
            && value["inferenceGatewayApiKey"].as_str() == Some(local_token)
    });
    if active {
        let _ = state.start(credential).await;
    }
}

#[cfg(target_os = "macos")]
pub(crate) fn launch(path: &Path) -> Result<(), AdapterFailure> {
    std::process::Command::new("/usr/bin/open")
        .arg(path)
        .spawn()
        .map(|_| ())
        .map_err(|_| AdapterFailure::LaunchFailed)
}

#[cfg(target_os = "windows")]
pub(crate) fn launch(path: &Path) -> Result<(), AdapterFailure> {
    std::process::Command::new(path)
        .spawn()
        .map(|_| ())
        .map_err(|_| AdapterFailure::LaunchFailed)
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
pub(crate) fn launch(_path: &Path) -> Result<(), AdapterFailure> {
    Err(AdapterFailure::UnsupportedProfile)
}

#[cfg(test)]
mod tests {
    use super::*;

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

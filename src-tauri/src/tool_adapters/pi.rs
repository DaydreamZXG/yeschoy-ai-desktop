use std::{
    path::{Path, PathBuf},
    time::Duration,
};

use serde_json::{json, Map, Value};
use tokio::process::Command;

use crate::{
    tool_adapters::{
        common::{self, ConfigFailure, FileTransaction},
        AdapterFailure, ResolvedInstallation,
    },
    tool_credentials,
};

pub(crate) struct Prepared {
    transaction: FileTransaction,
    models_path: PathBuf,
    settings_path: PathBuf,
    origin: String,
    model: String,
    helper: String,
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

fn parse_document(bytes: Option<&[u8]>) -> Result<Value, ()> {
    match bytes {
        Some(bytes) if !bytes.is_empty() => {
            let source = std::str::from_utf8(bytes).map_err(|_| ())?;
            let value: Value = json5::from_str(source).map_err(|_| ())?;
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

fn render_models(
    existing: Option<&[u8]>,
    origin: &str,
    model: &str,
    helper: &str,
) -> Result<Vec<u8>, ()> {
    let mut root = parse_document(existing)?;
    let root = root.as_object_mut().ok_or(())?;
    let providers = root
        .entry("providers")
        .or_insert_with(|| Value::Object(Map::new()))
        .as_object_mut()
        .ok_or(())?;
    let provider = providers
        .entry("yeschoy")
        .or_insert_with(|| Value::Object(Map::new()))
        .as_object_mut()
        .ok_or(())?;
    provider.insert(
        "baseUrl".into(),
        format!("{}/v1", origin.trim_end_matches('/')).into(),
    );
    provider.insert("api".into(), "openai-completions".into());
    provider.insert("apiKey".into(), format!("!{helper}").into());
    provider.insert("authHeader".into(), true.into());
    provider.insert(
        "models".into(),
        json!([{
            "id": model,
            "name": model,
            "input": ["text"]
        }]),
    );
    pretty(&Value::Object(root.clone()))
}

fn render_settings(existing: Option<&[u8]>, model: &str) -> Result<Vec<u8>, ()> {
    let mut root = parse_document(existing)?;
    let root = root.as_object_mut().ok_or(())?;
    root.insert("defaultProvider".into(), "yeschoy".into());
    root.insert("defaultModel".into(), model.into());
    pretty(&Value::Object(root.clone()))
}

pub(crate) fn prepare(home: &Path, origin: &str, model: &str) -> Result<Prepared, AdapterFailure> {
    if std::env::var_os("PI_CODING_AGENT_DIR").is_some_and(|value| !value.is_empty()) {
        return Err(AdapterFailure::ExternalOverride);
    }
    let directory = home.join(".pi").join("agent");
    let models_path = directory.join("models.json");
    let settings_path = directory.join("settings.json");
    let models_before = common::snapshot(&models_path)
        .map_err(|_| AdapterFailure::ConfigurationFailed("configuration_read_failed"))?;
    let settings_before = common::snapshot(&settings_path)
        .map_err(|_| AdapterFailure::ConfigurationFailed("configuration_read_failed"))?;
    let helper = tool_credentials::shell_helper_command("pi")
        .map_err(|_| AdapterFailure::SecureStorageUnavailable)?;
    let models_after = render_models(models_before.as_deref(), origin, model, &helper)
        .map_err(|_| AdapterFailure::ConfigurationFailed("configuration_parse_failed"))?;
    let settings_after = render_settings(settings_before.as_deref(), model)
        .map_err(|_| AdapterFailure::ConfigurationFailed("configuration_parse_failed"))?;
    let mut transaction =
        FileTransaction::stage_with_snapshot(models_path.clone(), models_before, models_after)
            .map_err(config_error)?;
    transaction
        .push_with_snapshot(settings_path.clone(), settings_before, settings_after)
        .map_err(config_error)?;
    Ok(Prepared {
        transaction,
        models_path,
        settings_path,
        origin: origin.to_owned(),
        model: model.to_owned(),
        helper,
    })
}

impl Prepared {
    pub(crate) fn changes(&self) -> &[common::FileChange] {
        self.transaction.changes()
    }

    pub(crate) fn commit(&mut self) -> Result<(), AdapterFailure> {
        self.transaction.commit().map_err(config_error)?;
        let models = common::snapshot(&self.models_path)
            .map_err(|_| AdapterFailure::ConfigurationFailed("configuration_readback_failed"))?
            .ok_or(AdapterFailure::ConfigurationFailed(
                "configuration_readback_failed",
            ))?;
        let settings = common::snapshot(&self.settings_path)
            .map_err(|_| AdapterFailure::ConfigurationFailed("configuration_readback_failed"))?
            .ok_or(AdapterFailure::ConfigurationFailed(
                "configuration_readback_failed",
            ))?;
        let models: Value = serde_json::from_slice(&models)
            .map_err(|_| AdapterFailure::ConfigurationFailed("configuration_readback_failed"))?;
        let settings: Value = serde_json::from_slice(&settings)
            .map_err(|_| AdapterFailure::ConfigurationFailed("configuration_readback_failed"))?;
        let provider = &models["providers"]["yeschoy"];
        let correct = provider["baseUrl"].as_str()
            == Some(format!("{}/v1", self.origin.trim_end_matches('/')).as_str())
            && provider["api"].as_str() == Some("openai-completions")
            && provider["apiKey"].as_str() == Some(format!("!{}", self.helper).as_str())
            && provider["authHeader"].as_bool() == Some(true)
            && provider["models"][0]["id"].as_str() == Some(&self.model)
            && settings["defaultProvider"].as_str() == Some("yeschoy")
            && settings["defaultModel"].as_str() == Some(&self.model);
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

pub(crate) async fn verify(
    installation: &ResolvedInstallation,
    model: &str,
) -> Result<(), AdapterFailure> {
    let working = common::temporary_working_directory("pi-verify")
        .map_err(|_| AdapterFailure::VerificationFailed("verification_workspace_failed"))?;
    let mut command = Command::new(&installation.path);
    command.current_dir(&working).args([
        "--print",
        "--no-session",
        "--no-tools",
        "--provider",
        "yeschoy",
        "--model",
        model,
        "仅回复 YESCHOY_OK，不要使用工具。",
    ]);
    let result = common::run_bounded(command, Duration::from_secs(120)).await;
    let _ = std::fs::remove_dir_all(&working);
    let result =
        result.map_err(|_| AdapterFailure::VerificationFailed("tool_request_timed_out"))?;
    if result.success && !result.stdout.is_empty() {
        Ok(())
    } else {
        Err(AdapterFailure::VerificationFailed("tool_request_failed"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_merge_keeps_other_providers_and_has_no_literal_key() {
        let before = br#"{
          // existing user provider
          providers: { local: { baseUrl: 'http://127.0.0.1:11434/v1' } },
          future: true,
        }"#;
        let bytes = render_models(
            Some(before),
            "https://yeschoy.com",
            "glm-5.3",
            "'/Applications/野菜 API.app/client' credential-helper pi",
        )
        .unwrap();
        let value: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(value["future"], true);
        assert_eq!(
            value["providers"]["local"]["baseUrl"],
            "http://127.0.0.1:11434/v1"
        );
        assert_eq!(value["providers"]["yeschoy"]["models"][0]["id"], "glm-5.3");
        assert!(!String::from_utf8(bytes).unwrap().contains("sk-secret"));
    }
}

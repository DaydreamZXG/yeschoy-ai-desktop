use std::{
    ffi::OsString,
    path::{Path, PathBuf},
    time::Duration,
};

use serde_yaml::{Mapping, Value};
use tokio::process::Command;

use crate::{
    tool_adapters::{
        common::{self, ConfigFailure, FileTransaction},
        AdapterFailure, ResolvedInstallation,
    },
    tool_credentials,
};

const VERIFICATION_PROMPT: &str = "仅回复 YESCHOY_OK，不要调用工具。";

pub(crate) struct Prepared {
    transaction: FileTransaction,
    path: PathBuf,
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

fn mapping(value: &mut Value) -> Result<&mut Mapping, ()> {
    value.as_mapping_mut().ok_or(())
}

fn child_mapping<'a>(parent: &'a mut Mapping, key: &str) -> Result<&'a mut Mapping, ()> {
    let key = Value::String(key.to_owned());
    if !parent.contains_key(&key) {
        parent.insert(key.clone(), Value::Mapping(Mapping::new()));
    }
    parent
        .get_mut(&key)
        .and_then(Value::as_mapping_mut)
        .ok_or(())
}

fn render(existing: Option<&[u8]>, origin: &str, model: &str, helper: &str) -> Result<Vec<u8>, ()> {
    let mut root = match existing {
        Some(bytes) if !bytes.is_empty() => serde_yaml::from_slice(bytes).map_err(|_| ())?,
        _ => Value::Mapping(Mapping::new()),
    };
    let root = mapping(&mut root)?;
    let providers = child_mapping(root, "providers")?;
    let provider = child_mapping(providers, "yeschoy")?;
    provider.insert(
        Value::String("name".into()),
        Value::String("野菜 API".into()),
    );
    provider.insert(
        Value::String("api".into()),
        Value::String(format!("{}/v1", origin.trim_end_matches('/'))),
    );
    provider.insert(
        Value::String("transport".into()),
        Value::String("chat_completions".into()),
    );
    provider.insert(
        Value::String("key_cmd".into()),
        Value::String(helper.to_owned()),
    );
    provider.insert(
        Value::String("default_model".into()),
        Value::String(model.to_owned()),
    );
    provider.insert(
        Value::String("models".into()),
        Value::Sequence(vec![Value::String(model.to_owned())]),
    );
    provider.insert(Value::String("discover_models".into()), Value::Bool(false));
    for literal_key in ["api_key", "key_env"] {
        provider.remove(Value::String(literal_key.into()));
    }

    let model_config = child_mapping(root, "model")?;
    model_config.insert(
        Value::String("provider".into()),
        Value::String("custom:yeschoy".into()),
    );
    model_config.insert(
        Value::String("default".into()),
        Value::String(model.to_owned()),
    );
    for stale_key in ["api_key", "base_url"] {
        model_config.remove(Value::String(stale_key.into()));
    }

    let mut bytes = serde_yaml::to_string(&Value::Mapping(root.clone()))
        .map_err(|_| ())?
        .into_bytes();
    if !bytes.ends_with(b"\n") {
        bytes.push(b'\n');
    }
    Ok(bytes)
}

fn hermes_home(default_home: &Path, custom: Option<OsString>) -> Result<PathBuf, AdapterFailure> {
    if let Some(custom) = custom.filter(|value| !value.is_empty()) {
        let path = PathBuf::from(custom);
        if path.is_absolute()
            && path.parent().is_some()
            && !path
                .components()
                .any(|part| matches!(part, std::path::Component::ParentDir))
        {
            return Ok(path);
        }
        return Err(AdapterFailure::ExternalOverride);
    }
    #[cfg(target_os = "windows")]
    if let Some(local) = std::env::var_os("LOCALAPPDATA").filter(|value| !value.is_empty()) {
        return Ok(PathBuf::from(local).join("hermes"));
    }
    Ok(default_home.join(".hermes"))
}

pub(crate) fn prepare(home: &Path, origin: &str, model: &str) -> Result<Prepared, AdapterFailure> {
    let path = hermes_home(home, std::env::var_os("HERMES_HOME"))?.join("config.yaml");
    let before = common::snapshot(&path)
        .map_err(|_| AdapterFailure::ConfigurationFailed("configuration_read_failed"))?;
    let helper = tool_credentials::shell_helper_command("hermes")
        .map_err(|_| AdapterFailure::SecureStorageUnavailable)?;
    let after = render(before.as_deref(), origin, model, &helper)
        .map_err(|_| AdapterFailure::ConfigurationFailed("configuration_parse_failed"))?;
    Ok(Prepared {
        transaction: FileTransaction::stage_with_snapshot(path.clone(), before, after)
            .map_err(config_error)?,
        path,
        origin: origin.to_owned(),
        model: model.to_owned(),
        helper,
    })
}

impl Prepared {
    pub(crate) fn commit(&mut self) -> Result<(), AdapterFailure> {
        self.transaction.commit().map_err(config_error)?;
        let bytes = common::snapshot(&self.path)
            .map_err(|_| AdapterFailure::ConfigurationFailed("configuration_readback_failed"))?
            .ok_or(AdapterFailure::ConfigurationFailed(
                "configuration_readback_failed",
            ))?;
        let value: serde_json::Value = serde_yaml::from_slice(&bytes)
            .map_err(|_| AdapterFailure::ConfigurationFailed("configuration_readback_failed"))?;
        let provider = &value["providers"]["yeschoy"];
        let correct = provider["api"].as_str()
            == Some(format!("{}/v1", self.origin.trim_end_matches('/')).as_str())
            && provider["transport"].as_str() == Some("chat_completions")
            && provider["key_cmd"].as_str() == Some(&self.helper)
            && provider["api_key"].is_null()
            && provider["key_env"].is_null()
            && provider["models"][0].as_str() == Some(&self.model)
            && value["model"]["provider"].as_str() == Some("custom:yeschoy")
            && value["model"]["default"].as_str() == Some(&self.model);
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
    let working = common::temporary_working_directory("hermes-verify")
        .map_err(|_| AdapterFailure::VerificationFailed("verification_workspace_failed"))?;
    let mut command = Command::new(&installation.path);
    command.current_dir(&working).args([
        "-z",
        VERIFICATION_PROMPT,
        "--provider",
        "custom:yeschoy",
        "--model",
        model,
    ]);
    let result = common::run_bounded(command, Duration::from_secs(120)).await;
    let _ = std::fs::remove_dir_all(&working);
    let result =
        result.map_err(|_| AdapterFailure::VerificationFailed("tool_request_timed_out"))?;
    if result.success && !String::from_utf8_lossy(&result.stdout).trim().is_empty() {
        Ok(())
    } else {
        Err(AdapterFailure::VerificationFailed("tool_request_failed"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_merge_uses_command_key_and_preserves_unrelated_settings() {
        let before = br#"
display:
  interface: tui
providers:
  local:
    api: http://127.0.0.1:11434/v1
"#;
        let bytes = render(
            Some(before),
            "https://api.yeschoy.com",
            "glm-5.3",
            "'/Applications/野菜 API.app/client' credential-helper hermes",
        )
        .unwrap();
        let value: serde_json::Value = serde_yaml::from_slice(&bytes).unwrap();
        assert_eq!(value["display"]["interface"], "tui");
        assert_eq!(
            value["providers"]["local"]["api"],
            "http://127.0.0.1:11434/v1"
        );
        assert_eq!(value["model"]["provider"], "custom:yeschoy");
        assert_eq!(value["providers"]["yeschoy"]["models"][0], "glm-5.3");
        assert!(!String::from_utf8(bytes).unwrap().contains("sk-secret"));
    }
}

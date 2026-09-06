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
    model_ids: Vec<String>,
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

#[cfg(test)]
fn render(existing: Option<&[u8]>, origin: &str, model: &str, helper: &str) -> Result<Vec<u8>, ()> {
    render_catalog(existing, origin, model, helper, &[model.to_owned()])
}

fn render_catalog(
    existing: Option<&[u8]>,
    origin: &str,
    model: &str,
    helper: &str,
    model_ids: &[String],
) -> Result<Vec<u8>, ()> {
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
        Value::Sequence(model_ids.iter().cloned().map(Value::String).collect()),
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

pub(crate) fn hermes_home(
    default_home: &Path,
    custom: Option<OsString>,
) -> Result<PathBuf, AdapterFailure> {
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
    prepare_inner(home, origin, model, &[model.to_owned()])
}

pub(crate) fn prepare_catalog(
    home: &Path,
    _origin: &str,
    model: &str,
    model_ids: &[String],
) -> Result<Prepared, AdapterFailure> {
    crate::chat_gateway::validate_catalog(model, model_ids)?;
    prepare_inner(
        home,
        &crate::chat_gateway::base_url("hermes").unwrap(),
        model,
        model_ids,
    )
}

fn prepare_inner(
    home: &Path,
    origin: &str,
    model: &str,
    model_ids: &[String],
) -> Result<Prepared, AdapterFailure> {
    let path = hermes_home(home, std::env::var_os("HERMES_HOME"))?.join("config.yaml");
    let before = common::snapshot(&path)
        .map_err(|_| AdapterFailure::ConfigurationFailed("configuration_read_failed"))?;
    let helper = tool_credentials::shell_helper_command("hermes")
        .map_err(|_| AdapterFailure::SecureStorageUnavailable)?;
    let after = render_catalog(before.as_deref(), origin, model, &helper, model_ids)
        .map_err(|_| AdapterFailure::ConfigurationFailed("configuration_parse_failed"))?;
    Ok(Prepared {
        transaction: FileTransaction::stage_with_snapshot(path.clone(), before, after)
            .map_err(config_error)?,
        path,
        origin: origin.to_owned(),
        model: model.to_owned(),
        model_ids: model_ids.to_vec(),
        helper,
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
            && crate::chat_gateway::default_matches(
                provider["default_model"].as_str(),
                &self.model,
                &self.model_ids,
                strict_default,
            )
            && crate::chat_gateway::catalog_matches(&provider["models"], None, &self.model_ids)
            && provider["discover_models"].as_bool() == Some(false)
            && value["model"]["provider"].as_str() == Some("custom:yeschoy")
            && value["model"]["api_key"].is_null()
            && value["model"]["base_url"].is_null()
            && crate::chat_gateway::default_matches(
                value["model"]["default"].as_str(),
                &self.model,
                &self.model_ids,
                strict_default,
            );
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
    let result = result.map_err(|error| AdapterFailure::VerificationFailed(error.reason_code()))?;
    if !result.success {
        return Err(AdapterFailure::VerificationFailed("tool_request_failed"));
    }
    // Official -z scripted mode emits only the final answer on stdout:
    // https://hermes-agent.nousresearch.com/docs/reference/cli-commands
    if common::verification_reply(&result.stdout) {
        Ok(())
    } else {
        Err(AdapterFailure::VerificationFailed("tool_response_invalid"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ru042_hermes_catalog_defaults_change_only_within_registered_set() {
        let home = common::temporary_working_directory("hermes-model-set").unwrap();
        let path = home.join("config.yaml");
        let ids = vec!["model-a".into(), "model-b".into()];
        let origin = crate::chat_gateway::base_url("hermes").unwrap();
        let bytes = render_catalog(None, &origin, "model-a", "synthetic-helper", &ids).unwrap();
        let mut prepared = Prepared {
            transaction: FileTransaction::stage_with_snapshot(path.clone(), None, bytes).unwrap(),
            path: path.clone(),
            origin,
            model: "model-a".into(),
            model_ids: ids,
            helper: "synthetic-helper".into(),
        };
        prepared.commit().unwrap();
        let mut settings: serde_json::Value =
            serde_yaml::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(
            settings["providers"]["yeschoy"]["models"],
            serde_json::json!(["model-a", "model-b"])
        );
        settings["model"]["default"] = "model-b".into();
        settings["providers"]["yeschoy"]["default_model"] = "model-b".into();
        std::fs::write(&path, serde_yaml::to_string(&settings).unwrap()).unwrap();
        assert!(prepared.validate_existing().is_ok());
        assert!(prepared.validate_readback(true).is_err());
        settings["model"]["default"] = "not-enrolled".into();
        std::fs::write(&path, serde_yaml::to_string(&settings).unwrap()).unwrap();
        assert!(prepared.validate_existing().is_err());
        std::fs::remove_dir_all(home).unwrap();
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn hermes_verification_executes_oneshot_fixture_and_rejects_banner_echo_and_failed_reply()
    {
        use common::test_support::Script;
        let valid = Script::new("[ \"$1\" = -z ] || exit 11\n[ \"$3\" = --provider ] || exit 12\n[ \"$4\" = custom:yeschoy ] || exit 13\nprintf 'YESCHOY_OK\\n'");
        assert!(verify(&valid.installation(), "fixture-model").await.is_ok());
        for body in [
            "printf 'Hermes ready\\n'",
            "printf 'Usage: hermes [options]\\n'",
            "printf '%s\\n' \"$*\"",
            "printf '  \\n'",
            "printf 'YESCHOY_OK\\n'; exit 17",
        ] {
            let script = Script::new(body);
            assert!(
                verify(&script.installation(), "fixture-model")
                    .await
                    .is_err(),
                "{body}"
            );
        }
    }

    #[test]
    fn hermes_existing_validation_preserves_config_and_rejects_changed_key_command() {
        let directory = common::temporary_working_directory("hermes-readonly").unwrap();
        let path = directory.join("config.yaml");
        let origin = "https://yeschoy.com";
        let model = "fixture-model";
        let helper = "synthetic-helper credential-helper hermes";
        let bytes = render(Some(b"display:\n  interface: tui\n"), origin, model, helper).unwrap();
        std::fs::write(&path, &bytes).unwrap();
        let prepared = Prepared {
            transaction: FileTransaction::stage_with_snapshot(
                path.clone(),
                Some(bytes.clone()),
                bytes.clone(),
            )
            .unwrap(),
            path: path.clone(),
            origin: origin.into(),
            model: model.into(),
            model_ids: vec![model.into()],
            helper: helper.into(),
        };
        assert!(prepared.validate_existing().is_ok());
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
        let changed = String::from_utf8(bytes)
            .unwrap()
            .replace(helper, "different-helper");
        std::fs::write(&path, &changed).unwrap();
        assert!(prepared.validate_existing().is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), changed);
        std::fs::remove_dir_all(directory).unwrap();
    }

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

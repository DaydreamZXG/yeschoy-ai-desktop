use std::{
    ffi::OsString,
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

const SECRET_PROVIDER: &str = "yeschoy-keychain";
const SECRET_ID: &str = "providers/yeschoy/apiKey";
const VERIFICATION_PROMPT: &str = "仅回复 YESCHOY_OK，不要调用工具。";

pub(crate) struct Prepared {
    transaction: FileTransaction,
    path: PathBuf,
    origin: String,
    model: String,
    helper_executable: String,
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

fn child_object<'a>(
    parent: &'a mut Map<String, Value>,
    key: &str,
) -> Result<&'a mut Map<String, Value>, ()> {
    if !parent.contains_key(key) {
        parent.insert(key.into(), Value::Object(Map::new()));
    }
    parent.get_mut(key).and_then(Value::as_object_mut).ok_or(())
}

fn render(
    existing: Option<&[u8]>,
    origin: &str,
    model: &str,
    helper_executable: &str,
) -> Result<Vec<u8>, ()> {
    let mut value = parse_document(existing)?;
    let root = value.as_object_mut().ok_or(())?;

    let secrets = child_object(root, "secrets")?;
    let secret_providers = child_object(secrets, "providers")?;
    let secret_provider = secret_providers
        .entry(SECRET_PROVIDER)
        .or_insert_with(|| Value::Object(Map::new()))
        .as_object_mut()
        .ok_or(())?;
    secret_provider.insert("source".into(), "exec".into());
    secret_provider.insert("command".into(), helper_executable.into());
    secret_provider.insert(
        "args".into(),
        json!(["credential-helper-openclaw", "openclaw"]),
    );
    secret_provider.insert("jsonOnly".into(), true.into());
    secret_provider.insert("timeoutMs".into(), 5_000.into());
    secret_provider.insert("maxOutputBytes".into(), 4_096.into());

    let models = child_object(root, "models")?;
    if !models.contains_key("mode") {
        models.insert("mode".into(), "merge".into());
    }
    let providers = child_object(models, "providers")?;
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
    provider.insert("authHeader".into(), true.into());
    provider.insert(
        "apiKey".into(),
        json!({"source":"exec","provider":SECRET_PROVIDER,"id":SECRET_ID}),
    );
    provider.insert(
        "models".into(),
        json!([{"id":model,"name":model,"input":["text"]}]),
    );

    let agents = child_object(root, "agents")?;
    let defaults = child_object(agents, "defaults")?;
    let default_model = child_object(defaults, "model")?;
    default_model.insert("primary".into(), format!("yeschoy/{model}").into());
    let catalog = child_object(defaults, "models")?;
    catalog.insert(
        format!("yeschoy/{model}"),
        json!({"alias":format!("{model} · 野菜 API")}),
    );

    let mut bytes = serde_json::to_vec_pretty(&value).map_err(|_| ())?;
    bytes.push(b'\n');
    Ok(bytes)
}

fn safe_override(value: OsString) -> Result<PathBuf, AdapterFailure> {
    let path = PathBuf::from(value);
    if path.is_absolute()
        && path.parent().is_some()
        && !path
            .components()
            .any(|part| matches!(part, std::path::Component::ParentDir))
    {
        Ok(path)
    } else {
        Err(AdapterFailure::ExternalOverride)
    }
}

fn config_path(home: &Path) -> Result<PathBuf, AdapterFailure> {
    if let Some(path) = std::env::var_os("OPENCLAW_CONFIG_PATH").filter(|value| !value.is_empty()) {
        return safe_override(path);
    }
    if let Some(state) = std::env::var_os("OPENCLAW_STATE_DIR").filter(|value| !value.is_empty()) {
        return Ok(safe_override(state)?.join("openclaw.json"));
    }
    if let Some(openclaw_home) = std::env::var_os("OPENCLAW_HOME").filter(|value| !value.is_empty())
    {
        return Ok(safe_override(openclaw_home)?
            .join(".openclaw")
            .join("openclaw.json"));
    }
    Ok(home.join(".openclaw").join("openclaw.json"))
}

pub(crate) fn prepare(home: &Path, origin: &str, model: &str) -> Result<Prepared, AdapterFailure> {
    let path = config_path(home)?;
    let before = common::snapshot(&path)
        .map_err(|_| AdapterFailure::ConfigurationFailed("configuration_read_failed"))?;
    let helper_executable = tool_credentials::executable_path()
        .map_err(|_| AdapterFailure::SecureStorageUnavailable)?;
    let after = render(before.as_deref(), origin, model, &helper_executable)
        .map_err(|_| AdapterFailure::ConfigurationFailed("configuration_parse_failed"))?;
    Ok(Prepared {
        transaction: FileTransaction::stage_with_snapshot(path.clone(), before, after)
            .map_err(config_error)?,
        path,
        origin: origin.to_owned(),
        model: model.to_owned(),
        helper_executable,
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
        let value: Value = serde_json::from_slice(&bytes)
            .map_err(|_| AdapterFailure::ConfigurationFailed("configuration_readback_failed"))?;
        let provider = &value["models"]["providers"]["yeschoy"];
        let secret = &value["secrets"]["providers"][SECRET_PROVIDER];
        let correct = provider["baseUrl"].as_str()
            == Some(format!("{}/v1", self.origin.trim_end_matches('/')).as_str())
            && provider["api"].as_str() == Some("openai-completions")
            && provider["apiKey"]["source"].as_str() == Some("exec")
            && provider["apiKey"]["provider"].as_str() == Some(SECRET_PROVIDER)
            && provider["apiKey"]["id"].as_str() == Some(SECRET_ID)
            && provider["models"][0]["id"].as_str() == Some(&self.model)
            && value["agents"]["defaults"]["model"]["primary"].as_str()
                == Some(format!("yeschoy/{}", self.model).as_str())
            && secret["source"].as_str() == Some("exec")
            && secret["command"].as_str() == Some(&self.helper_executable)
            && secret["args"][0].as_str() == Some("credential-helper-openclaw");
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
    let working = common::temporary_working_directory("openclaw-verify")
        .map_err(|_| AdapterFailure::VerificationFailed("verification_workspace_failed"))?;
    let mut command = Command::new(&installation.path);
    command.current_dir(&working).args([
        "agent",
        "exec",
        VERIFICATION_PROMPT,
        "--cwd",
        working.to_string_lossy().as_ref(),
        "--model",
        format!("yeschoy/{model}").as_str(),
        "--timeout",
        "120",
        "--json",
    ]);
    let result = common::run_bounded(command, Duration::from_secs(150)).await;
    let _ = std::fs::remove_dir_all(&working);
    let result =
        result.map_err(|_| AdapterFailure::VerificationFailed("tool_request_timed_out"))?;
    let valid = serde_json::from_slice::<Value>(&result.stdout)
        .ok()
        .is_some_and(|value| {
            value["ok"].as_bool() == Some(true)
                && value["status"].as_str() == Some("ok")
                && value["final"]
                    .as_str()
                    .is_some_and(|text| !text.trim().is_empty())
        });
    if result.success && valid {
        Ok(())
    } else {
        Err(AdapterFailure::VerificationFailed("tool_request_failed"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merge_preserves_other_settings_and_uses_exec_secret_ref() {
        let before = br#"{
          // existing settings are retained semantically
          channels: { telegram: { enabled: true } },
          models: { mode: 'merge', providers: { local: { baseUrl: 'http://127.0.0.1:11434/v1' } } },
        }"#;
        let bytes = render(
            Some(before),
            "https://yeschoy.com",
            "glm-5.3",
            "/Applications/野菜 API.app/Contents/MacOS/野菜 API",
        )
        .unwrap();
        let value: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(value["channels"]["telegram"]["enabled"], true);
        assert_eq!(
            value["models"]["providers"]["local"]["baseUrl"],
            "http://127.0.0.1:11434/v1"
        );
        assert_eq!(
            value["models"]["providers"]["yeschoy"]["apiKey"]["source"],
            "exec"
        );
        assert!(!String::from_utf8(bytes).unwrap().contains("sk-secret"));
    }
}

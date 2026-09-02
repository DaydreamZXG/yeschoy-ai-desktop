use std::{path::Path, time::Duration};

use serde_json::{Map, Value};
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
    path: std::path::PathBuf,
    origin: String,
    model: String,
    helper: String,
}

fn config_error(error: ConfigFailure) -> AdapterFailure {
    match error {
        ConfigFailure::ExternalChange => AdapterFailure::ExternalOverride,
        ConfigFailure::Parse => AdapterFailure::ConfigurationFailed("configuration_parse_failed"),
        ConfigFailure::Read => AdapterFailure::ConfigurationFailed("configuration_read_failed"),
        ConfigFailure::Write => AdapterFailure::ConfigurationFailed("configuration_write_failed"),
        ConfigFailure::Readback => {
            AdapterFailure::ConfigurationFailed("configuration_readback_failed")
        }
        ConfigFailure::Rollback => {
            AdapterFailure::ConfigurationFailed("configuration_rollback_failed")
        }
    }
}

fn render(existing: Option<&[u8]>, origin: &str, model: &str, helper: &str) -> Result<Vec<u8>, ()> {
    let mut root = match existing {
        Some(bytes) if !bytes.is_empty() => {
            serde_json::from_slice::<Value>(bytes).map_err(|_| ())?
        }
        _ => Value::Object(Map::new()),
    };
    let object = root.as_object_mut().ok_or(())?;
    object.insert("apiKeyHelper".into(), helper.into());
    let environment = object
        .entry("env")
        .or_insert_with(|| Value::Object(Map::new()))
        .as_object_mut()
        .ok_or(())?;
    // These two literal-secret fields were owned by the 0.3 adapter. Removing
    // them is required so Claude Code cannot bypass apiKeyHelper precedence.
    environment.remove("ANTHROPIC_AUTH_TOKEN");
    environment.remove("ANTHROPIC_API_KEY");
    environment.insert("ANTHROPIC_BASE_URL".into(), origin.into());
    for key in [
        "ANTHROPIC_MODEL",
        "ANTHROPIC_DEFAULT_HAIKU_MODEL",
        "ANTHROPIC_DEFAULT_SONNET_MODEL",
        "ANTHROPIC_DEFAULT_OPUS_MODEL",
    ] {
        environment.insert(key.into(), model.into());
    }
    let mut bytes = serde_json::to_vec_pretty(&root).map_err(|_| ())?;
    bytes.push(b'\n');
    Ok(bytes)
}

fn higher_precedence_override() -> bool {
    [
        "ANTHROPIC_AUTH_TOKEN",
        "ANTHROPIC_API_KEY",
        "ANTHROPIC_BASE_URL",
    ]
    .into_iter()
    .any(|key| std::env::var_os(key).is_some_and(|value| !value.is_empty()))
}

pub(crate) fn prepare(home: &Path, origin: &str, model: &str) -> Result<Prepared, AdapterFailure> {
    if higher_precedence_override() {
        return Err(AdapterFailure::ExternalOverride);
    }
    let path = home.join(".claude").join("settings.json");
    let before = common::snapshot(&path)
        .map_err(|_| AdapterFailure::ConfigurationFailed("configuration_read_failed"))?;
    let helper = tool_credentials::shell_helper_command("claude_code")
        .map_err(|_| AdapterFailure::SecureStorageUnavailable)?;
    let after = render(before.as_deref(), origin, model, &helper)
        .map_err(|_| AdapterFailure::ConfigurationFailed("configuration_parse_failed"))?;
    let transaction =
        FileTransaction::stage_with_snapshot(path.clone(), before, after).map_err(config_error)?;
    Ok(Prepared {
        transaction,
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
        let value: Value = serde_json::from_slice(&bytes)
            .map_err(|_| AdapterFailure::ConfigurationFailed("configuration_readback_failed"))?;
        let environment = value.get("env").and_then(Value::as_object);
        let correct = value.get("apiKeyHelper").and_then(Value::as_str) == Some(&self.helper)
            && environment
                .and_then(|env| env.get("ANTHROPIC_BASE_URL"))
                .and_then(Value::as_str)
                == Some(&self.origin)
            && environment
                .and_then(|env| env.get("ANTHROPIC_MODEL"))
                .and_then(Value::as_str)
                == Some(&self.model)
            && environment.is_some_and(|env| {
                !env.contains_key("ANTHROPIC_AUTH_TOKEN") && !env.contains_key("ANTHROPIC_API_KEY")
            });
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
    let settings_path = super::user_home()
        .map(|home| home.join(".claude").join("settings.json"))
        .filter(|path| path.is_file())
        .ok_or(AdapterFailure::VerificationFailed(
            "verification_settings_missing",
        ))?;
    let working = common::temporary_working_directory("claude-code-verify")
        .map_err(|_| AdapterFailure::VerificationFailed("verification_workspace_failed"))?;
    let mut command = Command::new(&installation.path);
    command
        .current_dir(&working)
        .arg("--settings")
        .arg(settings_path)
        .args([
            "--print",
            "--bare",
            "--no-session-persistence",
            "--tools",
            "",
            "--model",
            model,
            "--output-format",
            "json",
            "仅回复 YESCHOY_OK，不要使用工具。",
        ]);
    let result = common::run_bounded(command, Duration::from_secs(120)).await;
    let _ = std::fs::remove_dir_all(&working);
    let result =
        result.map_err(|_| AdapterFailure::VerificationFailed("tool_request_timed_out"))?;
    if !result.success {
        return Err(AdapterFailure::VerificationFailed("tool_request_failed"));
    }
    let value: Value = serde_json::from_slice(&result.stdout)
        .map_err(|_| AdapterFailure::VerificationFailed("tool_response_invalid"))?;
    let has_result = value
        .get("result")
        .and_then(Value::as_str)
        .is_some_and(|value| !value.trim().is_empty());
    if has_result {
        Ok(())
    } else {
        Err(AdapterFailure::VerificationFailed("tool_response_empty"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_uses_helper_and_preserves_unrelated_settings() {
        let before = br#"{"permissions":{"allow":["Read"]},"env":{"KEEP":"yes","ANTHROPIC_AUTH_TOKEN":"sk-old"}}"#;
        let bytes = render(
            Some(before),
            "https://yeschoy.com",
            "glm-5.3",
            "'/Applications/野菜 API.app/client' credential-helper claude_code",
        )
        .unwrap();
        let value: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(value["permissions"]["allow"][0], "Read");
        assert_eq!(value["env"]["KEEP"], "yes");
        assert_eq!(value["env"]["ANTHROPIC_MODEL"], "glm-5.3");
        assert!(value["env"].get("ANTHROPIC_AUTH_TOKEN").is_none());
        assert!(value.to_string().find("sk-old").is_none());
    }
}

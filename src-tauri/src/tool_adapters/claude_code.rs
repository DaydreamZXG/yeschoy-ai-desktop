use std::{path::Path, time::Duration};

use serde_json::{Map, Value};
use tokio::process::Command;

use crate::{
    claude_bridge::{ClaudeBridgeRuntime, ClaudeTransport},
    tool_adapters::{
        common::{self, ConfigFailure, FileTransaction},
        AdapterFailure, ResolvedInstallation,
    },
    tool_credentials,
};

const PROXY_ADDRESS: &str = "127.0.0.1:15728";
const PROXY_BASE: &str = "http://127.0.0.1:15728/claude-code";

#[derive(Clone)]
pub(crate) struct ClaudeCodeRuntimeState {
    runtime: ClaudeBridgeRuntime,
}

impl Default for ClaudeCodeRuntimeState {
    fn default() -> Self {
        Self {
            runtime: ClaudeBridgeRuntime::new(PROXY_ADDRESS, "/claude-code"),
        }
    }
}

pub(crate) struct Prepared {
    transaction: FileTransaction,
    path: std::path::PathBuf,
    origin: String,
    model: String,
    helper: String,
    local_token: Option<String>,
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
    origin: &str,
    model: &str,
    transport: ClaudeTransport,
    existing_local_token: Option<&str>,
) -> Result<Prepared, AdapterFailure> {
    if higher_precedence_override() {
        return Err(AdapterFailure::ExternalOverride);
    }
    let path = home.join(".claude").join("settings.json");
    let before = common::snapshot(&path)
        .map_err(|_| AdapterFailure::ConfigurationFailed("configuration_read_failed"))?;
    let helper = tool_credentials::shell_helper_command("claude_code")
        .map_err(|_| AdapterFailure::SecureStorageUnavailable)?;
    let local_token = if transport == ClaudeTransport::ChatBridge {
        Some(
            existing_local_token
                .filter(|value| value.starts_with("ycg-") && value.len() == 68)
                .map(str::to_owned)
                .map(Ok)
                .unwrap_or_else(new_local_token)?,
        )
    } else {
        None
    };
    let configured_origin = if transport == ClaudeTransport::ChatBridge {
        PROXY_BASE
    } else {
        origin
    };
    let after = render(before.as_deref(), configured_origin, model, &helper)
        .map_err(|_| AdapterFailure::ConfigurationFailed("configuration_parse_failed"))?;
    let transaction =
        FileTransaction::stage_with_snapshot(path.clone(), before, after).map_err(config_error)?;
    Ok(Prepared {
        transaction,
        path,
        origin: configured_origin.to_owned(),
        model: model.to_owned(),
        helper,
        local_token,
    })
}

impl Prepared {
    pub(crate) fn local_token(&self) -> Option<&str> {
        self.local_token.as_deref()
    }

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
    state: &ClaudeCodeRuntimeState,
    installation: &ResolvedInstallation,
    model: &str,
    credential: &tool_credentials::ToolCredential,
) -> Result<(), AdapterFailure> {
    if credential.claude_transport.as_deref() == Some("chat_bridge") {
        state.start(credential.clone()).await?;
    }
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

impl ClaudeCodeRuntimeState {
    pub(crate) async fn start(
        &self,
        credential: tool_credentials::ToolCredential,
    ) -> Result<(), AdapterFailure> {
        self.runtime.start(credential).await.map(|_| ())
    }

    pub(crate) async fn stop(&self) {
        self.runtime.stop().await;
    }
}

pub(crate) async fn resume_if_configured(state: ClaudeCodeRuntimeState) {
    let Ok(credential) = tool_credentials::load("claude_code") else {
        return;
    };
    if credential.claude_transport.as_deref() != Some("chat_bridge") {
        return;
    }
    let Some(home) = super::user_home() else {
        return;
    };
    let settings = common::snapshot(&home.join(".claude").join("settings.json"))
        .ok()
        .flatten()
        .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok());
    let active = settings.as_ref().is_some_and(|value| {
        value
            .pointer("/env/ANTHROPIC_BASE_URL")
            .and_then(Value::as_str)
            == Some(PROXY_BASE)
    });
    if active {
        let _ = state.start(credential).await;
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

    #[test]
    fn chat_transport_uses_loopback_and_local_token() {
        let token = new_local_token().unwrap();
        assert_eq!(PROXY_BASE, "http://127.0.0.1:15728/claude-code");
        assert!(token.starts_with("ycg-") && token.len() == 68);
    }
}

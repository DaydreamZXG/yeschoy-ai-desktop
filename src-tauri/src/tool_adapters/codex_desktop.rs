use std::{
    path::{Path, PathBuf},
    time::Duration,
};

use tokio::process::Command;
use toml_edit::{value, Array, DocumentMut, Item, Table};

use crate::{
    codex_bridge,
    tool_adapters::{
        common::{self, ConfigFailure, FileTransaction},
        AdapterFailure, ResolvedInstallation,
    },
    tool_credentials,
};

const OWNED_CATALOG: &str = "yeschoy-model-catalog.json";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum CodexTransport {
    DirectResponses,
    ChatBridge,
}

impl CodexTransport {
    pub(crate) fn credential_value(self) -> &'static str {
        match self {
            Self::DirectResponses => "direct_responses",
            Self::ChatBridge => "chat_bridge",
        }
    }
}

pub(crate) struct Prepared {
    transaction: FileTransaction,
    path: PathBuf,
    catalog_path: PathBuf,
    catalog: Vec<u8>,
    base_url: String,
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

fn render(
    existing: Option<&[u8]>,
    base_url: &str,
    model: &str,
    helper_executable: &str,
) -> Result<Vec<u8>, AdapterFailure> {
    let source = existing
        .map(std::str::from_utf8)
        .transpose()
        .map_err(|_| AdapterFailure::ConfigurationFailed("configuration_parse_failed"))?
        .unwrap_or("");
    let mut document = if source.trim().is_empty() {
        DocumentMut::new()
    } else {
        source
            .parse::<DocumentMut>()
            .map_err(|_| AdapterFailure::ConfigurationFailed("configuration_parse_failed"))?
    };
    if document
        .get("model_catalog_json")
        .and_then(Item::as_str)
        .is_some_and(|path| path != OWNED_CATALOG)
    {
        return Err(AdapterFailure::ExternalOverride);
    }
    document["model_provider"] = value("yeschoy");
    document["model"] = value(model);
    document["model_catalog_json"] = value(OWNED_CATALOG);

    if document.get("model_providers").is_none() {
        let mut table = Table::new();
        table.set_implicit(true);
        document.insert("model_providers", Item::Table(table));
    }
    let providers = document
        .get_mut("model_providers")
        .and_then(Item::as_table_like_mut)
        .ok_or(AdapterFailure::ConfigurationFailed(
            "configuration_parse_failed",
        ))?;
    if providers.get("yeschoy").is_none() {
        providers.insert("yeschoy", Item::Table(Table::new()));
    }
    let provider = providers
        .get_mut("yeschoy")
        .and_then(Item::as_table_like_mut)
        .ok_or(AdapterFailure::ConfigurationFailed(
            "configuration_parse_failed",
        ))?;
    provider.insert("name", value("野菜API"));
    provider.insert("base_url", value(base_url));
    provider.insert("wire_api", value("responses"));
    // Command-backed auth is mutually exclusive with all literal/env auth
    // fields. Removing these only inside our dedicated provider leaves the
    // user's official OpenAI login and auth.json untouched.
    for key in [
        "requires_openai_auth",
        "env_key",
        "experimental_bearer_token",
    ] {
        provider.remove(key);
    }
    let mut auth = Table::new();
    auth.insert("command", value(helper_executable));
    let mut args = Array::new();
    args.push("credential-helper");
    args.push("codex_desktop");
    auth.insert("args", value(args));
    auth.insert("timeout_ms", value(5_000));
    auth.insert("refresh_interval_ms", value(0));
    provider.insert("auth", Item::Table(auth));
    Ok(document.to_string().into_bytes())
}

fn catalog(model: &str, transport: CodexTransport) -> Result<Vec<u8>, AdapterFailure> {
    let template = match transport {
        CodexTransport::DirectResponses => {
            include_str!("../resources/codex_native_responses_template.json")
        }
        CodexTransport::ChatBridge => include_str!("../resources/gpt5_5_template.json"),
    };
    let mut entry: serde_json::Value = serde_json::from_str(template)
        .map_err(|_| AdapterFailure::ConfigurationFailed("configuration_parse_failed"))?;
    let object = entry
        .as_object_mut()
        .ok_or(AdapterFailure::ConfigurationFailed(
            "configuration_parse_failed",
        ))?;
    object.insert("slug".into(), serde_json::json!(model));
    object.insert("display_name".into(), serde_json::json!(model));
    object.insert("description".into(), serde_json::json!(model));
    object.insert("priority".into(), serde_json::json!(0));
    object.insert("context_window".into(), serde_json::json!(128_000));
    object.insert("max_context_window".into(), serde_json::json!(128_000));
    object.insert("additional_speed_tiers".into(), serde_json::json!([]));
    object.insert("service_tiers".into(), serde_json::json!([]));
    object.remove("availability_nux");
    object.remove("upgrade");
    serde_json::to_vec_pretty(&serde_json::json!({"models": [entry]}))
        .map_err(|_| AdapterFailure::ConfigurationFailed("configuration_write_failed"))
}

pub(crate) fn prepare(
    home: &Path,
    origin: &str,
    model: &str,
    transport: CodexTransport,
) -> Result<Prepared, AdapterFailure> {
    if std::env::var_os("CODEX_HOME").is_some_and(|value| !value.is_empty()) {
        return Err(AdapterFailure::ExternalOverride);
    }
    let path = home.join(".codex").join("config.toml");
    let catalog_path = home.join(".codex").join(OWNED_CATALOG);
    let before = common::snapshot(&path)
        .map_err(|_| AdapterFailure::ConfigurationFailed("configuration_read_failed"))?;
    let helper_executable = tool_credentials::executable_path()
        .map_err(|_| AdapterFailure::SecureStorageUnavailable)?;
    let base_url = match transport {
        CodexTransport::DirectResponses => format!("{}/v1", origin.trim_end_matches('/')),
        CodexTransport::ChatBridge => codex_bridge::BASE_URL.to_owned(),
    };
    let after = render(before.as_deref(), &base_url, model, &helper_executable)?;
    let catalog = catalog(model, transport)?;
    let mut transaction =
        FileTransaction::stage_with_snapshot(path.clone(), before, after).map_err(config_error)?;
    transaction
        .push(catalog_path.clone(), catalog.clone())
        .map_err(config_error)?;
    Ok(Prepared {
        transaction,
        path,
        catalog_path,
        catalog,
        base_url,
        model: model.to_owned(),
        helper_executable,
    })
}

impl Prepared {
    pub(crate) fn commit(&mut self) -> Result<(), AdapterFailure> {
        self.transaction.commit().map_err(config_error)?;
        let source = common::snapshot(&self.path)
            .map_err(|_| AdapterFailure::ConfigurationFailed("configuration_readback_failed"))?
            .and_then(|bytes| String::from_utf8(bytes).ok())
            .ok_or(AdapterFailure::ConfigurationFailed(
                "configuration_readback_failed",
            ))?;
        let document = source
            .parse::<DocumentMut>()
            .map_err(|_| AdapterFailure::ConfigurationFailed("configuration_readback_failed"))?;
        let provider = &document["model_providers"]["yeschoy"];
        let catalog = common::snapshot(&self.catalog_path)
            .map_err(|_| AdapterFailure::ConfigurationFailed("configuration_readback_failed"))?;
        let correct = document["model_provider"].as_str() == Some("yeschoy")
            && document["model"].as_str() == Some(&self.model)
            && document["model_catalog_json"].as_str() == Some(OWNED_CATALOG)
            && provider["base_url"].as_str() == Some(self.base_url.as_str())
            && provider["wire_api"].as_str() == Some("responses")
            && provider["auth"]["command"].as_str() == Some(&self.helper_executable)
            && provider.get("requires_openai_auth").is_none()
            && provider.get("env_key").is_none()
            && provider.get("experimental_bearer_token").is_none()
            && catalog.as_deref() == Some(self.catalog.as_slice())
            && !self.catalog.windows(3).any(|window| window == b"sk-");
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

pub(crate) fn bundled_runtime(app_path: &Path) -> Option<PathBuf> {
    #[cfg(target_os = "macos")]
    {
        let path = app_path.join("Contents").join("Resources").join("codex");
        path.is_file().then_some(path)
    }
    #[cfg(target_os = "windows")]
    {
        let parent = app_path.parent()?;
        [
            parent.join("resources").join("codex.exe"),
            parent.join("Resources").join("codex.exe"),
            parent.join("codex.exe"),
        ]
        .into_iter()
        .find(|path| path.is_file())
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        let _ = app_path;
        None
    }
}

pub(crate) async fn verify_and_launch(
    installation: &ResolvedInstallation,
) -> Result<(), AdapterFailure> {
    let runtime = bundled_runtime(&installation.path).ok_or(AdapterFailure::LaunchFailed)?;
    let working = common::temporary_working_directory("codex-desktop-verify")
        .map_err(|_| AdapterFailure::VerificationFailed("verification_workspace_failed"))?;
    let mut command = Command::new(runtime);
    command.current_dir(&working).args([
        "--ask-for-approval",
        "never",
        "exec",
        "--ephemeral",
        "--skip-git-repo-check",
        "--json",
        "--color",
        "never",
        "--sandbox",
        "read-only",
        "仅回复 YESCHOY_OK，不要使用工具。",
    ]);
    let result = common::run_bounded(command, Duration::from_secs(120)).await;
    let _ = std::fs::remove_dir_all(&working);
    let result =
        result.map_err(|_| AdapterFailure::VerificationFailed("tool_request_timed_out"))?;
    if !result.success || !completed_response(&result.stdout) {
        return Err(AdapterFailure::VerificationFailed("tool_request_failed"));
    }
    launch(&installation.path)
}

fn completed_response(output: &[u8]) -> bool {
    let events: Vec<serde_json::Value> = output
        .split(|byte| *byte == b'\n')
        .filter_map(|line| serde_json::from_slice(line).ok())
        .collect();
    let failed = events
        .iter()
        .any(|e| matches!(e["type"].as_str(), Some("error" | "turn.failed")));
    let replied = events.iter().any(|e| {
        e["type"] == "item.completed"
            && e["item"]["type"] == "agent_message"
            && e["item"]["text"]
                .as_str()
                .is_some_and(|s| !s.trim().is_empty())
    });
    !failed && replied && events.iter().any(|e| e["type"] == "turn.completed")
}

#[cfg(target_os = "macos")]
fn launch(path: &Path) -> Result<(), AdapterFailure> {
    std::process::Command::new("/usr/bin/open")
        .arg(path)
        .spawn()
        .map(|_| ())
        .map_err(|_| AdapterFailure::LaunchFailed)
}

#[cfg(target_os = "windows")]
fn launch(path: &Path) -> Result<(), AdapterFailure> {
    std::process::Command::new(path)
        .spawn()
        .map(|_| ())
        .map_err(|_| AdapterFailure::LaunchFailed)
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn launch(_path: &Path) -> Result<(), AdapterFailure> {
    Err(AdapterFailure::UnsupportedProfile)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn startup_events_and_failed_turns_are_not_success() {
        assert!(!completed_response(
            br#"{"type":"thread.started"}
{"type":"turn.started"}"#
        ));
        let reply = b"{\"type\":\"item.completed\",\"item\":{\"type\":\"agent_message\",\"text\":\"YESCHOY_OK\"}}\n{\"type\":\"turn.completed\"}";
        assert!(completed_response(reply));
        let mut failed = reply.to_vec();
        failed.extend_from_slice(b"\n{\"type\":\"turn.failed\"}");
        assert!(!completed_response(&failed));
    }

    #[test]
    fn render_preserves_official_auth_and_uses_command_auth() {
        let source =
            b"model_reasoning_effort = \"high\"\n[notice]\nhide_full_access_warning = true\n";
        let bytes = render(
            Some(source),
            "https://api.yeschoy.com/v1",
            "glm-5.3",
            "/Applications/野菜 API.app/Contents/MacOS/野菜 API",
        )
        .unwrap();
        let text = String::from_utf8(bytes).unwrap();
        let document = text.parse::<DocumentMut>().unwrap();
        assert_eq!(document["model_reasoning_effort"].as_str(), Some("high"));
        assert_eq!(
            document["model_providers"]["yeschoy"]["wire_api"].as_str(),
            Some("responses")
        );
        assert_eq!(
            document["model_providers"]["yeschoy"]["auth"]["args"][1].as_str(),
            Some("codex_desktop")
        );
        assert!(!text.contains("sk-secret"));
    }

    #[test]
    fn catalog_is_exact_and_profile_specific() {
        let direct =
            String::from_utf8(catalog("gpt-direct", CodexTransport::DirectResponses).unwrap())
                .unwrap();
        let bridged =
            String::from_utf8(catalog("gpt-chat", CodexTransport::ChatBridge).unwrap()).unwrap();
        assert!(direct.contains("gpt-direct"));
        assert!(!direct.contains("apply_patch_tool_type"));
        assert!(bridged.contains("gpt-chat"));
        assert!(bridged.contains("apply_patch_tool_type"));
    }

    #[test]
    fn user_owned_catalog_is_never_replaced() {
        let result = render(
            Some(b"model_catalog_json = \"my-models.json\"\n"),
            "https://yeschoy.com/v1",
            "gpt-5.5",
            "/tmp/helper",
        );
        assert!(matches!(result, Err(AdapterFailure::ExternalOverride)));
    }

    #[test]
    fn configuration_and_owned_catalog_commit_and_rollback_together() {
        let home = common::temporary_working_directory("codex-catalog-transaction").unwrap();
        let config_path = home.join(".codex").join("config.toml");
        let original = b"model_reasoning_effort = \"high\"\n";
        common::atomic_write(&config_path, original).unwrap();

        let mut prepared = prepare(
            &home,
            "https://yeschoy.com",
            "gpt-5.5",
            CodexTransport::ChatBridge,
        )
        .unwrap();
        prepared.commit().unwrap();
        assert_eq!(
            std::fs::read_to_string(&prepared.catalog_path)
                .unwrap()
                .matches("gpt-5.5")
                .count(),
            3
        );
        prepared.rollback().unwrap();
        assert_eq!(std::fs::read(&config_path).unwrap(), original);
        assert!(!prepared.catalog_path.exists());
        let _ = std::fs::remove_dir_all(home);
    }
}

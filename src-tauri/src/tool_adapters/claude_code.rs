use std::{path::Path, time::Duration};

use serde_json::{json, Map, Value};
use tokio::process::Command;

use crate::{
    claude_bridge::ClaudeTransport,
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
    model_ids: Vec<String>,
    modern: bool,
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

fn render_catalog(
    existing: Option<&[u8]>,
    origin: &str,
    model: &str,
    helper: &str,
    model_ids: &[String],
) -> Result<Vec<u8>, ()> {
    let mut value: Value =
        serde_json::from_slice(&render(existing, origin, model, helper)?).map_err(|_| ())?;
    let env = value["env"].as_object_mut().ok_or(())?;
    // Persist the actual default in `model`, which native /model may update.
    // Never label unrelated upstream models as Claude's built-in families.
    for key in [
        "ANTHROPIC_MODEL",
        "ANTHROPIC_DEFAULT_HAIKU_MODEL",
        "ANTHROPIC_DEFAULT_SONNET_MODEL",
        "ANTHROPIC_DEFAULT_OPUS_MODEL",
    ] {
        env.remove(key);
    }
    value["model"] = json!(model);
    value["availableModels"] = json!(model_ids);
    // `behavesAs` controls Claude Code's client-side prompt/capability handling;
    // the exact model ID is still sent through the bridge. Known models receive
    // the closest native effort family, while unregistered IDs use a conservative
    // compatibility fallback so they remain selectable.
    value["modelPicker"] = json!({"replaceBuiltInOptions":true,"options":model_ids.iter().map(|id| json!({
        "model":id,
        "label":crate::tool_model_profile::display_name(id),
        "description":id,
        "behavesAs":crate::tool_model_profile::claude_code_behaves_as(id)
    })).collect::<Vec<_>>()});
    let mut bytes = serde_json::to_vec_pretty(&value).map_err(|_| ())?;
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

pub(crate) fn prepare(
    home: &Path,
    origin: &str,
    model: &str,
    transport: ClaudeTransport,
    existing_local_token: Option<&str>,
) -> Result<Prepared, AdapterFailure> {
    prepare_inner(
        home,
        origin,
        model,
        transport,
        existing_local_token,
        &[model.to_owned()],
        false,
    )
}

pub(crate) fn prepare_catalog(
    home: &Path,
    origin: &str,
    model: &str,
    transport: ClaudeTransport,
    existing_local_token: Option<&str>,
    model_ids: &[String],
) -> Result<Prepared, AdapterFailure> {
    crate::tool_adapters::common::validate_catalog(model, model_ids)?;
    prepare_inner(
        home,
        origin,
        model,
        transport,
        existing_local_token,
        model_ids,
        true,
    )
}

fn prepare_inner(
    home: &Path,
    origin: &str,
    model: &str,
    _transport: ClaudeTransport,
    _existing_local_token: Option<&str>,
    model_ids: &[String],
    modern: bool,
) -> Result<Prepared, AdapterFailure> {
    if higher_precedence_override() {
        return Err(AdapterFailure::ExternalOverride);
    }
    let path = home.join(".claude").join("settings.json");
    let before = common::snapshot(&path)
        .map_err(|_| AdapterFailure::ConfigurationFailed("configuration_read_failed"))?;
    let helper = tool_credentials::shell_helper_command("claude_code")
        .map_err(|_| AdapterFailure::SecureStorageUnavailable)?;
    // The canonical credential owner generates/stores the local token. Claude
    // Code reads it through apiKeyHelper, so no token belongs in Prepared or
    // the settings file. Keep the legacy argument shape for callers.
    // Claude Code reads the relay origin straight from its own settings file.
    // The former loopback proxy added a listener, a capability token and a
    // watchdog without changing the protocol.
    let configured_origin = origin;
    let after = if modern {
        render_catalog(
            before.as_deref(),
            configured_origin,
            model,
            &helper,
            model_ids,
        )
    } else {
        render(before.as_deref(), configured_origin, model, &helper)
    }
    .map_err(|_| AdapterFailure::ConfigurationFailed("configuration_parse_failed"))?;
    let transaction =
        FileTransaction::stage_with_snapshot(path.clone(), before, after).map_err(config_error)?;
    Ok(Prepared {
        transaction,
        path,
        origin: configured_origin.to_owned(),
        model: model.to_owned(),
        model_ids: model_ids.to_vec(),
        modern,
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
        let value: Value = serde_json::from_slice(&bytes)
            .map_err(|_| AdapterFailure::ConfigurationFailed("configuration_readback_failed"))?;
        let environment = value.get("env").and_then(Value::as_object);
        let correct = value.get("apiKeyHelper").and_then(Value::as_str) == Some(&self.helper)
            && environment
                .and_then(|env| env.get("ANTHROPIC_BASE_URL"))
                .and_then(Value::as_str)
                == Some(&self.origin)
            && environment.is_some_and(|env| {
                !env.contains_key("ANTHROPIC_AUTH_TOKEN")
                    && !env.contains_key("ANTHROPIC_API_KEY")
                    && (self.modern
                        || [
                            "ANTHROPIC_MODEL",
                            "ANTHROPIC_DEFAULT_HAIKU_MODEL",
                            "ANTHROPIC_DEFAULT_SONNET_MODEL",
                            "ANTHROPIC_DEFAULT_OPUS_MODEL",
                        ]
                        .into_iter()
                        .all(|key| env.get(key).and_then(Value::as_str) == Some(&self.model)))
            })
            && (!self.modern
                || (crate::tool_adapters::common::default_matches(
                    value["model"].as_str(),
                    &self.model,
                    &self.model_ids,
                    strict_default,
                ) && crate::tool_adapters::common::catalog_matches(
                    &value["availableModels"],
                    None,
                    &self.model_ids,
                ) && crate::tool_adapters::common::catalog_matches(
                    &value["modelPicker"]["options"],
                    Some("model"),
                    &self.model_ids,
                ) && value["modelPicker"]["options"]
                    .as_array()
                    .is_some_and(|models| {
                        models.iter().all(|model| {
                            model["model"].as_str().is_some_and(|id| {
                                model["label"].as_str()
                                    == Some(crate::tool_model_profile::display_name(id))
                                    && model["description"].as_str() == Some(id)
                                    && model["behavesAs"].as_str()
                                        == Some(crate::tool_model_profile::claude_code_behaves_as(
                                            id,
                                        ))
                            })
                        })
                    })
                    && value["modelPicker"]["replaceBuiltInOptions"].as_bool() == Some(true)
                    && environment.is_some_and(|env| {
                        [
                            "ANTHROPIC_MODEL",
                            "ANTHROPIC_DEFAULT_HAIKU_MODEL",
                            "ANTHROPIC_DEFAULT_SONNET_MODEL",
                            "ANTHROPIC_DEFAULT_OPUS_MODEL",
                        ]
                        .iter()
                        .all(|key| !env.contains_key(*key))
                    })));
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
    _credential: &tool_credentials::ToolCredential,
) -> Result<(), AdapterFailure> {
    let settings_path = super::user_home()
        .map(|home| home.join(".claude").join("settings.json"))
        .filter(|path| path.is_file())
        .ok_or(AdapterFailure::VerificationFailed(
            "verification_settings_missing",
        ))?;
    verify_with_settings(installation, model, &settings_path).await
}

async fn verify_with_settings(
    installation: &ResolvedInstallation,
    model: &str,
    settings_path: &Path,
) -> Result<(), AdapterFailure> {
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
    common::apply_cli_runtime_path(&mut command, &installation.path);
    let result = common::run_bounded(command, Duration::from_secs(120)).await;
    let _ = std::fs::remove_dir_all(&working);
    let result = result.map_err(|error| AdapterFailure::VerificationFailed(error.reason_code()))?;
    if !result.success {
        return Err(AdapterFailure::VerificationFailed("tool_request_failed"));
    }
    let value: Value = serde_json::from_slice(&result.stdout)
        .map_err(|_| AdapterFailure::VerificationFailed("tool_response_invalid"))?;
    let has_result = value
        .get("result")
        .and_then(Value::as_str)
        .is_some_and(|value| common::verification_reply(value.as_bytes()))
        && value.get("is_error").and_then(Value::as_bool) != Some(true);
    if has_result {
        Ok(())
    } else {
        Err(AdapterFailure::VerificationFailed("tool_response_invalid"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ru042_claude_code_catalog_uses_real_picker_ids_and_member_defaults() {
        let home = common::temporary_working_directory("claude-code-model-set").unwrap();
        let ids = vec!["model-a".into(), "org/model-b".into()];
        let token = format!("ycg-{}", "a".repeat(64));
        let mut prepared = prepare_catalog(
            &home,
            "https://yeschoy.com",
            "model-a",
            ClaudeTransport::DirectAnthropic,
            Some(&token),
            &ids,
        )
        .unwrap();
        prepared.commit().unwrap();
        assert!(!std::fs::read_to_string(&prepared.path)
            .unwrap()
            .contains(&token));
        let mut settings: Value =
            serde_json::from_slice(&std::fs::read(&prepared.path).unwrap()).unwrap();
        assert_eq!(
            settings["env"]["ANTHROPIC_BASE_URL"],
            "https://yeschoy.com"
        );
        assert!(settings["env"]
            .get("ANTHROPIC_DEFAULT_SONNET_MODEL")
            .is_none());
        assert_eq!(
            settings["modelPicker"]["options"][1],
            json!({
                "model":"org/model-b",
                "label":"org/model-b",
                "description":"org/model-b",
                "behavesAs":crate::tool_model_profile::CLAUDE_BEHAVES_AS
            })
        );
        settings["model"] = "org/model-b".into();
        std::fs::write(&prepared.path, serde_json::to_vec(&settings).unwrap()).unwrap();
        assert!(prepared.validate_existing().is_ok());
        assert!(prepared.validate_readback(true).is_err());
        settings["modelPicker"]["options"][1]
            .as_object_mut()
            .unwrap()
            .remove("behavesAs");
        std::fs::write(&prepared.path, serde_json::to_vec(&settings).unwrap()).unwrap();
        assert!(prepared.validate_existing().is_err());
        settings["modelPicker"]["options"][1]["behavesAs"] =
            crate::tool_model_profile::CLAUDE_BEHAVES_AS.into();
        settings["model"] = "not-enrolled".into();
        std::fs::write(&prepared.path, serde_json::to_vec(&settings).unwrap()).unwrap();
        assert!(prepared.validate_existing().is_err());
        std::fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn ru054_claude_code_unknown_models_declare_behavior_without_changing_ids() {
        ru042_claude_code_catalog_uses_real_picker_ids_and_member_defaults();
    }

    #[test]
    fn ru056_claude_code_catalog_projects_each_models_native_effort_family() {
        let bytes = render_catalog(
            None,
            "https://yeschoy.com",
            "deepseek-v4-flash",
            "synthetic-helper",
            &[
                "deepseek-v4-flash".into(),
                "gpt-6-astra".into(),
                "gpt-5.6-sol".into(),
                "future-model".into(),
            ],
        )
        .unwrap();
        let settings: Value = serde_json::from_slice(&bytes).unwrap();
        let options = settings["modelPicker"]["options"].as_array().unwrap();
        let behaviors = options
            .iter()
            .map(|option| {
                (
                    option["model"].as_str().unwrap(),
                    option["behavesAs"].as_str().unwrap(),
                )
            })
            .collect::<std::collections::HashMap<_, _>>();
        assert_eq!(behaviors["deepseek-v4-flash"], "claude-sonnet-4-6");
        assert_eq!(behaviors["gpt-6-astra"], "claude-opus-5");
        assert_eq!(behaviors["gpt-5.6-sol"], "claude-sonnet-5");
        assert_eq!(
            behaviors["future-model"],
            crate::tool_model_profile::CLAUDE_BEHAVES_AS
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn claude_verification_executes_json_fixture_and_rejects_echo_error_and_failed_reply() {
        use common::test_support::Script;
        let valid = Script::new("[ \"$1\" = --settings ] || exit 11\n[ \"$3\" = --print ] || exit 12\nprintf '%s\\n' '{\"result\":\"YESCHOY_OK\",\"is_error\":false}'");
        // The fixture never opens this path; no real HOME/config/credential lookup.
        let settings = valid.path.with_file_name("synthetic-settings.json");
        assert!(
            verify_with_settings(&valid.installation(), "fixture-model", &settings)
                .await
                .is_ok()
        );
        for body in [
            "printf 'Usage: claude [options]\\n'",
            "printf '%s\\n' \"$*\"",
            "printf '%s\\n' '{\"result\":\"reply YESCHOY_OK\",\"is_error\":false}'",
            "printf '%s\\n' '{\"result\":\"YESCHOY_OK\",\"is_error\":true}'",
            "printf '%s\\n' '{\"result\":\"YESCHOY_OK\",\"is_error\":false}'; exit 17",
        ] {
            let script = Script::new(body);
            assert!(
                verify_with_settings(&script.installation(), "fixture-model", &settings)
                    .await
                    .is_err(),
                "{body}"
            );
        }
    }

    #[test]
    fn claude_existing_validation_is_read_only_and_checks_all_owned_model_fields() {
        let directory = common::temporary_working_directory("claude-readonly").unwrap();
        let path = directory.join("settings.json");
        let origin = "https://yeschoy.com";
        let model = "fixture-model";
        let helper = "synthetic-helper credential-helper claude_code";
        let bytes = render(
            Some(br#"{"permissions":{"allow":["Read"]}}"#),
            origin,
            model,
            helper,
        )
        .unwrap();
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
            modern: false,
            helper: helper.into(),
        };
        assert!(prepared.validate_existing().is_ok());
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
        for key in [
            "ANTHROPIC_MODEL",
            "ANTHROPIC_DEFAULT_HAIKU_MODEL",
            "ANTHROPIC_DEFAULT_SONNET_MODEL",
            "ANTHROPIC_DEFAULT_OPUS_MODEL",
            "ANTHROPIC_API_KEY",
        ] {
            let mut value: Value = serde_json::from_slice(&bytes).unwrap();
            value["env"][key] = "external-value".into();
            let changed = serde_json::to_vec(&value).unwrap();
            std::fs::write(&path, &changed).unwrap();
            assert!(prepared.validate_existing().is_err(), "{key}");
            assert_eq!(std::fs::read(&path).unwrap(), changed);
        }
        std::fs::remove_dir_all(directory).unwrap();
    }

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
    fn direct_transport_writes_the_relay_origin_and_a_helper_command() {
        let settings: Value = serde_json::from_slice(
            &render(
                None,
                "https://yeschoy.com",
                "fixture-model",
                "synthetic-helper credential-helper claude_code",
            )
            .unwrap(),
        )
        .unwrap();
        assert_eq!(
            settings["env"]["ANTHROPIC_BASE_URL"],
            "https://yeschoy.com"
        );
        assert_eq!(
            settings["apiKeyHelper"],
            "synthetic-helper credential-helper claude_code"
        );
        assert!(settings["env"].get("ANTHROPIC_API_KEY").is_none());
    }
}

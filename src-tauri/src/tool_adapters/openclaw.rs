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
    model_ids: Vec<String>,
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

#[cfg(test)]
fn render(
    existing: Option<&[u8]>,
    origin: &str,
    model: &str,
    helper_executable: &str,
) -> Result<Vec<u8>, ()> {
    render_catalog(
        existing,
        origin,
        model,
        helper_executable,
        &[model.to_owned()],
    )
}

fn render_catalog(
    existing: Option<&[u8]>,
    origin: &str,
    model: &str,
    helper_executable: &str,
    model_ids: &[String],
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
    let models = crate::tool_model_profile::native_chat_catalog(
        provider.get("models"),
        model_ids,
        crate::tool_model_profile::ModelConsumer::OpenClaw,
    );
    provider.insert("models".into(), models);

    let agents = child_object(root, "agents")?;
    let defaults = child_object(agents, "defaults")?;
    let default_model = child_object(defaults, "model")?;
    default_model.insert("primary".into(), format!("yeschoy/{model}").into());
    let catalog = child_object(defaults, "models")?;
    // These entries belong to this provider. Remove retired entries without
    // touching models or aliases belonging to other providers.
    catalog.retain(|key, _| {
        key.strip_prefix("yeschoy/")
            .is_none_or(|id| model_ids.iter().any(|model| model == id))
    });
    for id in model_ids {
        let entry = catalog
            .entry(format!("yeschoy/{id}"))
            .or_insert_with(|| json!({}))
            .as_object_mut()
            .ok_or(())?;
        if !entry.contains_key("alias") || entry["alias"] == format!("{id} · 野菜 API") {
            entry.insert(
                "alias".into(),
                json!(format!(
                    "{} · 野菜 API",
                    crate::tool_model_profile::display_name(id)
                )),
            );
        }
    }

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

pub(crate) fn config_path(home: &Path) -> Result<PathBuf, AdapterFailure> {
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
        &crate::chat_gateway::base_url("openclaw").unwrap(),
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
    let path = config_path(home)?;
    let before = common::snapshot(&path)
        .map_err(|_| AdapterFailure::ConfigurationFailed("configuration_read_failed"))?;
    let helper_executable = tool_credentials::executable_path()
        .map_err(|_| AdapterFailure::SecureStorageUnavailable)?;
    let after = render_catalog(
        before.as_deref(),
        origin,
        model,
        &helper_executable,
        model_ids,
    )
    .map_err(|_| AdapterFailure::ConfigurationFailed("configuration_parse_failed"))?;
    Ok(Prepared {
        transaction: FileTransaction::stage_with_snapshot(path.clone(), before, after)
            .map_err(config_error)?,
        path,
        origin: origin.to_owned(),
        model: model.to_owned(),
        model_ids: model_ids.to_vec(),
        helper_executable,
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
        let provider = &value["models"]["providers"]["yeschoy"];
        let secret = &value["secrets"]["providers"][SECRET_PROVIDER];
        let correct = provider["baseUrl"].as_str()
            == Some(format!("{}/v1", self.origin.trim_end_matches('/')).as_str())
            && provider["api"].as_str() == Some("openai-completions")
            && provider["authHeader"].as_bool() == Some(true)
            && provider["apiKey"]["source"].as_str() == Some("exec")
            && provider["apiKey"]["provider"].as_str() == Some(SECRET_PROVIDER)
            && provider["apiKey"]["id"].as_str() == Some(SECRET_ID)
            && crate::chat_gateway::catalog_matches(
                &provider["models"],
                Some("id"),
                &self.model_ids,
            )
            && crate::chat_gateway::default_matches(
                value["agents"]["defaults"]["model"]["primary"]
                    .as_str()
                    .and_then(|value| value.strip_prefix("yeschoy/")),
                &self.model,
                &self.model_ids,
                strict_default,
            )
            && secret["source"].as_str() == Some("exec")
            && secret["command"].as_str() == Some(&self.helper_executable)
            && secret["args"] == json!(["credential-helper-openclaw", "openclaw"])
            && secret["jsonOnly"].as_bool() == Some(true);
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
    let result = result.map_err(|error| AdapterFailure::VerificationFailed(error.reason_code()))?;
    if !result.success {
        return Err(AdapterFailure::VerificationFailed("tool_request_failed"));
    }
    let valid = serde_json::from_slice::<Value>(&result.stdout)
        .ok()
        .is_some_and(|value| {
            value["ok"].as_bool() == Some(true)
                && value["status"].as_str() == Some("ok")
                && value["final"]
                    .as_str()
                    .is_some_and(|text| common::verification_reply(text.as_bytes()))
        });
    if valid {
        Ok(())
    } else {
        Err(AdapterFailure::VerificationFailed("tool_response_invalid"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ru043_openclaw_reapply_keeps_user_params_and_supported_off() {
        let existing = br#"{"models":{"providers":{"yeschoy":{"models":[{"id":"gpt-5.6-sol","api":"old-api","baseUrl":"https://old.invalid/v1","headers":{"Authorization":"synthetic-old-key"},"maxTokens":4096,"contextWindow":65536}]}}},"agents":{"defaults":{"thinkingDefault":"low","models":{"yeschoy/gpt-5.6-sol":{"alias":"My assistant","params":{"maxTokens":2048}},"yeschoy/retired":{"alias":"old"}}}}}"#;
        let bytes = render_catalog(
            Some(existing),
            "http://127.0.0.1:15730/openclaw",
            "gpt-5.6-sol",
            "/synthetic/helper",
            &["gpt-5.6-sol".into()],
        )
        .unwrap();
        let value: Value = serde_json::from_slice(&bytes).unwrap();
        let model = &value["models"]["providers"]["yeschoy"]["models"][0];
        assert_eq!(model["maxTokens"], 4096);
        assert_eq!(model["thinkingLevelMap"]["off"], "none");
        assert!(model["thinkingLevelMap"]["max"].is_null());
        for key in ["api", "baseUrl", "headers"] {
            assert!(model.get(key).is_none());
        }
        assert_eq!(
            value["models"]["providers"]["yeschoy"]["baseUrl"],
            "http://127.0.0.1:15730/openclaw/v1"
        );
        assert_eq!(value["agents"]["defaults"]["thinkingDefault"], "low");
        assert_eq!(
            value["agents"]["defaults"]["models"]["yeschoy/gpt-5.6-sol"]["params"]["maxTokens"],
            2048
        );
        assert_eq!(
            value["agents"]["defaults"]["models"]["yeschoy/gpt-5.6-sol"]["alias"],
            "My assistant"
        );
        assert!(value["agents"]["defaults"]["models"]
            .get("yeschoy/retired")
            .is_none());
    }

    #[test]
    fn ru042_openclaw_catalog_removes_stale_owned_aliases_and_allows_member_default() {
        let home = common::temporary_working_directory("openclaw-model-set").unwrap();
        let path = home.join("openclaw.json");
        let ids = vec!["model-a".into(), "org/model-b".into()];
        let origin = crate::chat_gateway::base_url("openclaw").unwrap();
        let before = br#"{"agents":{"defaults":{"models":{"yeschoy/retired":{"alias":"old"},"other/keep":{"alias":"keep"}}}}}"#;
        let bytes =
            render_catalog(Some(before), &origin, "model-a", "/synthetic/helper", &ids).unwrap();
        let mut prepared = Prepared {
            transaction: FileTransaction::stage_with_snapshot(path.clone(), None, bytes).unwrap(),
            path: path.clone(),
            origin,
            model: "model-a".into(),
            model_ids: ids,
            helper_executable: "/synthetic/helper".into(),
        };
        prepared.commit().unwrap();
        let mut settings: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert!(settings["agents"]["defaults"]["models"]
            .get("yeschoy/retired")
            .is_none());
        assert_eq!(
            settings["agents"]["defaults"]["models"]["other/keep"]["alias"],
            "keep"
        );
        settings["agents"]["defaults"]["model"]["primary"] = "yeschoy/org/model-b".into();
        std::fs::write(&path, serde_json::to_vec(&settings).unwrap()).unwrap();
        assert!(prepared.validate_existing().is_ok());
        assert!(prepared.validate_readback(true).is_err());
        settings["agents"]["defaults"]["model"]["primary"] = "yeschoy/retired".into();
        std::fs::write(&path, serde_json::to_vec(&settings).unwrap()).unwrap();
        assert!(prepared.validate_existing().is_err());
        std::fs::remove_dir_all(home).unwrap();
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn openclaw_verification_executes_json_fixture_and_rejects_echo_error_and_failed_reply() {
        use common::test_support::Script;
        let valid = Script::new("[ \"$1\" = agent ] || exit 11\n[ \"$2\" = exec ] || exit 12\nprintf '%s\\n' '{\"ok\":true,\"status\":\"ok\",\"final\":\"YESCHOY_OK\"}'");
        assert!(verify(&valid.installation(), "fixture-model").await.is_ok());
        for body in [
            "printf 'OpenClaw ready\\n'",
            "printf '%s\\n' \"$*\"",
            "printf '%s\\n' '{\"ok\":true,\"status\":\"ok\",\"final\":\"reply YESCHOY_OK\"}'",
            "printf '%s\\n' '{\"ok\":false,\"status\":\"error\",\"final\":\"YESCHOY_OK\"}'",
            "printf '%s\\n' '{\"ok\":true,\"status\":\"ok\",\"final\":\"YESCHOY_OK\"}'; exit 17",
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
    fn openclaw_existing_validation_is_read_only_and_checks_full_secret_invocation() {
        let directory = common::temporary_working_directory("openclaw-readonly").unwrap();
        let path = directory.join("openclaw.json");
        let origin = "https://yeschoy.com";
        let model = "fixture-model";
        let helper = "/synthetic/helper";
        let bytes = render(
            Some(br#"{"channels":{"keep":true}}"#),
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
            helper_executable: helper.into(),
        };
        assert!(prepared.validate_existing().is_ok());
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
        for args in [
            json!(["credential-helper-openclaw", "pi"]),
            json!(["credential-helper-openclaw", "openclaw", "extra"]),
        ] {
            let mut value: Value = serde_json::from_slice(&bytes).unwrap();
            value["secrets"]["providers"][SECRET_PROVIDER]["args"] = args;
            let changed = serde_json::to_vec(&value).unwrap();
            std::fs::write(&path, &changed).unwrap();
            assert!(prepared.validate_existing().is_err());
            assert_eq!(std::fs::read(&path).unwrap(), changed);
        }
        std::fs::remove_dir_all(directory).unwrap();
    }

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

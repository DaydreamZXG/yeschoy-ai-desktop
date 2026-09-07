use std::{
    path::{Path, PathBuf},
    time::Duration,
};

use reqwest::{redirect::Policy, Client, StatusCode};
use serde_json::{json, Value};
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

fn catalog_value(model: &str, transport: CodexTransport) -> Result<Value, AdapterFailure> {
    let template = match transport {
        CodexTransport::DirectResponses => {
            include_str!("../resources/codex_native_responses_template.json")
        }
        CodexTransport::ChatBridge => include_str!("../resources/gpt5_5_template.json"),
    };
    let mut entry: Value = serde_json::from_str(template)
        .map_err(|_| AdapterFailure::ConfigurationFailed("configuration_parse_failed"))?;
    let object = entry
        .as_object_mut()
        .ok_or(AdapterFailure::ConfigurationFailed(
            "configuration_parse_failed",
        ))?;
    object.insert("slug".into(), serde_json::json!(model));
    object.insert(
        "display_name".into(),
        serde_json::json!(crate::tool_model_profile::display_name(model)),
    );
    object.insert("description".into(), serde_json::json!(model));
    object.insert("priority".into(), serde_json::json!(0));
    let profile = crate::tool_model_profile::profile(model);
    let levels: Vec<_> = profile
        .into_iter()
        .flat_map(|p| &p.reasoning_levels)
        .map(|effort| json!({"effort":effort,"description":effort}))
        .collect();
    object.insert("supported_reasoning_levels".into(), json!(levels));
    object.insert(
        "default_reasoning_level".into(),
        json!(profile.and_then(|p| p.default_reasoning.as_deref())),
    );
    object.insert(
        "supports_reasoning_summaries".into(),
        json!(!levels.is_empty()),
    );
    object.insert(
        "context_window".into(),
        json!(profile.and_then(|p| p.context_window)),
    );
    object.insert(
        "max_context_window".into(),
        json!(profile.and_then(|p| p.context_window)),
    );
    object.insert(
        "input_modalities".into(),
        profile
            .and_then(|p| p.input.as_ref())
            .map_or_else(|| json!(["text"]), |input| json!(input)),
    );
    object.insert("additional_speed_tiers".into(), serde_json::json!([]));
    object.insert("service_tiers".into(), serde_json::json!([]));
    object.remove("availability_nux");
    object.remove("upgrade");
    Ok(json!({"models": [entry]}))
}

fn catalog(model: &str, transport: CodexTransport) -> Result<Vec<u8>, AdapterFailure> {
    serde_json::to_vec_pretty(&catalog_value(model, transport)?)
        .map_err(|_| AdapterFailure::ConfigurationFailed("configuration_write_failed"))
}

pub(crate) fn bridge_catalog_models(model_ids: &[String]) -> Result<Value, AdapterFailure> {
    let mut models = Vec::with_capacity(model_ids.len());
    for model in model_ids {
        // All modern clients speak Responses to the local gateway. Do not
        // copy one route's model-specific Chat capabilities to other routes.
        let value = catalog_value(model, CodexTransport::DirectResponses)?;
        models.push(value["models"][0].clone());
    }
    Ok(json!({"models":models}))
}

pub(crate) fn prepare(
    home: &Path,
    origin: &str,
    model: &str,
    transport: CodexTransport,
) -> Result<Prepared, AdapterFailure> {
    prepare_inner(home, origin, model, transport, &[model.to_owned()], false)
}

pub(crate) fn prepare_catalog(
    home: &Path,
    origin: &str,
    model: &str,
    transport: CodexTransport,
    model_ids: &[String],
) -> Result<Prepared, AdapterFailure> {
    crate::chat_gateway::validate_catalog(model, model_ids)?;
    prepare_inner(home, origin, model, transport, model_ids, true)
}

fn prepare_inner(
    home: &Path,
    origin: &str,
    model: &str,
    transport: CodexTransport,
    model_ids: &[String],
    modern: bool,
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
    let base_url = if modern {
        codex_bridge::BASE_URL.to_owned()
    } else {
        match transport {
            CodexTransport::DirectResponses => format!("{}/v1", origin.trim_end_matches('/')),
            CodexTransport::ChatBridge => codex_bridge::BASE_URL.to_owned(),
        }
    };
    let after = render(before.as_deref(), &base_url, model, &helper_executable)?;
    let catalog = if modern {
        serde_json::to_vec_pretty(&bridge_catalog_models(model_ids)?)
            .map_err(|_| AdapterFailure::ConfigurationFailed("configuration_write_failed"))?
    } else {
        catalog(model, transport)?
    };
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
            && crate::chat_gateway::default_matches(
                document["model"].as_str(),
                &self.model,
                &self.model_ids,
                strict_default,
            )
            && document["model_catalog_json"].as_str() == Some(OWNED_CATALOG)
            && provider["base_url"].as_str() == Some(self.base_url.as_str())
            && provider["wire_api"].as_str() == Some("responses")
            && provider["auth"]["command"].as_str() == Some(&self.helper_executable)
            && provider["auth"]["args"].as_array().is_some_and(|args| {
                args.len() == 2
                    && args.get(0).and_then(toml_edit::Value::as_str) == Some("credential-helper")
                    && args.get(1).and_then(toml_edit::Value::as_str) == Some("codex_desktop")
            })
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

pub(crate) async fn verify_and_launch(
    installation: &ResolvedInstallation,
    credential: &tool_credentials::ToolCredential,
) -> Result<(), AdapterFailure> {
    ensure_credential_ready(credential).await?;
    // This is a provider connection check issued by this assistant. It does
    // not prove that the desktop application itself sent a successful request.
    verify_provider(credential).await?;
    launch(&installation.path)
}

pub(crate) async fn ensure_credential_ready(
    credential: &tool_credentials::ToolCredential,
) -> Result<(), AdapterFailure> {
    let helper = tool_credentials::executable_path()
        .map_err(|_| AdapterFailure::VerificationFailed("credential_helper_failed"))?;
    let mut command = Command::new(helper);
    command.args(["credential-helper", "codex_desktop"]);
    let result = common::run_bounded(command, Duration::from_secs(6))
        .await
        .map_err(|_| AdapterFailure::VerificationFailed("credential_helper_failed"))?;
    let resolved = std::str::from_utf8(&result.stdout)
        .ok()
        .map(str::trim)
        .unwrap_or("");
    if !result.success
        || !codex_bridge::secure_equal(resolved, credential.client_token("codex_desktop"))
    {
        return Err(AdapterFailure::VerificationFailed(
            "credential_helper_failed",
        ));
    }
    Ok(())
}

fn provider_url(credential: &tool_credentials::ToolCredential) -> Result<String, AdapterFailure> {
    if credential.has_model_set() {
        return Ok(format!("{}/responses", codex_bridge::BASE_URL));
    }
    match credential.codex_transport.as_deref() {
        Some("direct_responses") => Ok(format!(
            "{}/v1/responses",
            credential.origin.trim_end_matches('/')
        )),
        Some("chat_bridge") => Ok(format!("{}/responses", codex_bridge::BASE_URL)),
        _ => Err(AdapterFailure::ConfigurationFailed("invalid_request")),
    }
}

fn status_failure(status: StatusCode) -> Option<&'static str> {
    match status.as_u16() {
        200..=299 => None,
        401 | 403 => Some("authentication_failed"),
        404 | 405 => Some("endpoint_unavailable"),
        408 => Some("provider_timed_out"),
        429 => Some("provider_busy"),
        400 | 409 | 422 => Some("model_request_rejected"),
        500..=599 => Some("provider_unavailable"),
        _ => Some("provider_request_failed"),
    }
}

fn completed_provider_response(value: &Value) -> bool {
    let completed = value
        .get("status")
        .and_then(Value::as_str)
        .is_none_or(|status| status == "completed");
    let output_text = value
        .get("output_text")
        .and_then(Value::as_str)
        .is_some_and(|text| !text.trim().is_empty());
    let output_item = value
        .get("output")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|item| item.get("content").and_then(Value::as_array))
        .flatten()
        .filter_map(|part| part.get("text").and_then(Value::as_str))
        .any(|text| !text.trim().is_empty());
    completed && (output_text || output_item)
}

async fn verify_provider(
    credential: &tool_credentials::ToolCredential,
) -> Result<(), AdapterFailure> {
    const MAX_RESPONSE_BYTES: u64 = 2 * 1024 * 1024;
    let client = Client::builder()
        .no_proxy()
        .redirect(Policy::none())
        .connect_timeout(Duration::from_secs(15))
        .read_timeout(Duration::from_secs(600))
        .build()
        .map_err(|_| AdapterFailure::VerificationFailed("provider_unavailable"))?;
    let response = client
        .post(provider_url(credential)?)
        .bearer_auth(credential.client_token("codex_desktop"))
        .json(&json!({
            "model": credential.model_id,
            "input": [{
                "role": "user",
                "content": [{
                    "type": "input_text",
                    "text": "仅回复 YESCHOY_OK，不要使用工具。"
                }]
            }],
            "stream": false
        }))
        .send()
        .await
        .map_err(|error| {
            AdapterFailure::VerificationFailed(if error.is_timeout() {
                "provider_timed_out"
            } else {
                "provider_unavailable"
            })
        })?;
    if let Some(reason) = status_failure(response.status()) {
        return Err(AdapterFailure::VerificationFailed(reason));
    }
    if response
        .content_length()
        .is_some_and(|length| length > MAX_RESPONSE_BYTES)
    {
        return Err(AdapterFailure::VerificationFailed(
            "invalid_provider_response",
        ));
    }
    let bytes = response
        .bytes()
        .await
        .map_err(|_| AdapterFailure::VerificationFailed("invalid_provider_response"))?;
    if bytes.len() as u64 > MAX_RESPONSE_BYTES {
        return Err(AdapterFailure::VerificationFailed(
            "invalid_provider_response",
        ));
    }
    let value = serde_json::from_slice::<Value>(&bytes)
        .map_err(|_| AdapterFailure::VerificationFailed("invalid_provider_response"))?;
    completed_provider_response(&value)
        .then_some(())
        .ok_or(AdapterFailure::VerificationFailed(
            "invalid_provider_response",
        ))
}

pub(crate) fn launch(path: &Path) -> Result<(), AdapterFailure> {
    super::desktop_launch::launch("codex_desktop", path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ru043_codex_catalog_efforts_and_unknown_defaults() {
        let catalog = bridge_catalog_models(&[
            "gpt-6-astra".into(),
            "deepseek-v4-flash".into(),
            "unknown/model".into(),
        ])
        .unwrap();
        let gpt = &catalog["models"][0];
        assert_eq!(gpt["slug"], "gpt-6-astra");
        assert_eq!(gpt["display_name"], "GPT-6 Astra");
        assert_eq!(gpt["context_window"], 1_050_000);
        let levels: Vec<_> = gpt["supported_reasoning_levels"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v["effort"].as_str().unwrap())
            .collect();
        assert_eq!(levels, ["low", "medium", "high", "xhigh", "max"]);
        let ds = &catalog["models"][1];
        assert_eq!(ds["default_reasoning_level"], "high");
        assert_eq!(ds["input_modalities"], json!(["text"]));
        let unknown = &catalog["models"][2];
        assert_eq!(unknown["display_name"], "unknown/model");
        assert_eq!(unknown["supported_reasoning_levels"], json!([]));
        assert!(unknown["default_reasoning_level"].is_null());
        assert!(unknown["context_window"].is_null());
        let config = render(
            Some(b"model_reasoning_effort = 'low'\n"),
            codex_bridge::BASE_URL,
            "gpt-6-astra",
            "/synthetic/helper",
        )
        .unwrap();
        assert_eq!(
            String::from_utf8(config)
                .unwrap()
                .parse::<DocumentMut>()
                .unwrap()["model_reasoning_effort"]
                .as_str(),
            Some("low")
        );
    }

    #[test]
    fn ru042_codex_catalog_is_conservative_and_accepts_registered_native_default() {
        let home = common::temporary_working_directory("codex-model-set").unwrap();
        let path = home.join("config.toml");
        let catalog_path = home.join(OWNED_CATALOG);
        let ids = vec!["model-a".into(), "org/model-b".into()];
        let catalog = serde_json::to_vec_pretty(&bridge_catalog_models(&ids).unwrap()).unwrap();
        let source = render(None, codex_bridge::BASE_URL, "model-a", "/synthetic/helper").unwrap();
        let mut transaction =
            FileTransaction::stage_with_snapshot(path.clone(), None, source).unwrap();
        transaction
            .push(catalog_path.clone(), catalog.clone())
            .unwrap();
        let mut prepared = Prepared {
            transaction,
            path: path.clone(),
            catalog_path,
            catalog,
            base_url: codex_bridge::BASE_URL.into(),
            model: "model-a".into(),
            model_ids: ids,
            helper_executable: "/synthetic/helper".into(),
        };
        prepared.commit().unwrap();
        let catalog_value = bridge_catalog_models(&prepared.model_ids).unwrap();
        assert_eq!(catalog_value["models"][1]["slug"], "org/model-b");
        assert!(catalog_value["models"]
            .as_array()
            .unwrap()
            .iter()
            .all(|model| model.get("apply_patch_tool_type").is_none()));
        let mut source = std::fs::read_to_string(&path)
            .unwrap()
            .parse::<DocumentMut>()
            .unwrap();
        source["model"] = value("org/model-b");
        std::fs::write(&path, source.to_string()).unwrap();
        assert!(prepared.validate_existing().is_ok());
        assert!(prepared.validate_readback(true).is_err());
        source["model"] = value("not-enrolled");
        std::fs::write(&path, source.to_string()).unwrap();
        assert!(prepared.validate_existing().is_err());
        std::fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn provider_response_requires_completed_assistant_text() {
        assert!(completed_provider_response(&json!({
            "status": "completed",
            "output": [{
                "type": "message",
                "content": [{"type": "output_text", "text": "YESCHOY_OK"}]
            }]
        })));
        assert!(!completed_provider_response(&json!({
            "status": "failed",
            "output_text": "YESCHOY_OK"
        })));
        assert!(!completed_provider_response(&json!({
            "status": "completed",
            "output": []
        })));
    }

    #[test]
    fn provider_statuses_have_closed_recovery_reasons() {
        assert_eq!(
            status_failure(StatusCode::UNAUTHORIZED),
            Some("authentication_failed")
        );
        assert_eq!(
            status_failure(StatusCode::TOO_MANY_REQUESTS),
            Some("provider_busy")
        );
        assert_eq!(
            status_failure(StatusCode::UNPROCESSABLE_ENTITY),
            Some("model_request_rejected")
        );
        assert_eq!(status_failure(StatusCode::OK), None);
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

    #[test]
    fn daily_open_rejects_changed_helper_target_without_writing() {
        let home = common::temporary_working_directory("codex-open-helper").unwrap();
        let mut prepared = prepare(
            &home,
            "https://yeschoy.com",
            "test-model",
            CodexTransport::DirectResponses,
        )
        .unwrap();
        prepared.commit().unwrap();
        let original = std::fs::read_to_string(&prepared.path).unwrap();
        for args in [
            vec!["credential-helper", "pi"],
            vec!["other-command", "codex_desktop"],
            vec!["credential-helper", "codex_desktop", "extra"],
            vec!["credential-helper"],
        ] {
            let mut config = original.parse::<DocumentMut>().unwrap();
            config["model_providers"]["yeschoy"]["auth"]["args"] =
                value(args.into_iter().collect::<Array>());
            let changed = config.to_string();
            std::fs::write(&prepared.path, &changed).unwrap();
            assert!(prepared.validate_existing().is_err());
            assert_eq!(std::fs::read_to_string(&prepared.path).unwrap(), changed);
        }
        std::fs::remove_dir_all(home).unwrap();
    }
}

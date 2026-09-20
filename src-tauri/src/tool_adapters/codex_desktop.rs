use std::{
    ffi::OsStr,
    path::{Path, PathBuf},
    time::Duration,
};

use serde_json::{json, Value};
use tokio::process::Command;
use toml_edit::{value, Array, DocumentMut, Item, Table};

use crate::{
    codex_bridge, codex_responses_bridge,
    local_bridge::LocalBridgeRuntime,
    tool_adapters::{
        common::{self, ConfigFailure, FileTransaction},
        AdapterFailure,
    },
    tool_credentials,
};

const OWNED_CATALOG: &str = "yeschoy-model-catalog.json";
const PROXY_ADDRESS: &str = "127.0.0.1:15731";
/// Codex 会在这个值后面接 `/responses`，所以它要以 `/v1` 结尾。
const PROXY_BASE: &str = "http://127.0.0.1:15731/codex/v1";

#[derive(Clone)]
pub(crate) struct CodexRuntimeState {
    runtime: LocalBridgeRuntime,
}

impl Default for CodexRuntimeState {
    fn default() -> Self {
        Self {
            runtime: codex_responses_bridge::codex_runtime(PROXY_ADDRESS, "/codex"),
        }
    }
}

impl CodexRuntimeState {
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
    provider_id: String,
    provider_auth: ProviderAuth,
}

#[derive(Clone)]
enum ProviderAuth {
    Command(String),
    /// The scoped per-tool key issued by the relay. It authenticates directly
    /// against the relay's public origin, so no local gateway is involved.
    ApiKey(String),
}

/// Relay keys are opaque `sk-` tokens. The bound matches the credential record
/// validator; control characters can never reach a TOML value.
fn valid_provider_key(key: &str) -> bool {
    (16..=256).contains(&key.len())
        && key.starts_with("sk-")
        && !key
            .chars()
            .any(|value| value.is_control() || value.is_whitespace())
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

#[cfg(test)]
fn render(
    existing: Option<&[u8]>,
    base_url: &str,
    model: &str,
    provider_auth: &ProviderAuth,
) -> Result<Vec<u8>, AdapterFailure> {
    render_with_provider(existing, base_url, model, provider_auth, None).map(|(bytes, _)| bytes)
}

fn valid_provider_id(value: &str) -> bool {
    (1..=64).contains(&value.len())
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
}

fn custom_provider_id(document: &DocumentMut) -> Option<&str> {
    let current = document.get("model_provider").and_then(Item::as_str)?;
    if current == "yeschoy" {
        return Some(current);
    }
    // Never shadow Codex's built-in OpenAI identity. Third-party switchers use
    // named custom providers; retaining that identifier lets existing threads
    // resolve their pinned provider while their traffic is temporarily routed
    // through YesChoy.
    if current == "openai" || !valid_provider_id(current) {
        return None;
    }
    let declared = document
        .get("model_providers")
        .and_then(Item::as_table_like)
        .is_some_and(|providers| providers.get(current).is_some());
    if declared || current == "custom" {
        Some(current)
    } else {
        None
    }
}

pub(crate) fn provider_id_from_snapshot(bytes: Option<&[u8]>) -> Option<String> {
    let source = std::str::from_utf8(bytes?).ok()?;
    let document = source.parse::<DocumentMut>().ok()?;
    custom_provider_id(&document)
        .filter(|provider| *provider != "yeschoy")
        .map(str::to_owned)
}

fn takeover_provider_id(document: &DocumentMut, preferred: Option<&str>) -> String {
    preferred
        .filter(|provider| *provider != "openai" && valid_provider_id(provider))
        .map(str::to_owned)
        .or_else(|| custom_provider_id(document).map(str::to_owned))
        .unwrap_or_else(|| "yeschoy".into())
}

fn render_with_provider(
    existing: Option<&[u8]>,
    base_url: &str,
    model: &str,
    provider_auth: &ProviderAuth,
    preferred_provider: Option<&str>,
) -> Result<(Vec<u8>, String), AdapterFailure> {
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
    let provider_id = takeover_provider_id(&document, preferred_provider);
    // Existing Codex threads pin the provider identifier. Reusing a switcher's
    // active custom identifier allows those threads to continue under YesChoy
    // without rewriting chat history. The activation transaction snapshots the
    // complete original table and restores it later.
    document["model_provider"] = value(&provider_id);
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
    // Replace the temporarily owned provider table instead of retaining old
    // headers, environment keys or authentication fields from another service.
    providers.insert(&provider_id, Item::Table(Table::new()));
    let provider = providers
        .get_mut(&provider_id)
        .and_then(Item::as_table_like_mut)
        .ok_or(AdapterFailure::ConfigurationFailed(
            "configuration_parse_failed",
        ))?;
    provider.insert("name", value("野菜API"));
    provider.insert("base_url", value(base_url));
    provider.insert("wire_api", value("responses"));
    // Authentication is scoped to our dedicated provider. Removing these
    // fields here leaves the user's official OpenAI login and auth.json
    // untouched while preventing any fallback to that official account.
    for key in [
        "requires_openai_auth",
        "env_key",
        "experimental_bearer_token",
        "auth",
    ] {
        provider.remove(key);
    }
    match provider_auth {
        ProviderAuth::Command(helper_executable) => {
            let mut auth = Table::new();
            auth.insert("command", value(helper_executable));
            let mut args = Array::new();
            args.push("credential-helper");
            args.push("codex_desktop");
            auth.insert("args", value(args));
            auth.insert("timeout_ms", value(5_000));
            auth.insert("refresh_interval_ms", value(0));
            provider.insert("auth", Item::Table(auth));
        }
        ProviderAuth::ApiKey(key) => {
            // Direct connection to the relay origin. `requires_openai_auth` is
            // deliberately absent: it exists to attach the user's official
            // ChatGPT credential, and that credential must never be sent to a
            // remote host. `experimental_bearer_token` alone authenticates the
            // scoped per-tool key and is what the relay expects.
            provider.insert("experimental_bearer_token", value(key));
        }
    }
    Ok((document.to_string().into_bytes(), provider_id))
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
    prepare_inner(
        home,
        origin,
        model,
        transport,
        &[model.to_owned()],
        false,
        None,
        None,
    )
}

pub(crate) fn prepare_catalog(
    home: &Path,
    origin: &str,
    model: &str,
    transport: CodexTransport,
    provider_key: Option<&str>,
    model_ids: &[String],
) -> Result<Prepared, AdapterFailure> {
    prepare_catalog_with_provider_hint(
        home,
        origin,
        model,
        transport,
        provider_key,
        model_ids,
        None,
    )
}

pub(crate) fn prepare_catalog_with_provider_hint(
    home: &Path,
    origin: &str,
    model: &str,
    transport: CodexTransport,
    provider_key: Option<&str>,
    model_ids: &[String],
    provider_hint: Option<&str>,
) -> Result<Prepared, AdapterFailure> {
    crate::tool_adapters::common::validate_catalog(model, model_ids)?;
    prepare_inner(
        home,
        origin,
        model,
        transport,
        model_ids,
        true,
        provider_key,
        provider_hint,
    )
}

// This is the Codex config-write path. Grouping these into a struct is a
// refactor of the riskiest module in the app for no behavioural gain; the
// arguments are already each named at every call site.
#[expect(
    clippy::too_many_arguments,
    reason = "config-write path; regrouping carries risk without removing an argument"
)]
fn prepare_inner(
    home: &Path,
    origin: &str,
    model: &str,
    transport: CodexTransport,
    model_ids: &[String],
    modern: bool,
    provider_key: Option<&str>,
    provider_hint: Option<&str>,
) -> Result<Prepared, AdapterFailure> {
    let config_dir = config_dir(home)?;
    let path = config_dir.join("config.toml");
    let catalog_path = config_dir.join(OWNED_CATALOG);
    let before = common::snapshot(&path)
        .map_err(|_| AdapterFailure::ConfigurationFailed("configuration_read_failed"))?;
    let provider_auth = if modern {
        let key = provider_key
            .filter(|key| valid_provider_key(key))
            .ok_or(AdapterFailure::SecureStorageUnavailable)?;
        ProviderAuth::ApiKey(key.to_owned())
    } else {
        ProviderAuth::Command(
            tool_credentials::executable_path()
                .map_err(|_| AdapterFailure::SecureStorageUnavailable)?,
        )
    };
    // Codex 指向本地桥，不再直接指向中转。
    //
    // 不是因为中转的 Responses 不能用 —— 多数模型能用，那些**照旧原样透传**，
    // 一个字节都不改（真的 `encrypted_content` 不能被转一圈换成我们伪造的）。
    // 是因为有些渠道的上游只有 Chat，中转转不过去，而 Codex 0.155.1 起
    // `wire_api = "chat"` 已被移除，改配置绕不开，转换必须有人做。
    //
    // 为什么不是「按模型写 base_url」：Codex 的 config.toml 是**一个 provider、
    // 一个 base_url、一整份模型目录**，用户在 Codex 里切模型。按模型写死，
    // 用户一切模型地址就错了。所以地址恒定，**转不转由桥按请求里的模型决定**。
    let _ = origin;
    let base_url = PROXY_BASE.to_owned();
    let (after, provider_id) = render_with_provider(
        before.as_deref(),
        &base_url,
        model,
        &provider_auth,
        provider_hint,
    )?;
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
        provider_id,
        provider_auth,
    })
}

/// 应用重启后把桥重新拉起来。
///
/// 不做这一步的后果很直接：Codex 的 `config.toml` 指向
/// `127.0.0.1:15731/codex/v1`，用户重启我们的应用之后那个端口没人监听，
/// Codex 就连不上了 —— 而用户并没有改过任何配置。
///
/// 只在 Codex 确实还指着我们这座桥时才起：读它自己的配置来判断，
/// 不靠我们这边记的状态。用户手改回直连、或换了别的 provider，都不该
/// 被我们又占一个端口。
pub(crate) async fn resume_if_configured(state: CodexRuntimeState) {
    let Ok(credential) = tool_credentials::load("codex_desktop") else {
        return;
    };
    let Some(home) = super::user_home() else {
        return;
    };
    let Ok(dir) = config_dir(&home) else {
        return;
    };
    let Ok(text) = std::fs::read_to_string(dir.join("config.toml")) else {
        return;
    };
    if config_points_at_our_bridge(&text) {
        let _ = state.start(credential).await;
    }
}

/// 这份 `config.toml` 现在还指着我们这座桥吗。
///
/// 抽成纯函数是因为 `resume_if_configured` 剩下的部分全是 I/O（钥匙串、
/// home、绑端口），测不了；而会出错的恰恰是这里。
///
/// **只看当前生效的那个 provider**（`model_provider` 指向谁），不扫全表。
/// 用户配置里同时留着好几个 provider 是常态 —— 他们切去 OpenAI 官方之后，
/// 我们那条记录还在。扫全表就会在用户已经不用我们的时候照样占住端口。
///
/// 按 `base_url` 认，不按 provider 名字认：接入时可以写进用户自己命名的
/// provider（`provider_hint`），名字不固定，地址才是。
fn config_points_at_our_bridge(text: &str) -> bool {
    let Ok(document) = text.parse::<DocumentMut>() else {
        return false;
    };
    let Some(active) = document.get("model_provider").and_then(Item::as_str) else {
        return false;
    };
    document
        .get("model_providers")
        // 不先 `as_table()`：内联表（`model_providers = { … }`）不是
        // `Item::Table`，转换会返回 None，于是桥静默地不恢复。
        // `validate_readback` 本来就是直接 `get`，这里跟它一致。
        .and_then(|providers| providers.get(active))
        .and_then(|provider| provider.get("base_url"))
        .and_then(Item::as_str)
        == Some(PROXY_BASE)
}

fn config_dir_from_override(
    home: &Path,
    override_dir: Option<&OsStr>,
) -> Result<PathBuf, AdapterFailure> {
    let Some(raw) = override_dir.filter(|value| !value.is_empty()) else {
        return Ok(home.join(".codex"));
    };
    let path = PathBuf::from(raw);
    if !path.is_absolute()
        || path
            .components()
            .any(|part| matches!(part, std::path::Component::ParentDir))
    {
        return Err(AdapterFailure::ExternalOverride);
    }
    Ok(path)
}

pub(crate) fn config_dir(home: &Path) -> Result<PathBuf, AdapterFailure> {
    // Read through the login shell: `CODEX_HOME` is normally exported from a
    // shell rc, which a Dock-launched app never sources. Reading `std::env`
    // here wrote a perfectly valid config into `~/.codex` while the `codex`
    // CLI kept loading a different directory.
    let override_dir = crate::shell_environment::var_os("CODEX_HOME");
    config_dir_from_override(home, override_dir.as_deref())
}

impl Prepared {
    pub(crate) fn provider_id(&self) -> &str {
        &self.provider_id
    }

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
        // Live settings can still use the old helper/gateway auth, or have
        // been edited by Codex/another switcher. toml_edit's [] accessor
        // panics on a missing key (unlike serde_json); a changed profile must
        // fail readback normally, not abort the entire connection inspection.
        let provider = document
            .get("model_providers")
            .and_then(|providers| providers.get(&self.provider_id))
            .and_then(Item::as_table_like)
            .ok_or(AdapterFailure::ConfigurationFailed(
                "configuration_readback_failed",
            ))?;
        let catalog = common::snapshot(&self.catalog_path)
            .map_err(|_| AdapterFailure::ConfigurationFailed("configuration_readback_failed"))?;
        let auth_correct = match &self.provider_auth {
            ProviderAuth::Command(helper_executable) => {
                provider
                    .get("auth")
                    .and_then(Item::as_table_like)
                    .is_some_and(|auth| {
                        auth.get("command").and_then(Item::as_str) == Some(helper_executable)
                            && auth
                                .get("args")
                                .and_then(Item::as_array)
                                .is_some_and(|args| {
                                    args.len() == 2
                                        && args.get(0).and_then(toml_edit::Value::as_str)
                                            == Some("credential-helper")
                                        && args.get(1).and_then(toml_edit::Value::as_str)
                                            == Some("codex_desktop")
                                })
                    })
                    && provider.get("requires_openai_auth").is_none()
                    && provider.get("env_key").is_none()
                    && provider.get("experimental_bearer_token").is_none()
            }
            ProviderAuth::ApiKey(key) => {
                provider
                    .get("experimental_bearer_token")
                    .and_then(Item::as_str)
                    == Some(key)
                    && provider.get("requires_openai_auth").is_none()
                    && provider.get("auth").is_none()
                    && provider.get("env_key").is_none()
            }
        };
        let correct = document.get("model_provider").and_then(Item::as_str)
            == Some(self.provider_id.as_str())
            && crate::tool_adapters::common::default_matches(
                document.get("model").and_then(Item::as_str),
                &self.model,
                &self.model_ids,
                strict_default,
            )
            && document.get("model_catalog_json").and_then(Item::as_str) == Some(OWNED_CATALOG)
            && provider.get("base_url").and_then(Item::as_str) == Some(self.base_url.as_str())
            && provider.get("wire_api").and_then(Item::as_str) == Some("responses")
            && auth_correct
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

pub(crate) async fn ensure_credential_ready(
    credential: &tool_credentials::ToolCredential,
) -> Result<(), AdapterFailure> {
    if credential.has_model_set() {
        // Direct connection: the scoped per-tool key is already written into
        // the Codex provider config. There is no local listener to warm up,
        // restart, or authenticate against.
        return Ok(());
    }
    let helper = tool_credentials::executable_path()
        .map_err(|_| AdapterFailure::ConfigurationFailed("credential_helper_failed"))?;
    let mut command = Command::new(helper);
    command.args(["credential-helper", "codex_desktop"]);
    let result = common::run_bounded(command, Duration::from_secs(6))
        .await
        .map_err(|_| AdapterFailure::ConfigurationFailed("credential_helper_failed"))?;
    let resolved = std::str::from_utf8(&result.stdout)
        .ok()
        .map(str::trim)
        .unwrap_or("");
    if !result.success
        || !codex_bridge::secure_equal(resolved, credential.client_token("codex_desktop"))
    {
        return Err(AdapterFailure::ConfigurationFailed(
            "credential_helper_failed",
        ));
    }
    Ok(())
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
            &ProviderAuth::Command("/synthetic/helper".into()),
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
        let provider_auth = ProviderAuth::Command("/synthetic/helper".into());
        let source = render(None, codex_bridge::BASE_URL, "model-a", &provider_auth).unwrap();
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
            provider_id: "yeschoy".into(),
            provider_auth,
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
    fn render_preserves_official_auth_and_uses_command_auth() {
        let source =
            b"model_reasoning_effort = \"high\"\n[notice]\nhide_full_access_warning = true\n";
        let bytes = render(
            Some(source),
            "https://api.yeschoy.com/v1",
            "glm-5.3",
            &ProviderAuth::Command("/Applications/野菜 API.app/Contents/MacOS/野菜 API".into()),
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
    fn modern_provider_writes_only_the_scoped_relay_key() {
        let key = format!("sk-{}", "a".repeat(48));
        let bytes = render(
            Some(
                b"[model_providers.yeschoy]\nrequires_openai_auth = true\nenv_key = 'OPENAI_API_KEY'\n[model_providers.yeschoy.auth]\ncommand = 'old-helper'\n",
            ),
            "https://yeschoy.com/v1",
            "gpt-6-astra",
            &ProviderAuth::ApiKey(key.clone()),
        )
        .unwrap();
        let text = String::from_utf8(bytes).unwrap();
        let document = text.parse::<DocumentMut>().unwrap();
        let provider = &document["model_providers"]["yeschoy"];
        assert_eq!(
            provider["experimental_bearer_token"].as_str(),
            Some(key.as_str())
        );
        // The official ChatGPT credential must never be attached to a remote
        // provider, so `requires_openai_auth` stays absent.
        assert!(provider.get("requires_openai_auth").is_none());
        assert!(provider.get("auth").is_none());
        assert!(provider.get("env_key").is_none());
        assert!(!text.contains("OPENAI_API_KEY"));
        assert!(!text.contains("old-helper"));
    }

    #[test]
    fn custom_switcher_provider_is_temporarily_taken_over_for_existing_threads() {
        let key = format!("sk-{}", "c".repeat(48));
        let original = br#"model_provider = "custom"
model = "deepseek-chat"
model_catalog_json = "cc-switch-models.json"

[model_providers.custom]
name = "DeepSeek"
base_url = "https://api.deepseek.com"
wire_api = "responses"
env_key = "DEEPSEEK_API_KEY"

[model_providers.other]
name = "Keep me"
base_url = "https://example.invalid/v1"
"#;
        let (bytes, provider_id) = render_with_provider(
            Some(original),
            "https://yeschoy.com/v1",
            "gpt-6-astra",
            &ProviderAuth::ApiKey(key.clone()),
            None,
        )
        .unwrap();
        let text = String::from_utf8(bytes).unwrap();
        let document = text.parse::<DocumentMut>().unwrap();
        assert_eq!(provider_id, "custom");
        assert_eq!(document["model_provider"].as_str(), Some("custom"));
        assert_eq!(document["model"].as_str(), Some("gpt-6-astra"));
        assert_eq!(
            document["model_providers"]["custom"]["base_url"].as_str(),
            Some("https://yeschoy.com/v1")
        );
        assert_eq!(
            document["model_providers"]["custom"]["experimental_bearer_token"].as_str(),
            Some(key.as_str())
        );
        assert!(document["model_providers"]["custom"]
            .get("env_key")
            .is_none());
        assert_eq!(
            document["model_providers"]["other"]["base_url"].as_str(),
            Some("https://example.invalid/v1")
        );
    }

    #[test]
    fn built_in_or_unresolved_provider_is_not_shadowed() {
        for source in [
            "model_provider = 'openai'\n",
            "model_provider = 'missing'\n",
            "model_provider = 'bad/provider'\n",
        ] {
            let (bytes, provider_id) = render_with_provider(
                Some(source.as_bytes()),
                "https://yeschoy.com/v1",
                "gpt-6-astra",
                &ProviderAuth::Command("/synthetic/helper".into()),
                None,
            )
            .unwrap();
            let document = String::from_utf8(bytes)
                .unwrap()
                .parse::<DocumentMut>()
                .unwrap();
            assert_eq!(provider_id, "yeschoy");
            assert_eq!(document["model_provider"].as_str(), Some("yeschoy"));
        }
    }

    #[test]
    fn completed_legacy_receipt_hint_migrates_to_the_original_custom_provider() {
        let home = common::temporary_working_directory("codex-provider-hint").unwrap();
        let config_path = home.join(".codex/config.toml");
        let original = br#"model_provider = "custom"
[model_providers.custom]
base_url = "https://api.deepseek.com"
wire_api = "responses"
"#;
        assert_eq!(
            provider_id_from_snapshot(Some(original)).as_deref(),
            Some("custom")
        );
        let current = br#"model_provider = "yeschoy"
model = "gpt-old"
[model_providers.yeschoy]
base_url = "https://yeschoy.com/v1"
wire_api = "responses"
experimental_bearer_token = "sk-old-old-old-old"
"#;
        common::atomic_write(&config_path, current).unwrap();
        let key = format!("sk-{}", "d".repeat(48));
        let models = vec!["gpt-6-astra".to_owned()];
        let mut prepared = prepare_catalog_with_provider_hint(
            &home,
            "https://yeschoy.com",
            "gpt-6-astra",
            CodexTransport::DirectResponses,
            Some(&key),
            &models,
            Some("custom"),
        )
        .unwrap();
        prepared.commit().unwrap();
        assert_eq!(prepared.provider_id, "custom");
        assert!(prepared.validate_existing().is_ok());
        let active = std::fs::read_to_string(&config_path)
            .unwrap()
            .parse::<DocumentMut>()
            .unwrap();
        assert_eq!(active["model_provider"].as_str(), Some("custom"));
        // 地址是本地桥，不是中转 —— 见 `prepare_inner` 里那段说明。
        // 这条测试守的是「自定义 provider 被原样保留」，不是地址本身。
        assert_eq!(
            active["model_providers"]["custom"]["base_url"].as_str(),
            Some(PROXY_BASE)
        );
        prepared.rollback().unwrap();
        assert_eq!(std::fs::read(&config_path).unwrap(), current);
        std::fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn modern_catalog_requires_and_commits_the_scoped_relay_key() {
        let home = common::temporary_working_directory("codex-relay-key").unwrap();
        let models = vec!["gpt-6-astra".to_string(), "gpt-5.6-sol".to_string()];
        assert!(matches!(
            prepare_catalog(
                &home,
                "https://yeschoy.com",
                "gpt-6-astra",
                CodexTransport::DirectResponses,
                None,
                &models,
            ),
            Err(AdapterFailure::SecureStorageUnavailable)
        ));
        let key = format!("sk-{}", "b".repeat(48));
        let mut prepared = prepare_catalog(
            &home,
            "https://yeschoy.com",
            "gpt-6-astra",
            CodexTransport::DirectResponses,
            Some(&key),
            &models,
        )
        .unwrap();
        prepared.commit().unwrap();
        let config = std::fs::read_to_string(home.join(".codex/config.toml")).unwrap();
        assert!(config.contains(&format!("experimental_bearer_token = \"{key}\"")));
        assert!(config.contains(&format!("base_url = \"{PROXY_BASE}\"")));
        assert!(!config.contains("requires_openai_auth"));
        assert!(!config.contains("credential-helper"));
        std::fs::remove_dir_all(home).unwrap();
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
    fn user_owned_catalog_is_temporarily_shadowed_without_touching_its_file() {
        let rendered = render(
            Some(b"model_catalog_json = \"my-models.json\"\n"),
            "https://yeschoy.com/v1",
            "gpt-5.5",
            &ProviderAuth::Command("/tmp/helper".into()),
        )
        .unwrap();
        let document = String::from_utf8(rendered)
            .unwrap()
            .parse::<DocumentMut>()
            .unwrap();
        assert_eq!(document["model_catalog_json"].as_str(), Some(OWNED_CATALOG));
    }

    #[test]
    fn codex_home_override_selects_the_directory_codex_really_reads() {
        let root = std::env::temp_dir();
        let home = root.join("example-user");
        let custom = root.join("example-codex-home");
        assert_eq!(
            config_dir_from_override(&home, None).unwrap(),
            home.join(".codex")
        );
        assert_eq!(
            config_dir_from_override(&home, Some(custom.as_os_str())).unwrap(),
            custom
        );
        assert!(matches!(
            config_dir_from_override(&home, Some(OsStr::new("../other-codex"))),
            Err(AdapterFailure::ExternalOverride)
        ));
    }

    #[test]
    fn configuration_and_owned_catalog_commit_and_rollback_together() {
        let home = common::temporary_working_directory("codex-catalog-transaction").unwrap();
        let config_path = home.join(".codex").join("config.toml");
        let external_catalog_path = home.join(".codex").join("customer-models.json");
        let external_catalog = br#"{"models":[{"slug":"customer-model"}]}"#;
        let original =
            b"model_reasoning_effort = \"high\"\nmodel_catalog_json = \"customer-models.json\"\n";
        common::atomic_write(&config_path, original).unwrap();
        common::atomic_write(&external_catalog_path, external_catalog).unwrap();

        let mut prepared = prepare(
            &home,
            "https://yeschoy.com",
            "gpt-5.5",
            CodexTransport::ChatBridge,
        )
        .unwrap();
        prepared.commit().unwrap();
        assert!(std::fs::read_to_string(&config_path)
            .unwrap()
            .contains(&format!("model_catalog_json = \"{OWNED_CATALOG}\"")));
        assert_eq!(
            std::fs::read(&external_catalog_path).unwrap(),
            external_catalog
        );
        // Counting raw `gpt-5.5` occurrences couples this test to the
        // display_name fallback (an unrelated catalog.json entry changes the
        // count). Assert the fields explicitly instead.
        let owned: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&prepared.catalog_path).unwrap())
                .unwrap();
        let entry = &owned["models"][0];
        assert_eq!(entry["slug"], "gpt-5.5");
        assert_eq!(entry["description"], "gpt-5.5");
        assert_eq!(
            entry["display_name"],
            serde_json::json!(crate::tool_model_profile::display_name("gpt-5.5"))
        );
        prepared.rollback().unwrap();
        assert_eq!(std::fs::read(&config_path).unwrap(), original);
        assert_eq!(
            std::fs::read(&external_catalog_path).unwrap(),
            external_catalog
        );
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

    // Explicit fixture paths: never consult CODEX_HOME, the user's settings,
    // Credential Manager, or a running Codex process in these regressions.
    fn inspection_fixture(provider_auth: ProviderAuth) -> (PathBuf, Prepared, String) {
        let home = common::temporary_working_directory("codex-inspection-fixture").unwrap();
        let path = home.join("config.toml");
        let catalog_path = home.join(OWNED_CATALOG);
        let model_ids = vec!["model-a".into(), "model-b".into()];
        let catalog =
            serde_json::to_vec_pretty(&bridge_catalog_models(&model_ids).unwrap()).unwrap();
        let base_url = "https://yeschoy.com/v1";
        let source = render(None, base_url, "model-a", &provider_auth).unwrap();
        std::fs::write(&path, &source).unwrap();
        std::fs::write(&catalog_path, &catalog).unwrap();
        let prepared = Prepared {
            transaction: FileTransaction::default(),
            path,
            catalog_path,
            catalog,
            base_url: base_url.into(),
            model: "model-a".into(),
            model_ids,
            provider_id: "yeschoy".into(),
            provider_auth,
        };
        assert!(prepared.validate_existing().is_ok());
        (home, prepared, String::from_utf8(source).unwrap())
    }

    #[test]
    fn inspection_regression_legacy_helper_profile_requires_reconnect_without_panicking() {
        let (home, mut prepared, original) =
            inspection_fixture(ProviderAuth::Command("synthetic-helper".into()));
        // An old receipt/credential can survive the direct-connect upgrade.
        // Readback expects the new scoped-key field, which the old file lacks.
        prepared.provider_auth = ProviderAuth::ApiKey("sk-synthetic-inspection-key".into());
        let result = prepared.validate_existing();
        assert!(result.is_err());
        assert_eq!(std::fs::read_to_string(&prepared.path).unwrap(), original);
        std::fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn inspection_regression_missing_and_wrong_type_fields_are_read_only_failures() {
        for provider_auth in [
            ProviderAuth::Command("synthetic-helper".into()),
            ProviderAuth::ApiKey("sk-synthetic-inspection-key".into()),
        ] {
            let (home, prepared, original) = inspection_fixture(provider_auth);
            let mut paths = vec![
                vec!["model_provider"],
                vec!["model"],
                vec!["model_catalog_json"],
                vec!["model_providers"],
                vec!["model_providers", "yeschoy"],
                vec!["model_providers", "yeschoy", "base_url"],
                vec!["model_providers", "yeschoy", "wire_api"],
            ];
            match &prepared.provider_auth {
                ProviderAuth::Command(_) => paths.extend([
                    vec!["model_providers", "yeschoy", "auth"],
                    vec!["model_providers", "yeschoy", "auth", "command"],
                    vec!["model_providers", "yeschoy", "auth", "args"],
                ]),
                ProviderAuth::ApiKey(_) => paths.push(vec![
                    "model_providers",
                    "yeschoy",
                    "experimental_bearer_token",
                ]),
            }
            for path in paths {
                for replacement in [
                    None,
                    Some(value(false)),
                    Some(value(123)),
                    Some(value(Array::new())),
                    Some(Item::Table(Table::new())),
                ] {
                    let mut document = original.parse::<DocumentMut>().unwrap();
                    let (key, parents) = path.split_last().unwrap();
                    let mut parent: &mut dyn toml_edit::TableLike = document.as_table_mut();
                    for name in parents {
                        parent = parent.get_mut(name).unwrap().as_table_like_mut().unwrap();
                    }
                    if let Some(replacement) = replacement {
                        parent.insert(key, replacement);
                    } else {
                        parent.remove(key);
                    }
                    let changed = document.to_string();
                    std::fs::write(&prepared.path, &changed).unwrap();
                    assert!(prepared.validate_existing().is_err(), "path={path:?}");
                    assert_eq!(std::fs::read_to_string(&prepared.path).unwrap(), changed);
                    assert_eq!(
                        std::fs::read(&prepared.catalog_path).unwrap(),
                        prepared.catalog
                    );
                }
            }
            for changed in ["", "[invalid", "model_provider = 'other'\n"] {
                std::fs::write(&prepared.path, changed).unwrap();
                assert!(prepared.validate_existing().is_err());
                assert_eq!(std::fs::read_to_string(&prepared.path).unwrap(), changed);
            }
            std::fs::remove_file(&prepared.path).unwrap();
            assert!(prepared.validate_existing().is_err());
            assert!(!prepared.path.exists());
            std::fs::remove_dir_all(home).unwrap();
        }
    }

    #[test]
    fn inspection_regression_registered_model_switch_stays_connected() {
        let (home, prepared, original) =
            inspection_fixture(ProviderAuth::ApiKey("sk-synthetic-inspection-key".into()));
        let mut document = original.parse::<DocumentMut>().unwrap();
        document["model"] = value("model-b");
        let changed = document.to_string();
        std::fs::write(&prepared.path, &changed).unwrap();
        assert!(prepared.validate_existing().is_ok());
        assert!(prepared.validate_readback(true).is_err());
        assert_eq!(std::fs::read_to_string(&prepared.path).unwrap(), changed);
        std::fs::remove_dir_all(home).unwrap();
    }

    #[tokio::test]
    async fn inspection_regression_legacy_readback_does_not_abort_the_worker() {
        static LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
        let (home, mut prepared, original) =
            inspection_fixture(ProviderAuth::Command("synthetic-helper".into()));
        prepared.provider_auth = ProviderAuth::ApiKey("sk-synthetic-inspection-key".into());
        let path = prepared.path.clone();
        let result =
            crate::tool_activation::inspect_on_worker(&LOCK, Duration::from_secs(2), move || {
                prepared.validate_existing()
            })
            .await;
        assert!(matches!(
            result,
            Ok(Err(AdapterFailure::ConfigurationFailed(
                "configuration_readback_failed"
            )))
        ));
        assert!(LOCK.try_lock().is_ok());
        assert_eq!(std::fs::read_to_string(path).unwrap(), original);
        std::fs::remove_dir_all(home).unwrap();
    }

    /// 我们自己写出去的配置，必须被自己认出来。
    ///
    /// 夹具用真正的写入函数造，不手写 TOML —— 手写的夹具只能证明「读的这半
    /// 边自洽」，写的那半边一改（provider 名、键名、嵌套方式）它照样绿，
    /// 而线上的症状是重启应用后 Codex 连不上，用户什么都没改过。
    #[test]
    fn what_we_write_is_what_we_recognise_on_restart() {
        for auth in [
            ProviderAuth::ApiKey(format!("sk-{}", "a".repeat(48))),
            ProviderAuth::Command("/opt/yeschoy/helper".into()),
        ] {
            let written = render(None, PROXY_BASE, "kimi-k3", &auth).unwrap();
            let text = String::from_utf8(written).unwrap();
            assert!(
                config_points_at_our_bridge(&text),
                "没认出自己写的配置：\n{text}"
            );
        }
        // 写进用户自己命名的 provider 也一样 —— 按地址认，不按名字认。
        let (written, provider_id) = render_with_provider(
            None,
            PROXY_BASE,
            "kimi-k3",
            &ProviderAuth::ApiKey("sk-x".into()),
            Some("my-own-name"),
        )
        .unwrap();
        assert_eq!(provider_id, "my-own-name");
        assert!(config_points_at_our_bridge(
            &String::from_utf8(written).unwrap()
        ));
    }

    /// 用户已经不用我们了的时候，**不许**把桥拉起来占住端口。
    ///
    /// 最要紧的是第二条：用户切去别的 provider 之后，我们那条记录通常还
    /// 留在配置里。只要扫全表而不是只看当前生效的那个，就会在用户明确
    /// 切走之后照样绑端口——而他们并没有要我们这么做。
    #[test]
    fn a_config_that_no_longer_points_here_does_not_bring_the_bridge_back() {
        let ours = format!("base_url = \"{PROXY_BASE}\"");
        for (name, text) in [
            (
                "用户改回了直连中转",
                "model_provider = \"yeschoy\"\n\
                 [model_providers.yeschoy]\n\
                 base_url = \"https://yeschoy.com/v1\"\n"
                    .to_owned(),
            ),
            (
                "用户切去了别的 provider，我们那条还留着",
                format!(
                    "model_provider = \"openai\"\n\
                     [model_providers.openai]\n\
                     base_url = \"https://api.openai.com/v1\"\n\
                     [model_providers.yeschoy]\n\
                     {ours}\n"
                ),
            ),
            (
                "没有 model_provider",
                format!("[model_providers.yeschoy]\n{ours}\n"),
            ),
            (
                "生效的 provider 根本不存在",
                format!("model_provider = \"ghost\"\n[model_providers.yeschoy]\n{ours}\n"),
            ),
            (
                "provider 里没有 base_url",
                "model_provider = \"yeschoy\"\n[model_providers.yeschoy]\nwire_api = \"responses\"\n"
                    .to_owned(),
            ),
            ("TOML 坏了", "model_provider = \"yeschoy\"\n[[[".to_owned()),
            ("空文件", String::new()),
            (
                "端口对但路径不对",
                "model_provider = \"yeschoy\"\n\
                 [model_providers.yeschoy]\n\
                 base_url = \"http://127.0.0.1:15731/claude-desktop/v1\"\n"
                    .to_owned(),
            ),
        ] {
            assert!(
                !config_points_at_our_bridge(&text),
                "{name}：不该恢复却恢复了\n{text}"
            );
        }
    }

    /// 内联表写法也要认。
    ///
    /// Codex 自己不这么写，但用户手改过的配置可能是。这里原本先调
    /// `Item::as_table()`，内联表不是 `Item::Table`，于是返回 None ——
    /// 桥**静默地**不恢复，用户只看到「重启之后 Codex 连不上了」。
    #[test]
    fn an_inline_table_config_is_still_recognised() {
        let text = format!(
            "model_provider = \"yeschoy\"\n\
             model_providers = {{ yeschoy = {{ base_url = \"{PROXY_BASE}\", wire_api = \"responses\" }} }}\n"
        );
        assert!(config_points_at_our_bridge(&text), "{text}");
    }
}

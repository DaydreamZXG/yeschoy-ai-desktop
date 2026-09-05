use std::time::{SystemTime, UNIX_EPOCH};

use reqwest::{Method, Url};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::{
    account_v2::{
        billing_groups, ensure_session_epoch, native_account_json, native_session_access,
        native_session_epoch, AccountV2State, NativeSessionFailure,
    },
    claude_bridge::ClaudeTransport,
    codex_bridge::CodexBridgeRuntimeState,
    connection_recovery::{self, Receipt, Store},
    connectivity_core::request_id_is_valid,
    tool_adapters::{
        self, claude_code, claude_desktop, codex_desktop, dsh_web, hermes, openclaw, pi,
        AdapterFailure, ResolvedInstallation,
    },
    tool_credentials::{self, CredentialFailure, ToolCredential},
};

const TOKEN_PAGE_SIZE: &str = "100";
pub(crate) static ACTIVATION_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ToolActivationRequest {
    request_id: String,
    line_id: String,
    tool_id: String,
    model_id: String,
    installation_id: String,
    billing_group: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolActivationProjection {
    request_id: String,
    schema_version: u8,
    status: &'static str,
    tool_id: String,
    model_id: String,
    billing_group: String,
    observed_at_epoch_ms: u64,
    reason_code: &'static str,
}

impl ToolActivationProjection {
    fn new(
        request: &ToolActivationRequest,
        status: &'static str,
        reason_code: &'static str,
    ) -> Self {
        Self {
            request_id: request.request_id.clone(),
            schema_version: 3,
            status,
            tool_id: request.tool_id.clone(),
            model_id: request.model_id.clone(),
            billing_group: request.billing_group.clone(),
            observed_at_epoch_ms: now_epoch_ms(),
            reason_code,
        }
    }
}

#[derive(Clone, Copy, Debug)]
enum ActivationFailure {
    SignedOut,
    UnsupportedModel,
    UnsupportedGroup,
    ServerUnavailable,
    Adapter(AdapterFailure),
    ConfigurationFailed(&'static str),
}

impl ActivationFailure {
    fn projection(self, request: &ToolActivationRequest) -> ToolActivationProjection {
        match self {
            Self::SignedOut => ToolActivationProjection::new(request, "signed_out", "signed_out"),
            Self::UnsupportedModel => {
                ToolActivationProjection::new(request, "unsupported_model", "model_not_available")
            }
            Self::UnsupportedGroup => ToolActivationProjection::new(
                request,
                "unsupported_group",
                "group_not_available_for_model",
            ),
            Self::ServerUnavailable => {
                ToolActivationProjection::new(request, "server_unavailable", "server_unavailable")
            }
            Self::Adapter(error) => match error {
                AdapterFailure::ToolNotFound => {
                    ToolActivationProjection::new(request, "tool_not_found", "tool_not_found")
                }
                AdapterFailure::MultipleInstallations => ToolActivationProjection::new(
                    request,
                    "multiple_installations",
                    "installation_selection_required",
                ),
                AdapterFailure::MissingRuntime => ToolActivationProjection::new(
                    request,
                    "missing_runtime",
                    "required_runtime_not_found",
                ),
                AdapterFailure::UnsupportedProfile => ToolActivationProjection::new(
                    request,
                    "unsupported_profile",
                    "profile_not_supported",
                ),
                AdapterFailure::ExternalOverride => ToolActivationProjection::new(
                    request,
                    "external_override",
                    "higher_precedence_override",
                ),
                AdapterFailure::SecureStorageUnavailable => ToolActivationProjection::new(
                    request,
                    "secure_storage_unavailable",
                    "secure_storage_unavailable",
                ),
                AdapterFailure::ConfigurationFailed(reason) => {
                    ToolActivationProjection::new(request, "configuration_failed", reason)
                }
                AdapterFailure::LaunchFailed => {
                    ToolActivationProjection::new(request, "launch_failed", "tool_launch_failed")
                }
                AdapterFailure::VerificationFailed(reason) => {
                    ToolActivationProjection::new(request, "verification_failed", reason)
                }
            },
            Self::ConfigurationFailed(reason) => {
                ToolActivationProjection::new(request, "configuration_failed", reason)
            }
        }
    }
}

struct TokenLease {
    id: u64,
    key: String,
    created: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ModelTransport {
    Claude(ClaudeTransport),
    Codex(codex_desktop::CodexTransport),
}

fn now_epoch_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u64::MAX as u128) as u64
}

fn bounded_plain_text(value: &str, maximum: usize) -> bool {
    !value.is_empty()
        && value.chars().count() <= maximum
        && !value.chars().any(|character| {
            character.is_control()
                || matches!(character, '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
        })
}

fn request_is_valid(request: &ToolActivationRequest) -> bool {
    request_id_is_valid(&request.request_id)
        && matches!(
            request.line_id.as_str(),
            "mainland_optimized" | "global_accelerated"
        )
        && matches!(
            request.tool_id.as_str(),
            "claude_code"
                | "claude_desktop"
                | "codex_desktop"
                | "pi"
                | "dsh_web"
                | "hermes"
                | "openclaw"
        )
        && bounded_plain_text(&request.model_id, 200)
        && bounded_plain_text(&request.billing_group, 128)
        && request.billing_group != "auto"
        && request.installation_id.len() <= 128
        && request
            .installation_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
}

fn data(value: &Value) -> Option<&Value> {
    let object = value.as_object()?;
    object
        .get("success")?
        .as_bool()?
        .then(|| object.get("data"))
        .flatten()
}

fn server_success(status: u16, value: &Value) -> bool {
    (200..300).contains(&status)
        && value
            .as_object()
            .and_then(|object| object.get("success"))
            .and_then(Value::as_bool)
            == Some(true)
}

async fn validate_model(
    origin: &str,
    access_token: &str,
    model_id: &str,
    tool_id: &str,
    billing_group: &str,
) -> Result<Option<ModelTransport>, ActivationFailure> {
    let (status, value) = native_account_json(
        Method::GET,
        &format!("{origin}/api/user/models"),
        access_token,
        None,
    )
    .await
    .map_err(|_| ActivationFailure::ServerUnavailable)?;
    if matches!(status, 401 | 403) {
        return Err(ActivationFailure::SignedOut);
    }
    if !server_success(status, &value) {
        return Err(ActivationFailure::ServerUnavailable);
    }
    let available = data(&value)
        .and_then(Value::as_array)
        .is_some_and(|models| models.iter().any(|model| model.as_str() == Some(model_id)));
    if !available {
        return Err(ActivationFailure::UnsupportedModel);
    }
    let (pricing_status, pricing) = native_account_json(
        Method::GET,
        &format!("{origin}/api/pricing"),
        access_token,
        None,
    )
    .await
    .map_err(|_| ActivationFailure::ServerUnavailable)?;
    if matches!(pricing_status, 401 | 403) {
        return Err(ActivationFailure::SignedOut);
    }
    if !server_success(pricing_status, &pricing) {
        return Err(ActivationFailure::ServerUnavailable);
    }
    if !billing_groups(&pricing, model_id)
        .iter()
        .any(|g| g.id == billing_group)
    {
        return Err(ActivationFailure::UnsupportedGroup);
    }
    if tool_id == "codex_desktop" {
        codex_transport(&pricing, model_id)
            .map(ModelTransport::Codex)
            .map(Some)
            .ok_or(ActivationFailure::UnsupportedModel)
    } else if matches!(tool_id, "claude_code" | "claude_desktop") {
        claude_transport(&pricing, model_id)
            .map(ModelTransport::Claude)
            .map(Some)
            .ok_or(ActivationFailure::UnsupportedModel)
    } else if model_supports_tool(&pricing, model_id, tool_id) {
        Ok(None)
    } else {
        Err(ActivationFailure::UnsupportedModel)
    }
}

fn model_supports_tool(pricing: &Value, model_id: &str, tool_id: &str) -> bool {
    let required_endpoint = match tool_id {
        "claude_code" | "claude_desktop" => {
            return claude_transport(pricing, model_id).is_some();
        }
        "codex_desktop" => {
            return codex_transport(pricing, model_id).is_some();
        }
        "pi" | "dsh_web" | "hermes" | "openclaw" => "openai",
        _ => return false,
    };
    data(pricing)
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_object)
        .find(|row| row.get("model_name").and_then(Value::as_str) == Some(model_id))
        .and_then(|row| row.get("supported_endpoint_types"))
        .and_then(Value::as_array)
        .is_some_and(|endpoints| {
            endpoints
                .iter()
                .any(|endpoint| endpoint.as_str() == Some(required_endpoint))
        })
}

fn model_endpoints<'a>(pricing: &'a Value, model_id: &str) -> Option<&'a Vec<Value>> {
    data(pricing)
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_object)
        .find(|row| row.get("model_name").and_then(Value::as_str) == Some(model_id))?
        .get("supported_endpoint_types")?
        .as_array()
}

fn claude_transport(pricing: &Value, model_id: &str) -> Option<ClaudeTransport> {
    let endpoints = model_endpoints(pricing, model_id)?;
    if endpoints
        .iter()
        .any(|endpoint| endpoint.as_str() == Some("anthropic"))
    {
        Some(ClaudeTransport::DirectAnthropic)
    } else if endpoints
        .iter()
        .any(|endpoint| endpoint.as_str() == Some("openai"))
    {
        Some(ClaudeTransport::ChatBridge)
    } else {
        None
    }
}

fn codex_transport(pricing: &Value, model_id: &str) -> Option<codex_desktop::CodexTransport> {
    let endpoints = model_endpoints(pricing, model_id)?;
    if endpoints
        .iter()
        .any(|endpoint| endpoint.as_str() == Some("openai-response"))
    {
        Some(codex_desktop::CodexTransport::DirectResponses)
    } else if endpoints
        .iter()
        .any(|endpoint| endpoint.as_str() == Some("openai"))
    {
        Some(codex_desktop::CodexTransport::ChatBridge)
    } else {
        None
    }
}

#[cfg(test)]
fn token_name(tool_id: &str) -> &'static str {
    match tool_id {
        "claude_code" => "野菜API Claude Code",
        "claude_desktop" => "野菜API Claude Desktop",
        "codex_desktop" => "野菜API Codex Desktop",
        "pi" => "野菜API Pi",
        "dsh_web" => "野菜API DSH web",
        "hermes" => "野菜API Hermes",
        "openclaw" => "野菜API OpenClaw",
        _ => "野菜API Desktop",
    }
}

fn token_search_url(origin: &str, name: &str) -> Result<String, ActivationFailure> {
    let mut url = Url::parse(&format!("{origin}/api/token/search"))
        .map_err(|_| ActivationFailure::ServerUnavailable)?;
    url.query_pairs_mut()
        .append_pair("keyword", name)
        .append_pair("p", "1")
        .append_pair("size", TOKEN_PAGE_SIZE);
    Ok(url.to_string())
}

async fn find_token_id(
    api: &impl TokenApi,
    origin: &str,
    name: &str,
    group: &str,
    exact: bool,
) -> Result<Option<u64>, ActivationFailure> {
    let url = token_search_url(origin, name)?;
    let (status, value) = api.request(Method::GET, &url, None).await?;
    if matches!(status, 401 | 403) {
        return Err(ActivationFailure::SignedOut);
    }
    if !server_success(status, &value) {
        return Err(ActivationFailure::ServerUnavailable);
    }
    let id = data(&value)
        .and_then(Value::as_object)
        .and_then(|page| page.get("items"))
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_object)
        .filter(|token| reusable_token(token, name, group, exact))
        .filter_map(|token| token.get("id").and_then(Value::as_u64))
        .max();
    Ok(id)
}

fn token_prefix(tool_id: &str, group: &str) -> String {
    let code = match tool_id {
        "claude_code" => "cc",
        "claude_desktop" => "cd",
        "codex_desktop" => "cx",
        "pi" => "pi",
        "dsh_web" => "ds",
        "hermes" => "hm",
        "openclaw" => "oc",
        _ => "tool",
    };
    // A lookup label only. Always compare the complete group returned by NewAPI.
    let hash = group.bytes().fold(0xcbf29ce484222325u64, |h, b| {
        (h ^ u64::from(b)).wrapping_mul(0x100000001b3)
    });
    format!("野菜API {code}-{hash:016x}")
}

fn reusable_token(
    token: &serde_json::Map<String, Value>,
    name: &str,
    group: &str,
    exact: bool,
) -> bool {
    let actual = token.get("name").and_then(Value::as_str).unwrap_or("");
    let name_matches = if exact {
        actual == name
    } else {
        actual
            .strip_prefix(&format!("{name}-"))
            .is_some_and(|suffix| {
                suffix.len() == 16 && suffix.bytes().all(|c| c.is_ascii_hexdigit())
            })
    };
    let expiry = token
        .get("expired_time")
        .and_then(Value::as_i64)
        .unwrap_or(0);
    name_matches
        && token.get("group").and_then(Value::as_str) == Some(group)
        && token.get("status").and_then(Value::as_i64) == Some(1)
        && (expiry == -1 || expiry > (now_epoch_ms() / 1000) as i64)
}

fn token_request(name: &str, group: &str) -> Value {
    json!({
        "name": name,
        "remain_quota": 0,
        "expired_time": -1,
        "unlimited_quota": true,
        "model_limits_enabled": false,
        "model_limits": "",
        "allow_ips": "",
        "group": group,
        "auto_groups": [],
        "cross_group_retry": false
    })
}

fn normalize_api_key(value: &str) -> Option<String> {
    let value = value.trim();
    if value.len() < 16
        || value.len() > 256
        || value
            .chars()
            .any(|character| character.is_control() || character.is_whitespace())
    {
        return None;
    }
    Some(if value.starts_with("sk-") {
        value.to_owned()
    } else {
        format!("sk-{value}")
    })
}

async fn fetch_token_key(
    api: &impl TokenApi,
    origin: &str,
    id: u64,
) -> Result<String, ActivationFailure> {
    let (status, value) = api
        .request(Method::POST, &format!("{origin}/api/token/{id}/key"), None)
        .await?;
    if matches!(status, 401 | 403) {
        return Err(ActivationFailure::SignedOut);
    }
    if !server_success(status, &value) {
        return Err(ActivationFailure::ServerUnavailable);
    }
    data(&value)
        .and_then(Value::as_object)
        .and_then(|value| value.get("key"))
        .and_then(Value::as_str)
        .and_then(normalize_api_key)
        .ok_or(ActivationFailure::ServerUnavailable)
}

async fn acquire_token(
    origin: &str,
    access_token: &str,
    tool_id: &str,
    group: &str,
) -> Result<TokenLease, ActivationFailure> {
    acquire_token_using(&NativeTokenApi { access_token }, origin, tool_id, group).await
}

trait TokenApi {
    fn request(
        &self,
        method: Method,
        url: &str,
        body: Option<Value>,
    ) -> impl std::future::Future<Output = Result<(u16, Value), ActivationFailure>> + Send;
}

struct NativeTokenApi<'a> {
    access_token: &'a str,
}
impl TokenApi for NativeTokenApi<'_> {
    async fn request(
        &self,
        method: Method,
        url: &str,
        body: Option<Value>,
    ) -> Result<(u16, Value), ActivationFailure> {
        native_account_json(method, url, self.access_token, body)
            .await
            .map_err(|_| ActivationFailure::ServerUnavailable)
    }
}

async fn acquire_token_using(
    api: &impl TokenApi,
    origin: &str,
    tool_id: &str,
    group: &str,
) -> Result<TokenLease, ActivationFailure> {
    let prefix = token_prefix(tool_id, group);
    if let Some(id) = find_token_id(api, origin, &prefix, group, false).await? {
        // Never rewrite an existing token's group, quota, expiry or restrictions.
        return Ok(TokenLease {
            id,
            key: fetch_token_key(api, origin, id).await?,
            created: false,
        });
    }

    let mut nonce = [0u8; 8];
    getrandom::fill(&mut nonce).map_err(|_| ActivationFailure::ServerUnavailable)?;
    let name = format!("{prefix}-{:016x}", u64::from_be_bytes(nonce));

    let (status, value) = api
        .request(
            Method::POST,
            &format!("{origin}/api/token/"),
            Some(token_request(&name, group)),
        )
        .await?;
    if matches!(status, 401 | 403) {
        return Err(ActivationFailure::SignedOut);
    }
    if !server_success(status, &value) {
        return Err(ActivationFailure::ServerUnavailable);
    }
    // Standard NewAPI returns success with no data/key. Read the exact newly
    // created name back, including its group, then use the dedicated key API.
    let id = find_token_id(api, origin, &name, group, true)
        .await?
        .ok_or(ActivationFailure::ServerUnavailable)?;
    let key = match fetch_token_key(api, origin, id).await {
        Ok(key) => key,
        Err(error) => {
            let _ = api
                .request(Method::DELETE, &format!("{origin}/api/token/{id}"), None)
                .await;
            return Err(error);
        }
    };
    Ok(TokenLease {
        id,
        key,
        created: true,
    })
}

async fn delete_created_token(origin: &str, access_token: &str, lease: &TokenLease) {
    if lease.created {
        let _ = native_account_json(
            Method::DELETE,
            &format!("{origin}/api/token/{}", lease.id),
            access_token,
            None,
        )
        .await;
    }
}

enum PreparedAdapter {
    ClaudeCode(claude_code::Prepared),
    ClaudeDesktop(claude_desktop::Prepared),
    CodexDesktop(codex_desktop::Prepared),
    Pi(pi::Prepared),
    DshWeb(dsh_web::Prepared),
    Hermes(hermes::Prepared),
    OpenClaw(openclaw::Prepared),
}

impl PreparedAdapter {
    fn changes(&self) -> &[tool_adapters::common::FileChange] {
        match self {
            Self::ClaudeCode(v) => v.changes(),
            Self::ClaudeDesktop(v) => v.changes(),
            Self::CodexDesktop(v) => v.changes(),
            Self::Pi(v) => v.changes(),
            Self::DshWeb(v) => v.changes(),
            Self::Hermes(v) => v.changes(),
            Self::OpenClaw(v) => v.changes(),
        }
    }

    fn commit(&mut self) -> Result<(), AdapterFailure> {
        match self {
            Self::ClaudeCode(value) => value.commit(),
            Self::ClaudeDesktop(value) => value.commit(),
            Self::CodexDesktop(value) => value.commit(),
            Self::Pi(value) => value.commit(),
            Self::DshWeb(value) => value.commit(),
            Self::Hermes(value) => value.commit(),
            Self::OpenClaw(value) => value.commit(),
        }
    }

    fn rollback(&mut self) -> Result<(), AdapterFailure> {
        match self {
            Self::ClaudeCode(value) => value.rollback(),
            Self::ClaudeDesktop(value) => value.rollback(),
            Self::CodexDesktop(value) => value.rollback(),
            Self::Pi(value) => value.rollback(),
            Self::DshWeb(value) => value.rollback(),
            Self::Hermes(value) => value.rollback(),
            Self::OpenClaw(value) => value.rollback(),
        }
    }

    fn local_gateway_token(&self) -> Option<&str> {
        match self {
            Self::ClaudeCode(value) => value.local_token(),
            Self::ClaudeDesktop(value) => Some(value.local_token()),
            _ => None,
        }
    }
}

fn prepare_adapter(
    request: &ToolActivationRequest,
    origin: &str,
    previous: Option<&ToolCredential>,
    model_transport: Option<ModelTransport>,
) -> Result<PreparedAdapter, AdapterFailure> {
    let home = tool_adapters::user_home()
        .filter(|path| path.is_absolute() && path.is_dir())
        .ok_or(AdapterFailure::ConfigurationFailed("home_unavailable"))?;
    match request.tool_id.as_str() {
        "claude_code" => claude_code::prepare(
            &home,
            origin,
            &request.model_id,
            match model_transport {
                Some(ModelTransport::Claude(value)) => value,
                _ => return Err(AdapterFailure::ConfigurationFailed("invalid_request")),
            },
            previous.and_then(|record| record.local_gateway_token.as_deref()),
        )
        .map(PreparedAdapter::ClaudeCode),
        "claude_desktop" => claude_desktop::prepare(
            &home,
            &request.model_id,
            previous.and_then(|record| record.local_gateway_token.as_deref()),
        )
        .map(PreparedAdapter::ClaudeDesktop),
        "codex_desktop" => codex_desktop::prepare(
            &home,
            origin,
            &request.model_id,
            match model_transport {
                Some(ModelTransport::Codex(value)) => value,
                _ => return Err(AdapterFailure::ConfigurationFailed("invalid_request")),
            },
        )
        .map(PreparedAdapter::CodexDesktop),
        "pi" => pi::prepare(&home, origin, &request.model_id).map(PreparedAdapter::Pi),
        "dsh_web" => {
            dsh_web::prepare(&home, origin, &request.model_id).map(PreparedAdapter::DshWeb)
        }
        "hermes" => hermes::prepare(&home, origin, &request.model_id).map(PreparedAdapter::Hermes),
        "openclaw" => {
            openclaw::prepare(&home, origin, &request.model_id).map(PreparedAdapter::OpenClaw)
        }
        _ => Err(AdapterFailure::ConfigurationFailed("invalid_request")),
    }
}

async fn verify_adapter(
    request: &ToolActivationRequest,
    installation: &ResolvedInstallation,
    credential: &ToolCredential,
    claude_code_runtime: &claude_code::ClaudeCodeRuntimeState,
    claude_runtime: &claude_desktop::ClaudeDesktopRuntimeState,
    dsh_runtime: &dsh_web::DshRuntimeState,
) -> Result<(), AdapterFailure> {
    match request.tool_id.as_str() {
        "claude_code" => {
            claude_code::verify(
                claude_code_runtime,
                installation,
                &request.model_id,
                credential,
            )
            .await
        }
        "claude_desktop" => {
            claude_desktop::verify_and_launch(claude_runtime, installation, credential.clone())
                .await
        }
        "codex_desktop" => codex_desktop::verify_and_launch(installation, credential).await,
        "pi" => pi::verify(installation, &request.model_id).await,
        "dsh_web" => {
            dsh_web::verify_launch_and_keep(
                dsh_runtime,
                installation,
                &credential.api_key,
                &request.model_id,
            )
            .await
        }
        "hermes" => hermes::verify(installation, &request.model_id).await,
        "openclaw" => openclaw::verify(installation, &request.model_id).await,
        _ => Err(AdapterFailure::ConfigurationFailed("invalid_request")),
    }
}

#[allow(clippy::too_many_arguments)] // Explicit transaction/runtime inputs; no hidden mutable global rollback context.
async fn restore_after_failure(
    request: &ToolActivationRequest,
    prepared: &mut PreparedAdapter,
    credential_before: Option<&str>,
    previous_record: Option<ToolCredential>,
    origin: &str,
    access_token: &str,
    lease: &TokenLease,
    claude_code_runtime: &claude_code::ClaudeCodeRuntimeState,
    claude_runtime: &claude_desktop::ClaudeDesktopRuntimeState,
    codex_bridge: &CodexBridgeRuntimeState,
    bridge_started: bool,
) -> Option<AdapterFailure> {
    let rollback_failure = prepared.rollback().err();
    let credential_failure = tool_credentials::restore(&request.tool_id, credential_before).err();
    if request.tool_id == "claude_code" {
        claude_code_runtime.stop().await;
        if previous_record
            .as_ref()
            .and_then(|record| record.claude_transport.as_deref())
            == Some("chat_bridge")
        {
            if let Some(previous) = previous_record.clone() {
                let _ = claude_code_runtime.start(previous).await;
            }
        }
    }
    if request.tool_id == "claude_desktop" {
        claude_runtime.stop().await;
        if let Some(previous) = previous_record.clone() {
            let _ = claude_runtime.start(previous).await;
        }
    }
    if request.tool_id == "codex_desktop" && bridge_started {
        codex_bridge.stop().await;
        if previous_record
            .as_ref()
            .and_then(|record| record.codex_transport.as_deref())
            == Some("chat_bridge")
        {
            let _ = codex_bridge.ensure_started().await;
        }
    }
    delete_created_token(origin, access_token, lease).await;
    if rollback_failure.is_some() {
        Some(AdapterFailure::ConfigurationFailed(
            "configuration_rollback_failed",
        ))
    } else if credential_failure.is_some() {
        Some(AdapterFailure::SecureStorageUnavailable)
    } else {
        None
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ActivationTargetScanRequest {
    request_id: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActivationTargetScanResponse {
    request_id: String,
    schema_version: u8,
    platform: &'static str,
    targets: Vec<tool_adapters::TargetProjection>,
}

#[tauri::command]
pub async fn scan_activation_targets_v1(
    request: ActivationTargetScanRequest,
) -> Result<ActivationTargetScanResponse, String> {
    if !request_id_is_valid(&request.request_id) {
        return Err("invalid_activation_target_scan".into());
    }
    Ok(ActivationTargetScanResponse {
        request_id: request.request_id,
        schema_version: 1,
        platform: crate::tool_discovery::platform_name(),
        targets: tool_adapters::scan_targets().await,
    })
}

#[tauri::command]
pub async fn configure_desktop_tool_v2(
    account_state: tauri::State<'_, AccountV2State>,
    claude_code_runtime: tauri::State<'_, claude_code::ClaudeCodeRuntimeState>,
    claude_runtime: tauri::State<'_, claude_desktop::ClaudeDesktopRuntimeState>,
    dsh_runtime: tauri::State<'_, dsh_web::DshRuntimeState>,
    codex_bridge: tauri::State<'_, CodexBridgeRuntimeState>,
    request: ToolActivationRequest,
) -> Result<ToolActivationProjection, String> {
    if !request_is_valid(&request) {
        return Err("invalid_tool_activation_request".into());
    }

    // Bind the click before waiting for discovery, locks or account bootstrap.
    let session_epoch = match native_session_epoch(&account_state) {
        Ok(epoch) => epoch,
        Err(_) => return Ok(ActivationFailure::ServerUnavailable.projection(&request)),
    };
    let _activation_guard = ACTIVATION_LOCK.lock().await;
    let _process_guard = match connection_recovery::operation_lock() {
        Ok(guard) => guard,
        Err(_) => {
            return Ok(
                ActivationFailure::ConfigurationFailed("recovery_pending").projection(&request)
            )
        }
    };
    // Resolve current identity/runtime before account or token access. An
    // unavailable or ambiguous installation can never create a server token.
    let installation =
        match tool_adapters::resolve_installation(&request.tool_id, &request.installation_id).await
        {
            Ok(value) => value,
            Err(error) => return Ok(ActivationFailure::Adapter(error).projection(&request)),
        };

    let (origin, access_token) =
        match native_session_access(&account_state, &request.line_id, session_epoch).await {
            Ok(value) => value,
            Err(NativeSessionFailure::SignedOut) => {
                return Ok(ActivationFailure::SignedOut.projection(&request))
            }
            Err(NativeSessionFailure::ServerUnavailable) => {
                return Ok(ActivationFailure::ServerUnavailable.projection(&request))
            }
            Err(NativeSessionFailure::AccountChanged) => {
                return Ok(
                    ActivationFailure::ConfigurationFailed("account_changed").projection(&request)
                )
            }
        };
    let model_transport = match validate_model(
        &origin,
        &access_token,
        &request.model_id,
        &request.tool_id,
        &request.billing_group,
    )
    .await
    {
        Ok(value) => value,
        Err(error) => return Ok(error.projection(&request)),
    };

    let credential_before = match tool_credentials::snapshot(&request.tool_id) {
        Ok(value) => value,
        Err(_) => {
            return Ok(
                ActivationFailure::Adapter(AdapterFailure::SecureStorageUnavailable)
                    .projection(&request),
            )
        }
    };
    let previous_record = match tool_credentials::load(&request.tool_id) {
        Ok(value) => Some(value),
        Err(CredentialFailure::Missing) if credential_before.is_none() => None,
        Err(_) => {
            return Ok(
                ActivationFailure::Adapter(AdapterFailure::SecureStorageUnavailable)
                    .projection(&request),
            )
        }
    };

    if ensure_session_epoch(&account_state, session_epoch).is_err() {
        return Ok(ActivationFailure::ConfigurationFailed("account_changed").projection(&request));
    }
    let lease = match acquire_token(
        &origin,
        &access_token,
        &request.tool_id,
        &request.billing_group,
    )
    .await
    {
        Ok(value) => value,
        Err(error) => return Ok(error.projection(&request)),
    };
    if ensure_session_epoch(&account_state, session_epoch).is_err() {
        delete_created_token(&origin, &access_token, &lease).await;
        return Ok(ActivationFailure::ConfigurationFailed("account_changed").projection(&request));
    }
    let mut prepared =
        match prepare_adapter(&request, &origin, previous_record.as_ref(), model_transport) {
            Ok(value) => value,
            Err(error) => {
                delete_created_token(&origin, &access_token, &lease).await;
                return Ok(ActivationFailure::Adapter(error).projection(&request));
            }
        };
    // Persist recovery BEFORE changing credentials or any target file.
    let recovery = match Store::open(true) {
        Ok(Some(value)) => value,
        _ => {
            delete_created_token(&origin, &access_token, &lease).await;
            return Ok(
                ActivationFailure::ConfigurationFailed("recovery_storage_unavailable")
                    .projection(&request),
            );
        }
    };
    let receipt = Receipt {
        tool_id: request.tool_id.clone(),
        model_id: request.model_id.clone(),
        line_id: request.line_id.clone(),
        billing_group: request.billing_group.clone(),
        updated_at_epoch_ms: now_epoch_ms(),
        requires_background: request.tool_id == "claude_desktop"
            || request.tool_id == "dsh_web"
            || matches!(
                model_transport,
                Some(ModelTransport::Claude(ClaudeTransport::ChatBridge))
                    | Some(ModelTransport::Codex(
                        codex_desktop::CodexTransport::ChatBridge
                    ))
            ),
    };
    let mut recovery_record =
        match recovery.begin(receipt, prepared.changes(), previous_record.as_ref()) {
            Ok(record) => record,
            Err(error) => {
                delete_created_token(&origin, &access_token, &lease).await;
                let reason = if error == connection_recovery::Failure::Changed {
                    "recovery_pending"
                } else {
                    "recovery_storage_unavailable"
                };
                return Ok(ActivationFailure::ConfigurationFailed(reason).projection(&request));
            }
        };
    let bridge_started = if model_transport
        == Some(ModelTransport::Codex(
            codex_desktop::CodexTransport::ChatBridge,
        )) {
        match codex_bridge.ensure_started().await {
            Ok(value) => value,
            Err(_) => {
                let _ = recovery.abandon(&recovery_record);
                delete_created_token(&origin, &access_token, &lease).await;
                return Ok(
                    ActivationFailure::Adapter(AdapterFailure::ConfigurationFailed(
                        "local_bridge_unavailable",
                    ))
                    .projection(&request),
                );
            }
        }
    } else {
        false
    };
    let credential = ToolCredential {
        api_key: lease.key.clone(),
        origin: origin.clone(),
        model_id: request.model_id.clone(),
        local_gateway_token: prepared.local_gateway_token().map(str::to_owned),
        codex_transport: match model_transport {
            Some(ModelTransport::Codex(value)) => Some(value.credential_value().to_owned()),
            _ => None,
        },
        claude_transport: match model_transport {
            Some(ModelTransport::Claude(value)) => Some(value.credential_value().to_owned()),
            _ => None,
        },
    };
    if tool_credentials::store(&request.tool_id, &credential).is_err() {
        let _ = recovery.abandon(&recovery_record);
        if bridge_started {
            codex_bridge.stop().await;
        }
        delete_created_token(&origin, &access_token, &lease).await;
        return Ok(
            ActivationFailure::Adapter(AdapterFailure::SecureStorageUnavailable)
                .projection(&request),
        );
    }
    if let Err(error) = prepared.commit() {
        let cleanup = restore_after_failure(
            &request,
            &mut prepared,
            credential_before.as_deref(),
            previous_record.clone(),
            &origin,
            &access_token,
            &lease,
            &claude_code_runtime,
            &claude_runtime,
            &codex_bridge,
            bridge_started,
        )
        .await;
        if cleanup.is_none() {
            let _ = recovery.abandon(&recovery_record);
        }
        return Ok(ActivationFailure::Adapter(cleanup.unwrap_or(error)).projection(&request));
    }
    if let Err(error) = verify_adapter(
        &request,
        &installation,
        &credential,
        &claude_code_runtime,
        &claude_runtime,
        &dsh_runtime,
    )
    .await
    {
        let cleanup = restore_after_failure(
            &request,
            &mut prepared,
            credential_before.as_deref(),
            previous_record.clone(),
            &origin,
            &access_token,
            &lease,
            &claude_code_runtime,
            &claude_runtime,
            &codex_bridge,
            bridge_started,
        )
        .await;
        if cleanup.is_none() {
            let _ = recovery.abandon(&recovery_record);
        }
        return Ok(ActivationFailure::Adapter(cleanup.unwrap_or(error)).projection(&request));
    }

    if recovery.finish(&mut recovery_record).is_err() {
        if request.tool_id == "dsh_web" {
            dsh_runtime.stop().await;
        }
        let cleanup = restore_after_failure(
            &request,
            &mut prepared,
            credential_before.as_deref(),
            previous_record.clone(),
            &origin,
            &access_token,
            &lease,
            &claude_code_runtime,
            &claude_runtime,
            &codex_bridge,
            bridge_started,
        )
        .await;
        if cleanup.is_none() {
            let _ = recovery.abandon(&recovery_record);
        }
        if let Some(error) = cleanup {
            return Ok(ActivationFailure::Adapter(error).projection(&request));
        }
        return Ok(
            ActivationFailure::ConfigurationFailed("recovery_receipt_failed").projection(&request),
        );
    }

    if request.tool_id == "codex_desktop"
        && model_transport
            == Some(ModelTransport::Codex(
                codex_desktop::CodexTransport::DirectResponses,
            ))
    {
        codex_bridge.stop().await;
    }
    if request.tool_id == "claude_code"
        && model_transport == Some(ModelTransport::Claude(ClaudeTransport::DirectAnthropic))
    {
        claude_code_runtime.stop().await;
    }

    Ok(ToolActivationProjection::new(
        &request,
        "ready",
        "tool_request_verified",
    ))
}

const CONNECTION_TOOLS: [&str; 7] = [
    "claude_code",
    "claude_desktop",
    "codex_desktop",
    "pi",
    "dsh_web",
    "hermes",
    "openclaw",
];

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ConnectionRequest {
    request_id: String,
    operation: String,
    tool_id: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionProjection {
    tool_id: String,
    state: &'static str,
    model_id: String,
    line_id: String,
    billing_group: String,
    updated_at_epoch_ms: u64,
    restore_mode: &'static str,
    requires_background: bool,
    reason_code: &'static str,
}

impl ConnectionProjection {
    fn empty(tool: &str) -> Self {
        Self {
            tool_id: tool.into(),
            state: "not_connected",
            model_id: String::new(),
            line_id: String::new(),
            billing_group: String::new(),
            updated_at_epoch_ms: 0,
            restore_mode: "none",
            requires_background: false,
            reason_code: "not_connected",
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionResponse {
    request_id: String,
    schema_version: u8,
    status: &'static str,
    connections: Vec<ConnectionProjection>,
    reason_code: &'static str,
}

fn inspect_connection(
    tool: &str,
    store: std::result::Result<Option<&Store>, ()>,
) -> ConnectionProjection {
    let mut projection = ConnectionProjection::empty(tool);
    let stored = match store {
        Ok(Some(store)) => store.load(tool),
        Ok(None) => Ok(None),
        Err(()) => Err(connection_recovery::Failure::Storage),
    };
    match stored {
        Ok(Some(record)) => {
            projection.state = if record.pending {
                "recovery_pending"
            } else if connection_recovery::configuration_matches(&record) {
                "connected"
            } else {
                "changed"
            };
            projection.restore_mode = if record.original_known {
                "original"
            } else {
                "remove_yeschoy"
            };
            projection.model_id = record.receipt.model_id;
            projection.line_id = record.receipt.line_id;
            projection.billing_group = record.receipt.billing_group;
            projection.updated_at_epoch_ms = record.receipt.updated_at_epoch_ms;
            projection.requires_background = record.receipt.requires_background;
            projection.reason_code = projection.state;
        }
        Err(_) => {
            projection.state = "unavailable";
            projection.reason_code = "recovery_storage_unavailable";
        }
        Ok(None) => match tool_credentials::load(tool) {
            Ok(credential) => {
                projection.state = "legacy";
                projection.restore_mode = "remove_yeschoy";
                projection.model_id = credential.model_id.clone();
                projection.line_id = if credential.origin == "https://api.yeschoy.com" {
                    "global_accelerated"
                } else {
                    "mainland_optimized"
                }
                .into();
                projection.requires_background = needs_background(tool, &credential);
                projection.reason_code = "original_settings_unavailable";
            }
            Err(CredentialFailure::Missing) => {}
            Err(_) => {
                projection.state = "unavailable";
                projection.reason_code = "secure_storage_unavailable";
            }
        },
    }
    projection
}

pub(crate) fn needs_background(tool: &str, credential: &ToolCredential) -> bool {
    matches!(tool, "claude_desktop" | "dsh_web")
        || credential.claude_transport.as_deref() == Some("chat_bridge")
        || credential.codex_transport.as_deref() == Some("chat_bridge")
}

fn legacy_paths(tool: &str) -> Result<Vec<std::path::PathBuf>, AdapterFailure> {
    let home = tool_adapters::user_home()
        .ok_or(AdapterFailure::ConfigurationFailed("home_unavailable"))?;
    Ok(match tool {
        "claude_code" => vec![home.join(".claude/settings.json")],
        "codex_desktop" => vec![
            home.join(".codex/config.toml"),
            home.join(".codex/yeschoy-model-catalog.json"),
        ],
        "claude_desktop" => {
            let (a, b, c, d) = claude_desktop::current_paths(&home)?;
            vec![a, b, c, d]
        }
        "pi" => vec![
            home.join(".pi/agent/models.json"),
            home.join(".pi/agent/settings.json"),
        ],
        "dsh_web" => {
            vec![dsh_web::dsh_home(&home, std::env::var_os("DSH_HOME"))?.join("settings.yaml")]
        }
        "hermes" => {
            vec![hermes::hermes_home(&home, std::env::var_os("HERMES_HOME"))?.join("config.yaml")]
        }
        "openclaw" => vec![openclaw::config_path(&home)?],
        _ => return Err(AdapterFailure::UnsupportedProfile),
    })
}

fn legacy_recovery(store: &Store, tool: &str) -> Result<Option<connection_recovery::Record>, ()> {
    let credential = match tool_credentials::load(tool) {
        Ok(value) => value,
        Err(CredentialFailure::Missing) => return Ok(None),
        Err(_) => return Err(()),
    };
    let mut changes = Vec::new();
    for path in legacy_paths(tool).map_err(|_| ())? {
        let before = tool_adapters::common::snapshot(&path).map_err(|_| ())?;
        if let Some(bytes) = &before {
            let transaction = tool_adapters::common::FileTransaction::stage_with_snapshot(
                path,
                before.clone(),
                bytes.clone(),
            )
            .map_err(|_| ())?;
            changes.extend_from_slice(transaction.changes());
        }
    }
    if changes.is_empty() {
        return Ok(None);
    }
    let receipt = Receipt {
        tool_id: tool.into(),
        model_id: credential.model_id.clone(),
        line_id: if credential.origin == "https://api.yeschoy.com" {
            "global_accelerated"
        } else {
            "mainland_optimized"
        }
        .into(),
        billing_group: String::new(),
        updated_at_epoch_ms: 0,
        requires_background: needs_background(tool, &credential),
    };
    store
        .begin(receipt, &changes, Some(&credential))
        .map(Some)
        .map_err(|_| ())
}

#[tauri::command]
pub async fn manage_tool_connections_v1(
    claude_code_runtime: tauri::State<'_, claude_code::ClaudeCodeRuntimeState>,
    claude_runtime: tauri::State<'_, claude_desktop::ClaudeDesktopRuntimeState>,
    dsh_runtime: tauri::State<'_, dsh_web::DshRuntimeState>,
    codex_bridge: tauri::State<'_, CodexBridgeRuntimeState>,
    request: ConnectionRequest,
) -> Result<ConnectionResponse, String> {
    if !request_id_is_valid(&request.request_id)
        || !matches!(request.operation.as_str(), "inspect" | "restore")
        || !(CONNECTION_TOOLS.contains(&request.tool_id.as_str())
            || (request.operation == "inspect" && request.tool_id.is_empty()))
    {
        return Err("invalid_connection_request".into());
    }
    let _guard = ACTIVATION_LOCK.lock().await;
    let _process_guard = if request.operation == "restore" {
        Some(connection_recovery::operation_lock().map_err(|_| "connection_operation_busy")?)
    } else {
        None
    };
    let store = Store::open(request.operation == "restore");
    let mut status = "ok";
    let mut reason = "local_state";
    if request.operation == "restore" {
        let result = (|| -> Result<bool, ()> {
            let store = store.as_ref().map_err(|_| ())?.as_ref().ok_or(())?;
            let record = store.load(&request.tool_id).map_err(|_| ())?;
            let mut record = match record {
                Some(record) => Some(record),
                None => legacy_recovery(store, &request.tool_id)?,
            };
            let mut kept = false;
            if let Some(record) = &mut record {
                record.pending = true;
                store.save(record).map_err(|_| ())?;
                kept = connection_recovery::restore_files(record).map_err(|_| ())?;
            }
            Ok(kept)
        })();
        if let Ok(kept) = result {
            // Stop only this helper-owned runtime, never the third-party app or
            // another provider. A running app may need to reopen its settings.
            match request.tool_id.as_str() {
                "claude_code" => claude_code_runtime.stop().await,
                "claude_desktop" => claude_runtime.stop().await,
                "codex_desktop" => codex_bridge.stop().await,
                "dsh_web" => dsh_runtime.stop().await,
                _ => {}
            }
            let cleaned = tool_credentials::restore(&request.tool_id, None).is_ok()
                && store
                    .as_ref()
                    .ok()
                    .and_then(|s| s.as_ref())
                    .is_some_and(|s| s.remove(&request.tool_id).is_ok());
            if cleaned {
                status = if kept {
                    "restored_with_changes"
                } else {
                    "restored"
                };
                reason = if kept {
                    "later_changes_preserved"
                } else {
                    "local_settings_restored"
                };
            } else {
                status = "recovery_failed";
                reason = "recovery_cleanup_failed";
            }
        } else {
            status = "recovery_failed";
            reason = "recovery_not_completed";
        }
    }
    let connections = CONNECTION_TOOLS
        .iter()
        .map(|tool| {
            inspect_connection(
                tool,
                match &store {
                    Ok(store) => Ok(store.as_ref()),
                    Err(_) => Err(()),
                },
            )
        })
        .collect();
    Ok(ConnectionResponse {
        request_id: request.request_id,
        schema_version: 1,
        status,
        connections,
        reason_code: reason,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct FakeTokens {
        tokens: std::sync::Mutex<Vec<Value>>,
        requests: std::sync::Mutex<Vec<(Method, String)>>,
        fail_keys: std::sync::atomic::AtomicBool,
    }

    impl TokenApi for FakeTokens {
        async fn request(
            &self,
            method: Method,
            url: &str,
            body: Option<Value>,
        ) -> Result<(u16, Value), ActivationFailure> {
            let path = Url::parse(url).unwrap().path().to_string();
            self.requests
                .lock()
                .unwrap()
                .push((method.clone(), path.clone()));
            let mut tokens = self.tokens.lock().unwrap();
            if method == Method::GET && path == "/api/token/search" {
                return Ok((200, json!({"success":true,"data":{"items":tokens.clone()}})));
            }
            if method == Method::POST && path == "/api/token/" {
                let mut token = body.unwrap();
                assert!(token["name"].as_str().unwrap().len() <= 50);
                assert_ne!(token["group"], "");
                token["id"] = json!(tokens.len() + 1);
                token["status"] = json!(1);
                tokens.push(token);
                // The standard server does not return a key or data here.
                return Ok((200, json!({"success":true,"message":""})));
            }
            let id: u64 = path.split('/').nth(3).unwrap_or("").parse().unwrap_or(0);
            if method == Method::POST && path.ends_with("/key") {
                if self.fail_keys.load(std::sync::atomic::Ordering::Relaxed) {
                    return Err(ActivationFailure::ServerUnavailable);
                }
                assert!(tokens.iter().any(|t| t["id"] == id));
                return Ok((
                    200,
                    json!({"success":true,"data":{"key":format!("synthetic-only-test-token-{id:04}")}}),
                ));
            }
            if method == Method::DELETE {
                tokens.retain(|t| t["id"] != id);
                return Ok((200, json!({"success":true})));
            }
            panic!("unexpected token operation: {method} {path}");
        }
    }

    #[tokio::test]
    async fn token_creation_without_key_reuses_only_the_selected_group_on_replay() {
        let api = FakeTokens::default();
        let first = acquire_token_using(&api, "https://mock.invalid", "pi", "default")
            .await
            .unwrap();
        assert!(first.created);
        let replay = acquire_token_using(&api, "https://mock.invalid", "pi", "default")
            .await
            .unwrap();
        assert!(!replay.created);
        assert_eq!(first.id, replay.id);
        assert_eq!(first.key, replay.key);
        let discounted = acquire_token_using(&api, "https://mock.invalid", "pi", "国模特价分组")
            .await
            .unwrap();
        assert!(discounted.created);
        assert_ne!(first.id, discounted.id);
        let tokens = api.tokens.lock().unwrap();
        assert_eq!(tokens[0]["group"], "default");
        assert_eq!(tokens[1]["group"], "国模特价分组");
        assert!(!api
            .requests
            .lock()
            .unwrap()
            .iter()
            .any(|(method, _)| method == Method::PUT));
    }

    #[tokio::test]
    async fn failed_key_read_removes_only_the_new_token_and_leaves_original_group_unchanged() {
        let api = FakeTokens::default();
        acquire_token_using(&api, "https://mock.invalid", "pi", "default")
            .await
            .unwrap();
        let before = api.tokens.lock().unwrap().clone();
        api.fail_keys
            .store(true, std::sync::atomic::Ordering::Relaxed);
        assert!(
            acquire_token_using(&api, "https://mock.invalid", "pi", "discount")
                .await
                .is_err()
        );
        assert_eq!(*api.tokens.lock().unwrap(), before);
        // Failure while reading an existing key must not delete it.
        assert!(
            acquire_token_using(&api, "https://mock.invalid", "pi", "default")
                .await
                .is_err()
        );
        assert_eq!(*api.tokens.lock().unwrap(), before);
    }

    #[test]
    fn token_identity_checks_real_group_status_and_expiry_not_just_the_label() {
        let prefix = token_prefix("pi", "special");
        let mut token = json!({"name":format!("{prefix}-0123456789abcdef"),"group":"special","status":1,"expired_time":-1});
        assert!(reusable_token(
            token.as_object().unwrap(),
            &prefix,
            "special",
            false
        ));
        token["group"] = json!("default");
        assert!(!reusable_token(
            token.as_object().unwrap(),
            &prefix,
            "special",
            false
        ));
        token["group"] = json!("special");
        token["status"] = json!(2);
        assert!(!reusable_token(
            token.as_object().unwrap(),
            &prefix,
            "special",
            false
        ));
        token["status"] = json!(1);
        token["expired_time"] = json!(1);
        assert!(!reusable_token(
            token.as_object().unwrap(),
            &prefix,
            "special",
            false
        ));
        assert_ne!(prefix, token_prefix("pi", "default"));
    }

    #[test]
    fn request_requires_one_of_seven_exact_targets_and_selection_shape() {
        let valid = ToolActivationRequest {
            request_id: "activation-1".into(),
            line_id: "mainland_optimized".into(),
            tool_id: "pi".into(),
            model_id: "glm-5.3".into(),
            installation_id: "i0123456789abcdef".into(),
            billing_group: "default".into(),
        };
        assert!(request_is_valid(&valid));
        let mut invalid = valid;
        invalid.tool_id = "opencode".into();
        assert!(!request_is_valid(&invalid));
    }

    #[test]
    fn protocol_support_is_target_specific() {
        let pricing = json!({
            "success": true,
            "data": [
                {"model_name": "chat", "supported_endpoint_types": ["openai"]},
                {"model_name": "responses", "supported_endpoint_types": ["openai-response"]},
                {"model_name": "messages", "supported_endpoint_types": ["anthropic"]},
                {"model_name": "both", "supported_endpoint_types": ["openai", "anthropic"]}
            ]
        });
        assert!(model_supports_tool(&pricing, "chat", "pi"));
        assert!(model_supports_tool(&pricing, "chat", "dsh_web"));
        assert!(model_supports_tool(&pricing, "chat", "hermes"));
        assert!(model_supports_tool(&pricing, "chat", "openclaw"));
        assert!(model_supports_tool(&pricing, "responses", "codex_desktop"));
        assert!(model_supports_tool(&pricing, "messages", "claude_code"));
        assert!(model_supports_tool(&pricing, "messages", "claude_desktop"));
        assert!(model_supports_tool(&pricing, "chat", "claude_code"));
        assert!(model_supports_tool(&pricing, "chat", "claude_desktop"));
        assert!(model_supports_tool(&pricing, "chat", "codex_desktop"));
        assert_eq!(
            codex_transport(&pricing, "responses"),
            Some(codex_desktop::CodexTransport::DirectResponses)
        );
        assert_eq!(
            codex_transport(&pricing, "chat"),
            Some(codex_desktop::CodexTransport::ChatBridge)
        );
        assert_eq!(
            claude_transport(&pricing, "messages"),
            Some(ClaudeTransport::DirectAnthropic)
        );
        assert_eq!(
            claude_transport(&pricing, "chat"),
            Some(ClaudeTransport::ChatBridge)
        );
        assert_eq!(
            claude_transport(&pricing, "both"),
            Some(ClaudeTransport::DirectAnthropic)
        );
    }

    #[test]
    fn token_names_are_distinct_per_target() {
        let names = [
            token_name("claude_code"),
            token_name("claude_desktop"),
            token_name("codex_desktop"),
            token_name("pi"),
            token_name("dsh_web"),
            token_name("hermes"),
            token_name("openclaw"),
        ];
        for (index, left) in names.iter().enumerate() {
            assert!(!names[index + 1..].contains(left));
        }
    }
}

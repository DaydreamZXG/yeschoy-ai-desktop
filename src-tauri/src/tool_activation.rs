use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex as StdMutex,
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use reqwest::{Method, Url};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tauri::Emitter;

use crate::{
    account_v2::{
        billing_groups, ensure_session_epoch, native_account_json, native_session_access,
        native_session_epoch, AccountV2State, NativeSessionFailure,
    },
    claude_bridge::ClaudeTransport,
    connection_recovery::{self, Receipt, Store},
    connectivity_core::request_id_is_valid,
    shutdown_coordinator,
    tool_adapters::{
        self, claude_code, claude_desktop, codex_desktop, desktop_lifecycle, dsh_web, pi,
        AdapterFailure, ResolvedInstallation,
    },
    tool_credentials::{self, CredentialFailure, ToolCredential, ToolModelRoute},
};

const TOKEN_PAGE_SIZE: &str = "100";
pub(crate) static ACTIVATION_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
const ACTIVATION_PROGRESS_EVENT: &str = "yeschoy://activation-progress";
const ACTIVATION_PROGRESS_TOTAL: u8 = 7;
const CONNECTION_LOCK_WAIT_TIMEOUT: Duration = Duration::from_secs(8);
// Match the renderer's existing inspection budget, including lock wait and I/O.
const CONNECTION_INSPECTION_TIMEOUT: Duration = Duration::from_secs(15);

#[derive(Default)]
pub(crate) struct ActivationOperationState {
    active: StdMutex<HashMap<String, Arc<ActivationCancellation>>>,
}

#[derive(Default)]
struct ActivationCancellation {
    requested: AtomicBool,
}

impl ActivationCancellation {
    fn request(&self) {
        self.requested.store(true, Ordering::Release);
    }

    fn is_requested(&self) -> bool {
        self.requested.load(Ordering::Acquire)
    }
}

struct ActivationRegistration<'a> {
    request_id: String,
    cancellation: Arc<ActivationCancellation>,
    state: &'a ActivationOperationState,
}

impl Drop for ActivationRegistration<'_> {
    fn drop(&mut self) {
        if let Ok(mut active) = self.state.active.lock() {
            let current = active.get(&self.request_id);
            if current.is_some_and(|value| Arc::ptr_eq(value, &self.cancellation)) {
                active.remove(&self.request_id);
            }
        }
    }
}

impl ActivationOperationState {
    fn begin(&self, request_id: &str) -> Result<ActivationRegistration<'_>, ()> {
        let cancellation = Arc::new(ActivationCancellation::default());
        let mut active = self.active.lock().map_err(|_| ())?;
        // There is one shared activation/configuration transaction. Reject a
        // second click immediately instead of queueing it behind the native
        // lock and making the renderer look frozen.
        if !active.is_empty() {
            return Err(());
        }
        active.insert(request_id.to_owned(), cancellation.clone());
        Ok(ActivationRegistration {
            request_id: request_id.to_owned(),
            cancellation,
            state: self,
        })
    }

    fn cancel(&self, request_id: &str) -> Result<bool, ()> {
        let active = self.active.lock().map_err(|_| ())?;
        Ok(active.get(request_id).is_some_and(|value| {
            value.request();
            true
        }))
    }
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ActivationProgress {
    request_id: String,
    tool_id: String,
    stage: &'static str,
    completed_steps: u8,
    total_steps: u8,
}

fn emit_activation_progress(
    app: &tauri::AppHandle,
    request: &ToolActivationRequest,
    stage: &'static str,
    completed_steps: u8,
) {
    if app
        .emit_to(
            "main",
            ACTIVATION_PROGRESS_EVENT,
            ActivationProgress {
                request_id: request.request_id.clone(),
                tool_id: request.tool_id.clone(),
                stage,
                completed_steps,
                total_steps: ACTIVATION_PROGRESS_TOTAL,
            },
        )
        .is_err()
    {
        log::warn!("tool_activation stage=progress_emit_failed");
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ActivationCancelRequest {
    request_id: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActivationCancelResponse {
    request_id: String,
    status: &'static str,
}

#[tauri::command]
pub fn cancel_tool_activation_v1(
    state: tauri::State<'_, ActivationOperationState>,
    request: ActivationCancelRequest,
) -> Result<ActivationCancelResponse, String> {
    if !request_id_is_valid(&request.request_id) {
        return Err("invalid_activation_cancel_request".into());
    }
    let found = state
        .cancel(&request.request_id)
        .map_err(|_| "activation_state_unavailable")?;
    Ok(ActivationCancelResponse {
        request_id: request.request_id,
        status: if found {
            "cancel_requested"
        } else {
            "not_found"
        },
    })
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ToolActivationRequest {
    request_id: String,
    line_id: String,
    tool_id: String,
    model_id: String,
    installation_id: String,
    billing_group: String,
    #[serde(default)]
    models: Option<Vec<ModelBinding>>,
    #[serde(default)]
    installation_job_id: Option<String>,
    #[serde(default)]
    restart_running_app: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ModelBinding {
    pub(crate) model_id: String,
    pub(crate) billing_group: String,
}

impl ToolActivationRequest {
    fn bindings(&self) -> Vec<ModelBinding> {
        self.models.clone().unwrap_or_else(|| {
            vec![ModelBinding {
                model_id: self.model_id.clone(),
                billing_group: self.billing_group.clone(),
            }]
        })
    }
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
    models: Vec<ModelBinding>,
}

impl ToolActivationProjection {
    fn new(
        request: &ToolActivationRequest,
        status: &'static str,
        reason_code: &'static str,
    ) -> Self {
        Self {
            request_id: request.request_id.clone(),
            schema_version: 4,
            status,
            tool_id: request.tool_id.clone(),
            model_id: request.model_id.clone(),
            billing_group: request.billing_group.clone(),
            observed_at_epoch_ms: now_epoch_ms(),
            reason_code,
            models: request.bindings(),
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
                AdapterFailure::LaunchError(reason) => {
                    ToolActivationProjection::new(request, "launch_failed", reason)
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
    retire_after_commit: Vec<u64>,
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
        )
        && bounded_plain_text(&request.model_id, 200)
        && bounded_plain_text(&request.billing_group, 128)
        && request.billing_group != "auto"
        && valid_model_bindings(
            &request.bindings(),
            &request.model_id,
            &request.billing_group,
        )
        && request.installation_id.len() <= 128
        && request
            .installation_job_id
            .as_ref()
            .is_none_or(|id| request_id_is_valid(id))
        && request
            .installation_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
        && (!request.restart_running_app || desktop_lifecycle::requires_reload(&request.tool_id))
}

fn valid_model_bindings(models: &[ModelBinding], default: &str, group: &str) -> bool {
    let mut ids = std::collections::HashSet::new();
    !models.is_empty()
        && models.len() <= 200
        && models.iter().all(|m| {
            bounded_plain_text(&m.model_id, 200)
                && !m.model_id.contains(',')
                && bounded_plain_text(&m.billing_group, 128)
                && m.billing_group != "auto"
                && ids.insert(m.model_id.as_str())
        })
        && models
            .iter()
            .any(|m| m.model_id == default && m.billing_group == group)
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

async fn validate_models(
    origin: &str,
    access_token: &str,
    models: &[ModelBinding],
    tool_id: &str,
) -> Result<Vec<Option<ModelTransport>>, ActivationFailure> {
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
        .is_some_and(|available| {
            models.iter().all(|binding| {
                available
                    .iter()
                    .any(|m| m.as_str() == Some(&binding.model_id))
            })
        });
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
    models
        .iter()
        .map(|binding| {
            let model_id = binding.model_id.as_str();
            let billing_group = binding.billing_group.as_str();
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
        })
        .collect()
}

fn model_supports_tool(pricing: &Value, model_id: &str, tool_id: &str) -> bool {
    let required_endpoint = match tool_id {
        "claude_code" | "claude_desktop" => {
            return claude_transport(pricing, model_id).is_some();
        }
        "codex_desktop" => {
            return codex_transport(pricing, model_id).is_some();
        }
        "pi" | "dsh_web" => "openai",
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
    // The relay serves the Anthropic protocol for every chat model, so an
    // `anthropic` capability flag is no longer required to connect directly.
    // It remains the preferred evidence when present.
    if endpoints
        .iter()
        .any(|endpoint| matches!(endpoint.as_str(), Some("anthropic") | Some("openai")))
    {
        Some(ClaudeTransport::DirectAnthropic)
    } else {
        None
    }
}

fn codex_transport(pricing: &Value, model_id: &str) -> Option<codex_desktop::CodexTransport> {
    let endpoints = model_endpoints(pricing, model_id)?;
    // Same reasoning as `claude_transport`: the relay converts the Responses
    // protocol for every chat model, so `openai` alone is enough to go direct.
    if endpoints
        .iter()
        .any(|endpoint| matches!(endpoint.as_str(), Some("openai-response") | Some("openai")))
    {
        Some(codex_desktop::CodexTransport::DirectResponses)
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
        _ => "野菜API Desktop",
    }
}

/// 服务端用量日志里的 token 名称反查工具。客户端为每个工具创建独立 token，
/// 名称形如 `野菜API cx-<分组哈希>-<随机后缀>`，因此按工具代码前缀归因。
pub(crate) fn tool_for_token_name(name: &str) -> Option<&'static str> {
    [
        "claude_code",
        "claude_desktop",
        "codex_desktop",
        "pi",
        "dsh_web",
    ]
    .into_iter()
    .find(|tool| name.starts_with(&format!("野菜API {}-", token_code(tool))))
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
    model_ids: &[String],
    exact: bool,
) -> Result<(Option<u64>, Vec<u64>), ActivationFailure> {
    let url = token_search_url(origin, name)?;
    let (status, value) = api.request(Method::GET, &url, None).await?;
    if matches!(status, 401 | 403) {
        return Err(ActivationFailure::SignedOut);
    }
    if !server_success(status, &value) {
        return Err(ActivationFailure::ServerUnavailable);
    }
    let mut owned = data(&value)
        .and_then(Value::as_object)
        .and_then(|page| page.get("items"))
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_object)
        .filter(|token| owned_token(token, name, group, exact))
        .filter_map(|token| {
            token
                .get("id")
                .and_then(Value::as_u64)
                .map(|id| (id, reusable_token(token, name, group, model_ids, exact)))
        })
        .collect::<Vec<_>>();
    owned.sort_unstable_by_key(|(id, _)| *id);
    let selected = owned
        .iter()
        .rev()
        .find_map(|(id, reusable)| reusable.then_some(*id));
    let retire = owned
        .into_iter()
        .filter_map(|(id, _)| (Some(id) != selected).then_some(id))
        .collect();
    Ok((selected, retire))
}

fn token_code(tool_id: &str) -> &'static str {
    match tool_id {
        "claude_code" => "cc",
        "claude_desktop" => "cd",
        "codex_desktop" => "cx",
        "pi" => "pi",
        "dsh_web" => "ds",
        _ => "tool",
    }
}

fn token_prefix(tool_id: &str, group: &str) -> String {
    let code = token_code(tool_id);
    // A lookup label only. Always compare the complete group returned by NewAPI.
    let hash = group.bytes().fold(0xcbf29ce484222325u64, |h, b| {
        (h ^ u64::from(b)).wrapping_mul(0x100000001b3)
    });
    format!("野菜API {code}-{hash:016x}")
}

fn owned_token(
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
    name_matches && token.get("group").and_then(Value::as_str) == Some(group)
}

fn reusable_token(
    token: &serde_json::Map<String, Value>,
    name: &str,
    group: &str,
    model_ids: &[String],
    exact: bool,
) -> bool {
    let expiry = token
        .get("expired_time")
        .and_then(Value::as_i64)
        .unwrap_or(0);
    owned_token(token, name, group, exact)
        && token.get("status").and_then(Value::as_i64) == Some(1)
        && (expiry == -1 || expiry > (now_epoch_ms() / 1000) as i64)
        && token.get("model_limits_enabled").and_then(Value::as_bool) == Some(true)
        && token
            .get("model_limits")
            .and_then(Value::as_str)
            .is_some_and(|actual| {
                let mut actual = actual.split(',').collect::<Vec<_>>();
                let mut expected = model_ids.iter().map(String::as_str).collect::<Vec<_>>();
                actual.sort_unstable();
                expected.sort_unstable();
                !actual.iter().any(|id| id.is_empty()) && actual == expected
            })
}

fn token_request(name: &str, group: &str, model_ids: &[String]) -> Value {
    json!({
        "name": name,
        "remain_quota": 0,
        "expired_time": -1,
        "unlimited_quota": true,
        "model_limits_enabled": true,
        "model_limits": model_ids.join(","),
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
    model_ids: &[String],
) -> Result<TokenLease, ActivationFailure> {
    acquire_token_using(
        &NativeTokenApi { access_token },
        origin,
        tool_id,
        group,
        model_ids,
    )
    .await
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
    model_ids: &[String],
) -> Result<TokenLease, ActivationFailure> {
    let prefix = token_prefix(tool_id, group);
    let (existing, retire_after_commit) =
        find_token_id(api, origin, &prefix, group, model_ids, false).await?;
    if let Some(id) = existing {
        // Reuse only an exact group and model scope; broad legacy keys are
        // retired after the new local transaction commits successfully.
        return Ok(TokenLease {
            id,
            key: fetch_token_key(api, origin, id).await?,
            created: false,
            retire_after_commit,
        });
    }

    let mut nonce = [0u8; 8];
    getrandom::fill(&mut nonce).map_err(|_| ActivationFailure::ServerUnavailable)?;
    let name = format!("{prefix}-{:016x}", u64::from_be_bytes(nonce));

    if shutdown_coordinator::global().is_shutting_down() {
        return Err(ActivationFailure::ConfigurationFailed(
            "assistant_shutting_down",
        ));
    }
    let (status, value) = api
        .request(
            Method::POST,
            &format!("{origin}/api/token/"),
            Some(token_request(&name, group, model_ids)),
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
    let id = find_token_id(api, origin, &name, group, model_ids, true)
        .await?
        .0
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
        retire_after_commit,
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

async fn delete_created_tokens(origin: &str, access_token: &str, leases: &[(String, TokenLease)]) {
    futures::future::join_all(
        leases
            .iter()
            .map(|(_, lease)| delete_created_token(origin, access_token, lease)),
    )
    .await;
}

fn retire_superseded_tokens(origin: &str, access_token: &str, leases: &[(String, TokenLease)]) {
    let ids = leases
        .iter()
        .flat_map(|(_, lease)| lease.retire_after_commit.iter().copied())
        .collect::<std::collections::HashSet<_>>();
    if ids.is_empty() {
        return;
    }
    let origin = origin.to_owned();
    let access_token = access_token.to_owned();
    // Old scoped keys are no longer on the critical path once the new local
    // transaction has committed. Retire them in the background so several
    // bounded DELETE calls cannot hold the application restart for minutes.
    tokio::spawn(async move {
        for id in ids {
            let retired = native_account_json(
                Method::DELETE,
                &format!("{origin}/api/token/{id}"),
                &access_token,
                None,
            )
            .await;
            if !matches!(retired, Ok((status, ref value)) if server_success(status, value)) {
                log::warn!("tool_token_cleanup stage=retire_failed");
            }
        }
    });
}

async fn revoke_owned_tool_tokens(
    origin: &str,
    access_token: &str,
    tool_id: &str,
    credential: &ToolCredential,
) -> bool {
    if credential.models.is_empty() {
        return true;
    }
    let api = NativeTokenApi { access_token };
    let mut grouped = std::collections::BTreeMap::<String, Vec<String>>::new();
    for model in &credential.models {
        grouped
            .entry(model.billing_group.clone())
            .or_default()
            .push(model.model_id.clone());
    }
    let mut complete = true;
    for (group, mut model_ids) in grouped {
        model_ids.sort_unstable();
        model_ids.dedup();
        let prefix = token_prefix(tool_id, &group);
        let ids = match find_token_id(&api, origin, &prefix, &group, &model_ids, false).await {
            Ok((selected, stale)) => selected.into_iter().chain(stale).collect::<Vec<_>>(),
            Err(_) => {
                complete = false;
                continue;
            }
        };
        for id in ids {
            match api
                .request(Method::DELETE, &format!("{origin}/api/token/{id}"), None)
                .await
            {
                Ok((status, value)) if server_success(status, &value) => {}
                _ => complete = false,
            }
        }
    }
    complete
}

fn credential_for_models(
    request: &ToolActivationRequest,
    origin: &str,
    transports: &[Option<ModelTransport>],
    leases: &[(String, TokenLease)],
    previous: Option<&ToolCredential>,
) -> Result<ToolCredential, ActivationFailure> {
    let models = request
        .bindings()
        .iter()
        .zip(transports)
        .map(|(m, transport)| {
            let lease = leases
                .iter()
                .find(|(group, _)| group == &m.billing_group)
                .ok_or(ActivationFailure::ConfigurationFailed("invalid_request"))?;
            Ok(ToolModelRoute {
                model_id: m.model_id.clone(),
                billing_group: m.billing_group.clone(),
                origin: origin.into(),
                api_key: lease.1.key.clone(),
                claude_transport: match transport {
                    Some(ModelTransport::Claude(t)) => Some(t.credential_value().into()),
                    _ => None,
                },
                codex_transport: match transport {
                    Some(ModelTransport::Codex(t)) => Some(t.credential_value().into()),
                    _ => None,
                },
            })
        })
        .collect::<Result<Vec<_>, ActivationFailure>>()?;
    let default = models
        .iter()
        .find(|m| m.model_id == request.model_id)
        .ok_or(ActivationFailure::ConfigurationFailed("invalid_request"))?;
    let local_token = match previous
        .and_then(|p| p.local_gateway_token.as_ref())
        .filter(|v| v.starts_with("ycg-") && v.len() == 68)
    {
        Some(v) => v.clone(),
        None => {
            let mut bytes = [0u8; 32];
            getrandom::fill(&mut bytes).map_err(|_| {
                ActivationFailure::Adapter(AdapterFailure::SecureStorageUnavailable)
            })?;
            format!(
                "ycg-{}",
                bytes.iter().map(|b| format!("{b:02x}")).collect::<String>()
            )
        }
    };
    Ok(ToolCredential {
        model_id: default.model_id.clone(),
        api_key: default.api_key.clone(),
        origin: origin.into(),
        claude_transport: default.claude_transport.clone(),
        codex_transport: default.codex_transport.clone(),
        local_gateway_token: Some(local_token),
        models,
    })
}

enum PreparedAdapter {
    ClaudeCode(claude_code::Prepared),
    ClaudeDesktop(claude_desktop::Prepared),
    CodexDesktop(codex_desktop::Prepared),
    Pi(pi::Prepared),
    DshWeb(dsh_web::Prepared),
}

impl PreparedAdapter {
    fn changes(&self) -> &[tool_adapters::common::FileChange] {
        match self {
            Self::ClaudeCode(v) => v.changes(),
            Self::ClaudeDesktop(v) => v.changes(),
            Self::CodexDesktop(v) => v.changes(),
            Self::Pi(v) => v.changes(),
            Self::DshWeb(v) => v.changes(),
        }
    }

    fn commit(&mut self) -> Result<(), AdapterFailure> {
        match self {
            Self::ClaudeCode(value) => value.commit(),
            Self::ClaudeDesktop(value) => value.commit(),
            Self::CodexDesktop(value) => value.commit(),
            Self::Pi(value) => value.commit(),
            Self::DshWeb(value) => value.commit(),
        }
    }

    fn rollback(&mut self) -> Result<(), AdapterFailure> {
        match self {
            Self::ClaudeCode(value) => value.rollback(),
            Self::ClaudeDesktop(value) => value.rollback(),
            Self::CodexDesktop(value) => value.rollback(),
            Self::Pi(value) => value.rollback(),
            Self::DshWeb(value) => value.rollback(),
        }
    }
}

fn prepare_adapter(
    request: &ToolActivationRequest,
    credential: &ToolCredential,
    model_transport: Option<ModelTransport>,
    codex_provider_hint: Option<&str>,
) -> Result<PreparedAdapter, AdapterFailure> {
    let home = tool_adapters::user_home()
        .filter(|path| path.is_absolute() && path.is_dir())
        .ok_or(AdapterFailure::ConfigurationFailed("home_unavailable"))?;
    let models = credential.model_ids();
    let local = credential.local_gateway_token.as_deref();
    // Each adapter owns its endpoint choice. Claude Code's catalog adapter and
    // Claude Desktop use local pass-throughs; the remaining surfaces point at
    // the relay directly.
    let origin = credential.origin.clone();
    let provider_key = credential.upstream_key();
    match request.tool_id.as_str() {
        "claude_code" => claude_code::prepare_catalog(
            &home,
            &origin,
            &request.model_id,
            match model_transport {
                Some(ModelTransport::Claude(v)) => v,
                _ => return Err(AdapterFailure::ConfigurationFailed("invalid_request")),
            },
            local,
            &models,
        )
        .map(PreparedAdapter::ClaudeCode),
        "claude_desktop" => {
            claude_desktop::prepare_catalog(&home, &request.model_id, local, &models)
                .map(PreparedAdapter::ClaudeDesktop)
        }
        "codex_desktop" => codex_desktop::prepare_catalog_with_provider_hint(
            &home,
            &origin,
            &request.model_id,
            match model_transport {
                Some(ModelTransport::Codex(v)) => v,
                _ => return Err(AdapterFailure::ConfigurationFailed("invalid_request")),
            },
            Some(provider_key),
            &models,
            codex_provider_hint,
        )
        .map(PreparedAdapter::CodexDesktop),
        "pi" => {
            pi::prepare_catalog(&home, &origin, &request.model_id, &models).map(PreparedAdapter::Pi)
        }
        "dsh_web" => dsh_web::prepare_catalog(&home, &origin, &request.model_id, &models)
            .map(PreparedAdapter::DshWeb),
        _ => Err(AdapterFailure::ConfigurationFailed("invalid_request")),
    }
}

async fn start_local_adapter(
    request: &ToolActivationRequest,
    credential: &ToolCredential,
    claude_code_runtime: &claude_code::ClaudeCodeRuntimeState,
    claude_runtime: &claude_desktop::ClaudeDesktopRuntimeState,
) -> Result<(), AdapterFailure> {
    match request.tool_id.as_str() {
        "claude_code" => claude_code_runtime.start(credential.clone()).await,
        "claude_desktop" => claude_runtime.start(credential.clone()).await.map(|_| ()),
        "codex_desktop" => codex_desktop::ensure_credential_ready(credential).await,
        "pi" | "dsh_web" => Ok(()),
        _ => Err(AdapterFailure::ConfigurationFailed("invalid_request")),
    }
}

async fn open_configured_adapter(
    request: &ToolActivationRequest,
    installation: &ResolvedInstallation,
    credential: &ToolCredential,
    dsh_runtime: &dsh_web::DshRuntimeState,
) -> Result<(), AdapterFailure> {
    match request.tool_id.as_str() {
        "claude_desktop" => claude_desktop::launch(&installation.path),
        "codex_desktop" => codex_desktop::launch(&installation.path),
        "dsh_web" => {
            dsh_web::open_existing(dsh_runtime, installation, credential.upstream_key()).await
        }
        "claude_code" | "pi" => {
            let home = tool_adapters::user_home()
                .ok_or(AdapterFailure::ConfigurationFailed("home_unavailable"))?;
            tool_adapters::terminal_launch::launch_async(installation, &request.tool_id, &home).await
        }
        _ => Err(AdapterFailure::ConfigurationFailed("invalid_request")),
    }
}

struct DesktopReloadGuard {
    tool_id: String,
    path: PathBuf,
    closed: bool,
}

impl DesktopReloadGuard {
    fn new(request: &ToolActivationRequest, installation: &ResolvedInstallation) -> Self {
        Self {
            tool_id: request.tool_id.clone(),
            path: installation.path.clone(),
            closed: false,
        }
    }

    fn record_normal_quit(&mut self, closed: bool) {
        self.closed |= closed;
    }

    fn disarm(&mut self) {
        self.closed = false;
    }
}

impl Drop for DesktopReloadGuard {
    fn drop(&mut self) {
        if !self.closed {
            return;
        }
        let result = match self.tool_id.as_str() {
            "claude_desktop" => claude_desktop::launch(&self.path),
            "codex_desktop" => codex_desktop::launch(&self.path),
            _ => Ok(()),
        };
        if result.is_err() {
            log::warn!("desktop_reload stage=rollback_reopen result=failed");
        }
    }
}

async fn stop_helper_runtime(
    tool: &str,
    claude_code_runtime: &claude_code::ClaudeCodeRuntimeState,
    claude_runtime: &claude_desktop::ClaudeDesktopRuntimeState,
    dsh_runtime: &dsh_web::DshRuntimeState,
) {
    match tool {
        "claude_code" => claude_code_runtime.stop().await,
        "claude_desktop" => claude_runtime.stop().await,
        "dsh_web" => dsh_runtime.stop().await,
        // Codex and Pi connect straight to the relay origin.
        _ => {}
    }
}

#[allow(clippy::too_many_arguments)] // Explicit transaction/runtime inputs; no hidden mutable global rollback context.
async fn restore_after_failure(
    request: &ToolActivationRequest,
    prepared: &mut PreparedAdapter,
    recovery_record: &connection_recovery::Record,
    credential_before: Option<&str>,
    previous_record: Option<ToolCredential>,
    origin: &str,
    access_token: &str,
    leases: &[(String, TokenLease)],
    claude_code_runtime: &claude_code::ClaudeCodeRuntimeState,
    claude_runtime: &claude_desktop::ClaudeDesktopRuntimeState,
    dsh_runtime: &dsh_web::DshRuntimeState,
    permit: &shutdown_coordinator::OperationPermit,
) -> Option<AdapterFailure> {
    let rollback_failed = if desktop_lifecycle::requires_reload(&request.tool_id) {
        connection_recovery::restore_attempt(recovery_record).is_err()
    } else {
        prepared.rollback().is_err()
    };
    if rollback_failed {
        // The live files may still reference the new key. Keep both keys and
        // the encrypted pending checkpoint until recovery really succeeds.
        return restoration_failure(true, false);
    }
    let credential_failure = tool_credentials::restore(&request.tool_id, credential_before).err();
    if credential_failure.is_some() {
        return restoration_failure(false, true);
    }
    let runtime_result = permit
        .cancel_safe(async {
            if request.tool_id == "dsh_web" {
                dsh_runtime.stop().await;
            }
            if request.tool_id == "claude_code" {
                claude_code_runtime.stop().await;
                if let Some(previous) = previous_record.clone().filter(|credential| {
                    credential.has_model_set() && !shutdown_coordinator::global().is_shutting_down()
                }) {
                    claude_code_runtime.start(previous).await?;
                }
            }
            if request.tool_id == "claude_desktop" {
                claude_runtime.stop().await;
                if let Some(previous) = previous_record
                    .clone()
                    .filter(|_| !shutdown_coordinator::global().is_shutting_down())
                {
                    claude_runtime.start(previous).await?;
                }
            }
            Ok::<(), AdapterFailure>(())
        })
        .await;
    if matches!(runtime_result, Ok(Err(_))) {
        return Some(AdapterFailure::ConfigurationFailed(
            "previous_connection_runtime_failed",
        ));
    }
    delete_created_tokens(origin, access_token, leases).await;
    None
}

// The durable receipt and old-key retirement must follow startup observation.
// Kept separate so real file fixtures can exercise every completion ordering.
async fn finalize_after_start<Fut>(
    configured: Result<(), AdapterFailure>,
    start: Fut,
    finish: impl FnOnce() -> Result<(), AdapterFailure>,
) -> Result<(), AdapterFailure>
where
    Fut: std::future::Future<Output = Result<(), AdapterFailure>>,
{
    configured?;
    start.await?;
    finish()
}

// Never restore files or revoke credentials while an attempted launch might
// still be using them. A refused/unknown stop leaves the checkpoint intact.
async fn recover_after_stop<Stop, Restore, RestoreFuture>(
    stopped: Stop,
    restore: Restore,
) -> Option<AdapterFailure>
where
    Stop: std::future::Future<Output = Result<(), AdapterFailure>>,
    Restore: FnOnce() -> RestoreFuture,
    RestoreFuture: std::future::Future<Output = Option<AdapterFailure>>,
{
    if stopped.await.is_err() {
        return Some(AdapterFailure::ConfigurationFailed(
            "desktop_recovery_waiting_for_exit",
        ));
    }
    restore().await
}

// A failed restoration happens after writes. Keep it distinct from an initial
// secure-storage failure so the UI cannot claim the settings were untouched.
fn restoration_failure(files_failed: bool, credential_failed: bool) -> Option<AdapterFailure> {
    if files_failed {
        Some(AdapterFailure::ConfigurationFailed(
            "configuration_rollback_failed",
        ))
    } else if credential_failed {
        Some(AdapterFailure::ConfigurationFailed(
            "credential_restore_failed",
        ))
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
#[allow(clippy::too_many_arguments)] // Tauri injects the independently owned runtime states.
pub async fn configure_desktop_tool_v2(
    app: tauri::AppHandle,
    activation_state: tauri::State<'_, ActivationOperationState>,
    account_state: tauri::State<'_, AccountV2State>,
    installation_state: tauri::State<'_, crate::app_installation::AppInstallationState>,
    claude_code_runtime: tauri::State<'_, claude_code::ClaudeCodeRuntimeState>,
    claude_runtime: tauri::State<'_, claude_desktop::ClaudeDesktopRuntimeState>,
    dsh_runtime: tauri::State<'_, dsh_web::DshRuntimeState>,
    request: ToolActivationRequest,
) -> Result<ToolActivationProjection, String> {
    if !request_is_valid(&request) {
        return Err("invalid_tool_activation_request".into());
    }
    let registration = activation_state
        .begin(&request.request_id)
        .map_err(|_| "activation_already_running")?;
    let cancelled = || {
        ActivationFailure::ConfigurationFailed(if registration.cancellation.is_requested() {
            "activation_cancelled"
        } else {
            "assistant_shutting_down"
        })
        .projection(&request)
    };
    emit_activation_progress(&app, &request, "queued", 0);
    // 排障时间线：只记录阶段与耗时，不记录请求内容、密钥或路径。
    let started = std::time::Instant::now();
    macro_rules! stage {
        ($name:literal) => {
            log::info!(
                "activation stage={} tool={} restart={} elapsed_ms={}",
                $name,
                request.tool_id,
                request.restart_running_app,
                started.elapsed().as_millis()
            );
        };
    }
    stage!("enter");
    let permit = match shutdown_coordinator::global().admit_operation() {
        Ok(p) => p,
        Err(_) => return Ok(cancelled()),
    };
    let is_cancelled = || permit.is_cancelled() || registration.cancellation.is_requested();
    let session_epoch = if let Some(id) = request.installation_job_id.as_deref() {
        let intent = crate::app_installation::Intent {
            line_id: request.line_id.clone(),
            model_id: request.model_id.clone(),
            billing_group: request.billing_group.clone(),
            models: request.bindings(),
        };
        match installation_state.claim(
            id,
            &request.tool_id,
            &request.installation_id,
            &intent,
            &account_state,
        ) {
            Ok(epoch) => epoch,
            Err(reason) => {
                return Ok(ActivationFailure::ConfigurationFailed(reason).projection(&request))
            }
        }
    } else {
        match native_session_epoch(&account_state) {
            Ok(epoch) => epoch,
            Err(_) => return Ok(ActivationFailure::ServerUnavailable.projection(&request)),
        }
    };
    let _activation_guard = match permit.cancel_safe(ACTIVATION_LOCK.lock()).await {
        Ok(g) => g,
        Err(_) => return Ok(cancelled()),
    };
    stage!("lock");
    if is_cancelled() {
        return Ok(cancelled());
    }
    emit_activation_progress(&app, &request, "checking_application", 1);
    let _process_guard = match connection_recovery::operation_lock() {
        Ok(g) => g,
        Err(_) => {
            return Ok(
                ActivationFailure::ConfigurationFailed("recovery_pending").projection(&request)
            )
        }
    };
    let installation = match permit
        .cancel_safe(tool_adapters::resolve_installation(
            &request.tool_id,
            &request.installation_id,
        ))
        .await
    {
        Ok(Ok(v)) => v,
        Ok(Err(e)) => return Ok(ActivationFailure::Adapter(e).projection(&request)),
        Err(_) => return Ok(cancelled()),
    };
    if desktop_lifecycle::requires_reload(&request.tool_id) {
        match desktop_lifecycle::is_running(&request.tool_id, &installation.path).await {
            Ok(true) if !request.restart_running_app => {
                return Ok(ToolActivationProjection::new(
                    &request,
                    "application_running",
                    "save_work_before_restart",
                ))
            }
            Ok(_) => {}
            Err(error) => return Ok(ActivationFailure::Adapter(error).projection(&request)),
        }
    }
    let mut reload_guard = DesktopReloadGuard::new(&request, &installation);
    let mut recovery = match Store::open(false) {
        Ok(store) => store,
        Err(_) => {
            return Ok(
                ActivationFailure::ConfigurationFailed("recovery_storage_unavailable")
                    .projection(&request),
            )
        }
    };
    if let Some(store) = recovery.as_ref() {
        let pending = match store.load(&request.tool_id) {
            Ok(record) => record.filter(|record| record.pending),
            Err(_) => {
                return Ok(
                    ActivationFailure::ConfigurationFailed("recovery_storage_unavailable")
                        .projection(&request),
                )
            }
        };
        if let Some(pending) = pending {
            // A pending record may restore application files. Desktop apps
            // must be stopped before that local mutation, using the same
            // explicit save-work consent as a normal update.
            if desktop_lifecycle::requires_reload(&request.tool_id) {
                if request.restart_running_app {
                    match permit
                        .cancel_safe(desktop_lifecycle::quit_for_reconfigure(
                            &request.tool_id,
                            &installation.path,
                        ))
                        .await
                    {
                        Ok(Ok(closed)) => reload_guard.record_normal_quit(closed),
                        Ok(Err(AdapterFailure::LaunchError("graceful_restart_required"))) => {
                            return Ok(ToolActivationProjection::new(
                                &request,
                                "application_running",
                                "graceful_restart_required",
                            ));
                        }
                        Ok(Err(error)) => {
                            return Ok(ActivationFailure::Adapter(error).projection(&request));
                        }
                        Err(_) => return Ok(cancelled()),
                    }
                } else {
                    match desktop_lifecycle::is_running(&request.tool_id, &installation.path).await
                    {
                        Ok(true) => {
                            return Ok(ToolActivationProjection::new(
                                &request,
                                "application_running",
                                "save_work_before_restart",
                            ));
                        }
                        Ok(false) => {}
                        Err(error) => {
                            return Ok(ActivationFailure::Adapter(error).projection(&request));
                        }
                    }
                }
            }
            let reopen_after_recovery = reload_guard.closed;
            reload_guard.disarm();
            if permit
                .cancel_safe(stop_helper_runtime(
                    &request.tool_id,
                    &claude_code_runtime,
                    &claude_runtime,
                    &dsh_runtime,
                ))
                .await
                .is_err()
            {
                return Ok(cancelled());
            }
            let previous = pending.rollback_credential().cloned();
            match store.recover_pending(&request.tool_id, || match previous.as_ref() {
                Some(credential) => tool_credentials::store(&request.tool_id, credential).is_ok(),
                None => tool_credentials::restore(&request.tool_id, None).is_ok(),
            }) {
                Ok(true) => {
                    crate::request_diagnostics::clear(&request.tool_id);
                    if request.tool_id == "claude_code" {
                        if let Some(previous) =
                            previous.clone().filter(ToolCredential::has_model_set)
                        {
                            if claude_code_runtime.start(previous).await.is_err() {
                                reload_guard.disarm();
                                return Ok(ActivationFailure::ConfigurationFailed(
                                    "previous_connection_runtime_failed",
                                )
                                .projection(&request));
                            }
                        }
                    } else if request.tool_id == "claude_desktop" {
                        if let Some(previous) = previous {
                            if claude_runtime.start(previous).await.is_err() {
                                reload_guard.disarm();
                                return Ok(ActivationFailure::ConfigurationFailed(
                                    "previous_connection_runtime_failed",
                                )
                                .projection(&request));
                            }
                        }
                    }
                    reload_guard.record_normal_quit(reopen_after_recovery);
                }
                Ok(false) => {}
                Err(failure) => {
                    use connection_recovery::PendingRecoveryFailure as RecoveryFailure;
                    let reason = match failure {
                        RecoveryFailure::Load => "recovery_storage_unavailable",
                        RecoveryFailure::Files => "configuration_rollback_failed",
                        RecoveryFailure::Credential => "credential_restore_failed",
                        RecoveryFailure::Receipt => "recovery_receipt_failed",
                    };
                    return Ok(ActivationFailure::ConfigurationFailed(reason).projection(&request));
                }
            }
        }
    }
    // Session refresh and token issuance may mutate the account. Never drop
    // these futures on shutdown; finish the bounded request then clean up.
    if is_cancelled() {
        return Ok(cancelled());
    }
    emit_activation_progress(&app, &request, "authenticating", 2);
    let (origin, access_token) =
        match native_session_access(&account_state, &request.line_id, session_epoch).await {
            Ok(v) => v,
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
    let bindings = request.bindings();
    if is_cancelled() {
        return Ok(cancelled());
    }
    emit_activation_progress(&app, &request, "checking_models", 3);
    let transports = match permit
        .cancel_safe(validate_models(
            &origin,
            &access_token,
            &bindings,
            &request.tool_id,
        ))
        .await
    {
        Ok(Ok(v)) => v,
        Ok(Err(e)) => return Ok(e.projection(&request)),
        Err(_) => return Ok(cancelled()),
    };
    let credential_before = match tool_credentials::snapshot(&request.tool_id) {
        Ok(v) => v,
        Err(_) => {
            return Ok(
                ActivationFailure::Adapter(AdapterFailure::SecureStorageUnavailable)
                    .projection(&request),
            )
        }
    };
    let previous_record = match tool_credentials::load(&request.tool_id) {
        Ok(v) => Some(v),
        Err(CredentialFailure::Missing) if credential_before.is_none() => None,
        Err(_) => {
            return Ok(
                ActivationFailure::Adapter(AdapterFailure::SecureStorageUnavailable)
                    .projection(&request),
            )
        }
    };
    stage!("credentials");
    let mut leases = Vec::new();
    emit_activation_progress(&app, &request, "securing_access", 4);
    for binding in &bindings {
        if leases
            .iter()
            .any(|(group, _)| group == &binding.billing_group)
        {
            continue;
        }
        if is_cancelled() || ensure_session_epoch(&account_state, session_epoch).is_err() {
            delete_created_tokens(&origin, &access_token, &leases).await;
            return Ok(if is_cancelled() {
                cancelled()
            } else {
                ActivationFailure::ConfigurationFailed("account_changed").projection(&request)
            });
        }
        let mut group_model_ids = bindings
            .iter()
            .filter(|candidate| candidate.billing_group == binding.billing_group)
            .map(|candidate| candidate.model_id.clone())
            .collect::<Vec<_>>();
        group_model_ids.sort_unstable();
        match acquire_token(
            &origin,
            &access_token,
            &request.tool_id,
            &binding.billing_group,
            &group_model_ids,
        )
        .await
        {
            Ok(lease) => leases.push((binding.billing_group.clone(), lease)),
            Err(e) => {
                delete_created_tokens(&origin, &access_token, &leases).await;
                return Ok(e.projection(&request));
            }
        }
    }
    if is_cancelled() || ensure_session_epoch(&account_state, session_epoch).is_err() {
        delete_created_tokens(&origin, &access_token, &leases).await;
        return Ok(if is_cancelled() {
            cancelled()
        } else {
            ActivationFailure::ConfigurationFailed("account_changed").projection(&request)
        });
    }
    stage!("tokens_start");
    let credential = match credential_for_models(
        &request,
        &origin,
        &transports,
        &leases,
        previous_record.as_ref(),
    ) {
        Ok(v) => v,
        Err(e) => {
            delete_created_tokens(&origin, &access_token, &leases).await;
            return Ok(e.projection(&request));
        }
    };
    stage!("tokens_ready");
    let default_transport = bindings
        .iter()
        .position(|m| m.model_id == request.model_id)
        .and_then(|i| transports[i]);
    let recovery = match recovery.take() {
        Some(store) => store,
        None => match Store::open(true) {
            Ok(Some(store)) => store,
            _ => {
                delete_created_tokens(&origin, &access_token, &leases).await;
                return Ok(
                    ActivationFailure::ConfigurationFailed("recovery_storage_unavailable")
                        .projection(&request),
                );
            }
        },
    };
    if desktop_lifecycle::requires_reload(&request.tool_id) {
        if request.restart_running_app {
            match permit
                .cancel_safe(desktop_lifecycle::quit_for_reconfigure(
                    &request.tool_id,
                    &installation.path,
                ))
                .await
            {
                Ok(Ok(closed)) => reload_guard.record_normal_quit(closed),
                Ok(Err(AdapterFailure::LaunchError("graceful_restart_required"))) => {
                    delete_created_tokens(&origin, &access_token, &leases).await;
                    return Ok(ToolActivationProjection::new(
                        &request,
                        "application_running",
                        "graceful_restart_required",
                    ));
                }
                Ok(Err(error)) => {
                    delete_created_tokens(&origin, &access_token, &leases).await;
                    return Ok(ActivationFailure::Adapter(error).projection(&request));
                }
                Err(_) => {
                    delete_created_tokens(&origin, &access_token, &leases).await;
                    return Ok(cancelled());
                }
            }
        } else {
            match desktop_lifecycle::is_running(&request.tool_id, &installation.path).await {
                Ok(true) => {
                    delete_created_tokens(&origin, &access_token, &leases).await;
                    return Ok(ToolActivationProjection::new(
                        &request,
                        "application_running",
                        "save_work_before_restart",
                    ));
                }
                Ok(false) => {}
                Err(error) => {
                    delete_created_tokens(&origin, &access_token, &leases).await;
                    return Ok(ActivationFailure::Adapter(error).projection(&request));
                }
            }
        }
    }
    stage!("lifecycle");
    // Desktop apps can flush their own config while closing. Snapshot only
    // after the graceful shutdown has completed; otherwise the transaction
    // mistakes that legitimate final write for a competing configuration
    // tool and sends the user into a false "higher precedence" failure.
    emit_activation_progress(&app, &request, "preparing_settings", 5);
    // A pre-0.4.19 receipt may currently show `yeschoy`, while its encrypted
    // original snapshot still records CC Switch's provider identifier. Reuse
    // that identifier so upgrading users get the compatibility fix without
    // first restoring or touching config.toml themselves.
    let codex_provider_hint = if request.tool_id == "codex_desktop" {
        match recovery.load(&request.tool_id) {
            Ok(Some(record)) if record.original_known && !record.pending => record
                .files
                .iter()
                .find(|file| {
                    file.path
                        .file_name()
                        .is_some_and(|name| name == "config.toml")
                })
                .and_then(|file| codex_desktop::provider_id_from_snapshot(file.before.as_deref())),
            Ok(_) => None,
            Err(_) => {
                delete_created_tokens(&origin, &access_token, &leases).await;
                return Ok(
                    ActivationFailure::ConfigurationFailed("recovery_storage_unavailable")
                        .projection(&request),
                );
            }
        }
    } else {
        None
    };
    let mut prepared = match prepare_adapter(
        &request,
        &credential,
        default_transport,
        codex_provider_hint.as_deref(),
    ) {
        Ok(v) => v,
        Err(e) => {
            delete_created_tokens(&origin, &access_token, &leases).await;
            return Ok(ActivationFailure::Adapter(e).projection(&request));
        }
    };
    stage!("prepared");
    let receipt = Receipt {
        tool_id: request.tool_id.clone(),
        model_id: request.model_id.clone(),
        line_id: request.line_id.clone(),
        billing_group: request.billing_group.clone(),
        updated_at_epoch_ms: now_epoch_ms(),
        requires_background: true,
    };
    let mut recovery_record =
        match recovery.begin(receipt, prepared.changes(), previous_record.as_ref()) {
            Ok(v) => v,
            Err(e) => {
                delete_created_tokens(&origin, &access_token, &leases).await;
                return Ok(ActivationFailure::ConfigurationFailed(
                    if e == connection_recovery::Failure::Changed {
                        "recovery_pending"
                    } else {
                        "recovery_storage_unavailable"
                    },
                )
                .projection(&request));
            }
        };
    // Helper-owned runtimes are started by `start_local_adapter` only after
    // both the credential and settings commits have succeeded.
    if is_cancelled() {
        let _ = recovery.abandon(&recovery_record);
        delete_created_tokens(&origin, &access_token, &leases).await;
        return Ok(cancelled());
    }
    // No cancellation point between credential publication and local file commit.
    let reopen_previous = reload_guard.closed;
    // From here failures require an explicit successful rollback, not a Drop
    // that could reopen a partially-written or unrecoverable configuration.
    reload_guard.disarm();
    emit_activation_progress(&app, &request, "applying_settings", 6);
    let local_result = tool_credentials::store(&request.tool_id, &credential)
        .map_err(|_| AdapterFailure::SecureStorageUnavailable)
        .and_then(|()| prepared.commit());
    stage!("committed");
    log::info!(
        "activation stage=local_commit tool={} ok={}",
        request.tool_id,
        local_result.is_ok()
    );
    let configured = match local_result {
        Err(e) => Err(e),
        Ok(()) => match permit
            .cancel_safe(start_local_adapter(
                &request,
                &credential,
                &claude_code_runtime,
                &claude_runtime,
            ))
            .await
        {
            Ok(result) => result,
            Err(_) => Err(AdapterFailure::ConfigurationFailed(
                "assistant_shutting_down",
            )),
        },
    };
    let mut desktop_open_attempted = false;
    let result = finalize_after_start(
        configured,
        async {
            if desktop_lifecycle::requires_reload(&request.tool_id) {
                if is_cancelled() {
                    return Err(AdapterFailure::ConfigurationFailed("activation_cancelled"));
                }
                desktop_open_attempted = true;
                emit_activation_progress(&app, &request, "opening_application", 7);
                emit_activation_progress(&app, &request, "checking_application_started", 7);
                desktop_lifecycle::open_and_wait(&request.tool_id, &installation.path).await?;
            }
            Ok(())
        },
        || {
            if is_cancelled() {
                return Err(AdapterFailure::ConfigurationFailed(
                    if registration.cancellation.is_requested() {
                        "activation_cancelled"
                    } else {
                        "assistant_shutting_down"
                    },
                ));
            }
            if ensure_session_epoch(&account_state, session_epoch).is_err() {
                return Err(AdapterFailure::ConfigurationFailed("account_changed"));
            }
            recovery
                .finish(&mut recovery_record)
                .map_err(|_| AdapterFailure::ConfigurationFailed("recovery_receipt_failed"))
        },
    )
    .await;
    if let Err(error) = result {
        log::warn!(
            "activation stage=rollback tool={} reason={:?}",
            request.tool_id,
            error
        );
        emit_activation_progress(&app, &request, "restoring_settings", 6);
        let cleanup = recover_after_stop(
            async {
                if desktop_open_attempted {
                    desktop_lifecycle::quit_for_reconfigure(&request.tool_id, &installation.path)
                        .await?;
                }
                Ok(())
            },
            || {
                restore_after_failure(
                    &request,
                    &mut prepared,
                    &recovery_record,
                    credential_before.as_deref(),
                    previous_record,
                    &origin,
                    &access_token,
                    &leases,
                    &claude_code_runtime,
                    &claude_runtime,
                    &dsh_runtime,
                    &permit,
                )
            },
        )
        .await;
        if let Some(failure) = cleanup {
            return Ok(ActivationFailure::Adapter(failure).projection(&request));
        }
        if recovery.abandon(&recovery_record).is_err() {
            return Ok(
                ActivationFailure::ConfigurationFailed("recovery_receipt_failed")
                    .projection(&request),
            );
        }
        if (reopen_previous || desktop_open_attempted)
            && !shutdown_coordinator::global().is_shutting_down()
            && desktop_lifecycle::open_and_wait(&request.tool_id, &installation.path)
                .await
                .is_err()
        {
            return Ok(
                ActivationFailure::ConfigurationFailed("previous_app_reopen_failed")
                    .projection(&request),
            );
        }
        return Ok(if desktop_open_attempted {
            let reason = if matches!(error, AdapterFailure::LaunchError(_)) {
                "desktop_start_failed_restored"
            } else {
                "desktop_change_failed_restored"
            };
            ActivationFailure::ConfigurationFailed(reason).projection(&request)
        } else {
            ActivationFailure::Adapter(error).projection(&request)
        });
    }
    let open_result = if desktop_lifecycle::requires_reload(&request.tool_id) {
        Ok(()) // Already opened and observed before the transaction finished.
    } else {
        emit_activation_progress(&app, &request, "opening_application", 7);
        open_configured_adapter(&request, &installation, &credential, &dsh_runtime).await
    };
    stage!("opened");
    log::info!(
        "activation stage=open tool={} ok={}",
        request.tool_id,
        open_result.is_ok()
    );
    // The committed configuration now references only scoped keys. Broad or
    // stale helper-owned predecessors can be retired without risking rollback
    // to a key that was deleted mid-transaction. This cleanup is deliberately
    // scheduled only after the user-facing application open has been tried.
    retire_superseded_tokens(&origin, &access_token, &leases);
    if let Err(error) = open_result {
        let reason = match error {
            AdapterFailure::LaunchError(reason) => reason,
            _ => "configuration_ready_open_failed",
        };
        return Ok(ToolActivationProjection::new(
            &request,
            "launch_failed",
            reason,
        ));
    }
    emit_activation_progress(&app, &request, "complete", 7);
    Ok(ToolActivationProjection::new(
        &request,
        "ready",
        if desktop_lifecycle::requires_reload(&request.tool_id) {
            "desktop_start_observed"
        } else {
            "configuration_ready"
        },
    ))
}

const CONNECTION_TOOLS: [&str; 5] = [
    "claude_code",
    "claude_desktop",
    "codex_desktop",
    "pi",
    "dsh_web",
];

/// Called only by the exit owner after admission closes and active operations
/// drain. Blocking storage work stays off the UI/async executor. No remote keys
/// are revoked, and missing originals fail visibly instead of inventing them.
pub(crate) async fn restore_connection_for_exit(tool: &'static str) -> Result<(), ()> {
    let _guard = tokio::time::timeout(CONNECTION_LOCK_WAIT_TIMEOUT, ACTIVATION_LOCK.lock())
        .await.map_err(|_| ())?;
    let needs_restore = tokio::time::timeout(CONNECTION_INSPECTION_TIMEOUT, tokio::task::spawn_blocking(move || {
        let store = Store::open(false).map_err(|_| ())?;
        let projection = inspect_connection(tool, Ok(store.as_ref()));
        match projection.state {
            "not_connected" => Ok(false),
            "unavailable" | "legacy" => Err(()),
            _ if projection.restore_mode != "original" => Err(()),
            _ => Ok(true),
        }
    })).await.map_err(|_| ())?.map_err(|_| ())??;
    if !needs_restore { return Ok(()); }
    crate::exit_restore::after_normal_close(async {
        if desktop_lifecycle::requires_reload(tool) {
            // Multiple installations are ambiguous: never guess which to close.
            let installation = tool_adapters::resolve_installation(tool, "").await.map_err(|_| ())?;
            desktop_lifecycle::quit_for_exit_restore(tool, &installation.path).await.map_err(|_| ())?;
        }
        Ok(())
    }, || async {
        tokio::task::spawn_blocking(move || {
            let _process_guard = connection_recovery::operation_lock().map_err(|_| ())?;
            let store = Store::open(false).map_err(|_| ())?.ok_or(())?;
            let Some(mut record) = store.load(tool).map_err(|_| ())? else { return Ok(()); };
            if !record.original_known { return Err(()); }
            record.pending = true;
            store.save(&record).map_err(|_| ())?;
            connection_recovery::restore_files(&record).map_err(|_| ())?;
            tool_credentials::restore(tool, None).map_err(|_| ())?;
            store.remove(tool).map_err(|_| ())?;
            crate::request_diagnostics::clear(tool);
            Ok(())
        }).await.map_err(|_| ())?
    }).await
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ConnectionRequest {
    request_id: String,
    operation: String,
    tool_id: String,
    /// PRD 9.1: deleting a tool's local configuration must separately ask
    /// whether its dedicated keys are revoked too. Older frontends omit the
    /// field and keep the historical revoke-by-default behavior.
    revoke_tokens: Option<bool>,
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
    models: Vec<ModelBinding>,
    #[serde(skip_serializing_if = "Option::is_none")]
    last_request: Option<crate::request_diagnostics::RequestObservation>,
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
            models: vec![],
            last_request: None,
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
    // Keychain/Credential Manager reads can block. Reuse one read per tool for
    // both legacy detection and the model projection.
    let credential = tool_credentials::load(tool);
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
            } else if connection_recovery::configuration_matches(&record)
                && !connection_recovery::requires_gateway_migration(tool, &record)
            {
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
        Ok(None) => match &credential {
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
                projection.requires_background = needs_background(tool, credential);
                projection.reason_code = "original_settings_unavailable";
            }
            Err(CredentialFailure::Missing) => {}
            Err(_) => {
                projection.state = "unavailable";
                projection.reason_code = "secure_storage_unavailable";
            }
        },
    }
    if let Ok(credential) = &credential {
        projection.models = credential
            .models
            .iter()
            .map(|m| ModelBinding {
                model_id: m.model_id.clone(),
                billing_group: m.billing_group.clone(),
            })
            .collect();
        if projection.models.is_empty()
            && !projection.model_id.is_empty()
            && !projection.billing_group.is_empty()
        {
            projection.models.push(ModelBinding {
                model_id: projection.model_id.clone(),
                billing_group: projection.billing_group.clone(),
            });
        }
        if projection.state == "changed"
            && credential.has_model_set()
            && tool_adapters::user_home().is_some_and(|home| {
                crate::open_connection::validate_settings(&home, tool, credential).is_ok()
            })
        {
            projection.state = "connected";
            projection.reason_code = "connected";
        }
        projection.requires_background = needs_background(tool, credential);
    }
    projection.last_request = crate::request_diagnostics::latest(tool);
    projection
}

pub(crate) async fn inspect_on_worker<T: Send + 'static>(
    lock: &'static tokio::sync::Mutex<()>,
    deadline: Duration,
    inspect: impl FnOnce() -> T + Send + 'static,
) -> Result<T, &'static str> {
    tokio::time::timeout(deadline, async move {
        let guard = tokio::time::timeout(CONNECTION_LOCK_WAIT_TIMEOUT, lock.lock())
            .await
            .map_err(|_| "connection_operation_busy")?;
        // Same blocking-worker boundary as CC Switch's provider commands. Keep
        // the write lock in the worker even if the caller times out: an OS read
        // cannot be cancelled safely, and must not overlap configuration writes.
        tauri::async_runtime::spawn_blocking(move || {
            let _guard = guard;
            inspect()
        })
        .await
        .map_err(|_| "connection_inspection_failed")
    })
    .await
    .map_err(|_| "connection_inspect_timed_out")?
}

fn inspect_connections(store: Result<Option<&Store>, ()>) -> Vec<ConnectionProjection> {
    inspect_connections_with(|tool| inspect_connection(tool, store))
}

fn inspect_connections_with(
    mut inspect: impl FnMut(&str) -> ConnectionProjection,
) -> Vec<ConnectionProjection> {
    CONNECTION_TOOLS
        .iter()
        .map(|tool| {
            // A single adapter must not discard the other six local results.
            // No writes/recovery take place here; preserve an unknown state
            // for the failed tool and never expose the raw panic payload.
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| inspect(tool))).unwrap_or_else(
                |_| {
                    let mut projection = ConnectionProjection::empty(tool);
                    projection.state = "unavailable";
                    projection.reason_code = "connection_inspection_failed";
                    projection
                },
            )
        })
        .collect()
}

pub(crate) fn needs_background(tool: &str, credential: &ToolCredential) -> bool {
    credential.has_model_set()
        || matches!(tool, "claude_desktop" | "dsh_web")
        || credential.claude_transport.as_deref() == Some("chat_bridge")
        || credential.codex_transport.as_deref() == Some("chat_bridge")
}

fn legacy_paths(tool: &str) -> Result<Vec<std::path::PathBuf>, AdapterFailure> {
    let home = tool_adapters::user_home()
        .ok_or(AdapterFailure::ConfigurationFailed("home_unavailable"))?;
    Ok(match tool {
        "claude_code" => vec![home.join(".claude/settings.json")],
        "codex_desktop" => {
            let config_dir = codex_desktop::config_dir(&home)?;
            vec![
                config_dir.join("config.toml"),
                config_dir.join("yeschoy-model-catalog.json"),
            ]
        }
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
    account_state: tauri::State<'_, AccountV2State>,
    claude_code_runtime: tauri::State<'_, claude_code::ClaudeCodeRuntimeState>,
    claude_runtime: tauri::State<'_, claude_desktop::ClaudeDesktopRuntimeState>,
    dsh_runtime: tauri::State<'_, dsh_web::DshRuntimeState>,
    request: ConnectionRequest,
) -> Result<ConnectionResponse, String> {
    if !request_id_is_valid(&request.request_id)
        || !matches!(request.operation.as_str(), "inspect" | "restore")
        || !(CONNECTION_TOOLS.contains(&request.tool_id.as_str())
            || (request.operation == "inspect" && request.tool_id.is_empty()))
    {
        return Err("invalid_connection_request".into());
    }
    let permit = shutdown_coordinator::global()
        .admit_operation()
        .map_err(|_| "assistant_shutting_down")?;
    if request.operation == "inspect" {
        let started = std::time::Instant::now();
        let result = permit
            .cancel_safe(inspect_on_worker(
                &ACTIVATION_LOCK,
                CONNECTION_INSPECTION_TIMEOUT,
                || {
                    let store = Store::open(false);
                    inspect_connections(match &store {
                        Ok(store) => Ok(store.as_ref()),
                        Err(_) => Err(()),
                    })
                },
            ))
            .await
            .unwrap_or(Err("assistant_shutting_down"));
        let connections = result.map_err(|code| {
            // Never log raw storage errors, paths, account data or credentials.
            log::warn!(
                "connection_inspection request_id={} code={} elapsed_ms={}",
                request.request_id,
                code,
                started.elapsed().as_millis()
            );
            code.to_string()
        })?;
        for connection in connections.iter().filter(|c| c.state == "unavailable") {
            log::warn!(
                "connection_inspection request_id={} code=connection_partial_unavailable tool_id={} reason={}",
                request.request_id,
                connection.tool_id,
                connection.reason_code
            );
        }
        return Ok(ConnectionResponse {
            request_id: request.request_id,
            schema_version: 2,
            status: "ok",
            connections,
            reason_code: "local_state",
        });
    }
    let _guard = tokio::time::timeout(
        CONNECTION_LOCK_WAIT_TIMEOUT,
        permit.cancel_safe(ACTIVATION_LOCK.lock()),
    )
    .await
    .map_err(|_| "connection_operation_busy")?
    .map_err(|_| "assistant_shutting_down")?;
    let _process_guard = if request.operation == "restore" {
        Some(connection_recovery::operation_lock().map_err(|_| "connection_operation_busy")?)
    } else {
        None
    };
    let store = Store::open(request.operation == "restore");
    let mut status = "ok";
    let mut reason = "local_state";
    if request.operation == "restore" {
        let credential_for_cleanup = tool_credentials::load(&request.tool_id).ok();
        let session_epoch = native_session_epoch(&account_state).ok();
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
            let _ = permit
                .cancel_safe(async {
                    stop_helper_runtime(
                        &request.tool_id,
                        &claude_code_runtime,
                        &claude_runtime,
                        &dsh_runtime,
                    )
                    .await;
                })
                .await;
            // Remote key handling comes before the local credential cleanup:
            // when revocation is requested but cannot complete, the credential
            // and the pending receipt (already saved above) stay in place so
            // the restore action remains available as the retry entry.
            let revoke_tokens = request.revoke_tokens.unwrap_or(true);
            let token_cleanup_complete = if !revoke_tokens {
                true
            } else {
                match (credential_for_cleanup.as_ref(), session_epoch) {
                    (Some(credential), Some(epoch)) => {
                        match native_session_access(
                            &account_state,
                            if credential.origin == "https://api.yeschoy.com" {
                                "global_accelerated"
                            } else {
                                "mainland_optimized"
                            },
                            epoch,
                        )
                        .await
                        {
                            Ok((origin, access_token)) => {
                                revoke_owned_tool_tokens(
                                    &origin,
                                    &access_token,
                                    &request.tool_id,
                                    credential,
                                )
                                .await
                            }
                            Err(_) => false,
                        }
                    }
                    (None, _) => true,
                    _ => false,
                }
            };
            if revoke_tokens && !token_cleanup_complete {
                status = if kept {
                    "restored_with_changes"
                } else {
                    "restored"
                };
                reason = "local_settings_restored_token_cleanup_pending";
            } else {
                // Complete the local file/key transaction.
                let cleaned = tool_credentials::restore(&request.tool_id, None).is_ok()
                    && store
                        .as_ref()
                        .ok()
                        .and_then(|s| s.as_ref())
                        .is_some_and(|s| s.remove(&request.tool_id).is_ok());
                crate::request_diagnostics::clear(&request.tool_id);
                if cleaned {
                    status = if kept {
                        "restored_with_changes"
                    } else {
                        "restored"
                    };
                    reason = if !revoke_tokens {
                        "local_settings_restored_token_kept"
                    } else if kept {
                        "later_changes_preserved"
                    } else {
                        "local_settings_restored"
                    };
                } else {
                    status = "recovery_failed";
                    reason = "recovery_cleanup_failed";
                }
            }
        } else {
            status = "recovery_failed";
            reason = "recovery_not_completed";
        }
    }
    let connections = inspect_connections(match &store {
        Ok(store) => Ok(store.as_ref()),
        Err(_) => Err(()),
    });
    Ok(ConnectionResponse {
        request_id: request.request_id,
        schema_version: 2,
        status,
        connections,
        reason_code: reason,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn refused_stop_does_not_restore_files_or_keys_or_delete_live_tokens() {
        assert!(matches!(
            recover_after_stop(
                async { Err(AdapterFailure::LaunchError("graceful_restart_required")) },
                || async { panic!("must not change live files or revoke live keys") }
            )
            .await,
            Some(AdapterFailure::ConfigurationFailed(
                "desktop_recovery_waiting_for_exit"
            ))
        ));
        assert!(matches!(
            recover_after_stop(async { Ok(()) }, || async {
                Some(AdapterFailure::ConfigurationFailed(
                    "credential_restore_failed",
                ))
            })
            .await,
            Some(AdapterFailure::ConfigurationFailed(
                "credential_restore_failed"
            ))
        ));
        assert!(recover_after_stop(async { Ok(()) }, || async { None })
            .await
            .is_none());
    }

    #[tokio::test]
    async fn desktop_receipt_cannot_complete_before_startup_observation() {
        use std::cell::RefCell;
        for startup_ok in [false, true] {
            let events = RefCell::new(Vec::new());
            let result = finalize_after_start(
                Ok(()),
                async {
                    events.borrow_mut().push("start-observed");
                    if startup_ok {
                        Ok(())
                    } else {
                        Err(AdapterFailure::LaunchError("desktop_start_unconfirmed"))
                    }
                },
                || {
                    events.borrow_mut().push("finish-receipt");
                    Ok(())
                },
            )
            .await;
            assert_eq!(result.is_ok(), startup_ok);
            assert_eq!(
                *events.borrow(),
                if startup_ok {
                    vec!["start-observed", "finish-receipt"]
                } else {
                    vec!["start-observed"]
                }
            );
        }
        assert!(finalize_after_start(
            Err(AdapterFailure::ConfigurationFailed(
                "synthetic-write-failed"
            )),
            async { panic!("must not launch after a failed configuration") },
            || panic!("must not finish after a failed configuration")
        )
        .await
        .is_err());
        assert!(finalize_after_start(Ok(()), async { Ok(()) }, || Err(
            AdapterFailure::ConfigurationFailed("activation_cancelled")
        ))
        .await
        .is_err());
    }

    #[tokio::test]
    async fn inspection_regression_one_adapter_panic_keeps_four_other_results() {
        static LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
        let result = inspect_on_worker(&LOCK, Duration::from_secs(2), || {
            inspect_connections_with(|tool| {
                if tool == "codex_desktop" {
                    panic!("synthetic-private-value-must-not-be-in-response");
                }
                let mut projection = ConnectionProjection::empty(tool);
                if tool == "claude_desktop" {
                    projection.state = "connected";
                    projection.reason_code = "connected";
                }
                projection
            })
        })
        .await
        .unwrap();
        assert_eq!(result.len(), 5);
        assert_eq!(result[1].state, "connected");
        assert_eq!(result[2].state, "unavailable");
        assert_eq!(result[2].reason_code, "connection_inspection_failed");
        assert_eq!(
            result.iter().filter(|c| c.state == "not_connected").count(),
            3
        );
        assert!(!serde_json::to_string(&result)
            .unwrap()
            .contains("synthetic-private"));
        assert!(LOCK.try_lock().is_ok());
        let next = inspect_on_worker(&LOCK, Duration::from_secs(2), || {
            inspect_connections_with(ConnectionProjection::empty)
        })
        .await
        .unwrap();
        assert!(next.iter().all(|c| c.state == "not_connected"));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn ru076_inspection_worker_keeps_runtime_responsive_and_write_lock_until_read_finishes() {
        static LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        let (finish_tx, finish_rx) = std::sync::mpsc::channel();
        let task = tokio::spawn(inspect_on_worker(
            &LOCK,
            Duration::from_secs(2),
            move || {
                started_tx.send(()).unwrap();
                finish_rx.recv_timeout(Duration::from_secs(2)).unwrap();
                7
            },
        ));
        started_rx.await.unwrap();
        // If the synchronous reader ran on this current-thread executor, we
        // could not reach this assertion or send its completion signal.
        assert!(LOCK.try_lock().is_err());
        tokio::task::yield_now().await;
        finish_tx.send(()).unwrap();
        assert_eq!(task.await.unwrap(), Ok(7));
        assert!(LOCK.try_lock().is_ok());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn ru076_timed_out_reader_does_not_release_write_lock_or_queue_another_os_read() {
        static LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        let (finish_tx, finish_rx) = std::sync::mpsc::channel();
        let task = tokio::spawn(inspect_on_worker(
            &LOCK,
            Duration::from_millis(80),
            move || {
                started_tx.send(()).unwrap();
                finish_rx.recv_timeout(Duration::from_secs(2)).unwrap();
            },
        ));
        started_rx.await.unwrap();
        assert_eq!(task.await.unwrap(), Err("connection_inspect_timed_out"));
        assert!(LOCK.try_lock().is_err());
        let invoked = Arc::new(AtomicBool::new(false));
        let worker_invoked = invoked.clone();
        assert_eq!(
            inspect_on_worker(&LOCK, Duration::from_millis(20), move || {
                worker_invoked.store(true, Ordering::SeqCst);
            })
            .await,
            Err("connection_inspect_timed_out")
        );
        assert!(!invoked.load(Ordering::SeqCst));
        finish_tx.send(()).unwrap();
        let _guard = tokio::time::timeout(Duration::from_secs(2), LOCK.lock())
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn ru076_panicking_reader_has_a_safe_failure_and_releases_the_lock() {
        static LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
        let result = inspect_on_worker(&LOCK, Duration::from_secs(2), || {
            panic!("synthetic private storage detail")
        })
        .await;
        assert_eq!(result, Err("connection_inspection_failed"));
        assert!(LOCK.try_lock().is_ok());
    }

    #[test]
    fn ru042_activation_models_require_explicit_unique_binding_and_default_member() {
        let json = json!({"requestId":"fixture", "lineId":"mainland_optimized", "toolId":"pi", "modelId":"a", "billingGroup":"cheap", "installationId":"i0123456789abcdef"});
        let legacy: ToolActivationRequest = serde_json::from_value(json.clone()).unwrap();
        assert!(request_is_valid(&legacy));
        let mut modern = json;
        modern["models"] = json!([{"modelId":"a","billingGroup":"cheap"},{"modelId":"b","billingGroup":"standard"}]);
        let r: ToolActivationRequest = serde_json::from_value(modern.clone()).unwrap();
        assert!(request_is_valid(&r));
        let leases = vec![
            (
                "cheap".into(),
                TokenLease {
                    id: 1,
                    key: "synthetic-key-cheap".into(),
                    created: false,
                    retire_after_commit: vec![],
                },
            ),
            (
                "standard".into(),
                TokenLease {
                    id: 2,
                    key: "synthetic-key-standard".into(),
                    created: false,
                    retire_after_commit: vec![],
                },
            ),
        ];
        let c =
            credential_for_models(&r, "https://yeschoy.com", &[None, None], &leases, None).unwrap();
        assert_eq!(
            c.resolve_model("b").unwrap().api_key,
            "synthetic-key-standard"
        );
        let c2 = credential_for_models(&r, "https://yeschoy.com", &[None, None], &leases, Some(&c))
            .unwrap();
        assert_eq!(c.local_gateway_token, c2.local_gateway_token);
        let out = serde_json::to_value(ToolActivationProjection::new(
            &r,
            "ready",
            "tool_request_verified",
        ))
        .unwrap();
        assert_eq!(out["schemaVersion"], 4);
        assert_eq!(out["models"].as_array().unwrap().len(), 2);
        assert!(!out.to_string().contains("synthetic-key"));
        for bad in [
            json!([]),
            json!([{"modelId":"b","billingGroup":"standard"}]),
            json!([{"modelId":"a","billingGroup":"cheap"},{"modelId":"a","billingGroup":"standard"}]),
        ] {
            modern["models"] = bad;
            assert!(!request_is_valid(
                &serde_json::from_value(modern.clone()).unwrap()
            ));
        }
    }

    #[test]
    fn restoration_errors_never_masquerade_as_initial_storage_failure() {
        assert_eq!(restoration_failure(false, false), None);
        assert_eq!(
            restoration_failure(true, false),
            Some(AdapterFailure::ConfigurationFailed(
                "configuration_rollback_failed"
            ))
        );
        assert_eq!(
            restoration_failure(false, true),
            Some(AdapterFailure::ConfigurationFailed(
                "credential_restore_failed"
            ))
        );
        assert_eq!(
            restoration_failure(true, true),
            Some(AdapterFailure::ConfigurationFailed(
                "configuration_rollback_failed"
            ))
        );
    }

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
        let default_models = vec!["model-a".to_string()];
        let first = acquire_token_using(
            &api,
            "https://mock.invalid",
            "pi",
            "default",
            &default_models,
        )
        .await
        .unwrap();
        assert!(first.created);
        let replay = acquire_token_using(
            &api,
            "https://mock.invalid",
            "pi",
            "default",
            &default_models,
        )
        .await
        .unwrap();
        assert!(!replay.created);
        assert_eq!(first.id, replay.id);
        assert_eq!(first.key, replay.key);
        let discounted_models = vec!["deepseek-v4-flash".to_string()];
        let discounted = acquire_token_using(
            &api,
            "https://mock.invalid",
            "pi",
            "国模特价分组",
            &discounted_models,
        )
        .await
        .unwrap();
        assert!(discounted.created);
        assert_ne!(first.id, discounted.id);
        let tokens = api.tokens.lock().unwrap();
        assert_eq!(tokens[0]["group"], "default");
        assert_eq!(tokens[1]["group"], "国模特价分组");
        assert_eq!(tokens[0]["model_limits_enabled"], true);
        assert_eq!(tokens[0]["model_limits"], "model-a");
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
        let default_models = vec!["model-a".to_string()];
        acquire_token_using(
            &api,
            "https://mock.invalid",
            "pi",
            "default",
            &default_models,
        )
        .await
        .unwrap();
        let before = api.tokens.lock().unwrap().clone();
        api.fail_keys
            .store(true, std::sync::atomic::Ordering::Relaxed);
        assert!(acquire_token_using(
            &api,
            "https://mock.invalid",
            "pi",
            "discount",
            &["model-b".to_string()],
        )
        .await
        .is_err());
        assert_eq!(*api.tokens.lock().unwrap(), before);
        // Failure while reading an existing key must not delete it.
        assert!(acquire_token_using(
            &api,
            "https://mock.invalid",
            "pi",
            "default",
            &default_models,
        )
        .await
        .is_err());
        assert_eq!(*api.tokens.lock().unwrap(), before);
    }

    #[tokio::test]
    async fn changed_model_scope_rotates_without_deleting_the_old_key_before_commit() {
        let api = FakeTokens::default();
        let first = acquire_token_using(
            &api,
            "https://mock.invalid",
            "codex_desktop",
            "default",
            &["model-b".to_string(), "model-a".to_string()],
        )
        .await
        .unwrap();
        assert!(first.created);

        let replay = acquire_token_using(
            &api,
            "https://mock.invalid",
            "codex_desktop",
            "default",
            &["model-a".to_string(), "model-b".to_string()],
        )
        .await
        .unwrap();
        assert!(!replay.created);
        assert_eq!(replay.id, first.id);

        let replacement = acquire_token_using(
            &api,
            "https://mock.invalid",
            "codex_desktop",
            "default",
            &["model-c".to_string()],
        )
        .await
        .unwrap();
        assert!(replacement.created);
        assert_ne!(replacement.id, first.id);
        assert_eq!(replacement.retire_after_commit, vec![first.id]);
        let tokens = api.tokens.lock().unwrap();
        assert_eq!(tokens.len(), 2, "old key stays valid until local commit");
        assert_eq!(tokens[1]["model_limits"], "model-c");
    }

    #[test]
    fn token_identity_checks_real_group_status_and_expiry_not_just_the_label() {
        let prefix = token_prefix("pi", "special");
        let models = vec!["model-a".to_string()];
        let mut token = json!({"name":format!("{prefix}-0123456789abcdef"),"group":"special","status":1,"expired_time":-1,"model_limits_enabled":true,"model_limits":"model-a"});
        assert!(reusable_token(
            token.as_object().unwrap(),
            &prefix,
            "special",
            &models,
            false
        ));
        token["group"] = json!("default");
        assert!(!reusable_token(
            token.as_object().unwrap(),
            &prefix,
            "special",
            &models,
            false
        ));
        token["group"] = json!("special");
        token["status"] = json!(2);
        assert!(!reusable_token(
            token.as_object().unwrap(),
            &prefix,
            "special",
            &models,
            false
        ));
        token["status"] = json!(1);
        token["expired_time"] = json!(1);
        assert!(!reusable_token(
            token.as_object().unwrap(),
            &prefix,
            "special",
            &models,
            false
        ));
        token["expired_time"] = json!(-1);
        token["model_limits_enabled"] = json!(false);
        assert!(!reusable_token(
            token.as_object().unwrap(),
            &prefix,
            "special",
            &models,
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
            models: None,
            installation_job_id: None,
            restart_running_app: false,
        };
        assert!(request_is_valid(&valid));
        let mut invalid = valid.clone();
        invalid.restart_running_app = true;
        assert!(!request_is_valid(&invalid));
        let mut invalid = valid;
        invalid.tool_id = "opencode".into();
        assert!(!request_is_valid(&invalid));

        let comma = json!({"requestId":"fixture", "lineId":"mainland_optimized", "toolId":"pi", "modelId":"model,a", "billingGroup":"default", "installationId":"i0123456789abcdef"});
        let comma: ToolActivationRequest = serde_json::from_value(comma).unwrap();
        assert!(!request_is_valid(&comma));
    }

    #[test]
    fn activation_cancellation_allows_only_one_live_transaction() {
        let state = ActivationOperationState::default();
        let registration = state.begin("activation-one").unwrap();
        assert!(state.begin("activation-one").is_err());
        assert!(state.begin("activation-two").is_err());
        assert!(!registration.cancellation.is_requested());
        assert_eq!(state.cancel("activation-one"), Ok(true));
        assert!(registration.cancellation.is_requested());
        drop(registration);
        assert_eq!(state.cancel("activation-one"), Ok(false));
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
            Some(codex_desktop::CodexTransport::DirectResponses)
        );
        assert_eq!(
            claude_transport(&pricing, "messages"),
            Some(ClaudeTransport::DirectAnthropic)
        );
        assert_eq!(
            claude_transport(&pricing, "chat"),
            Some(ClaudeTransport::DirectAnthropic)
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
        ];
        for (index, left) in names.iter().enumerate() {
            assert!(!names[index + 1..].contains(left));
        }
    }
}

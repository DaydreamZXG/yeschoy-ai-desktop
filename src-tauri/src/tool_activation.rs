use std::time::{SystemTime, UNIX_EPOCH};

use reqwest::{Method, Url};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::{
    account_v2::{
        native_account_json, native_session_access, AccountV2State, NativeSessionFailure,
    },
    connectivity_core::request_id_is_valid,
    tool_adapters::{
        self, claude_code, claude_desktop, codex_desktop, dsh_web, pi, AdapterFailure,
        ResolvedInstallation,
    },
    tool_credentials::{self, CredentialFailure, ToolCredential},
};

const TOKEN_PAGE_SIZE: &str = "100";

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ToolActivationRequest {
    request_id: String,
    line_id: String,
    tool_id: String,
    model_id: String,
    installation_id: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolActivationProjection {
    request_id: String,
    schema_version: u8,
    status: &'static str,
    tool_id: String,
    model_id: String,
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
            schema_version: 2,
            status,
            tool_id: request.tool_id.clone(),
            model_id: request.model_id.clone(),
            observed_at_epoch_ms: now_epoch_ms(),
            reason_code,
        }
    }
}

#[derive(Clone, Copy)]
enum ActivationFailure {
    SignedOut,
    UnsupportedModel,
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
                AdapterFailure::UnsupportedVersion => ToolActivationProjection::new(
                    request,
                    "unsupported_version",
                    "exact_version_not_supported",
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
            "claude_code" | "claude_desktop" | "codex_desktop" | "pi" | "dsh_web"
        )
        && bounded_plain_text(&request.model_id, 200)
        && request.installation_id.len() <= 128
        && request
            .installation_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
}

fn data(value: &Value) -> Option<&Value> {
    let object = value.as_object()?;
    (object.get("success")?.as_bool()? == true)
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
) -> Result<(), ActivationFailure> {
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
    if model_supports_tool(&pricing, model_id, tool_id) {
        Ok(())
    } else {
        Err(ActivationFailure::UnsupportedModel)
    }
}

fn model_supports_tool(pricing: &Value, model_id: &str, tool_id: &str) -> bool {
    let required_endpoint = match tool_id {
        "claude_code" | "claude_desktop" => "anthropic",
        "codex_desktop" => "openai-response",
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
    origin: &str,
    access_token: &str,
    name: &str,
) -> Result<Option<u64>, ActivationFailure> {
    let url = token_search_url(origin, name)?;
    let (status, value) = native_account_json(Method::GET, &url, access_token, None)
        .await
        .map_err(|_| ActivationFailure::ServerUnavailable)?;
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
        .filter(|token| token.get("name").and_then(Value::as_str) == Some(name))
        .filter_map(|token| token.get("id").and_then(Value::as_u64))
        .max();
    Ok(id)
}

fn token_request(name: &str, id: Option<u64>) -> Value {
    let mut body = json!({
        "name": name,
        "remain_quota": 0,
        "expired_time": -1,
        "unlimited_quota": true,
        "model_limits_enabled": false,
        "model_limits": "",
        "allow_ips": "",
        "group": "",
        "auto_groups": [],
        "cross_group_retry": false
    });
    if let Some(id) = id {
        body.as_object_mut()
            .expect("token request is an object")
            .insert("id".into(), id.into());
    }
    body
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
    origin: &str,
    access_token: &str,
    id: u64,
) -> Result<String, ActivationFailure> {
    let (status, value) = native_account_json(
        Method::POST,
        &format!("{origin}/api/token/{id}/key"),
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
) -> Result<TokenLease, ActivationFailure> {
    let name = token_name(tool_id);
    if let Some(id) = find_token_id(origin, access_token, name).await? {
        let (status, value) = native_account_json(
            Method::PUT,
            &format!("{origin}/api/token/"),
            access_token,
            Some(token_request(name, Some(id))),
        )
        .await
        .map_err(|_| ActivationFailure::ServerUnavailable)?;
        if matches!(status, 401 | 403) {
            return Err(ActivationFailure::SignedOut);
        }
        if !server_success(status, &value) {
            return Err(ActivationFailure::ServerUnavailable);
        }
        return Ok(TokenLease {
            id,
            key: fetch_token_key(origin, access_token, id).await?,
            created: false,
        });
    }

    let (status, value) = native_account_json(
        Method::POST,
        &format!("{origin}/api/token/"),
        access_token,
        Some(token_request(name, None)),
    )
    .await
    .map_err(|_| ActivationFailure::ServerUnavailable)?;
    if matches!(status, 401 | 403) {
        return Err(ActivationFailure::SignedOut);
    }
    if !server_success(status, &value) {
        return Err(ActivationFailure::ServerUnavailable);
    }
    let key = data(&value)
        .and_then(Value::as_object)
        .and_then(|value| value.get("key"))
        .and_then(Value::as_str)
        .and_then(normalize_api_key)
        .ok_or(ActivationFailure::ServerUnavailable)?;
    let id = find_token_id(origin, access_token, name)
        .await?
        .ok_or(ActivationFailure::ServerUnavailable)?;
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
}

impl PreparedAdapter {
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

    fn local_gateway_token(&self) -> Option<&str> {
        match self {
            Self::ClaudeDesktop(value) => Some(value.local_token()),
            _ => None,
        }
    }
}

fn prepare_adapter(
    request: &ToolActivationRequest,
    origin: &str,
    previous: Option<&ToolCredential>,
) -> Result<PreparedAdapter, AdapterFailure> {
    let home = tool_adapters::user_home()
        .filter(|path| path.is_absolute() && path.is_dir())
        .ok_or(AdapterFailure::ConfigurationFailed("home_unavailable"))?;
    match request.tool_id.as_str() {
        "claude_code" => {
            claude_code::prepare(&home, origin, &request.model_id).map(PreparedAdapter::ClaudeCode)
        }
        "claude_desktop" => claude_desktop::prepare(
            &home,
            &request.model_id,
            previous.and_then(|record| record.local_gateway_token.as_deref()),
        )
        .map(PreparedAdapter::ClaudeDesktop),
        "codex_desktop" => codex_desktop::prepare(&home, origin, &request.model_id)
            .map(PreparedAdapter::CodexDesktop),
        "pi" => pi::prepare(&home, origin, &request.model_id).map(PreparedAdapter::Pi),
        "dsh_web" => {
            dsh_web::prepare(&home, origin, &request.model_id).map(PreparedAdapter::DshWeb)
        }
        _ => Err(AdapterFailure::ConfigurationFailed("invalid_request")),
    }
}

async fn verify_adapter(
    request: &ToolActivationRequest,
    installation: &ResolvedInstallation,
    credential: &ToolCredential,
    claude_runtime: &claude_desktop::ClaudeDesktopRuntimeState,
    dsh_runtime: &dsh_web::DshRuntimeState,
) -> Result<(), AdapterFailure> {
    match request.tool_id.as_str() {
        "claude_code" => claude_code::verify(installation, &request.model_id).await,
        "claude_desktop" => {
            claude_desktop::verify_and_launch(claude_runtime, installation, credential.clone())
                .await
        }
        "codex_desktop" => codex_desktop::verify_and_launch(installation).await,
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
        _ => Err(AdapterFailure::ConfigurationFailed("invalid_request")),
    }
}

async fn restore_after_failure(
    request: &ToolActivationRequest,
    prepared: &mut PreparedAdapter,
    credential_before: Option<&str>,
    previous_record: Option<ToolCredential>,
    origin: &str,
    access_token: &str,
    lease: &TokenLease,
    claude_runtime: &claude_desktop::ClaudeDesktopRuntimeState,
) -> Option<AdapterFailure> {
    let rollback_failure = prepared.rollback().err();
    let credential_failure = tool_credentials::restore(&request.tool_id, credential_before).err();
    if request.tool_id == "claude_desktop" {
        claude_runtime.stop().await;
        if let Some(previous) = previous_record {
            let _ = claude_runtime.start(previous).await;
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
    claude_runtime: tauri::State<'_, claude_desktop::ClaudeDesktopRuntimeState>,
    dsh_runtime: tauri::State<'_, dsh_web::DshRuntimeState>,
    request: ToolActivationRequest,
) -> Result<ToolActivationProjection, String> {
    if !request_is_valid(&request) {
        return Err("invalid_tool_activation_request".into());
    }

    // Resolve identity and exact version before account or token access. An
    // unavailable or ambiguous installation can never create a server token.
    let installation =
        match tool_adapters::resolve_installation(&request.tool_id, &request.installation_id).await
        {
            Ok(value) => value,
            Err(error) => return Ok(ActivationFailure::Adapter(error).projection(&request)),
        };

    let (origin, access_token) = match native_session_access(&account_state, &request.line_id).await
    {
        Ok(value) => value,
        Err(NativeSessionFailure::SignedOut) => {
            return Ok(ActivationFailure::SignedOut.projection(&request))
        }
        Err(NativeSessionFailure::ServerUnavailable) => {
            return Ok(ActivationFailure::ServerUnavailable.projection(&request))
        }
    };
    if let Err(error) =
        validate_model(&origin, &access_token, &request.model_id, &request.tool_id).await
    {
        return Ok(error.projection(&request));
    }

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

    let lease = match acquire_token(&origin, &access_token, &request.tool_id).await {
        Ok(value) => value,
        Err(error) => return Ok(error.projection(&request)),
    };
    let mut prepared = match prepare_adapter(&request, &origin, previous_record.as_ref()) {
        Ok(value) => value,
        Err(error) => {
            delete_created_token(&origin, &access_token, &lease).await;
            return Ok(ActivationFailure::Adapter(error).projection(&request));
        }
    };
    let credential = ToolCredential {
        api_key: lease.key.clone(),
        origin: origin.clone(),
        model_id: request.model_id.clone(),
        local_gateway_token: prepared.local_gateway_token().map(str::to_owned),
    };
    if tool_credentials::store(&request.tool_id, &credential).is_err() {
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
            previous_record,
            &origin,
            &access_token,
            &lease,
            &claude_runtime,
        )
        .await;
        return Ok(ActivationFailure::Adapter(cleanup.unwrap_or(error)).projection(&request));
    }
    if let Err(error) = verify_adapter(
        &request,
        &installation,
        &credential,
        &claude_runtime,
        &dsh_runtime,
    )
    .await
    {
        let cleanup = restore_after_failure(
            &request,
            &mut prepared,
            credential_before.as_deref(),
            previous_record,
            &origin,
            &access_token,
            &lease,
            &claude_runtime,
        )
        .await;
        return Ok(ActivationFailure::Adapter(cleanup.unwrap_or(error)).projection(&request));
    }

    Ok(ToolActivationProjection::new(
        &request,
        "ready",
        "tool_request_verified",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_requires_one_of_five_exact_targets_and_selection_shape() {
        let valid = ToolActivationRequest {
            request_id: "activation-1".into(),
            line_id: "mainland_optimized".into(),
            tool_id: "pi".into(),
            model_id: "glm-5.3".into(),
            installation_id: "i0123456789abcdef".into(),
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
                {"model_name": "messages", "supported_endpoint_types": ["anthropic"]}
            ]
        });
        assert!(model_supports_tool(&pricing, "chat", "pi"));
        assert!(model_supports_tool(&pricing, "chat", "dsh_web"));
        assert!(model_supports_tool(&pricing, "responses", "codex_desktop"));
        assert!(model_supports_tool(&pricing, "messages", "claude_code"));
        assert!(model_supports_tool(&pricing, "messages", "claude_desktop"));
        assert!(!model_supports_tool(&pricing, "chat", "codex_desktop"));
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

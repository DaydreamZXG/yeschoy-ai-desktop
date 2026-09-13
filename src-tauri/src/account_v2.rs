use std::{
    collections::{BTreeMap, BTreeSet},
    process::Command,
    sync::{Mutex, OnceLock},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use keyring::v1::{Entry, Error as KeyringError};
use reqwest::{
    header::{ACCEPT, AUTHORIZATION, CONTENT_TYPE},
    redirect::Policy,
    Client, Method, StatusCode, Url,
};
use semver::Version;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::account_finance::{
    account_money, recent_savings, recent_requests_from_pages, usage_log_report, AccountMoney,
    RecentSavings, UsageLogReport, RECENT_LOGS_PATH, USAGE_LOG_PAGE_SIZE, USAGE_WINDOW_MS,
};
use crate::connectivity_core::{request_id_is_valid, CONNECTIVITY_LINES};

const CLIENT_VERSION: &str = env!("CARGO_PKG_VERSION");
const MAX_RESPONSE_BYTES: usize = 2 * 1024 * 1024;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(20);
const KEYRING_SERVICE: &str = "com.yeschoy.desktop.account";
const KEYRING_USER: &str = "current-session-v2";
const AUTHORIZATION_PATH: &str = "/api/desktop/v2/device-authorizations";
const TOKEN_PATH: &str = "/api/desktop/v2/device-authorizations/token";
const REFRESH_PATH: &str = "/api/desktop/v2/sessions/refresh";
const LOGOUT_PATH: &str = "/api/desktop/v2/sessions/current";
const ACCOUNT_PATH: &str = "/api/user/self";
const USAGE_PATH: &str = "/api/log/self/stat";
const MODELS_PATH: &str = "/api/user/models";
const PRICING_PATH: &str = "/api/pricing";
const TOOL_KEYS_PATH: &str = "/api/token/";
const AUTHORIZATION_PAGE_PATH: &str = "/desktop-authorize";
const CANONICAL_AUTHORIZATION_PAGE_ORIGIN: &str = "https://yeschoy.com";
const PARTNER_AUTHORIZATION_PAGE_ORIGIN: &str = "https://ai.yeschoy.io";
const COMPILED_AUTHORIZATION_PAGE_ORIGIN: &str = env!("YESCHOY_AUTHORIZATION_PAGE_ORIGIN");

static HTTP_CLIENT: OnceLock<Result<Client, ()>> = OnceLock::new();

#[derive(Default)]
pub struct AccountV2State {
    runtime: Mutex<AccountRuntime>,
    refresh_guard: tokio::sync::Mutex<()>,
    poll_guard: tokio::sync::Mutex<()>,
}

#[derive(Default)]
struct AccountRuntime {
    pending: Option<PendingAuthorization>,
    access: Option<AccessSession>,
    wallet_url: Option<String>,
    authorization_epoch: u64,
}

#[derive(Clone)]
struct PendingAuthorization {
    line_id: String,
    device_code: String,
    user_code: String,
    expires_at_epoch_ms: u64,
    poll_after_seconds: u64,
}

#[derive(Clone)]
struct AccessSession {
    access_token: String,
    access_expires_at: i64,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct StoredSession {
    #[serde(default)]
    line_id: String,
    refresh_token: String,
    session_id: String,
    refresh_expires_at: i64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AccountRequest {
    request_id: String,
    line_id: String,
}

#[derive(Debug, Serialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct AccountSummary {
    available: bool,
    display_name: String,
    username: String,
    balance_quota: String,
    used_quota: String,
    request_count: String,
    quota_per_unit: String,
}

#[derive(Debug, Serialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct UsageSummary {
    available: bool,
    consumed_quota: String,
    request_rate: String,
    token_count: String,
}

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct BillingGroup {
    pub(crate) id: String,
    description: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    ratio: Option<f64>,
}

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ModelBilling {
    groups: Vec<BillingGroup>,
    #[serde(skip_serializing_if = "Option::is_none")]
    base_input_usd: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    base_output_usd: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    cache_read_usd: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    cache_write_usd: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    request_usd: Option<f64>,
    expression: String,
}

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct AccountModel {
    id: String,
    description: String,
    billing_mode: &'static str,
    supported_endpoint_types: Vec<String>,
    pricing_available: bool,
    official_input_cny_per_million: String,
    official_output_cny_per_million: String,
    actual_input_cny_per_million: String,
    actual_output_cny_per_million: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    billing: Option<ModelBilling>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountProjection {
    request_id: String,
    schema_version: u8,
    status: &'static str,
    user_code: String,
    poll_after_seconds: u64,
    expires_at_epoch_ms: u64,
    observed_at_epoch_ms: u64,
    account: AccountSummary,
    usage: UsageSummary,
    models: Vec<AccountModel>,
    comparison_fx: String,
    money: AccountMoney,
    savings: RecentSavings,
    usage_log: UsageLogReport,
    reason_code: &'static str,
}

impl AccountProjection {
    fn empty(request_id: String, status: &'static str, reason_code: &'static str) -> Self {
        Self {
            request_id,
            schema_version: 6,
            status,
            user_code: String::new(),
            poll_after_seconds: 0,
            expires_at_epoch_ms: 0,
            observed_at_epoch_ms: 0,
            account: AccountSummary::default(),
            usage: UsageSummary::default(),
            models: Vec::new(),
            comparison_fx: String::new(),
            money: AccountMoney::default(),
            savings: RecentSavings::default(),
            usage_log: UsageLogReport::default(),
            reason_code,
        }
    }

    fn pending(
        request_id: String,
        pending: &PendingAuthorization,
        reason_code: &'static str,
    ) -> Self {
        let mut projection = Self::empty(request_id, "authorization_pending", reason_code);
        projection.user_code = pending.user_code.clone();
        projection.poll_after_seconds = pending.poll_after_seconds;
        projection.expires_at_epoch_ms = pending.expires_at_epoch_ms;
        projection
    }
}

#[derive(Clone)]
struct DesktopBootstrap {
    device_authorization_available: bool,
    origin: String,
    wallet_url: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct BootstrapEnvelope {
    success: bool,
    data: BootstrapData,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct BootstrapData {
    schema_version: u8,
    service: String,
    contract_id: String,
    minimum_client_version: String,
    device_authorization_available: bool,
    account_read: bool,
    usage_read: bool,
    models_read: bool,
    pricing_read: bool,
    tool_keys_manage: bool,
    official_usd_cny_rate: f64,
    authorization_start_path: String,
    authorization_token_path: String,
    session_refresh_path: String,
    session_logout_path: String,
    account_path: String,
    usage_summary_path: String,
    usage_records_path: String,
    models_path: String,
    pricing_path: String,
    tool_keys_path: String,
    wallet_url: String,
}

#[derive(Debug, Deserialize)]
struct DataEnvelope<T> {
    success: bool,
    data: T,
}

#[derive(Debug, Deserialize)]
struct DeviceAuthorization {
    device_code: String,
    user_code: String,
    verification_uri: String,
    verification_uri_complete: String,
    expires_in: u64,
    interval: u64,
}

#[derive(Debug, Deserialize)]
struct AuthBundle {
    access_token: String,
    token_type: String,
    access_expires_at: i64,
    refresh_token: String,
    refresh_expires_at: i64,
    session_id: String,
}

#[derive(Debug)]
enum TransportFailure {
    Network,
    TimedOut,
    InvalidResponse,
    ResponseTooLarge,
}

struct JsonResponse {
    status: StatusCode,
    value: Value,
}

fn now_epoch_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u64::MAX as u128) as u64
}

fn now_epoch_seconds() -> i64 {
    (now_epoch_ms() / 1000).min(i64::MAX as u64) as i64
}

fn client() -> Result<&'static Client, ()> {
    HTTP_CLIENT
        .get_or_init(|| {
            Client::builder()
                .https_only(true)
                .redirect(Policy::none())
                .referer(false)
                .no_proxy()
                .timeout(REQUEST_TIMEOUT)
                .connect_timeout(Duration::from_secs(6))
                .user_agent(concat!("YesChoyDesktop/", env!("CARGO_PKG_VERSION")))
                .build()
                .map_err(|_| ())
        })
        .as_ref()
        .map_err(|_| ())
}

fn line_origin(line_id: &str) -> Option<&'static str> {
    CONNECTIVITY_LINES
        .iter()
        .find(|line| line.line_id == line_id)
        .map(|line| line.root_url)
}

fn request_is_valid(request: &AccountRequest) -> bool {
    request_id_is_valid(&request.request_id) && line_origin(&request.line_id).is_some()
}

async fn read_body(mut response: reqwest::Response) -> Result<JsonResponse, TransportFailure> {
    let status = response.status();
    let json_type = response
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| {
            value
                .split(';')
                .next()
                .unwrap_or_default()
                .trim()
                .eq_ignore_ascii_case("application/json")
        });
    if !json_type {
        return Err(TransportFailure::InvalidResponse);
    }
    if response
        .content_length()
        .is_some_and(|length| length > MAX_RESPONSE_BYTES as u64)
    {
        return Err(TransportFailure::ResponseTooLarge);
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(map_reqwest_error)? {
        if chunk.len() > MAX_RESPONSE_BYTES.saturating_sub(bytes.len()) {
            return Err(TransportFailure::ResponseTooLarge);
        }
        bytes.extend_from_slice(&chunk);
    }
    let value = serde_json::from_slice(&bytes).map_err(|_| TransportFailure::InvalidResponse)?;
    Ok(JsonResponse { status, value })
}

fn map_reqwest_error(error: reqwest::Error) -> TransportFailure {
    if error.is_timeout() {
        TransportFailure::TimedOut
    } else {
        TransportFailure::Network
    }
}

async fn send_json(
    method: Method,
    url: &str,
    access_token: Option<&str>,
    body: Option<Value>,
) -> Result<JsonResponse, TransportFailure> {
    let mut request = client()
        .map_err(|_| TransportFailure::Network)?
        .request(method, url)
        .header(ACCEPT, "application/json");
    if let Some(token) = access_token {
        request = request.header(AUTHORIZATION, format!("Bearer {token}"));
    }
    if let Some(body) = body {
        request = request
            .header(CONTENT_TYPE, "application/json")
            .body(body.to_string());
    }
    let response = request.send().await.map_err(map_reqwest_error)?;
    read_body(response).await
}

async fn fetch_bootstrap(line_id: &str) -> Result<DesktopBootstrap, AccountProjectionFailure> {
    let permit = crate::shutdown_coordinator::global()
        .admit_operation()
        .map_err(|_| AccountProjectionFailure::BackendUnavailable)?;
    permit
        .cancel_safe(fetch_bootstrap_read(line_id))
        .await
        .map_err(|_| AccountProjectionFailure::BackendUnavailable)?
}

async fn fetch_bootstrap_read(line_id: &str) -> Result<DesktopBootstrap, AccountProjectionFailure> {
    let origin = line_origin(line_id).ok_or(AccountProjectionFailure::InvalidRequest)?;
    let response = send_json(
        Method::GET,
        &format!("{origin}/api/desktop/v2/bootstrap"),
        None,
        None,
    )
    .await
    .map_err(AccountProjectionFailure::Transport)?;
    if !response.status.is_success() {
        return Err(AccountProjectionFailure::BackendUnavailable);
    }
    let envelope: BootstrapEnvelope = serde_json::from_value(response.value)
        .map_err(|_| AccountProjectionFailure::IncompatibleServer)?;
    let data = envelope.data;
    let minimum = Version::parse(&data.minimum_client_version)
        .map_err(|_| AccountProjectionFailure::IncompatibleServer)?;
    let current = Version::parse(CLIENT_VERSION).expect("compiled client version is valid");
    let paths_match = data.authorization_start_path == AUTHORIZATION_PATH
        && data.authorization_token_path == TOKEN_PATH
        && data.session_refresh_path == REFRESH_PATH
        && data.session_logout_path == LOGOUT_PATH
        && data.account_path == ACCOUNT_PATH
        && data.usage_summary_path == USAGE_PATH
        && data.usage_records_path == "/api/log/self"
        && data.models_path == MODELS_PATH
        && data.pricing_path == PRICING_PATH
        && data.tool_keys_path == TOOL_KEYS_PATH;
    if !envelope.success
        || data.schema_version != 2
        || data.service != "yeschoy-desktop"
        || data.contract_id != "desktop-integration-v2"
        || minimum > current
        || !data.account_read
        || !data.usage_read
        || !data.models_read
        || !data.pricing_read
        || !data.tool_keys_manage
        || !data.official_usd_cny_rate.is_finite()
        || data.official_usd_cny_rate <= 0.0
        || !paths_match
        || !wallet_url_is_allowed(&data.wallet_url)
    {
        return Err(AccountProjectionFailure::IncompatibleServer);
    }
    Ok(DesktopBootstrap {
        device_authorization_available: data.device_authorization_available,
        origin: origin.to_owned(),
        wallet_url: data.wallet_url,
    })
}

fn wallet_url_is_allowed(raw: &str) -> bool {
    let Ok(url) = Url::parse(raw) else {
        return false;
    };
    url.scheme() == "https"
        && url.host_str() == Some("yeschoy.com")
        && url.port().is_none()
        && url.username().is_empty()
        && url.password().is_none()
        && url.query().is_none()
        && url.fragment().is_none()
        && url.path().starts_with("/wallet")
}

fn authorization_url_is_allowed(raw: &str, user_code: &str) -> bool {
    let Ok(url) = Url::parse(raw) else {
        return false;
    };
    let query = url.query_pairs().collect::<Vec<_>>();
    url.scheme() == "https"
        && url.host_str() == Some("yeschoy.com")
        && url.port().is_none()
        && url.username().is_empty()
        && url.password().is_none()
        && url.path() == AUTHORIZATION_PAGE_PATH
        && url.fragment().is_none()
        && query.len() == 1
        && query[0].0 == "user_code"
        && query[0].1 == user_code
}

fn authorization_page_origin_is_allowed(raw: &str) -> bool {
    if !matches!(
        raw,
        CANONICAL_AUTHORIZATION_PAGE_ORIGIN | PARTNER_AUTHORIZATION_PAGE_ORIGIN
    ) {
        return false;
    }
    let Ok(url) = Url::parse(raw) else {
        return false;
    };
    url.scheme() == "https"
        && matches!(url.host_str(), Some("yeschoy.com" | "ai.yeschoy.io"))
        && url.port().is_none()
        && url.username().is_empty()
        && url.password().is_none()
        && url.path() == "/"
        && url.query().is_none()
        && url.fragment().is_none()
}

fn browser_authorization_url_for_origin(origin: &str, user_code: &str) -> Option<String> {
    if !authorization_page_origin_is_allowed(origin) || !user_code_is_valid(user_code) {
        return None;
    }
    let mut url = Url::parse(&format!("{origin}{AUTHORIZATION_PAGE_PATH}")).ok()?;
    url.query_pairs_mut().append_pair("user_code", user_code);
    Some(url.into())
}

fn browser_authorization_url(user_code: &str) -> Option<String> {
    browser_authorization_url_for_origin(COMPILED_AUTHORIZATION_PAGE_ORIGIN, user_code)
}

fn user_code_is_valid(value: &str) -> bool {
    value.len() == 9
        && value.as_bytes()[4] == b'-'
        && value
            .bytes()
            .enumerate()
            .all(|(index, byte)| index == 4 || byte.is_ascii_uppercase() || byte.is_ascii_digit())
}

#[cfg(target_os = "macos")]
fn open_system_browser(url: &str) -> Result<(), ()> {
    Command::new("/usr/bin/open")
        .arg(url)
        .spawn()
        .map(|_| ())
        .map_err(|_| ())
}

#[cfg(target_os = "windows")]
fn open_system_browser(url: &str) -> Result<(), ()> {
    Command::new("rundll32.exe")
        .arg("url.dll,FileProtocolHandler")
        .arg(url)
        .spawn()
        .map(|_| ())
        .map_err(|_| ())
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn open_system_browser(_url: &str) -> Result<(), ()> {
    Err(())
}

fn keyring_entry() -> Result<Entry, ()> {
    Entry::new(KEYRING_SERVICE, KEYRING_USER).map_err(|_| ())
}

fn save_stored_session(session: &StoredSession) -> Result<(), ()> {
    let payload = serde_json::to_string(session).map_err(|_| ())?;
    keyring_entry()?.set_password(&payload).map_err(|_| ())
}

fn load_stored_session() -> Result<Option<StoredSession>, ()> {
    let entry = keyring_entry()?;
    match entry.get_password() {
        Ok(payload) => serde_json::from_str(&payload).map(Some).map_err(|_| ()),
        Err(KeyringError::NoEntry) => Ok(None),
        Err(_) => Err(()),
    }
}

fn delete_stored_session() -> Result<(), ()> {
    let entry = keyring_entry()?;
    match entry.delete_credential() {
        Ok(()) | Err(KeyringError::NoEntry) => Ok(()),
        Err(_) => Err(()),
    }
}

fn install_auth_bundle(
    state: &AccountV2State,
    line_id: &str,
    bundle: AuthBundle,
    epoch: u64,
    device_code: Option<&str>,
) -> Result<(), ()> {
    let mut runtime = state.runtime.lock().map_err(|_| ())?;
    if runtime.authorization_epoch != epoch
        || device_code.is_some_and(|code| {
            runtime.pending.as_ref().map(|p| p.device_code.as_str()) != Some(code)
        })
    {
        return Err(());
    }
    if bundle.token_type != "Bearer"
        || bundle.access_token.len() < 32
        || bundle.refresh_token.len() < 32
        || bundle.session_id.is_empty()
        || bundle.access_expires_at <= now_epoch_seconds()
        || bundle.refresh_expires_at <= bundle.access_expires_at
    {
        return Err(());
    }
    let stored = StoredSession {
        line_id: line_id.to_owned(),
        refresh_token: bundle.refresh_token,
        session_id: bundle.session_id,
        refresh_expires_at: bundle.refresh_expires_at,
    };
    save_stored_session(&stored)?;
    runtime.access = Some(AccessSession {
        access_token: bundle.access_token,
        access_expires_at: bundle.access_expires_at,
    });
    runtime.pending = None;
    Ok(())
}

#[derive(Debug)]
enum AccountProjectionFailure {
    InvalidRequest,
    Transport(TransportFailure),
    BackendUnavailable,
    IncompatibleServer,
    SecureStorage,
    SessionExpired,
    InvalidResponse,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum NativeSessionFailure {
    SignedOut,
    ServerUnavailable,
    AccountChanged,
}

#[derive(Clone, Copy)]
pub(crate) struct NativeSessionEpoch(u64);

pub(crate) fn native_session_epoch(
    state: &AccountV2State,
) -> Result<NativeSessionEpoch, NativeSessionFailure> {
    state
        .runtime
        .lock()
        .map(|runtime| NativeSessionEpoch(runtime.authorization_epoch))
        .map_err(|_| NativeSessionFailure::ServerUnavailable)
}

pub(crate) fn ensure_session_epoch(
    state: &AccountV2State,
    expected: NativeSessionEpoch,
) -> Result<(), NativeSessionFailure> {
    if native_session_epoch(state)?.0 != expected.0 {
        return Err(NativeSessionFailure::AccountChanged);
    }
    Ok(())
}

async fn session_bound_step<T>(
    state: &AccountV2State,
    epoch: NativeSessionEpoch,
    step: impl std::future::Future<Output = Result<T, NativeSessionFailure>>,
) -> Result<T, NativeSessionFailure> {
    ensure_session_epoch(state, epoch)?;
    let result = step.await;
    ensure_session_epoch(state, epoch)?;
    result
}

pub(crate) async fn native_session_access(
    state: &AccountV2State,
    line_id: &str,
    epoch: NativeSessionEpoch,
) -> Result<(String, String), NativeSessionFailure> {
    let bootstrap = session_bound_step(state, epoch, async {
        fetch_bootstrap(line_id)
            .await
            .map_err(|_| NativeSessionFailure::ServerUnavailable)
    })
    .await?;
    let access_token = session_bound_step(state, epoch, async {
        refresh_access(state, &bootstrap, line_id)
            .await
            .map_err(|failure| match failure {
                AccountProjectionFailure::SessionExpired => NativeSessionFailure::SignedOut,
                _ => NativeSessionFailure::ServerUnavailable,
            })
    })
    .await?;
    Ok((bootstrap.origin, access_token))
}

pub(crate) async fn native_account_json(
    method: Method,
    url: &str,
    access_token: &str,
    body: Option<Value>,
) -> Result<(u16, Value), ()> {
    let response = send_json(method, url, Some(access_token), body)
        .await
        .map_err(|_| ())?;
    Ok((response.status.as_u16(), response.value))
}

/// 诊断层（PRD 6.7，批次 3 #2）复用的只读 HTTPS 客户端：
/// https-only、无重定向、无代理，与账户请求同一套约束。
pub(crate) fn shared_http_client() -> Result<&'static Client, ()> {
    client()
}

pub(crate) fn has_stored_session() -> bool {
    matches!(load_stored_session(), Ok(Some(_)))
}

pub(crate) enum ProbeSessionFailure {
    SignedOut,
    SecureStorage,
    ServerUnavailable,
}

/// 为诊断的 API Key 有效性层取一个可用的短期访问令牌。
/// 与账户读取共用刷新互斥与缓存；不落任何新状态。
pub(crate) async fn native_probe_access(
    state: &AccountV2State,
) -> Result<String, ProbeSessionFailure> {
    if let Ok(runtime) = state.runtime.lock() {
        if let Some(access) = runtime.access.clone() {
            if access.access_expires_at > now_epoch_seconds() + 30 {
                return Ok(access.access_token);
            }
        }
    }
    let stored = load_stored_session().map_err(|_| ProbeSessionFailure::SecureStorage)?;
    let Some(stored) = stored else {
        return Err(ProbeSessionFailure::SignedOut);
    };
    let line_id = if line_origin(&stored.line_id).is_some() {
        stored.line_id
    } else {
        CONNECTIVITY_LINES[0].line_id.to_owned()
    };
    let bootstrap = fetch_bootstrap(&line_id)
        .await
        .map_err(|_| ProbeSessionFailure::ServerUnavailable)?;
    refresh_access(state, &bootstrap, &line_id)
        .await
        .map_err(|failure| match failure {
            AccountProjectionFailure::SessionExpired => ProbeSessionFailure::SignedOut,
            AccountProjectionFailure::SecureStorage => ProbeSessionFailure::SecureStorage,
            _ => ProbeSessionFailure::ServerUnavailable,
        })
}

fn failure_projection(request_id: String, failure: AccountProjectionFailure) -> AccountProjection {
    match failure {
        AccountProjectionFailure::InvalidRequest => {
            AccountProjection::empty(request_id, "invalid_response", "invalid_request")
        }
        AccountProjectionFailure::Transport(TransportFailure::Network) => {
            AccountProjection::empty(request_id, "network_error", "network_error")
        }
        AccountProjectionFailure::Transport(TransportFailure::TimedOut) => {
            AccountProjection::empty(request_id, "network_error", "request_timed_out")
        }
        AccountProjectionFailure::Transport(TransportFailure::ResponseTooLarge) => {
            AccountProjection::empty(request_id, "invalid_response", "response_too_large")
        }
        AccountProjectionFailure::Transport(TransportFailure::InvalidResponse)
        | AccountProjectionFailure::InvalidResponse => {
            AccountProjection::empty(request_id, "invalid_response", "invalid_response")
        }
        AccountProjectionFailure::BackendUnavailable => {
            AccountProjection::empty(request_id, "backend_unavailable", "server_not_ready")
        }
        AccountProjectionFailure::IncompatibleServer => {
            AccountProjection::empty(request_id, "incompatible_server", "incompatible_server")
        }
        AccountProjectionFailure::SecureStorage => AccountProjection::empty(
            request_id,
            "secure_storage_unavailable",
            "secure_storage_unavailable",
        ),
        AccountProjectionFailure::SessionExpired => {
            AccountProjection::empty(request_id, "session_expired", "session_expired")
        }
    }
}

async fn refresh_access(
    state: &AccountV2State,
    bootstrap: &DesktopBootstrap,
    line_id: &str,
) -> Result<String, AccountProjectionFailure> {
    // Refresh tokens may rotate. Concurrent page requests must share one
    // refresh, rather than invalidating one another's credentials.
    let permit = crate::shutdown_coordinator::global()
        .admit_operation()
        .map_err(|_| AccountProjectionFailure::BackendUnavailable)?;
    let _refresh_guard = permit
        .cancel_safe(state.refresh_guard.lock())
        .await
        .map_err(|_| AccountProjectionFailure::BackendUnavailable)?;
    let epoch = state
        .runtime
        .lock()
        .map_err(|_| AccountProjectionFailure::SecureStorage)?
        .authorization_epoch;
    if let Some(access) = state
        .runtime
        .lock()
        .map_err(|_| AccountProjectionFailure::SecureStorage)?
        .access
        .clone()
    {
        if access.access_expires_at > now_epoch_seconds() + 30 {
            return Ok(access.access_token);
        }
    }
    let Some(stored) =
        load_stored_session().map_err(|_| AccountProjectionFailure::SecureStorage)?
    else {
        return Err(AccountProjectionFailure::SessionExpired);
    };
    if stored.refresh_expires_at <= now_epoch_seconds() {
        let runtime = state
            .runtime
            .lock()
            .map_err(|_| AccountProjectionFailure::SecureStorage)?;
        if runtime.authorization_epoch == epoch {
            let _ = delete_stored_session();
        }
        return Err(AccountProjectionFailure::SessionExpired);
    }

    // A login belongs to the 野菜API account, not to the currently selected
    // acceleration route. Refresh through the route that issued the session,
    // then use the short-lived access token with whichever route the user is
    // viewing. This keeps a mainland/global switch from looking like logout.
    let refresh_line_id = if line_origin(&stored.line_id).is_some() {
        stored.line_id.clone()
    } else {
        line_id.to_owned()
    };
    let refresh_origin = if refresh_line_id == line_id {
        bootstrap.origin.clone()
    } else {
        fetch_bootstrap(&refresh_line_id).await?.origin
    };
    if permit.is_cancelled() {
        return Err(AccountProjectionFailure::BackendUnavailable);
    }
    let response = send_json(
        Method::POST,
        &format!("{}{}", refresh_origin, REFRESH_PATH),
        None,
        Some(json!({
            "refresh_token": stored.refresh_token,
            "session_id": stored.session_id,
        })),
    )
    .await
    .map_err(AccountProjectionFailure::Transport)?;
    if response.status == StatusCode::UNAUTHORIZED || response.status == StatusCode::FORBIDDEN {
        if let Ok(mut runtime) = state.runtime.lock() {
            if runtime.authorization_epoch == epoch {
                let _ = delete_stored_session();
                runtime.access = None;
            }
        }
        return Err(AccountProjectionFailure::SessionExpired);
    }
    if !response.status.is_success() {
        return Err(AccountProjectionFailure::BackendUnavailable);
    }
    let envelope: DataEnvelope<AuthBundle> = serde_json::from_value(response.value)
        .map_err(|_| AccountProjectionFailure::InvalidResponse)?;
    if !envelope.success {
        return Err(AccountProjectionFailure::InvalidResponse);
    }
    let access_token = envelope.data.access_token.clone();
    install_auth_bundle(state, &refresh_line_id, envelope.data, epoch, None)
        .map_err(|_| AccountProjectionFailure::SecureStorage)?;
    Ok(access_token)
}

async fn account_data(
    request_id: String,
    bootstrap: &DesktopBootstrap,
    access_token: &str,
) -> Result<AccountProjection, AccountProjectionFailure> {
    let permit = crate::shutdown_coordinator::global()
        .admit_operation()
        .map_err(|_| AccountProjectionFailure::BackendUnavailable)?;
    permit
        .cancel_safe(account_data_read(request_id, bootstrap, access_token))
        .await
        .map_err(|_| AccountProjectionFailure::BackendUnavailable)?
}

async fn account_data_read(
    request_id: String,
    bootstrap: &DesktopBootstrap,
    access_token: &str,
) -> Result<AccountProjection, AccountProjectionFailure> {
    let account_url = format!("{}{}", bootstrap.origin, ACCOUNT_PATH);
    let usage_url = format!("{}{}", bootstrap.origin, USAGE_PATH);
    let models_url = format!("{}{}", bootstrap.origin, MODELS_PATH);
    let pricing_url = format!("{}{}", bootstrap.origin, PRICING_PATH);
    let status_url = format!("{}/api/status", bootstrap.origin);
    let logs_url = format!("{}{}", bootstrap.origin, RECENT_LOGS_PATH);
    let (account, usage, models, pricing, status, logs) = tokio::join!(
        send_json(Method::GET, &account_url, Some(access_token), None),
        send_json(Method::GET, &usage_url, Some(access_token), None),
        send_json(Method::GET, &models_url, Some(access_token), None),
        send_json(Method::GET, &pricing_url, Some(access_token), None),
        send_json(Method::GET, &status_url, None, None),
        send_json(Method::GET, &logs_url, Some(access_token), None),
    );
    if [&account, &usage, &models]
        .iter()
        .any(|response| matches!(response, Ok(value) if value.status == StatusCode::UNAUTHORIZED || value.status == StatusCode::FORBIDDEN))
        || matches!(&pricing, Ok(value) if value.status == StatusCode::UNAUTHORIZED)
    {
        // The access token was already obtained (or refreshed) through the
        // route that owns this account session. A second route can briefly lag
        // behind or reject that token while deployments converge. Treat that
        // as a route outage so the renderer keeps the signed-in projection;
        // only refresh_access may prove the account session itself expired.
        return Err(AccountProjectionFailure::BackendUnavailable);
    }
    let account_value = account.ok().filter(|value| value.status.is_success());
    let usage_value = usage.ok().filter(|value| value.status.is_success());
    let models_value = models.ok().filter(|value| value.status.is_success());
    let pricing_value = pricing.ok().filter(|value| value.status.is_success());
    let status_value = status.ok().filter(|value| value.status.is_success());
    let logs_value = logs.ok().filter(|value| value.status.is_success());
    // PRD 6.6：首页日志满页且尚未覆盖 30 天窗口时补拉后续页（最多 5 页）。
    let observed_now = now_epoch_ms();
    let log_pages = usage_log_pages(bootstrap, access_token, logs_value.as_ref(), observed_now).await;
    let account_summary = parse_account(
        account_value.as_ref().map(|value| &value.value),
        status_value.as_ref().map(|value| &value.value),
    );
    let usage_summary = parse_usage(usage_value.as_ref().map(|value| &value.value));
    let comparison_fx = data_object(status_value.as_ref().map(|value| &value.value))
        .and_then(|data| positive_number(data.get("usd_exchange_rate")));
    let models = parse_models(
        models_value.as_ref().map(|value| &value.value),
        pricing_value.as_ref().map(|value| &value.value),
        account_value.as_ref().map(|value| &value.value),
        comparison_fx,
    );
    if !account_summary.available {
        return Err(AccountProjectionFailure::InvalidResponse);
    }
    let money = account_money(
        status_value.as_ref().map(|v| &v.value),
        &account_summary.balance_quota,
        &account_summary.used_quota,
    );
    let savings = recent_savings(
        status_value.as_ref().map(|v| &v.value),
        logs_value.as_ref().map(|v| &v.value),
    );
    let usage_log = usage_log_report(
        status_value.as_ref().map(|v| &v.value),
        &log_pages,
        observed_now,
    );
    // 直连后本地不再观测请求，"最近中转记录"改由服务端消费日志提供，
    // 按客户端为每个工具创建的 token 名称归因。
    if let Some(line_id) = crate::request_diagnostics::line_for_origin(&bootstrap.origin) {
        for (tool, observation) in recent_requests_from_pages(&log_pages, line_id) {
            crate::request_diagnostics::publish(&tool, observation);
        }
    }
    let reason_code = if usage_summary.available
        && models_value.is_some()
        && pricing_value.is_some()
        && comparison_fx.is_some()
        && !money.currency.is_empty()
        && savings.status != "unavailable"
        && usage_log.status != "unavailable"
    {
        "none"
    } else {
        "partial_data"
    };
    Ok(AccountProjection {
        request_id,
        schema_version: 6,
        status: "signed_in",
        user_code: String::new(),
        poll_after_seconds: 0,
        expires_at_epoch_ms: 0,
        observed_at_epoch_ms: observed_now,
        account: account_summary,
        usage: usage_summary,
        models,
        comparison_fx: comparison_fx.map(decimal).unwrap_or_default(),
        money,
        savings,
        usage_log,
        reason_code,
    })
}

/// 分页拉取 `/api/log/self`：第 1 页来自六路并发；仅当首页满 100 条且
/// 尚未覆盖 30 天窗口时，并发补拉第 2..=5 页。任何一页失败即停止追加
/// （已有前缀足够，报告侧按截断口径呈现）。
async fn usage_log_pages(
    bootstrap: &DesktopBootstrap,
    access_token: &str,
    first_page: Option<&JsonResponse>,
    now_epoch_ms: u64,
) -> Vec<Value> {
    let Some(first) = first_page else {
        return Vec::new();
    };
    let mut pages = vec![first.value.clone()];
    let Some(items) = first
        .value
        .get("data")
        .and_then(|data| data.get("items"))
        .and_then(Value::as_array)
    else {
        return pages;
    };
    if items.len() < USAGE_LOG_PAGE_SIZE {
        return pages;
    }
    let cutoff = now_epoch_ms.saturating_sub(USAGE_WINDOW_MS);
    let oldest_ms = items
        .iter()
        .filter_map(|row| row.get("created_at").and_then(Value::as_u64))
        .filter(|seconds| *seconds > 0)
        .map(|seconds| seconds * 1000)
        .min()
        .unwrap_or(0);
    if oldest_ms <= cutoff {
        return pages;
    }
    let base = format!("{}{}", bootstrap.origin, "/api/log/self");
    let page_url = |page: usize| {
        format!("{base}?p={page}&page_size={USAGE_LOG_PAGE_SIZE}&type=2")
    };
    let (url_2, url_3, url_4, url_5) = (
        page_url(2),
        page_url(3),
        page_url(4),
        page_url(5),
    );
    let (second, third, fourth, fifth) = tokio::join!(
        send_json(Method::GET, &url_2, Some(access_token), None),
        send_json(Method::GET, &url_3, Some(access_token), None),
        send_json(Method::GET, &url_4, Some(access_token), None),
        send_json(Method::GET, &url_5, Some(access_token), None),
    );
    for response in [second, third, fourth, fifth] {
        match response {
            Ok(value) if value.status.is_success() => pages.push(value.value),
            _ => break,
        }
    }
    pages
}

fn data_value(value: Option<&Value>) -> Option<&Value> {
    let envelope = value?.as_object()?;
    if !envelope.get("success")?.as_bool()? {
        return None;
    }
    envelope.get("data")
}

fn data_object(value: Option<&Value>) -> Option<&serde_json::Map<String, Value>> {
    data_value(value)?.as_object()
}

fn non_negative_integer(value: Option<&Value>) -> Option<String> {
    let value = value?;
    if let Some(number) = value.as_u64() {
        return Some(number.to_string());
    }
    value
        .as_i64()
        .filter(|number| *number >= 0)
        .map(|number| number.to_string())
}

fn signed_integer(value: Option<&Value>) -> Option<String> {
    value?.as_i64().map(|number| number.to_string())
}

fn positive_number(value: Option<&Value>) -> Option<f64> {
    value?
        .as_f64()
        .filter(|number| number.is_finite() && *number > 0.0)
}

fn bounded_text(value: Option<&Value>, maximum: usize) -> String {
    let Some(value) = value.and_then(Value::as_str) else {
        return String::new();
    };
    if value.chars().count() > maximum
        || value.chars().any(|character| {
            character.is_control()
                || matches!(character, '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
        })
    {
        return String::new();
    }
    value.to_owned()
}

fn parse_account(account: Option<&Value>, status: Option<&Value>) -> AccountSummary {
    let Some(data) = data_object(account) else {
        return AccountSummary::default();
    };
    let Some(quota) = signed_integer(data.get("quota")) else {
        return AccountSummary::default();
    };
    let Some(used_quota) = non_negative_integer(data.get("used_quota")) else {
        return AccountSummary::default();
    };
    let Some(request_count) = non_negative_integer(data.get("request_count")) else {
        return AccountSummary::default();
    };
    let quota_per_unit = data_object(status)
        .and_then(|value| positive_number(value.get("quota_per_unit")))
        .map(decimal)
        .unwrap_or_default();
    AccountSummary {
        available: true,
        display_name: bounded_text(data.get("display_name"), 160),
        username: bounded_text(data.get("username"), 160),
        balance_quota: quota,
        used_quota,
        request_count,
        quota_per_unit,
    }
}

fn parse_usage(value: Option<&Value>) -> UsageSummary {
    let Some(data) = data_object(value) else {
        return UsageSummary::default();
    };
    let (Some(quota), Some(rpm), Some(tpm)) = (
        non_negative_integer(data.get("quota")),
        non_negative_integer(data.get("rpm")),
        non_negative_integer(data.get("tpm")),
    ) else {
        return UsageSummary::default();
    };
    UsageSummary {
        available: true,
        consumed_quota: quota,
        request_rate: rpm,
        token_count: tpm,
    }
}

fn nonnegative_number(value: Option<&Value>) -> Option<f64> {
    value
        .and_then(Value::as_f64)
        .filter(|n| n.is_finite() && *n >= 0.0)
}

pub(crate) fn billing_groups(pricing: &Value, model_id: &str) -> Vec<BillingGroup> {
    if pricing.get("success").and_then(Value::as_bool) != Some(true) {
        return Vec::new();
    }
    let Some(usable) = pricing.get("usable_group").and_then(Value::as_object) else {
        return Vec::new();
    };
    let rows = pricing.get("data").and_then(Value::as_array);
    usable
        .iter()
        .filter_map(|(id, description)| {
            if id.is_empty()
                || id == "auto"
                || id.chars().count() > 128
                || id.chars().any(|c| {
                    c.is_control() || matches!(c, '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
                })
            {
                return None;
            }
            let enabled = rows
                .into_iter()
                .flatten()
                .filter(|row| row["model_name"].as_str() == Some(model_id))
                .any(|row| {
                    row["enable_groups"].as_array().is_some_and(|groups| {
                        groups
                            .iter()
                            .any(|g| g.as_str() == Some(id) || g.as_str() == Some("all"))
                    })
                });
            enabled.then(|| BillingGroup {
                id: id.clone(),
                description: bounded_text(Some(description), 500),
                ratio: nonnegative_number(pricing.get("group_ratio").and_then(|v| v.get(id))),
            })
        })
        .take(128)
        .collect()
}

fn parse_models(
    models: Option<&Value>,
    pricing: Option<&Value>,
    account: Option<&Value>,
    comparison_fx: Option<f64>,
) -> Vec<AccountModel> {
    let available = data_value(models)
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .filter(|id| !id.is_empty() && id.chars().count() <= 200)
                .map(str::to_owned)
                .collect::<BTreeSet<_>>()
        })
        .unwrap_or_default();
    let Some(pricing_root) = pricing.and_then(Value::as_object) else {
        return available.into_iter().map(unpriced_model).collect();
    };
    if pricing_root.get("success").and_then(Value::as_bool) != Some(true) {
        return available.into_iter().map(unpriced_model).collect();
    }
    let user_group = data_object(account)
        .map(|data| bounded_text(data.get("group"), 128))
        .unwrap_or_default();
    let group_ratios = pricing_root
        .get("group_ratio")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    let account_ratio = group_ratios
        .get(&user_group)
        .and_then(|value| positive_number(Some(value)))
        .or_else(|| {
            group_ratios
                .get("default")
                .and_then(|value| positive_number(Some(value)))
        });
    let mut by_name = BTreeMap::new();
    if let Some(rows) = pricing_root.get("data").and_then(Value::as_array) {
        for row in rows.iter().take(4096) {
            let Some(object) = row.as_object() else {
                continue;
            };
            let name = bounded_text(object.get("model_name"), 200);
            if !name.is_empty() && available.contains(&name) {
                by_name.entry(name).or_insert_with(|| object.clone());
            }
        }
    }
    available
        .into_iter()
        .map(|id| {
            let Some(row) = by_name.get(&id) else {
                return unpriced_model(id);
            };
            // Pricing descriptions may contain internal upstream routing labels.
            // They are not reviewed customer-facing model metadata. Keep the
            // schema field empty instead of forwarding them across renderer IPC.
            let quota_type = row.get("quota_type").and_then(Value::as_i64);
            let billing_mode = match bounded_text(row.get("billing_mode"), 40).as_str() {
                "tiered_expr" => "tiered_expr",
                "per_request" => "per_request",
                "ratio" => "ratio",
                _ if quota_type == Some(0) => "ratio",
                _ if quota_type == Some(1) => "per_request",
                _ => "unknown",
            };
            let supported_endpoint_types = row
                .get("supported_endpoint_types")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .filter(|value| {
                    !value.is_empty()
                        && value.chars().count() <= 80
                        && !value.chars().any(char::is_control)
                })
                .take(32)
                .map(str::to_owned)
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect();
            let ratio = positive_number(row.get("model_ratio"));
            let completion = positive_number(row.get("completion_ratio"));
            let comparable = billing_mode == "ratio"
                && ratio.is_some()
                && completion.is_some()
                && account_ratio.is_some()
                && comparison_fx.is_some();
            let (official_input, official_output, actual_input, actual_output) = if comparable {
                let official_input = ratio.unwrap() * 2.0 * comparison_fx.unwrap();
                let official_output = official_input * completion.unwrap();
                let actual_input = official_input * account_ratio.unwrap();
                let actual_output = official_output * account_ratio.unwrap();
                (
                    money(official_input),
                    money(official_output),
                    money(actual_input),
                    money(actual_output),
                )
            } else {
                (String::new(), String::new(), String::new(), String::new())
            };
            let base_input = nonnegative_number(row.get("model_ratio")).map(|n| n * 2.0);
            let base_output = base_input
                .zip(nonnegative_number(row.get("completion_ratio")))
                .map(|(input, completion)| input * completion)
                .filter(|n| n.is_finite());
            AccountModel {
                id: id.clone(),
                description: String::new(),
                billing_mode,
                supported_endpoint_types,
                pricing_available: comparable,
                official_input_cny_per_million: official_input,
                official_output_cny_per_million: official_output,
                actual_input_cny_per_million: actual_input,
                actual_output_cny_per_million: actual_output,
                billing: Some(ModelBilling {
                    groups: billing_groups(pricing.unwrap(), &id),
                    base_input_usd: base_input.filter(|n| n.is_finite()),
                    base_output_usd: base_output,
                    cache_read_usd: base_input
                        .zip(nonnegative_number(row.get("cache_ratio")))
                        .map(|(input, ratio)| input * ratio)
                        .filter(|n| n.is_finite()),
                    cache_write_usd: base_input
                        .zip(nonnegative_number(row.get("cache_creation_ratio")))
                        .map(|(input, ratio)| input * ratio)
                        .filter(|n| n.is_finite()),
                    request_usd: nonnegative_number(row.get("model_price")),
                    expression: bounded_text(row.get("billing_expr"), 8192),
                }),
            }
        })
        .collect()
}

fn unpriced_model(id: String) -> AccountModel {
    AccountModel {
        id,
        description: String::new(),
        billing_mode: "unknown",
        supported_endpoint_types: Vec::new(),
        pricing_available: false,
        official_input_cny_per_million: String::new(),
        official_output_cny_per_million: String::new(),
        actual_input_cny_per_million: String::new(),
        actual_output_cny_per_million: String::new(),
        billing: None,
    }
}

fn decimal(value: f64) -> String {
    if value.fract().abs() < f64::EPSILON {
        format!("{value:.0}")
    } else {
        format!("{value:.6}").trim_end_matches('0').to_owned()
    }
}

fn money(value: f64) -> String {
    format!("{value:.4}")
        .trim_end_matches('0')
        .trim_end_matches('.')
        .to_owned()
}

async fn inspect_inner(
    state: &AccountV2State,
    request: &AccountRequest,
) -> Result<AccountProjection, AccountProjectionFailure> {
    let bootstrap = fetch_bootstrap(&request.line_id).await?;
    {
        let mut runtime = state
            .runtime
            .lock()
            .map_err(|_| AccountProjectionFailure::SecureStorage)?;
        runtime.wallet_url = Some(bootstrap.wallet_url.clone());
    }
    let access_token = match refresh_access(state, &bootstrap, &request.line_id).await {
        Ok(token) => token,
        Err(AccountProjectionFailure::SessionExpired) => {
            return Ok(AccountProjection::empty(
                request.request_id.clone(),
                "signed_out",
                if bootstrap.device_authorization_available {
                    "signed_out"
                } else {
                    "authorization_unavailable"
                },
            ));
        }
        Err(error) => return Err(error),
    };
    account_data(request.request_id.clone(), &bootstrap, &access_token).await
}

#[tauri::command]
pub async fn account_inspect_v2(
    state: tauri::State<'_, AccountV2State>,
    request: AccountRequest,
) -> Result<AccountProjection, String> {
    let _permit = crate::shutdown_coordinator::global()
        .admit_operation()
        .map_err(|_| "assistant_shutting_down")?;
    if !request_is_valid(&request) {
        return Err("invalid_account_request".into());
    }
    let epoch = state
        .runtime
        .lock()
        .map_err(|_| "account_state_unavailable")?
        .authorization_epoch;
    Ok(match inspect_inner(&state, &request).await {
        Ok(projection) => projection,
        Err(AccountProjectionFailure::SessionExpired) => {
            if let Ok(mut runtime) = state.runtime.lock() {
                if runtime.authorization_epoch == epoch {
                    let _ = delete_stored_session();
                    runtime.access = None;
                }
            }
            failure_projection(request.request_id, AccountProjectionFailure::SessionExpired)
        }
        Err(error) => failure_projection(request.request_id, error),
    })
}

/// Sanitize a raw hostname into a server-safe `device_name` value.
/// Returns None when the result would be empty; caps length at 64 chars.
fn sanitize_device_name(raw: &str) -> Option<String> {
    let cleaned: String = raw
        .chars()
        .filter(|c| !c.is_control())
        .collect::<String>()
        .trim()
        .to_owned();
    if cleaned.is_empty() {
        return None;
    }
    Some(cleaned.chars().take(64).collect())
}

/// Best-effort local device name (client autonomy #23): Windows exposes
/// COMPUTERNAME; other platforms fall back to the `hostname` command.
/// Failure is non-fatal — the begin request simply omits the field.
fn device_name() -> Option<String> {
    let raw = std::env::var("COMPUTERNAME").ok().or_else(|| {
        Command::new("hostname")
            .output()
            .ok()
            .filter(|output| output.status.success())
            .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_owned())
    })?;
    sanitize_device_name(&raw)
}

#[tauri::command]
pub async fn account_begin_authorization_v2(
    state: tauri::State<'_, AccountV2State>,
    request: AccountRequest,
) -> Result<AccountProjection, String> {
    let _permit = crate::shutdown_coordinator::global()
        .admit_operation()
        .map_err(|_| "assistant_shutting_down")?;
    if !request_is_valid(&request) {
        return Err("invalid_account_request".into());
    }
    let epoch = {
        let mut runtime = state
            .runtime
            .lock()
            .map_err(|_| "account_state_unavailable")?;
        runtime.authorization_epoch += 1;
        runtime.pending = None;
        runtime.authorization_epoch
    };
    let bootstrap = match fetch_bootstrap(&request.line_id).await {
        Ok(value) => value,
        Err(error) => return Ok(failure_projection(request.request_id, error)),
    };
    if !bootstrap.device_authorization_available {
        return Ok(AccountProjection::empty(
            request.request_id,
            "backend_unavailable",
            "authorization_unavailable",
        ));
    }
    if _permit.is_cancelled() {
        return Err("assistant_shutting_down".into());
    }
    // 客户端自治（#23）：附 device_name 供授权页展示。线上已验证服务端
    // 容忍未知字段（2026-09-13 probe 200）；获取失败时省略该字段。
    let mut body = json!({"client_name": "野菜API Desktop"});
    if let Some(name) = device_name() {
        body["device_name"] = Value::String(name);
    }
    let response = match send_json(
        Method::POST,
        &format!("{}{}", bootstrap.origin, AUTHORIZATION_PATH),
        None,
        Some(body),
    )
    .await
    {
        Ok(value) => value,
        Err(error) => {
            return Ok(failure_projection(
                request.request_id,
                AccountProjectionFailure::Transport(error),
            ))
        }
    };
    if !response.status.is_success() {
        return Ok(AccountProjection::empty(
            request.request_id,
            "backend_unavailable",
            "authorization_unavailable",
        ));
    }
    let envelope: DataEnvelope<DeviceAuthorization> = match serde_json::from_value(response.value) {
        Ok(value) => value,
        Err(_) => {
            return Ok(failure_projection(
                request.request_id,
                AccountProjectionFailure::InvalidResponse,
            ))
        }
    };
    let authorization = envelope.data;
    let valid = envelope.success
        && authorization.device_code.len() >= 32
        && authorization.device_code.len() <= 128
        && user_code_is_valid(&authorization.user_code)
        && authorization.expires_in > 0
        && authorization.expires_in <= 600
        && authorization.interval > 0
        && authorization.interval <= 60
        && authorization.verification_uri == "https://yeschoy.com/desktop-authorize"
        && authorization_url_is_allowed(
            &authorization.verification_uri_complete,
            &authorization.user_code,
        );
    if !valid {
        return Ok(failure_projection(
            request.request_id,
            AccountProjectionFailure::InvalidResponse,
        ));
    }
    let Some(browser_url) = browser_authorization_url(&authorization.user_code) else {
        return Ok(failure_projection(
            request.request_id,
            AccountProjectionFailure::InvalidResponse,
        ));
    };
    let pending = PendingAuthorization {
        line_id: request.line_id,
        device_code: authorization.device_code,
        user_code: authorization.user_code,
        expires_at_epoch_ms: now_epoch_ms()
            .saturating_add(authorization.expires_in.saturating_mul(1000)),
        poll_after_seconds: authorization.interval,
    };
    {
        let mut runtime = state
            .runtime
            .lock()
            .map_err(|_| "account_state_unavailable".to_owned())?;
        if runtime.authorization_epoch != epoch {
            return Ok(AccountProjection::empty(
                request.request_id,
                "cancelled",
                "authorization_cancelled",
            ));
        }
        runtime.pending = Some(pending.clone());
        runtime.wallet_url = Some(bootstrap.wallet_url);
    }
    if _permit.is_cancelled() {
        return Err("assistant_shutting_down".into());
    }
    let reason = if open_system_browser(&browser_url).is_ok() {
        "authorization_pending"
    } else {
        "browser_open_failed"
    };
    Ok(AccountProjection::pending(
        request.request_id,
        &pending,
        reason,
    ))
}

fn auth_error(value: &Value) -> (String, u64) {
    let Some(object) = value.as_object() else {
        return ("invalid_response".into(), 0);
    };
    let code = object
        .get("code")
        .and_then(Value::as_str)
        .unwrap_or("invalid_response")
        .to_owned();
    let retry = object
        .get("retry_after")
        .and_then(Value::as_u64)
        .unwrap_or(0)
        .min(60);
    (code, retry)
}

#[tauri::command]
pub async fn account_poll_authorization_v2(
    state: tauri::State<'_, AccountV2State>,
    request: AccountRequest,
) -> Result<AccountProjection, String> {
    let _permit = crate::shutdown_coordinator::global()
        .admit_operation()
        .map_err(|_| "assistant_shutting_down")?;
    if !request_is_valid(&request) {
        return Err("invalid_account_request".into());
    }
    let _poll_guard = _permit
        .cancel_safe(state.poll_guard.lock())
        .await
        .map_err(|_| "assistant_shutting_down")?;
    let (pending, epoch) = {
        let runtime = state
            .runtime
            .lock()
            .map_err(|_| "account_state_unavailable")?;
        (runtime.pending.clone(), runtime.authorization_epoch)
    };
    let Some(mut pending) = pending.filter(|pending| pending.line_id == request.line_id) else {
        // Authorization may already have succeeded while account-data loading
        // failed. Re-inspect the saved session instead of showing signed out.
        return Ok(match inspect_inner(&state, &request).await {
            Ok(projection) => projection,
            Err(error) => failure_projection(request.request_id, error),
        });
    };
    if pending.expires_at_epoch_ms <= now_epoch_ms() {
        if let Ok(mut runtime) = state.runtime.lock() {
            if runtime.authorization_epoch == epoch {
                runtime.pending = None;
            }
        }
        return Ok(AccountProjection::empty(
            request.request_id,
            "expired",
            "authorization_expired",
        ));
    }
    let bootstrap = match fetch_bootstrap(&request.line_id).await {
        Ok(value) => value,
        Err(error) => return Ok(failure_projection(request.request_id, error)),
    };
    if _permit.is_cancelled() {
        return Err("assistant_shutting_down".into());
    }
    let response = match send_json(
        Method::POST,
        &format!("{}{}", bootstrap.origin, TOKEN_PATH),
        None,
        Some(json!({"device_code": pending.device_code})),
    )
    .await
    {
        Ok(value) => value,
        Err(error) => {
            return Ok(failure_projection(
                request.request_id,
                AccountProjectionFailure::Transport(error),
            ))
        }
    };
    if !response.status.is_success() {
        let (code, retry_after) = auth_error(&response.value);
        return Ok(match code.as_str() {
            "authorization_pending" | "slow_down" => {
                if retry_after > 0 {
                    pending.poll_after_seconds = retry_after;
                    if let Ok(mut runtime) = state.runtime.lock() {
                        if runtime.authorization_epoch == epoch {
                            runtime.pending = Some(pending.clone());
                        }
                    }
                }
                let reason = if code == "slow_down" {
                    "slow_down"
                } else {
                    "authorization_pending"
                };
                AccountProjection::pending(request.request_id, &pending, reason)
            }
            "access_denied" => {
                if let Ok(mut runtime) = state.runtime.lock() {
                    if runtime.authorization_epoch == epoch {
                        runtime.pending = None;
                    }
                }
                AccountProjection::empty(request.request_id, "denied", "authorization_denied")
            }
            "expired_token" => {
                if let Ok(mut runtime) = state.runtime.lock() {
                    if runtime.authorization_epoch == epoch {
                        runtime.pending = None;
                    }
                }
                AccountProjection::empty(request.request_id, "expired", "authorization_expired")
            }
            "already_used" => {
                if let Ok(mut runtime) = state.runtime.lock() {
                    if runtime.authorization_epoch == epoch {
                        runtime.pending = None;
                    }
                }
                AccountProjection::empty(
                    request.request_id,
                    "expired",
                    "authorization_already_used",
                )
            }
            _ => failure_projection(
                request.request_id,
                AccountProjectionFailure::InvalidResponse,
            ),
        });
    }
    let envelope: DataEnvelope<AuthBundle> = match serde_json::from_value(response.value) {
        Ok(value) => value,
        Err(_) => {
            return Ok(failure_projection(
                request.request_id,
                AccountProjectionFailure::InvalidResponse,
            ))
        }
    };
    if state
        .runtime
        .lock()
        .map_err(|_| "account_state_unavailable")?
        .authorization_epoch
        != epoch
    {
        return Ok(AccountProjection::empty(
            request.request_id,
            "cancelled",
            "authorization_cancelled",
        ));
    }
    if !envelope.success
        || install_auth_bundle(
            &state,
            &request.line_id,
            envelope.data,
            epoch,
            Some(&pending.device_code),
        )
        .is_err()
    {
        return Ok(failure_projection(
            request.request_id,
            AccountProjectionFailure::SecureStorage,
        ));
    }
    match inspect_inner(&state, &request).await {
        Ok(projection) => Ok(projection),
        Err(error) => Ok(failure_projection(request.request_id, error)),
    }
}

#[tauri::command]
pub fn account_cancel_authorization_v2(
    state: tauri::State<'_, AccountV2State>,
    request: AccountRequest,
) -> Result<AccountProjection, String> {
    let _permit = crate::shutdown_coordinator::global()
        .admit_operation()
        .map_err(|_| "assistant_shutting_down")?;
    if !request_is_valid(&request) {
        return Err("invalid_account_request".into());
    }
    let mut runtime = state
        .runtime
        .lock()
        .map_err(|_| "account_state_unavailable".to_owned())?;
    runtime.pending = None;
    runtime.authorization_epoch += 1;
    Ok(AccountProjection::empty(
        request.request_id,
        "cancelled",
        "authorization_cancelled",
    ))
}

#[tauri::command]
pub async fn account_logout_v2(
    state: tauri::State<'_, AccountV2State>,
    request: AccountRequest,
) -> Result<AccountProjection, String> {
    let _permit = crate::shutdown_coordinator::global()
        .admit_operation()
        .map_err(|_| "assistant_shutting_down")?;
    if !request_is_valid(&request) {
        return Err("invalid_account_request".into());
    }
    let access = {
        let mut runtime = state
            .runtime
            .lock()
            .map_err(|_| "account_state_unavailable")?;
        runtime.authorization_epoch += 1;
        runtime.pending = None;
        if delete_stored_session().is_err() {
            return Ok(failure_projection(
                request.request_id,
                AccountProjectionFailure::SecureStorage,
            ));
        }
        runtime.wallet_url = None;
        runtime.access.take()
    };
    for tool in [
        "claude_code",
        "claude_desktop",
        "codex_desktop",
        "pi",
        "dsh_web",
    ] {
        crate::request_diagnostics::clear(tool);
    }
    if let Some(access) = access {
        if let Ok(bootstrap) = fetch_bootstrap(&request.line_id).await {
            let _ = send_json(
                Method::DELETE,
                &format!("{}{}", bootstrap.origin, LOGOUT_PATH),
                Some(&access.access_token),
                None,
            )
            .await;
        }
    }
    Ok(AccountProjection::empty(
        request.request_id,
        "signed_out",
        "logged_out",
    ))
}

#[tauri::command]
pub async fn account_open_wallet_v2(
    state: tauri::State<'_, AccountV2State>,
    request: AccountRequest,
) -> Result<(), String> {
    let _permit = crate::shutdown_coordinator::global()
        .admit_operation()
        .map_err(|_| "assistant_shutting_down")?;
    if !request_is_valid(&request) {
        return Err("invalid_account_request".into());
    }
    let bootstrap = fetch_bootstrap(&request.line_id)
        .await
        .map_err(|_| "wallet_unavailable".to_owned())?;
    let stored = state
        .runtime
        .lock()
        .map_err(|_| "wallet_unavailable".to_owned())?
        .wallet_url
        .clone();
    let wallet = stored
        .filter(|value| value == &bootstrap.wallet_url)
        .unwrap_or(bootstrap.wallet_url);
    if !wallet_url_is_allowed(&wallet) {
        return Err("wallet_unavailable".into());
    }
    if _permit.is_cancelled() {
        return Err("assistant_shutting_down".into());
    }
    open_system_browser(&wallet).map_err(|_| "browser_open_failed".to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn device_name_sanitizer_strips_control_chars_and_caps_length() {
        assert_eq!(sanitize_device_name("MacBook-Pro.local"), Some("MacBook-Pro.local".into()));
        assert_eq!(
            sanitize_device_name("  spaced \n host \n"),
            Some("spaced  host".into())
        );
        assert_eq!(sanitize_device_name("\u{1}\u{2}\u{7f}"), None);
        assert_eq!(sanitize_device_name("   "), None);
        assert_eq!(sanitize_device_name(""), None);
        let long = "a".repeat(100);
        assert_eq!(
            sanitize_device_name(&long).map(|v| v.chars().count()),
            Some(64)
        );
    }

    #[test]
    fn account_schema_four_omits_unknown_prices_but_keeps_zero() {
        let projection =
            AccountProjection::empty("account-test".into(), "signed_out", "signed_out");
        assert_eq!(
            serde_json::to_value(projection).unwrap()["schemaVersion"],
            6
        );
        let billing = ModelBilling {
            groups: vec![BillingGroup {
                id: "default".into(),
                description: String::new(),
                ratio: None,
            }],
            base_input_usd: Some(0.0),
            base_output_usd: Some(2.0),
            cache_read_usd: Some(0.0),
            cache_write_usd: None,
            request_usd: None,
            expression: String::new(),
        };
        let value = serde_json::to_value(billing).unwrap();
        assert_eq!(value["baseInputUsd"], 0.0);
        assert_eq!(value["cacheReadUsd"], 0.0);
        assert!(value.get("cacheWriteUsd").is_none());
        assert!(value.get("requestUsd").is_none());
        assert!(value["groups"][0].get("ratio").is_none());
    }

    #[test]
    fn cancelled_authorization_cannot_install_late_credentials() {
        // Both branches reject before any OS credential storage operation.
        let state = AccountV2State::default();
        state.runtime.lock().unwrap().authorization_epoch = 2;
        let bundle = || AuthBundle {
            access_token: "synthetic".into(),
            token_type: "Bearer".into(),
            access_expires_at: 0,
            refresh_token: "synthetic".into(),
            refresh_expires_at: 0,
            session_id: "synthetic".into(),
        };
        assert!(install_auth_bundle(&state, "mainland_optimized", bundle(), 1, None).is_err());
        assert!(install_auth_bundle(
            &state,
            "mainland_optimized",
            bundle(),
            2,
            Some("old-device-code")
        )
        .is_err());
        assert!(state.runtime.lock().unwrap().access.is_none());
    }

    #[tokio::test]
    async fn account_switch_before_session_step_never_polls_new_account_work() {
        let state = AccountV2State::default();
        let epoch = native_session_epoch(&state).unwrap();
        state.runtime.lock().unwrap().authorization_epoch += 1;
        let called = std::cell::Cell::new(false);
        let result = session_bound_step(&state, epoch, async {
            called.set(true);
            Ok("new-account-credential")
        })
        .await;
        assert_eq!(result, Err(NativeSessionFailure::AccountChanged));
        assert!(!called.get());
    }

    #[tokio::test]
    async fn account_switch_during_session_step_cannot_reach_tool_token_or_write() {
        let state = AccountV2State::default();
        let epoch = native_session_epoch(&state).unwrap();
        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        let (resume_tx, resume_rx) = tokio::sync::oneshot::channel();
        let step = session_bound_step(&state, epoch, async {
            started_tx.send(()).unwrap();
            resume_rx.await.unwrap();
            Ok("synthetic-new-account-credential")
        });
        let switch = async {
            started_rx.await.unwrap();
            state.runtime.lock().unwrap().authorization_epoch += 1;
            resume_tx.send(()).unwrap();
        };
        let (result, ()) = tokio::join!(step, switch);
        assert_eq!(result, Err(NativeSessionFailure::AccountChanged));
        assert_eq!(
            ensure_session_epoch(&state, epoch),
            Err(NativeSessionFailure::AccountChanged)
        );
    }

    #[test]
    fn request_and_url_allowlists_reject_caller_controlled_network_targets() {
        assert!(request_is_valid(&AccountRequest {
            request_id: "account-1".into(),
            line_id: "mainland_optimized".into(),
        }));
        assert!(!request_is_valid(&AccountRequest {
            request_id: "../secret".into(),
            line_id: "mainland_optimized".into(),
        }));
        assert!(!request_is_valid(&AccountRequest {
            request_id: "account-1".into(),
            line_id: "https://attacker.invalid".into(),
        }));
        assert!(wallet_url_is_allowed("https://yeschoy.com/wallet/"));
        assert!(!wallet_url_is_allowed(
            "https://yeschoy.com.attacker.invalid/wallet/"
        ));
        assert!(!wallet_url_is_allowed(
            "https://yeschoy.com/wallet/?next=https://attacker.invalid"
        ));
        assert!(authorization_url_is_allowed(
            "https://yeschoy.com/desktop-authorize?user_code=ABCD-2345",
            "ABCD-2345"
        ));
        assert!(!authorization_url_is_allowed(
            "https://yeschoy.com/desktop-authorize?user_code=ABCD-2345&next=x",
            "ABCD-2345"
        ));
    }

    #[test]
    fn authorization_page_origin_is_compile_time_and_exactly_allowlisted() {
        assert!(authorization_page_origin_is_allowed(
            CANONICAL_AUTHORIZATION_PAGE_ORIGIN
        ));
        assert!(authorization_page_origin_is_allowed(
            PARTNER_AUTHORIZATION_PAGE_ORIGIN
        ));
        assert!(authorization_page_origin_is_allowed(
            COMPILED_AUTHORIZATION_PAGE_ORIGIN
        ));
        for rejected in [
            "http://yeschoy.com",
            "https://yeschoy.com/desktop-authorize",
            "https://yeschoy.com?next=https://attacker.invalid",
            "https://ai.yeschoy.io:443",
            "https://user@ai.yeschoy.io",
            "https://ai.yeschoy.io.attacker.invalid",
            "https://attacker.invalid",
        ] {
            assert!(!authorization_page_origin_is_allowed(rejected));
        }
    }

    #[test]
    fn account_projection_preserves_negative_balance_and_rejects_negative_counters() {
        let status = json!({
            "success": true,
            "data": {"quota_per_unit": 500000}
        });
        let account = json!({
            "success": true,
            "data": {
                "display_name": "欠费账户",
                "username": "member@example.com",
                "quota": -125000,
                "used_quota": 120000,
                "request_count": 42
            }
        });

        let projected = parse_account(Some(&account), Some(&status));
        assert!(projected.available);
        assert_eq!(projected.balance_quota, "-125000");
        assert_eq!(projected.quota_per_unit, "500000");

        for field in ["used_quota", "request_count"] {
            let mut invalid = account.clone();
            invalid["data"][field] = json!(-1);
            assert!(!parse_account(Some(&invalid), Some(&status)).available);
        }
    }

    #[test]
    fn branded_authorization_url_is_rebuilt_from_validated_user_code_only() {
        assert_eq!(
            browser_authorization_url_for_origin(CANONICAL_AUTHORIZATION_PAGE_ORIGIN, "ABCD-2345")
                .as_deref(),
            Some("https://yeschoy.com/desktop-authorize?user_code=ABCD-2345")
        );
        assert_eq!(
            browser_authorization_url_for_origin(PARTNER_AUTHORIZATION_PAGE_ORIGIN, "ABCD-2345")
                .as_deref(),
            Some("https://ai.yeschoy.io/desktop-authorize?user_code=ABCD-2345")
        );
        assert!(browser_authorization_url_for_origin(
            PARTNER_AUTHORIZATION_PAGE_ORIGIN,
            "unsafe&next=https://attacker.invalid"
        )
        .is_none());
    }

    #[test]
    fn compiled_authorization_page_origin_drives_the_browser_target() {
        assert_eq!(
            browser_authorization_url("ABCD-2345"),
            Some(format!(
                "{COMPILED_AUTHORIZATION_PAGE_ORIGIN}/desktop-authorize?user_code=ABCD-2345"
            ))
        );
    }

    #[test]
    fn price_projection_is_ratio_based_and_never_prices_unsupported_rows() {
        let models = json!({"success": true, "data": ["model-a", "model-b"]});
        let account = json!({"success": true, "data": {"group": "default"}});
        let pricing = json!({
            "success": true,
            "group_ratio": {"default": 0.5},
            "data": [
                {"model_name": "model-a", "quota_type": 0, "model_ratio": 1.0, "completion_ratio": 3.0},
                {"model_name": "model-b", "quota_type": 1, "model_price": 0.1}
            ]
        });
        let rows = parse_models(Some(&models), Some(&pricing), Some(&account), Some(7.0));
        assert_eq!(rows.len(), 2);
        assert!(rows[0].pricing_available);
        assert_eq!(rows[0].official_input_cny_per_million, "14");
        assert_eq!(rows[0].actual_input_cny_per_million, "7");
        assert!(!rows[1].pricing_available);
        assert!(rows[1].official_input_cny_per_million.is_empty());
    }

    #[test]
    fn model_projection_omits_internal_descriptions_without_changing_prices() {
        let models = json!({"success":true,"data":["claude-haiku-4-5"]});
        let account = json!({"success":true,"data":{"group":"default"}});
        let mut pricing = json!({"success":true,"group_ratio":{"default":0.5},
        "usable_group":{"default":"标准分组"},"data":[{
            "model_name":"claude-haiku-4-5","description":"Synthetic upstream via internal connector",
            "quota_type":0,"model_ratio":1.0,"completion_ratio":3.0,
            "enable_groups":["default"],"supported_endpoint_types":["anthropic"]
        }]});
        let projected = parse_models(Some(&models), Some(&pricing), Some(&account), Some(1.0));
        assert_eq!(projected[0].id, "claude-haiku-4-5");
        assert!(projected[0].description.is_empty());
        assert_eq!(projected[0].actual_input_cny_per_million, "1");
        assert_eq!(
            projected[0].billing.as_ref().unwrap().groups[0].description,
            "标准分组"
        );
        let serialized = serde_json::to_value(&projected).unwrap();
        assert!(!serialized.to_string().contains("Synthetic upstream"));
        pricing["data"][0]["description"] = json!("A different private channel label");
        assert_eq!(
            serialized,
            serde_json::to_value(parse_models(
                Some(&models),
                Some(&pricing),
                Some(&account),
                Some(1.0)
            ))
            .unwrap()
        );
    }

    #[test]
    fn billing_group_intersection_keeps_zero_and_unknown_prices_without_guessing() {
        let pricing = json!({"success": true,
            "usable_group": {"default": "", "special": "账户专属", "free": "", "unpriced": "", "other": "", "auto": ""},
            "group_ratio": {"default": 1, "special": 0.35, "free": 0, "private": 0.1},
            "data": [{"model_name": "m", "enable_groups": ["default", "special", "free", "unpriced", "private", "auto"]}]
        });
        let groups = billing_groups(&pricing, "m");
        assert_eq!(
            groups.iter().map(|g| g.id.as_str()).collect::<Vec<_>>(),
            ["default", "free", "special", "unpriced"]
        );
        assert_eq!(
            groups.iter().find(|g| g.id == "free").unwrap().ratio,
            Some(0.0)
        );
        assert_eq!(
            groups.iter().find(|g| g.id == "special").unwrap().ratio,
            Some(0.35)
        );
        assert_eq!(
            groups.iter().find(|g| g.id == "unpriced").unwrap().ratio,
            None
        );
        assert!(billing_groups(&pricing, "missing-model").is_empty());
        assert!(billing_groups(&json!({"success": false}), "m").is_empty());
    }

    #[test]
    fn dynamic_billing_metadata_is_projected_without_claiming_flat_prices() {
        let pricing = json!({"success": true, "usable_group": {"discount": ""}, "group_ratio": {"discount": 0.35},
            "data": [{"model_name":"m", "enable_groups":["discount"], "billing_mode":"tiered_expr",
                "model_ratio":0.11,"completion_ratio":3,"billing_expr":"tier(\"base\", p * 3 + c * 9)"}]});
        let models = json!({"success":true,"data":["m"]});
        let projected = parse_models(Some(&models), Some(&pricing), None, Some(7.0));
        assert!(!projected[0].pricing_available);
        let billing = projected[0].billing.as_ref().unwrap();
        assert_eq!(billing.groups[0].ratio, Some(0.35));
        assert_eq!(billing.expression, "tier(\"base\", p * 3 + c * 9)");
    }

    #[test]
    fn projection_serialization_contains_no_secret_fields() {
        let projection = AccountProjection::empty("safe".into(), "signed_out", "signed_out");
        let value = serde_json::to_value(projection).unwrap();
        let serialized = value.to_string();
        for forbidden in [
            "accessToken",
            "refreshToken",
            "sessionId",
            "deviceCode",
            "walletUrl",
        ] {
            assert!(!serialized.contains(forbidden));
        }
    }
}

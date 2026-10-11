//! HTTPS exchange client for station deep-link connect codes (T3).
//!
//! Deliberately separate from the compile-time authorization-page allowlist in
//! `account_v2` / `oauth_pkce`. Third-party HTTPS origins are exchanged when
//! they pass `connect_origin_is_acceptable`.

use std::time::Duration;

use reqwest::{
    header::{ACCEPT, CONTENT_TYPE},
    redirect::Policy,
    Client, StatusCode,
};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::account_v2::{shared_http_client, StationConnectAuthBundle};
use crate::station_connect::{
    connect_origin_is_acceptable, CONNECT_CODE_MAX_LEN, CONNECT_CODE_MIN_LEN,
};

pub(crate) const CONNECT_CODES_ISSUE_PATH: &str = "/api/desktop/v2/connect-codes";
pub(crate) const CONNECT_CODES_EXCHANGE_PATH: &str = "/api/desktop/v2/connect-codes/exchange";

const REQUEST_TIMEOUT: Duration = Duration::from_secs(20);
const MAX_RESPONSE_BYTES: usize = 2 * 1024 * 1024;

/// When `false`, deep links are validated but no network exchange runs (server rollout).
pub(crate) fn station_connect_exchange_enabled() -> bool {
    match std::env::var("YESCHOY_STATION_CONNECT_EXCHANGE") {
        Ok(value) => matches!(value.as_str(), "1" | "true" | "TRUE" | "yes" | "YES"),
        Err(_) => true,
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum StationConnectExchangeError {
    Disabled,
    OriginNotAcceptable,
    OriginMismatch,
    Network,
    TimedOut,
    InvalidResponse,
    ResponseTooLarge,
    CodeExpired,
    CodeAlreadyUsed,
    ServerRejected,
    Contract,
    Storage,
}

impl StationConnectExchangeError {
    pub(crate) fn reason_code(self) -> &'static str {
        match self {
            Self::Disabled => "connect_exchange_disabled",
            Self::OriginNotAcceptable => "connect_origin_not_acceptable",
            Self::OriginMismatch => "connect_origin_mismatch",
            Self::Network => "connect_network_error",
            Self::TimedOut => "connect_network_timeout",
            Self::InvalidResponse => "connect_invalid_response",
            Self::ResponseTooLarge => "connect_response_too_large",
            Self::CodeExpired => "connect_code_expired",
            Self::CodeAlreadyUsed => "connect_code_already_used",
            Self::ServerRejected => "connect_server_rejected",
            Self::Contract => "connect_auth_contract",
            Self::Storage => "connect_secure_storage_unavailable",
        }
    }
}

#[derive(Debug)]
struct JsonResponse {
    status: StatusCode,
    value: Value,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ExchangeEnvelope {
    success: bool,
    data: Option<StationConnectAuthBundle>,
    code: Option<String>,
}

/// Build the POST URL for connect-code exchange and ensure it matches `origin`.
pub(crate) fn connect_exchange_url(origin: &str) -> Result<String, StationConnectExchangeError> {
    let canonical = connect_origin_is_acceptable(origin)
        .ok_or(StationConnectExchangeError::OriginNotAcceptable)?;
    let url = format!("{canonical}{CONNECT_CODES_EXCHANGE_PATH}");
    if exchange_url_origin(&url) != Some(canonical) {
        return Err(StationConnectExchangeError::OriginMismatch);
    }
    Ok(url)
}

pub(crate) async fn exchange_connect_code(
    origin: &str,
    code: &str,
) -> Result<StationConnectAuthBundle, StationConnectExchangeError> {
    if !station_connect_exchange_enabled() {
        return Err(StationConnectExchangeError::Disabled);
    }
    if connect_origin_is_acceptable(origin).is_none() {
        return Err(StationConnectExchangeError::OriginNotAcceptable);
    }
    if !connect_code_is_valid_for_exchange(code) {
        return Err(StationConnectExchangeError::Contract);
    }
    let url = connect_exchange_url(origin)?;
    perform_connect_exchange(&url, code).await
}

async fn perform_connect_exchange(
    url: &str,
    code: &str,
) -> Result<StationConnectAuthBundle, StationConnectExchangeError> {
    let body = connect_exchange_request_body(code);
    let response = post_json(url, body).await.map_err(map_transport_error)?;
    map_exchange_http_response(response.status, &response.value)
}

pub(crate) fn map_exchange_http_response(
    status: StatusCode,
    body: &Value,
) -> Result<StationConnectAuthBundle, StationConnectExchangeError> {
    if status.is_success() {
        return parse_exchange_success(body);
    }
    let code = exchange_error_code(body);
    if status == StatusCode::UNAUTHORIZED || status == StatusCode::FORBIDDEN {
        return Err(classify_exchange_error_code(code));
    }
    if status == StatusCode::NOT_FOUND {
        return Err(StationConnectExchangeError::ServerRejected);
    }
    if status.is_client_error() {
        return Err(classify_exchange_error_code(code));
    }
    if status.is_server_error() {
        return Err(StationConnectExchangeError::ServerRejected);
    }
    Err(StationConnectExchangeError::InvalidResponse)
}

fn parse_exchange_success(
    body: &Value,
) -> Result<StationConnectAuthBundle, StationConnectExchangeError> {
    let envelope: ExchangeEnvelope = serde_json::from_value(body.clone())
        .map_err(|_| StationConnectExchangeError::InvalidResponse)?;
    if !envelope.success {
        let code = envelope.code.as_deref().unwrap_or("invalid_response");
        return Err(classify_exchange_error_code(code));
    }
    let bundle = envelope
        .data
        .ok_or(StationConnectExchangeError::InvalidResponse)?;
    if auth_bundle_contract_ok(&bundle) {
        Ok(bundle)
    } else {
        Err(StationConnectExchangeError::Contract)
    }
}

fn auth_bundle_contract_ok(bundle: &StationConnectAuthBundle) -> bool {
    bundle.token_type == "Bearer"
        && bundle.access_token.len() >= 32
        && bundle.refresh_token.len() >= 32
        && !bundle.session_id.is_empty()
        && bundle.access_expires_at > now_epoch_seconds()
        && bundle.refresh_expires_at > bundle.access_expires_at
}

fn classify_exchange_error_code(code: &str) -> StationConnectExchangeError {
    match code {
        "expired_token" | "authorization_expired" | "connect_code_expired" | "code_expired" => {
            StationConnectExchangeError::CodeExpired
        }
        "already_used"
        | "authorization_already_used"
        | "connect_code_already_used"
        | "code_already_used"
        | "connect_code_reused" => StationConnectExchangeError::CodeAlreadyUsed,
        "origin_mismatch" | "connect_origin_mismatch" => {
            StationConnectExchangeError::OriginMismatch
        }
        _ => StationConnectExchangeError::ServerRejected,
    }
}

fn exchange_error_code(body: &Value) -> &str {
    body.get("code")
        .and_then(Value::as_str)
        .or_else(|| {
            body.get("error")
                .and_then(Value::as_object)
                .and_then(|object| object.get("code"))
                .and_then(Value::as_str)
        })
        .unwrap_or("invalid_response")
}

fn connect_code_is_valid_for_exchange(code: &str) -> bool {
    (CONNECT_CODE_MIN_LEN..=CONNECT_CODE_MAX_LEN).contains(&code.len())
        && code
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}

fn connect_exchange_request_body(code: &str) -> Value {
    json!({
        "code": code,
        "deviceId": uuid::Uuid::new_v4().to_string(),
        "platform": connect_platform(),
        "clientVersion": env!("CARGO_PKG_VERSION"),
    })
}

fn exchange_url_origin(url: &str) -> Option<String> {
    let parsed = reqwest::Url::parse(url).ok()?;
    let origin = parsed.origin().ascii_serialization();
    connect_origin_is_acceptable(&origin)
}

const fn connect_platform() -> &'static str {
    if cfg!(target_os = "macos") {
        "macos"
    } else if cfg!(target_os = "windows") {
        "windows"
    } else if cfg!(target_os = "linux") {
        "linux"
    } else {
        "unknown"
    }
}

fn now_epoch_seconds() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    (SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        .min(i64::MAX as u64)) as i64
}

fn map_transport_error(error: TransportFailure) -> StationConnectExchangeError {
    match error {
        TransportFailure::TimedOut => StationConnectExchangeError::TimedOut,
        TransportFailure::Network | TransportFailure::InvalidResponse => {
            StationConnectExchangeError::Network
        }
        TransportFailure::ResponseTooLarge => StationConnectExchangeError::ResponseTooLarge,
    }
}

#[derive(Debug)]
enum TransportFailure {
    Network,
    TimedOut,
    InvalidResponse,
    ResponseTooLarge,
}

fn map_reqwest_error(error: reqwest::Error) -> TransportFailure {
    if error.is_timeout() {
        TransportFailure::TimedOut
    } else {
        TransportFailure::Network
    }
}

async fn post_json(url: &str, body: Value) -> Result<JsonResponse, TransportFailure> {
    let client = http_client_for_url(url)?;
    let response = client
        .post(url)
        .header(ACCEPT, "application/json")
        .header(CONTENT_TYPE, "application/json")
        .body(body.to_string())
        .send()
        .await
        .map_err(map_reqwest_error)?;
    read_body(response).await
}

fn http_client_for_url(url: &str) -> Result<Client, TransportFailure> {
    if cfg!(test) && url.starts_with("http://127.0.0.1:") {
        return build_exchange_client(false);
    }
    exchange_http_client()
}

fn exchange_http_client() -> Result<Client, TransportFailure> {
    if let Ok(client) = shared_http_client() {
        return Ok(client.clone());
    }
    build_exchange_client(true)
}

fn build_exchange_client(https_only: bool) -> Result<Client, TransportFailure> {
    let mut builder = Client::builder()
        .redirect(Policy::none())
        .referer(false)
        .no_proxy()
        .timeout(REQUEST_TIMEOUT)
        .connect_timeout(Duration::from_secs(6))
        .user_agent(concat!("YesChoyDesktop/", env!("CARGO_PKG_VERSION")));
    if https_only {
        builder = builder.https_only(true);
    }
    builder.build().map_err(|_| TransportFailure::Network)
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
    let mut buffer = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(map_reqwest_error)? {
        if buffer.len() + chunk.len() > MAX_RESPONSE_BYTES {
            return Err(TransportFailure::ResponseTooLarge);
        }
        buffer.extend_from_slice(&chunk);
    }
    let value: Value =
        serde_json::from_slice(&buffer).map_err(|_| TransportFailure::InvalidResponse)?;
    Ok(JsonResponse { status, value })
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{routing::post, Json, Router};
    use serde_json::json;
    use tokio::net::TcpListener;

    const VALID_CODE: &str = "abcdefghijklmnopqrstuvwxyz012345";

    fn sample_bundle() -> StationConnectAuthBundle {
        let now = now_epoch_seconds();
        StationConnectAuthBundle {
            access_token: "a".repeat(48),
            token_type: "Bearer".to_owned(),
            access_expires_at: now + 3600,
            refresh_token: "r".repeat(48),
            refresh_expires_at: now + 86_400,
            session_id: "session-connect-test".to_owned(),
        }
    }

    #[test]
    fn connect_exchange_url_requires_https_canonical_origin() {
        assert_eq!(
            connect_exchange_url("https://example.com").unwrap(),
            "https://example.com/api/desktop/v2/connect-codes/exchange"
        );
        assert_eq!(
            connect_exchange_url("http://example.com").unwrap_err(),
            StationConnectExchangeError::OriginNotAcceptable
        );
    }

    #[test]
    fn map_exchange_errors_for_expired_replay_and_origin() {
        assert_eq!(
            map_exchange_http_response(
                StatusCode::BAD_REQUEST,
                &json!({"code": "connect_code_expired"}),
            )
            .unwrap_err(),
            StationConnectExchangeError::CodeExpired
        );
        assert_eq!(
            map_exchange_http_response(
                StatusCode::CONFLICT,
                &json!({"code": "connect_code_already_used"}),
            )
            .unwrap_err(),
            StationConnectExchangeError::CodeAlreadyUsed
        );
        assert_eq!(
            map_exchange_http_response(
                StatusCode::FORBIDDEN,
                &json!({"code": "connect_origin_mismatch"}),
            )
            .unwrap_err(),
            StationConnectExchangeError::OriginMismatch
        );
        assert_eq!(
            map_exchange_http_response(StatusCode::BAD_GATEWAY, &json!({})).unwrap_err(),
            StationConnectExchangeError::ServerRejected
        );
    }

    #[test]
    fn map_exchange_success_parses_envelope() {
        let bundle = sample_bundle();
        let body = json!({
            "success": true,
            "data": {
                "accessToken": bundle.access_token,
                "tokenType": bundle.token_type,
                "accessExpiresAt": bundle.access_expires_at,
                "refreshToken": bundle.refresh_token,
                "refreshExpiresAt": bundle.refresh_expires_at,
                "sessionId": bundle.session_id,
            }
        });
        let parsed = map_exchange_http_response(StatusCode::OK, &body).unwrap();
        assert_eq!(parsed.session_id, bundle.session_id);
    }

    #[tokio::test]
    async fn exchange_connect_code_at_url_posts_code_to_mock_origin() {
        let bundle = sample_bundle();
        let expected_session_id = bundle.session_id.clone();
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let app = Router::new().route(
            CONNECT_CODES_EXCHANGE_PATH,
            post(move |Json(payload): Json<Value>| {
                assert_eq!(payload["code"], VALID_CODE);
                let bundle = bundle.clone();
                async move {
                    Json(json!({
                        "success": true,
                        "data": {
                            "accessToken": bundle.access_token,
                            "tokenType": bundle.token_type,
                            "accessExpiresAt": bundle.access_expires_at,
                            "refreshToken": bundle.refresh_token,
                            "refreshExpiresAt": bundle.refresh_expires_at,
                            "sessionId": bundle.session_id,
                        }
                    }))
                }
            }),
        );
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        // Loopback mock uses HTTP; production exchange is HTTPS-only on the declared origin.
        let exchange_url = format!("http://127.0.0.1:{port}{CONNECT_CODES_EXCHANGE_PATH}");
        let parsed = perform_connect_exchange(&exchange_url, VALID_CODE)
            .await
            .expect("mock exchange");
        assert_eq!(parsed.session_id, expected_session_id);
    }

    #[tokio::test]
    async fn exchange_connect_code_rejects_http_origin_downgrade_before_network() {
        let err = exchange_connect_code("http://example.com", VALID_CODE)
            .await
            .unwrap_err();
        assert_eq!(err, StationConnectExchangeError::OriginNotAcceptable);
        assert_eq!(err.reason_code(), "connect_origin_not_acceptable");
    }

    #[tokio::test]
    async fn production_exchange_client_blocks_clear_http_downgrade() {
        let client = build_exchange_client(true).expect("https-only client");
        let response = client
            .get("http://example.com/api/desktop/v2/connect-codes/exchange")
            .send()
            .await;
        assert!(
            response.is_err(),
            "https-only client must not follow http://"
        );
    }

    #[tokio::test]
    async fn mock_exchange_maps_expired_code() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let app = Router::new().route(
            CONNECT_CODES_EXCHANGE_PATH,
            post(|_payload: Json<Value>| async move {
                (
                    StatusCode::BAD_REQUEST,
                    Json(json!({"code": "connect_code_expired"})),
                )
            }),
        );
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        let url = format!("http://127.0.0.1:{port}{CONNECT_CODES_EXCHANGE_PATH}");
        let expired = perform_connect_exchange(&url, VALID_CODE)
            .await
            .unwrap_err();
        assert_eq!(expired, StationConnectExchangeError::CodeExpired);
        assert_eq!(expired.reason_code(), "connect_code_expired");
    }

    #[tokio::test]
    async fn mock_exchange_maps_origin_tamper() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let app = Router::new().route(
            CONNECT_CODES_EXCHANGE_PATH,
            post(|_payload: Json<Value>| async move {
                (
                    StatusCode::FORBIDDEN,
                    Json(json!({"code": "connect_origin_mismatch"})),
                )
            }),
        );
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        let url = format!("http://127.0.0.1:{port}{CONNECT_CODES_EXCHANGE_PATH}");
        let tamper = perform_connect_exchange(&url, VALID_CODE)
            .await
            .unwrap_err();
        assert_eq!(tamper, StationConnectExchangeError::OriginMismatch);
        assert_eq!(tamper.reason_code(), "connect_origin_mismatch");
    }

    #[tokio::test]
    async fn mock_exchange_rejects_code_replay() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::sync::Arc;

        let bundle = sample_bundle();
        let expected_session = bundle.session_id.clone();
        let attempts = Arc::new(AtomicUsize::new(0));
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let app = Router::new().route(
            CONNECT_CODES_EXCHANGE_PATH,
            post(move |_payload: Json<Value>| {
                let bundle = bundle.clone();
                let attempts = Arc::clone(&attempts);
                async move {
                    let n = attempts.fetch_add(1, Ordering::SeqCst);
                    if n == 0 {
                        (
                            StatusCode::OK,
                            Json(json!({
                                "success": true,
                                "data": {
                                    "accessToken": bundle.access_token,
                                    "tokenType": bundle.token_type,
                                    "accessExpiresAt": bundle.access_expires_at,
                                    "refreshToken": bundle.refresh_token,
                                    "refreshExpiresAt": bundle.refresh_expires_at,
                                    "sessionId": bundle.session_id,
                                }
                            })),
                        )
                    } else {
                        (
                            StatusCode::CONFLICT,
                            Json(json!({"code": "connect_code_already_used"})),
                        )
                    }
                }
            }),
        );
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        let url = format!("http://127.0.0.1:{port}{CONNECT_CODES_EXCHANGE_PATH}");
        let replay_ok = perform_connect_exchange(&url, VALID_CODE)
            .await
            .expect("first exchange");
        assert_eq!(replay_ok.session_id, expected_session);
        let replay_again = perform_connect_exchange(&url, VALID_CODE)
            .await
            .unwrap_err();
        assert_eq!(replay_again, StationConnectExchangeError::CodeAlreadyUsed);
        assert_eq!(replay_again.reason_code(), "connect_code_already_used");
    }

    #[test]
    fn map_exchange_classifies_reused_code_alias() {
        assert_eq!(
            map_exchange_http_response(
                StatusCode::CONFLICT,
                &json!({"code": "connect_code_reused"}),
            )
            .unwrap_err(),
            StationConnectExchangeError::CodeAlreadyUsed
        );
    }
}

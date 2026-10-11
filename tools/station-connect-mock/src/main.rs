//! Local mock station: issue + exchange connect codes for desktop E2E dev.
//!
//! Not for production. See `docs/station-connect-e2e-mock.md`.

use std::{
    collections::HashMap,
    net::SocketAddr,
    sync::{Arc, Mutex},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use axum::{
    extract::{Json, State},
    http::{header, HeaderMap, StatusCode},
    response::{Html, IntoResponse, Response},
    routing::{get, post},
    Router,
};
use axum_extra::extract::CookieJar;
use cookie::{Cookie, SameSite};
use rand::{distributions::Alphanumeric, Rng};
use serde::{Deserialize, Serialize};
use serde_json::json;
use tower_http::cors::{Any, CorsLayer};
use tower_http::trace::TraceLayer;

/// Hard cap per product spec (≤ 5 minutes).
const CODE_TTL_SECS: i64 = 300;
const SESSION_COOKIE: &str = "yeschoy_desktop_session";
const ISSUE_PATH: &str = "/api/desktop/v2/connect-codes";
const EXCHANGE_PATH: &str = "/api/desktop/v2/connect-codes/exchange";
const BOOTSTRAP_PATH: &str = "/api/desktop/v2/bootstrap";
const NOTICES_PATH: &str = "/api/desktop/v2/notices";
const CONNECT_PAGE_PATH: &str = "/desktop/connect";
const MIN_CODE_LEN: usize = 32;

#[derive(Clone)]
struct AppState {
    codes: Arc<Mutex<HashMap<String, StoredCode>>>,
}

#[derive(Clone)]
struct StoredCode {
    origin: String,
    expires_at: i64,
    used: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct IssueRequest {
    origin: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ExchangeRequest {
    code: String,
    #[allow(dead_code)]
    device_id: Option<String>,
    #[allow(dead_code)]
    platform: Option<String>,
    #[allow(dead_code)]
    client_version: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct AuthBundle {
    access_token: String,
    token_type: String,
    access_expires_at: i64,
    refresh_token: String,
    refresh_expires_at: i64,
    session_id: String,
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let port = std::env::var("STATION_CONNECT_MOCK_PORT")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(8787_u16);

    let state = AppState {
        codes: Arc::new(Mutex::new(HashMap::new())),
    };

    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    let app = Router::new()
        .route("/", get(harness_page))
        .route(CONNECT_PAGE_PATH, get(connect_page))
        .route("/mock/login", post(mock_login))
        .route(BOOTSTRAP_PATH, get(bootstrap))
        .route(NOTICES_PATH, get(notices))
        .route(ISSUE_PATH, post(issue_code))
        .route(EXCHANGE_PATH, post(exchange_code))
        .layer(cors)
        .layer(TraceLayer::new_for_http())
        .with_state(state);

    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    tracing::info!("station-connect-mock listening on http://{addr}");
    tracing::info!("harness UI: http://{addr}/");
    let listener = tokio::net::TcpListener::bind(addr).await.expect("bind");
    axum::serve(listener, app).await.expect("serve");
}

async fn harness_page() -> Html<&'static str> {
    Html(HARNESS_HTML)
}

async fn connect_page() -> Html<&'static str> {
    Html(include_str!("../../../public/desktop/connect.html"))
}

async fn bootstrap(headers: HeaderMap) -> Response {
    let origin = match origin_from_request_host(&headers) {
        Ok(origin) => origin,
        Err(code) => return api_error(StatusCode::BAD_REQUEST, code),
    };
    api_success(mock_bootstrap_payload(&origin))
}

async fn notices(headers: HeaderMap) -> Response {
    if !bearer_present(&headers) {
        return api_error(StatusCode::UNAUTHORIZED, "session_required");
    }
    let origin = match origin_from_request_host(&headers) {
        Ok(origin) => origin,
        Err(code) => return api_error(StatusCode::BAD_REQUEST, code),
    };
    let now_ms = now_epoch() as u64 * 1000;
    api_success(json!({
        "notices": [{
            "id": "mock-station-welcome",
            "title": "Station announcements (mock)",
            "body": "This notice is served from the connected station origin, not hardcoded in the desktop OSS build.",
            "severity": "info",
            "publishedAtEpochMs": now_ms,
            "expiresAtEpochMs": 0,
            "banner": true,
            "action": {
                "kind": "link",
                "label": "Open site",
                "url": format!("{origin}/")
            }
        }]
    }))
}

fn bearer_present(headers: &HeaderMap) -> bool {
    headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.starts_with("Bearer ") && value.len() > 10)
}

fn mock_bootstrap_payload(origin: &str) -> serde_json::Value {
    json!({
        "schema_version": 2,
        "service": "yeschoy-desktop",
        "contract_id": "desktop-integration-v2",
        "minimum_client_version": "0.0.1",
        "device_authorization_available": false,
        "oauth_loopback_pkce_available": false,
        "account_read": true,
        "usage_read": true,
        "models_read": true,
        "pricing_read": true,
        "tool_keys_manage": true,
        "official_usd_cny_rate": 7.0,
        "authorization_start_path": "/api/desktop/v2/device-authorizations",
        "authorization_token_path": "/api/desktop/v2/device-authorizations/token",
        "session_refresh_path": "/api/desktop/v2/sessions/refresh",
        "session_logout_path": "/api/desktop/v2/sessions/current",
        "account_path": "/api/user/self",
        "usage_summary_path": "/api/log/self/stat",
        "usage_records_path": "/api/log/self",
        "models_path": "/api/user/models",
        "pricing_path": "/api/pricing",
        "tool_keys_path": "/api/token/",
        "wallet_url": format!("{origin}/wallet/"),
        "announcements_path": NOTICES_PATH
    })
}

async fn mock_login(jar: CookieJar) -> impl IntoResponse {
    let cookie = Cookie::build((SESSION_COOKIE, "mock-dev-session"))
        .path("/")
        .http_only(true)
        .same_site(SameSite::Lax)
        .build();
    (StatusCode::NO_CONTENT, jar.add(cookie))
}

async fn issue_code(
    State(state): State<AppState>,
    jar: CookieJar,
    headers: HeaderMap,
    Json(body): Json<IssueRequest>,
) -> Response {
    if !session_present(&jar, &headers) {
        return api_error(StatusCode::UNAUTHORIZED, "session_required");
    }

    let origin = match resolve_issue_origin(&headers, body.origin) {
        Ok(origin) => origin,
        Err(code) => return api_error(StatusCode::BAD_REQUEST, code),
    };

    let code = generate_connect_code();
    let now = now_epoch();
    let expires_at = now + CODE_TTL_SECS;
    {
        let mut guard = state.codes.lock().expect("codes lock");
        guard.insert(
            code.clone(),
            StoredCode {
                origin: origin.clone(),
                expires_at,
                used: false,
            },
        );
    }

    api_success(json!({
        "code": code,
        "origin": origin,
        "expiresAt": expires_at,
        "ttlSeconds": CODE_TTL_SECS,
    }))
}

async fn exchange_code(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<ExchangeRequest>,
) -> Response {
    if body.code.len() < MIN_CODE_LEN {
        return api_error(StatusCode::BAD_REQUEST, "connect_code_invalid");
    }

    let request_origin = match origin_from_request_host(&headers) {
        Ok(origin) => origin,
        Err(code) => return api_error(StatusCode::BAD_REQUEST, code),
    };

    let mut guard = state.codes.lock().expect("codes lock");
    let record = guard.get_mut(&body.code);
    let Some(record) = record else {
        return api_error(StatusCode::BAD_REQUEST, "connect_code_expired");
    };

    if record.used {
        return api_error(StatusCode::CONFLICT, "connect_code_already_used");
    }
    let now = now_epoch();
    if now >= record.expires_at {
        return api_error(StatusCode::BAD_REQUEST, "connect_code_expired");
    }
    if record.origin != request_origin {
        return api_error(StatusCode::FORBIDDEN, "connect_origin_mismatch");
    }

    record.used = true;
    let bundle = mock_auth_bundle(&body.code);
    api_success(json!(bundle))
}

fn session_present(jar: &CookieJar, headers: &HeaderMap) -> bool {
    if jar.get(SESSION_COOKIE).is_some() {
        return true;
    }
    headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.starts_with("Bearer ") && value.len() > 10)
}

fn resolve_issue_origin(
    headers: &HeaderMap,
    body_origin: Option<String>,
) -> Result<String, &'static str> {
    let origin = body_origin
        .or_else(|| {
            headers
                .get(header::ORIGIN)
                .and_then(|value| value.to_str().ok())
                .map(str::to_owned)
        })
        .ok_or("connect_origin_required")?;
    canonical_https_origin(&origin).ok_or("connect_origin_invalid")
}

fn origin_from_request_host(headers: &HeaderMap) -> Result<String, &'static str> {
    let host = headers
        .get(header::HOST)
        .and_then(|value| value.to_str().ok())
        .ok_or("connect_origin_required")?;
    let host = host.split(':').next().unwrap_or(host);
    if host_is_literal_ip(host) {
        return Err("connect_origin_invalid");
    }
    Ok(format!("https://{}", host.to_ascii_lowercase()))
}

fn canonical_https_origin(raw: &str) -> Option<String> {
    let url = url::Url::parse(raw).ok()?;
    if url.scheme() != "https" || url.port().is_some() {
        return None;
    }
    if !url.username().is_empty() || url.password().is_some() {
        return None;
    }
    if !matches!(url.path(), "/" | "") || url.query().is_some() || url.fragment().is_some() {
        return None;
    }
    let host = url.host_str()?;
    if host.is_empty() || host_is_literal_ip(host) {
        return None;
    }
    let canonical = format!("https://{}", host.to_ascii_lowercase());
    if raw != canonical && raw != format!("{canonical}/") {
        return None;
    }
    Some(canonical)
}

fn host_is_literal_ip(host: &str) -> bool {
    let bare = host
        .strip_prefix('[')
        .and_then(|value| value.strip_suffix(']'))
        .unwrap_or(host);
    bare.parse::<std::net::Ipv4Addr>().is_ok() || bare.parse::<std::net::Ipv6Addr>().is_ok()
}

fn generate_connect_code() -> String {
    rand::thread_rng()
        .sample_iter(&Alphanumeric)
        .take(48)
        .map(char::from)
        .collect()
}

fn mock_auth_bundle(seed: &str) -> AuthBundle {
    let now = now_epoch();
    let access = format!("mock_access_{}_{}", seed, "a".repeat(32));
    let refresh = format!("mock_refresh_{}_{}", seed, "r".repeat(32));
    AuthBundle {
        access_token: access,
        token_type: "Bearer".to_owned(),
        access_expires_at: now + 3600,
        refresh_token: refresh,
        refresh_expires_at: now + 86_400,
        session_id: format!("mock-session-{}", &seed[..16.min(seed.len())]),
    }
}

fn now_epoch() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::ZERO)
        .as_secs() as i64
}

fn api_success(data: serde_json::Value) -> Response {
    (
        StatusCode::OK,
        [(header::CONTENT_TYPE, "application/json")],
        json!({ "success": true, "data": data }).to_string(),
    )
        .into_response()
}

fn api_error(status: StatusCode, code: &str) -> Response {
    (
        status,
        [(header::CONTENT_TYPE, "application/json")],
        json!({ "success": false, "code": code }).to_string(),
    )
        .into_response()
}

const HARNESS_HTML: &str = r#"<!DOCTYPE html>
<html lang="zh-CN">
<head>
  <meta charset="utf-8" />
  <title>打开桌面助手 — Station Connect Mock</title>
  <style>
    body { font-family: system-ui, sans-serif; max-width: 40rem; margin: 2rem auto; padding: 0 1rem; }
    button { font-size: 1rem; padding: 0.6rem 1.2rem; cursor: pointer; }
    label { display: block; margin-top: 1rem; }
    input { width: 100%; box-sizing: border-box; padding: 0.4rem; }
    pre { background: #f4f4f4; padding: 0.75rem; overflow-x: auto; font-size: 0.85rem; }
    .err { color: #b00020; }
  </style>
</head>
<body>
  <h1>打开桌面助手（本地 Mock）</h1>
  <p>模拟已登录站点点击按钮：申请一次性 code → 跳转 <code>yeschoy://connect</code>。仅供 E2E；生产站见 OpenAPI 文档。</p>
  <label>HTTPS origin（与深链 <code>origin</code> 一致，需通过 TLS 反代到本 mock）
    <input id="origin" value="https://connect.localtest.me" />
  </label>
  <p><button type="button" id="login">1. 模拟登录（设置 session cookie）</button></p>
  <p><button type="button" id="open">2. 打开桌面助手</button></p>
  <p id="status"></p>
  <pre id="log"></pre>
  <p>未安装桌面端时，应降级到公开下载页（本 harness 仅演示深链，不检测安装）。</p>
  <script>
    const logEl = document.getElementById("log");
    const statusEl = document.getElementById("status");
    function log(msg) { logEl.textContent += msg + "\n"; }

    document.getElementById("login").onclick = async () => {
      statusEl.textContent = "";
      const r = await fetch("/mock/login", { method: "POST", credentials: "include" });
      log("mock/login → " + r.status);
      statusEl.textContent = r.ok ? "已设置 mock session cookie" : "登录失败";
    };

    document.getElementById("open").onclick = async () => {
      statusEl.textContent = "";
      statusEl.className = "";
      const origin = document.getElementById("origin").value.trim();
      try {
        const r = await fetch("/api/desktop/v2/connect-codes", {
          method: "POST",
          credentials: "include",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify({ origin }),
        });
        const body = await r.json();
        log("issue → " + r.status + " " + JSON.stringify(body));
        if (!body.success) {
          statusEl.textContent = body.code || "issue failed";
          statusEl.className = "err";
          return;
        }
        const code = body.data.code;
        const deep = "yeschoy://connect?origin=" + encodeURIComponent(body.data.origin) + "&code=" + encodeURIComponent(code);
        log("deep link: " + deep);
        statusEl.textContent = "正在拉起桌面助手…";
        window.location.href = deep;
      } catch (e) {
        statusEl.textContent = String(e);
        statusEl.className = "err";
      }
    };
  </script>
</body>
</html>
"#;

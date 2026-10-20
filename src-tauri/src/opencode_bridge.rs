//! OpenCode 的本地转发：`127.0.0.1:15732/opencode/v1`。
//!
//! OpenCode 的 provider 配置只能写一个固定的 `apiKey`，不能像 Pi 那样
//! 运行一条命令去取。如果把中转站的 key 直接写进 `opencode.json`，key 就
//! 以明文躺在磁盘上。所以这里多一座桥：
//!
//! - `opencode.json` 里只写**本地令牌**（`ycg-…`）。它只在本机 127.0.0.1 上、
//!   并且只在桌面助手开着时有用，拿去打中转站没有任何用处；
//! - 中转站的 key 留在系统钥匙串里，由这座桥按模型取出来再转发。
//!
//! 协议是 OpenAI Chat Completions 原样透传，不做任何转换。
use axum::{
    body::{to_bytes, Body},
    extract::State,
    http::{header, Request, StatusCode},
    response::Response,
    routing::any,
    Router,
};
use futures::StreamExt;
use serde_json::{json, Value};

use crate::local_bridge::{locally_authorized, BridgeState, LocalBridgeRuntime, SharedState};

pub(crate) const PROXY_ADDRESS: &str = "127.0.0.1:15732";
pub(crate) const PROXY_PREFIX: &str = "/opencode";
pub(crate) const PROXY_BASE: &str = "http://127.0.0.1:15732/opencode/v1";
const MAX_REQUEST_BODY_BYTES: usize = 32 * 1024 * 1024;

pub(crate) fn opencode_runtime() -> LocalBridgeRuntime {
    LocalBridgeRuntime::new(PROXY_ADDRESS, PROXY_PREFIX, router)
}

/// 全局唯一的一座 OpenCode 桥。接入、打开、退出都走它。
pub(crate) fn global() -> &'static LocalBridgeRuntime {
    static RUNTIME: std::sync::OnceLock<LocalBridgeRuntime> = std::sync::OnceLock::new();
    RUNTIME.get_or_init(opencode_runtime)
}

fn router(state: SharedState) -> Router {
    Router::new().fallback(any(dispatch)).with_state(state)
}

fn log_label(method: &str, path: &str) -> &'static str {
    match (method, path) {
        ("POST", "/v1/chat/completions") => "/v1/chat/completions",
        ("GET", "/v1/models") => "/v1/models",
        _ => "<other>",
    }
}

fn error(status: StatusCode, message: &str) -> Response {
    Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(
            json!({"error": {"message": message, "type": "yeschoy_bridge"}}).to_string(),
        ))
        .unwrap_or_else(|_| Response::new(Body::empty()))
}

async fn dispatch(State(state): State<SharedState>, request: Request<Body>) -> Response {
    let state = state.read().await.clone();
    let Some(path) = request.uri().path().strip_prefix(state.prefix) else {
        return error(StatusCode::NOT_FOUND, "unsupported OpenCode endpoint");
    };
    let path = path.to_owned();
    log::info!(
        "opencode_bridge request method={} path={}",
        request.method().as_str(),
        log_label(request.method().as_str(), &path),
    );
    if !locally_authorized(request.headers(), &state.local_token) {
        return error(StatusCode::UNAUTHORIZED, "local token required");
    }
    match (request.method().as_str(), path.as_str()) {
        ("GET", "/v1/models") => models(&state),
        ("POST", "/v1/chat/completions") => chat(&state, request).await,
        _ => error(StatusCode::NOT_FOUND, "unsupported OpenCode endpoint"),
    }
}

fn models(state: &BridgeState) -> Response {
    let data = state
        .credential
        .model_ids()
        .into_iter()
        .map(|id| json!({"id": id, "object": "model", "owned_by": "yeschoy"}))
        .collect::<Vec<_>>();
    Response::builder()
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(
            json!({"object": "list", "data": data}).to_string(),
        ))
        .unwrap_or_else(|_| error(StatusCode::INTERNAL_SERVER_ERROR, "response failed"))
}

async fn chat(state: &BridgeState, request: Request<Body>) -> Response {
    let Ok(bytes) = to_bytes(request.into_body(), MAX_REQUEST_BODY_BYTES).await else {
        return error(StatusCode::PAYLOAD_TOO_LARGE, "request too large");
    };
    let Ok(body) = serde_json::from_slice::<Value>(&bytes) else {
        return error(StatusCode::BAD_REQUEST, "request body must be JSON");
    };
    let model = body
        .get("model")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned();
    // 每把 key 只覆盖它自己计费分组里的模型：按模型取 key，取不到就本地说清楚。
    let credential = match state.credential.resolve_model(&model) {
        Ok(credential) => credential,
        Err(_) if !state.credential.has_model_set() => state.credential.clone(),
        Err(_) => {
            log::warn!("opencode_bridge stage=model_outside_key_scope");
            return error(
                StatusCode::BAD_REQUEST,
                &format!(
                    "{model} is not in the model list this app was connected with. Open the \
                     desktop assistant, add {model} to the OpenCode model list and apply again."
                ),
            );
        }
    };
    let url = format!(
        "{}/v1/chat/completions",
        credential.origin.trim_end_matches('/')
    );
    let upstream = state
        .client
        .post(url)
        .bearer_auth(credential.upstream_key())
        .header(header::CONTENT_TYPE, "application/json")
        .body(bytes)
        .send()
        .await;
    let Ok(upstream) = upstream else {
        return error(StatusCode::BAD_GATEWAY, "upstream request failed");
    };
    let status = upstream.status();
    if status.is_success() {
        let _ = state.events.send(crate::local_bridge::VerificationEvent);
    }
    let content_type = upstream
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("application/json")
        .to_owned();
    let stream = upstream
        .bytes_stream()
        .map(|chunk| chunk.map_err(std::io::Error::other));
    Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, content_type)
        .body(Body::from_stream(stream))
        .unwrap_or_else(|_| error(StatusCode::INTERNAL_SERVER_ERROR, "response failed"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels_only_known_paths() {
        assert_eq!(
            log_label("POST", "/v1/chat/completions"),
            "/v1/chat/completions"
        );
        assert_eq!(log_label("GET", "/v1/secret?key=x"), "<other>");
    }

    #[test]
    fn base_matches_address_and_prefix() {
        assert_eq!(
            PROXY_BASE,
            format!("http://{PROXY_ADDRESS}{PROXY_PREFIX}/v1")
        );
    }
}

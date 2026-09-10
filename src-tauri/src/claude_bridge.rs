//! Claude Desktop 的本地别名转发器。
//!
//! Claude Desktop 只接受“看起来像 Anthropic 模型”的 route 名，所以客户端在
//! profile 里写的是 `claude-sonnet-5-v<sha>` 这类确定性别名，应用请求时发出的
//! 也是别名。中转站原生支持 Anthropic 协议，因此这里只做三件事：校验本地令牌、
//! 把别名还原成账号里的真实模型、把请求原样转发到中转站并回传响应。
//!
//! 这里不再做协议转换、模型目录或本地请求观测：那些能力已由中转站承担。

use std::{sync::Arc, time::Duration};

use axum::{
    body::{to_bytes, Body},
    extract::State,
    http::{header, Request, StatusCode},
    response::{IntoResponse, Response},
    routing::any,
    Router,
};
use futures::StreamExt;
use serde_json::{json, Value};
use tokio::{
    net::TcpListener,
    sync::{broadcast, oneshot, Mutex, RwLock},
    task::JoinHandle,
};

use crate::{
    codex_bridge::secure_equal, tool_adapters::AdapterFailure, tool_credentials::ToolCredential,
};

/// Claude Desktop 单次请求的上限。超过时拒绝，而不是截断。
const MAX_REQUEST_BODY_BYTES: usize = 32 * 1024 * 1024;

/// 转发给中转站时需要原样带上的请求头。其余请求头不越过本地边界。
const FORWARDED_HEADERS: [&str; 4] = [
    "anthropic-version",
    "anthropic-beta",
    "content-type",
    "accept",
];

fn upstream_client() -> Result<reqwest::Client, reqwest::Error> {
    reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .retry(reqwest::retry::never())
        .connect_timeout(Duration::from_secs(15))
        .read_timeout(Duration::from_secs(600))
        .build()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ClaudeTransport {
    DirectAnthropic,
    ChatBridge,
}

impl ClaudeTransport {
    pub(crate) fn credential_value(self) -> &'static str {
        match self {
            Self::DirectAnthropic => "direct_anthropic",
            Self::ChatBridge => "chat_bridge",
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct VerificationEvent {
    pub(crate) model: String,
}

struct BridgeState {
    credential: ToolCredential,
    local_token: String,
    prefix: &'static str,
    client: reqwest::Client,
    events: broadcast::Sender<VerificationEvent>,
}

struct Running {
    shutdown: Option<oneshot::Sender<()>>,
    task: JoinHandle<()>,
    credential: ToolCredential,
    events: broadcast::Sender<VerificationEvent>,
    state: Arc<RwLock<Arc<BridgeState>>>,
}

#[derive(Clone)]
pub(crate) struct ClaudeBridgeRuntime {
    address: &'static str,
    prefix: &'static str,
    runtime: Arc<Mutex<Option<Running>>>,
}

impl ClaudeBridgeRuntime {
    pub(crate) fn new(address: &'static str, prefix: &'static str) -> Self {
        Self {
            address,
            prefix,
            runtime: Arc::new(Mutex::new(None)),
        }
    }

    pub(crate) async fn start(
        &self,
        credential: ToolCredential,
    ) -> Result<broadcast::Receiver<VerificationEvent>, AdapterFailure> {
        let local_token = credential
            .local_gateway_token
            .clone()
            .filter(|value| value.starts_with("ycg-") && value.len() == 68)
            .ok_or(AdapterFailure::SecureStorageUnavailable)?;
        // Opening an already configured app must not interrupt its requests:
        // check, update and start all happen under the same lock as stop.
        let mut slot = self.runtime.lock().await;
        if let Some(running) = slot.as_mut() {
            if !running.task.is_finished() {
                if running.credential != credential {
                    let previous = running.state.read().await.clone();
                    *running.state.write().await = Arc::new(BridgeState {
                        credential: credential.clone(),
                        local_token,
                        prefix: self.prefix,
                        client: previous.client.clone(),
                        events: running.events.clone(),
                    });
                    running.credential = credential;
                }
                return Ok(running.events.subscribe());
            }
        }
        if let Some(mut previous) = slot.take() {
            if let Some(shutdown) = previous.shutdown.take() {
                let _ = shutdown.send(());
            }
            previous.task.abort();
            let _ = previous.task.await;
        }
        let listener = TcpListener::bind(self.address)
            .await
            .map_err(|_| AdapterFailure::LaunchFailed)?;
        let (events, receiver) = broadcast::channel(8);
        let state = Arc::new(RwLock::new(Arc::new(BridgeState {
            credential: credential.clone(),
            local_token,
            prefix: self.prefix,
            client: upstream_client().map_err(|_| AdapterFailure::LaunchFailed)?,
            events: events.clone(),
        })));
        let router = Router::new()
            .fallback(any(dispatch))
            .with_state(state.clone());
        let (shutdown, shutdown_rx) = oneshot::channel::<()>();
        let task = tokio::spawn(async move {
            let _ = axum::serve(listener, router)
                .with_graceful_shutdown(async move {
                    let _ = shutdown_rx.await;
                })
                .await;
        });
        *slot = Some(Running {
            shutdown: Some(shutdown),
            task,
            credential,
            events,
            state,
        });
        Ok(receiver)
    }

    pub(crate) async fn stop(&self) {
        let runtime = self.runtime.lock().await.take();
        if let Some(mut runtime) = runtime {
            if let Some(shutdown) = runtime.shutdown.take() {
                let _ = shutdown.send(());
            }
            runtime.task.abort();
            let _ = runtime.task.await;
        }
    }
}

async fn dispatch(
    State(state): State<Arc<RwLock<Arc<BridgeState>>>>,
    request: Request<Body>,
) -> Response {
    let state = state.read().await.clone();
    let Some(path) = request.uri().path().strip_prefix(state.prefix) else {
        return error(StatusCode::NOT_FOUND, "unsupported Claude endpoint");
    };
    match (request.method().as_str(), path) {
        ("GET", "/v1/models") => models(&state),
        ("POST", "/v1/messages") => messages(&state, request).await,
        _ => error(StatusCode::NOT_FOUND, "unsupported Claude endpoint"),
    }
}

/// Claude Desktop 会用这个端点做模型发现。返回 Anthropic 形状的列表，
/// `id` 是客户端写入 profile 的 route 别名，显示名保留账号里的真实模型。
fn models(state: &BridgeState) -> Response {
    let ids: Vec<_> = state
        .credential
        .model_ids()
        .iter()
        .map(|id| crate::tool_model_profile::claude_gateway_route_id(id))
        .collect();
    let data: Vec<_> = state
        .credential
        .model_ids()
        .iter()
        .zip(ids.iter())
        .map(|(model, route)| {
            json!({
                "id": route,
                "type": "model",
                "display_name": crate::tool_model_profile::display_name(model),
                "supports1m": crate::tool_model_profile::supports_one_m_context(model),
            })
        })
        .collect();
    let body = json!({
        "data": data,
        "has_more": false,
        "first_id": ids.first(),
        "last_id": ids.last(),
    });
    (
        StatusCode::OK,
        [(header::CONTENT_TYPE, "application/json")],
        Body::from(body.to_string()),
    )
        .into_response()
}

async fn messages(state: &BridgeState, request: Request<Body>) -> Response {
    let bearer = request
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .unwrap_or("");
    if state.local_token.is_empty() || !secure_equal(bearer, &state.local_token) {
        return error(StatusCode::UNAUTHORIZED, "invalid local gateway token");
    }
    let forwarded: Vec<(header::HeaderName, String)> = request
        .headers()
        .iter()
        .filter(|(name, _)| FORWARDED_HEADERS.contains(&name.as_str()))
        .filter_map(|(name, value)| {
            value
                .to_str()
                .ok()
                .map(|value| (name.clone(), value.to_owned()))
        })
        .collect();
    let body = match to_bytes(request.into_body(), MAX_REQUEST_BODY_BYTES).await {
        Ok(bytes) => bytes,
        Err(_) => return error(StatusCode::PAYLOAD_TOO_LARGE, "request body too large"),
    };
    let mut value: Value = match serde_json::from_slice(&body) {
        Ok(value) => value,
        Err(_) => return error(StatusCode::BAD_REQUEST, "invalid request"),
    };
    let requested = value
        .get("model")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_owned();
    let model = resolve_model(state, &requested);
    value["model"] = json!(model);
    let _ = state.events.send(VerificationEvent {
        model: model.clone(),
    });

    let url = format!(
        "{}/v1/messages",
        state.credential.origin.trim_end_matches('/')
    );
    let mut outbound = state
        .client
        .post(url)
        .bearer_auth(state.credential.upstream_key())
        .json(&value);
    for (name, value) in forwarded {
        outbound = outbound.header(name, value);
    }
    let upstream = match outbound.send().await {
        Ok(response) => response,
        Err(_) => {
            return error(
                StatusCode::BAD_GATEWAY,
                "relay request failed; check the network and line",
            )
        }
    };
    let status = upstream.status();
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

/// Claude Desktop 发送的是 route 别名；能还原就还原成账号里的真实模型。
/// 已经是真实模型 ID 的请求原样放行，便于用户手动指定。
fn resolve_model(state: &BridgeState, requested: &str) -> String {
    state
        .credential
        .model_ids()
        .into_iter()
        .find(|id| crate::tool_model_profile::claude_gateway_route_matches(id, requested))
        .unwrap_or_else(|| requested.to_owned())
}

fn error(status: StatusCode, message: &'static str) -> Response {
    (
        status,
        axum::Json(json!({"type": "error", "error": {"type": "api_error", "message": message}})),
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tool_credentials::{ToolCredential, ToolModelRoute};

    fn state() -> BridgeState {
        let routes = vec![
            ToolModelRoute {
                model_id: "claude-sonnet-5".into(),
                billing_group: "group-a".into(),
                api_key: "sk-synthetic-a".into(),
                origin: "https://yeschoy.com".into(),
                claude_transport: None,
                codex_transport: None,
            },
            ToolModelRoute {
                model_id: "gpt-6-astra".into(),
                billing_group: "group-b".into(),
                api_key: "sk-synthetic-b".into(),
                origin: "https://yeschoy.com".into(),
                claude_transport: None,
                codex_transport: None,
            },
        ];
        let local_token = format!("ycg-{}", "a".repeat(64));
        BridgeState {
            credential: ToolCredential {
                api_key: "sk-synthetic-a".into(),
                origin: "https://yeschoy.com".into(),
                model_id: "claude-sonnet-5".into(),
                local_gateway_token: Some(local_token.clone()),
                codex_transport: None,
                claude_transport: None,
                models: routes,
            },
            local_token,
            prefix: "/claude-desktop",
            client: upstream_client().unwrap(),
            events: broadcast::channel(1).0,
        }
    }

    #[test]
    fn route_aliases_resolve_to_the_account_model() {
        let state = state();
        for model in ["claude-sonnet-5", "gpt-6-astra"] {
            let alias = crate::tool_model_profile::claude_gateway_route_id(model);
            assert_eq!(resolve_model(&state, &alias), model);
        }
    }

    #[test]
    fn context_regression_one_m_picker_route_keeps_real_model_identity() {
        let state = state();
        for model in ["claude-sonnet-5", "gpt-6-astra"] {
            for alias in [
                crate::tool_model_profile::claude_gateway_route_id(model),
                crate::tool_model_profile::legacy_claude_gateway_route_id(model),
            ] {
                for suffix in ["[1m]", " [1M] "] {
                    assert_eq!(resolve_model(&state, &format!("{alias}{suffix}")), model);
                }
            }
        }
    }

    #[test]
    fn real_and_unknown_model_ids_pass_through() {
        let state = state();
        assert_eq!(resolve_model(&state, "claude-sonnet-5"), "claude-sonnet-5");
        assert_eq!(resolve_model(&state, "future-model"), "future-model");
        for requested in ["future-model[1m]", "unknown/route [1M] ", "模型[1m]"] {
            assert_eq!(resolve_model(&state, requested), requested);
        }
        // A similar Claude role, or an alias enrolled in another profile,
        // cannot silently select one of this profile's models.
        let unregistered = crate::tool_model_profile::claude_gateway_route_id("claude-fable-5");
        let requested = format!("{unregistered}[1m]");
        assert_eq!(resolve_model(&state, &requested), requested);
    }

    #[tokio::test]
    async fn models_endpoint_lists_routes_with_real_labels() {
        let response = models(&state());
        assert_eq!(response.status(), StatusCode::OK);
        let bytes = to_bytes(response.into_body(), 64 * 1024).await.unwrap();
        let value: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(value["data"].as_array().unwrap().len(), 2);
        assert_eq!(
            value["data"][0]["id"].as_str(),
            Some(crate::tool_model_profile::claude_gateway_route_id("claude-sonnet-5").as_str())
        );
        assert_eq!(value["data"][0]["type"].as_str(), Some("model"));
        assert!(value["data"][0]["display_name"].as_str().is_some());
    }

    #[tokio::test]
    async fn context_regression_discovery_uses_each_real_models_capacity() {
        let mut state = state();
        for id in ["deepseek-v4-flash", "claude-haiku-4-5", "future-model"] {
            let mut route = state.credential.models[0].clone();
            route.model_id = id.into();
            state.credential.models.push(route);
        }
        let response = models(&state);
        let bytes = to_bytes(response.into_body(), 64 * 1024).await.unwrap();
        let value: Value = serde_json::from_slice(&bytes).unwrap();
        for (row, expected) in value["data"]
            .as_array()
            .unwrap()
            .iter()
            .zip([true, true, true, false, false])
        {
            assert_eq!(row["supports1m"], expected, "{}", row["display_name"]);
        }
        assert_eq!(value["data"].as_array().unwrap().len(), 5);
    }

    #[tokio::test]
    async fn context_regression_one_m_requests_preserve_payload_and_streaming() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let (sent, mut received) = tokio::sync::mpsc::unbounded_channel();
        let router = Router::new().route(
            "/v1/messages",
            axum::routing::post(
                move |headers: axum::http::HeaderMap, axum::Json(body): axum::Json<Value>| {
                    let sent = sent.clone();
                    async move {
                        sent.send((headers, body)).unwrap();
                        (
                            [(header::CONTENT_TYPE, "text/event-stream")],
                            "event: message_stop\ndata: {}\n\n",
                        )
                    }
                },
            ),
        );
        let server = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
        let mut state = state();
        state.credential.origin = origin.clone();
        for route in &mut state.credential.models {
            route.origin = origin.clone();
        }
        state.client = reqwest::Client::builder()
            .no_proxy()
            .timeout(Duration::from_secs(3))
            .build()
            .unwrap();
        for model in ["claude-sonnet-5", "gpt-6-astra"] {
            for alias in [
                crate::tool_model_profile::claude_gateway_route_id(model),
                crate::tool_model_profile::legacy_claude_gateway_route_id(model),
            ] {
                let mut expected = json!({
                    "model":format!("{alias}[1m]"), "stream":true, "max_tokens":128,
                    "thinking":{"type":"adaptive"}, "output_config":{"effort":"high"},
                    "messages":[{"role":"user", "content":"synthetic context probe"}]
                });
                let request = Request::builder()
                    .method("POST")
                    .uri("/claude-desktop/v1/messages")
                    .header(
                        header::AUTHORIZATION,
                        format!("Bearer {}", state.local_token),
                    )
                    .header("anthropic-version", "2023-06-01")
                    .header("anthropic-beta", "context-1m-2025-08-07")
                    .body(Body::from(expected.to_string()))
                    .unwrap();
                let response = messages(&state, request).await;
                assert_eq!(response.status(), StatusCode::OK);
                assert_eq!(
                    response.headers()[header::CONTENT_TYPE],
                    "text/event-stream"
                );
                let body = to_bytes(response.into_body(), 64 * 1024).await.unwrap();
                assert_eq!(body.as_ref(), b"event: message_stop\ndata: {}\n\n");
                let (headers, forwarded) =
                    tokio::time::timeout(Duration::from_secs(3), received.recv())
                        .await
                        .unwrap()
                        .unwrap();
                expected["model"] = model.into();
                assert_eq!(
                    forwarded, expected,
                    "only the local model alias should change"
                );
                assert_eq!(headers["anthropic-beta"], "context-1m-2025-08-07");
                assert_eq!(headers["anthropic-version"], "2023-06-01");
                assert_eq!(headers[header::AUTHORIZATION], "Bearer sk-synthetic-a");
            }
        }
        server.abort();
        let _ = server.await;
    }
}

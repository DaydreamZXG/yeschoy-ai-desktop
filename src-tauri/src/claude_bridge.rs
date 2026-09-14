//! Claude 客户端的本地轻量转发器。
//!
//! Claude Desktop 只接受“看起来像 Anthropic 模型”的 route 名，所以客户端在
//! profile 里写的是 `claude-sonnet-5-v<sha>` 这类确定性别名，应用请求时发出的
//! 也是别名。中转站原生支持 Anthropic 协议，因此这里只校验本地令牌、还原真实
//! 模型、转发请求并回传响应。
//!
//! Claude Code 还会把 `[1m]` 作为本地模型选择标记附在模型名后。转发器只对
//! 已下发的模型做精确归一化并补齐对应 Beta 头。若上游提供权威的上下文计数，
//! 转发器可以缩减过大的输出预留，但不会截断或改写用户消息。

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
/// Only error responses are buffered so the bridge can recognize an upstream
/// context-limit rejection. Successful streaming responses remain streaming.
const MAX_ERROR_RESPONSE_BODY_BYTES: usize = 1024 * 1024;

/// 转发给中转站时需要原样带上的请求头。其余请求头不越过本地边界。
const FORWARDED_HEADERS: [&str; 4] = [
    "anthropic-version",
    "anthropic-beta",
    "content-type",
    "accept",
];
const ONE_M_CONTEXT_BETA: &str = "context-1m-2025-08-07";

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
pub(crate) struct VerificationEvent;

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
    let models = state.credential.model_ids();
    let desktop = state.prefix.contains("desktop");
    let ids: Vec<_> = models
        .iter()
        .map(|id| {
            if desktop {
                crate::tool_model_profile::claude_gateway_route_id(id)
            } else {
                id.clone()
            }
        })
        .collect();
    let data: Vec<_> = models
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
    if !locally_authorized(request.headers(), &state.local_token) {
        return error(StatusCode::UNAUTHORIZED, "invalid local gateway token");
    }
    let mut forwarded: Vec<(header::HeaderName, String)> = request
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
    let (model, one_m_context) = match resolve_model(state, &requested) {
        Ok(model) => model,
        Err(()) => {
            return error(
                StatusCode::BAD_REQUEST,
                "1M context is unavailable for this model",
            )
        }
    };
    value["model"] = json!(model);
    if one_m_context {
        ensure_one_m_beta(&mut forwarded);
    }
    let _ = state.events.send(VerificationEvent);

    // Each catalog member may have a different scoped relay key. Resolve the
    // normalized model before forwarding; unknown unsuffixed IDs retain the
    // historical default-route behavior for user-authored configurations.
    let upstream_credential = state
        .credential
        .resolve_model(&model)
        .unwrap_or_else(|_| state.credential.clone());
    let upstream = match send_upstream(
        state,
        &upstream_credential,
        &forwarded,
        &value,
    )
    .await
    {
        Ok(response) => response,
        Err(_) => {
            return error(
                StatusCode::BAD_GATEWAY,
                "relay request failed; check the network and line",
            )
        }
    };
    if upstream.status().is_success() {
        return stream_upstream(upstream);
    }

    let original = match buffer_upstream(upstream).await {
        Ok(response) => response,
        Err(()) => {
            return error(
                StatusCode::BAD_GATEWAY,
                "relay returned an oversized error response",
            )
        }
    };
    let Some(overflow) = parse_context_overflow(&original.body) else {
        return original.into_response();
    };

    // The relay is the authority on tokenization. If the messages still fit
    // and only the requested completion crosses the boundary, preserve every
    // message and retry once with exactly the reported remaining capacity.
    let requested_max_tokens = value.get("max_tokens").and_then(Value::as_u64);
    if overflow.messages < overflow.maximum
        && requested_max_tokens == Some(overflow.completion)
    {
        let available = overflow.maximum - overflow.messages;
        if available > 0 && available < overflow.completion {
            value["max_tokens"] = json!(available);
            let retry = match send_upstream(
                state,
                &upstream_credential,
                &forwarded,
                &value,
            )
            .await
            {
                Ok(response) => response,
                // A transient retry failure must not hide the useful original
                // provider error.
                Err(_) => return original.into_response(),
            };
            if retry.status().is_success() {
                return stream_upstream(retry);
            }
            let retry = match buffer_upstream(retry).await {
                Ok(response) => response,
                Err(()) => {
                    return error(
                        StatusCode::BAD_GATEWAY,
                        "relay returned an oversized error response",
                    )
                }
            };
            if parse_context_overflow(&retry.body).is_none() {
                return retry.into_response();
            }
        }
    }

    // Do not leak the relay's nested host_call_failed 500 to Claude. A normal
    // Anthropic invalid-request response lets the client compact the thread or
    // ask the user to do so instead of treating this as a transient outage.
    context_too_long_error()
}

async fn send_upstream(
    state: &BridgeState,
    credential: &ToolCredential,
    forwarded: &[(header::HeaderName, String)],
    value: &Value,
) -> Result<reqwest::Response, reqwest::Error> {
    let url = format!(
        "{}/v1/messages",
        credential.origin.trim_end_matches('/')
    );
    let mut outbound = state.client.post(url).json(value);
    if state.prefix.contains("desktop") {
        outbound = outbound.bearer_auth(credential.upstream_key());
    } else {
        outbound = outbound.header("x-api-key", credential.upstream_key());
    }
    for (name, value) in forwarded {
        outbound = outbound.header(name, value);
    }
    outbound.send().await
}

fn stream_upstream(upstream: reqwest::Response) -> Response {
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

struct BufferedUpstream {
    status: StatusCode,
    content_type: String,
    body: Vec<u8>,
}

impl BufferedUpstream {
    fn into_response(self) -> Response {
        Response::builder()
            .status(self.status)
            .header(header::CONTENT_TYPE, self.content_type)
            .body(Body::from(self.body))
            .unwrap_or_else(|_| error(StatusCode::INTERNAL_SERVER_ERROR, "response failed"))
    }
}

async fn buffer_upstream(upstream: reqwest::Response) -> Result<BufferedUpstream, ()> {
    let status = upstream.status();
    let content_type = upstream
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("application/json")
        .to_owned();
    let mut body = Vec::new();
    let mut stream = upstream.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|_| ())?;
        let next_len = body.len().checked_add(chunk.len()).ok_or(())?;
        if next_len > MAX_ERROR_RESPONSE_BODY_BYTES {
            return Err(());
        }
        body.extend_from_slice(&chunk);
    }
    Ok(BufferedUpstream {
        status,
        content_type,
        body,
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ContextOverflow {
    maximum: u64,
    requested: u64,
    messages: u64,
    completion: u64,
}

fn leading_u64(value: &str) -> Option<u64> {
    let digits = value
        .as_bytes()
        .iter()
        .take_while(|byte| byte.is_ascii_digit())
        .count();
    (digits > 0).then(|| value[..digits].parse().ok()).flatten()
}

fn parse_context_overflow(body: &[u8]) -> Option<ContextOverflow> {
    let text = String::from_utf8_lossy(body);
    let maximum_marker = "maximum context length is ";
    let requested_marker = "However, you requested ";
    let messages_marker = " tokens (";
    let completion_marker = " in the messages, ";

    let start = text.find(maximum_marker)?;
    let text = &text[start + maximum_marker.len()..];
    let maximum = leading_u64(text)?;
    let text = &text[text.find(requested_marker)? + requested_marker.len()..];
    let requested = leading_u64(text)?;
    let text = &text[text.find(messages_marker)? + messages_marker.len()..];
    let messages = leading_u64(text)?;
    let text = &text[text.find(completion_marker)? + completion_marker.len()..];
    let completion = leading_u64(text)?;
    if requested <= maximum || messages.checked_add(completion)? != requested {
        return None;
    }
    Some(ContextOverflow {
        maximum,
        requested,
        messages,
        completion,
    })
}

fn locally_authorized(headers: &axum::http::HeaderMap, local_token: &str) -> bool {
    let bearer = headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "));
    let api_key = headers
        .get("x-api-key")
        .and_then(|value| value.to_str().ok());
    !local_token.is_empty()
        && [bearer, api_key]
            .into_iter()
            .flatten()
            .any(|value| secure_equal(value, local_token))
}

fn ensure_one_m_beta(headers: &mut Vec<(header::HeaderName, String)>) {
    if let Some((_, value)) = headers
        .iter_mut()
        .find(|(name, _)| name == "anthropic-beta")
    {
        if !value
            .split(',')
            .any(|candidate| candidate.trim() == ONE_M_CONTEXT_BETA)
        {
            if !value.trim().is_empty() {
                value.push(',');
            }
            value.push_str(ONE_M_CONTEXT_BETA);
        }
    } else {
        headers.push((
            header::HeaderName::from_static("anthropic-beta"),
            ONE_M_CONTEXT_BETA.into(),
        ));
    }
}

/// Normalize only an exact model enrolled in this tool credential. This keeps
/// arbitrary user spellings pass-through compatible while preventing a raw
/// `[1m]` marker from reaching the relay as if it were part of the model ID.
fn resolve_model(state: &BridgeState, requested: &str) -> Result<(String, bool), ()> {
    let (base, one_m_context) = crate::tool_model_profile::split_one_m_context_marker(requested);
    let model = state.credential.model_ids().into_iter().find(|id| {
        id == base || crate::tool_model_profile::claude_gateway_route_matches(id, requested)
    });
    match model {
        Some(model) => {
            if one_m_context && !crate::tool_model_profile::supports_one_m_context(&model) {
                Err(())
            } else {
                Ok((model, one_m_context))
            }
        }
        None if one_m_context => Err(()),
        None => Ok((requested.to_owned(), false)),
    }
}

fn error(status: StatusCode, message: &'static str) -> Response {
    (
        status,
        axum::Json(json!({"type": "error", "error": {"type": "api_error", "message": message}})),
    )
        .into_response()
}

fn context_too_long_error() -> Response {
    (
        StatusCode::BAD_REQUEST,
        axum::Json(json!({
            "type": "error",
            "error": {
                "type": "invalid_request_error",
                "message": "Prompt is too long. Compact the conversation and retry."
            }
        })),
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
            assert_eq!(resolve_model(&state, &alias), Ok((model.into(), false)));
        }
    }

    #[test]
    fn parses_authoritative_context_overflow_counts() {
        let body = br#"{"error":{"message":"This model's maximum context length is 1048576 tokens. However, you requested 1048744 tokens (1016744 in the messages, 32000 in the completion). Please reduce the length."}}"#;
        assert_eq!(
            parse_context_overflow(body),
            Some(ContextOverflow {
                maximum: 1_048_576,
                requested: 1_048_744,
                messages: 1_016_744,
                completion: 32_000,
            })
        );
        assert_eq!(
            parse_context_overflow(
                br#"maximum context length is 100 tokens. However, you requested 102 tokens (90 in the messages, 11 in the completion)"#
            ),
            None,
            "inconsistent provider counts must never drive a retry"
        );
        assert_eq!(parse_context_overflow(br#"temporary upstream error"#), None);
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
                    assert_eq!(
                        resolve_model(&state, &format!("{alias}{suffix}")),
                        Ok((model.into(), true))
                    );
                }
            }
        }
    }

    #[test]
    fn real_and_unknown_model_ids_pass_through() {
        let state = state();
        assert_eq!(
            resolve_model(&state, "claude-sonnet-5"),
            Ok(("claude-sonnet-5".into(), false))
        );
        assert_eq!(
            resolve_model(&state, "future-model"),
            Ok(("future-model".into(), false))
        );
        for requested in ["future-model[1m]", "unknown/route [1M] ", "模型[1m]"] {
            assert_eq!(resolve_model(&state, requested), Err(()));
        }
        // A similar Claude role, or an alias enrolled in another profile,
        // cannot silently select one of this profile's models.
        let unregistered = crate::tool_model_profile::claude_gateway_route_id("claude-fable-5");
        let requested = format!("{unregistered}[1m]");
        assert_eq!(resolve_model(&state, &requested), Err(()));
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

        let mut code = state();
        code.prefix = "/claude-code";
        let response = models(&code);
        let bytes = to_bytes(response.into_body(), 64 * 1024).await.unwrap();
        let value: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(value["data"][0]["id"], "claude-sonnet-5");
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
                let expected_key = if model == "gpt-6-astra" {
                    "Bearer sk-synthetic-b"
                } else {
                    "Bearer sk-synthetic-a"
                };
                assert_eq!(headers[header::AUTHORIZATION], expected_key);
            }
        }
        server.abort();
        let _ = server.await;
    }

    #[tokio::test]
    async fn claude_code_proxy_strips_one_m_suffix_and_adds_missing_beta() {
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
                        axum::Json(json!({"content":[{"type":"text","text":"ok"}]}))
                    }
                },
            ),
        );
        let server = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
        let mut state = state();
        state.prefix = "/claude-code";
        state.credential.origin = origin.clone();
        for route in &mut state.credential.models {
            route.origin = origin.clone();
        }
        state.credential.models.push(ToolModelRoute {
            model_id: "deepseek-v4.1-flash".into(),
            billing_group: "group-c".into(),
            api_key: "sk-synthetic-c".into(),
            origin: origin.clone(),
            claude_transport: None,
            codex_transport: None,
        });
        state.client = reqwest::Client::builder()
            .no_proxy()
            .timeout(Duration::from_secs(3))
            .build()
            .unwrap();
        let request = Request::builder()
            .method("POST")
            .uri("/claude-code/v1/messages")
            .header("x-api-key", &state.local_token)
            .header("anthropic-version", "2023-06-01")
            .body(Body::from(
                json!({
                    "model":"deepseek-v4.1-flash[1m]",
                    "max_tokens":128,
                    "messages":[{"role":"user","content":"test"}]
                })
                .to_string(),
            ))
            .unwrap();
        let response = messages(&state, request).await;
        assert_eq!(response.status(), StatusCode::OK);
        let (headers, forwarded) = tokio::time::timeout(Duration::from_secs(3), received.recv())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(forwarded["model"], "deepseek-v4.1-flash");
        assert_eq!(headers["anthropic-beta"], ONE_M_CONTEXT_BETA);
        assert_eq!(headers["x-api-key"], "sk-synthetic-c");
        assert!(headers.get(header::AUTHORIZATION).is_none());
        server.abort();
        let _ = server.await;
    }

    #[tokio::test]
    async fn context_overflow_retries_once_with_provider_reported_capacity() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let (sent, mut received) = tokio::sync::mpsc::unbounded_channel();
        let router = Router::new().route(
            "/v1/messages",
            axum::routing::post(move |axum::Json(body): axum::Json<Value>| {
                let sent = sent.clone();
                async move {
                    let max_tokens = body["max_tokens"].as_u64().unwrap();
                    sent.send(body).unwrap();
                    if max_tokens == 32_000 {
                        (
                            StatusCode::INTERNAL_SERVER_ERROR,
                            axum::Json(json!({
                                "error": {
                                    "message": "This model's maximum context length is 1048576 tokens. However, you requested 1048744 tokens (1016744 in the messages, 32000 in the completion). Please reduce the length."
                                }
                            })),
                        )
                            .into_response()
                    } else {
                        assert_eq!(max_tokens, 31_832);
                        (StatusCode::OK, axum::Json(json!({"content": []}))).into_response()
                    }
                }
            }),
        );
        let server = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
        let mut state = state();
        state.prefix = "/claude-code";
        state.credential.origin = origin.clone();
        for route in &mut state.credential.models {
            route.origin = origin.clone();
        }
        state.client = reqwest::Client::builder()
            .no_proxy()
            .timeout(Duration::from_secs(3))
            .build()
            .unwrap();

        let request = Request::builder()
            .method("POST")
            .uri("/claude-code/v1/messages")
            .header("x-api-key", &state.local_token)
            .body(Body::from(
                json!({
                    "model": "claude-sonnet-5[1m]",
                    "max_tokens": 32_000,
                    "messages": [{"role": "user", "content": "large context"}]
                })
                .to_string(),
            ))
            .unwrap();
        let response = messages(&state, request).await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(received.recv().await.unwrap()["max_tokens"], 32_000);
        assert_eq!(received.recv().await.unwrap()["max_tokens"], 31_832);
        assert!(received.try_recv().is_err(), "the bridge must retry only once");
        server.abort();
        let _ = server.await;
    }

    #[tokio::test]
    async fn input_overflow_becomes_anthropic_invalid_request_without_retry() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let (sent, mut received) = tokio::sync::mpsc::unbounded_channel();
        let router = Router::new().route(
            "/v1/messages",
            axum::routing::post(move |axum::Json(body): axum::Json<Value>| {
                let sent = sent.clone();
                async move {
                    sent.send(body).unwrap();
                    (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        axum::Json(json!({
                            "error": {
                                "message": "This model's maximum context length is 1048576 tokens. However, you requested 1080000 tokens (1049000 in the messages, 31000 in the completion)."
                            }
                        })),
                    )
                }
            }),
        );
        let server = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
        let mut state = state();
        state.prefix = "/claude-code";
        state.credential.origin = origin.clone();
        for route in &mut state.credential.models {
            route.origin = origin.clone();
        }
        state.client = reqwest::Client::builder()
            .no_proxy()
            .timeout(Duration::from_secs(3))
            .build()
            .unwrap();

        let request = Request::builder()
            .method("POST")
            .uri("/claude-code/v1/messages")
            .header("x-api-key", &state.local_token)
            .body(Body::from(
                json!({
                    "model": "claude-sonnet-5[1m]",
                    "max_tokens": 31_000,
                    "messages": [{"role": "user", "content": "oversized context"}]
                })
                .to_string(),
            ))
            .unwrap();
        let response = messages(&state, request).await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let body = to_bytes(response.into_body(), 64 * 1024).await.unwrap();
        let body: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(body["error"]["type"], "invalid_request_error");
        assert!(body["error"]["message"]
            .as_str()
            .unwrap()
            .contains("Compact"));
        assert!(received.recv().await.is_some());
        assert!(received.try_recv().is_err(), "input overflow must not retry");
        server.abort();
        let _ = server.await;
    }
}

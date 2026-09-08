use std::{collections::HashMap, sync::Arc, time::Duration};

use axum::{
    body::{to_bytes, Body},
    extract::State,
    http::{header, HeaderMap, Request, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::{
    net::TcpListener,
    sync::{oneshot, Mutex},
    task::JoinHandle,
};

use crate::loopback_http::{admit_ai_request, read_ai_request_body, AiRequestAdmissionError};
use crate::request_diagnostics::{self, RequestContext, RequestOutcome, StreamProtocol};
use crate::tool_credentials;

pub(crate) const BASE_URL: &str = "http://127.0.0.1:15722/yeschoy/v1";
const LISTEN_ADDRESS: &str = "127.0.0.1:15722";
const MAX_RESPONSE_BYTES: u64 = 16 * 1024 * 1024;

#[path = "proxy/providers/streaming.rs"]
pub(crate) mod claude_streaming;
#[path = "proxy/providers/transform.rs"]
pub(crate) mod claude_transform;
#[path = "proxy/providers/codex_chat_common.rs"]
pub(crate) mod codex_chat_common;
#[path = "proxy/providers/codex_chat_history.rs"]
pub(crate) mod codex_chat_history;
#[path = "proxy/providers/codex_responses_sse.rs"]
pub(crate) mod codex_responses_sse;
#[path = "proxy/error.rs"]
#[allow(dead_code)] // Shared upstream converter API; the desktop uses only its transport subset.
pub(crate) mod error;
#[path = "proxy/json_canonical.rs"]
#[allow(dead_code)] // Shared upstream utilities also exercised by converter tests.
pub(crate) mod json_canonical;
#[path = "proxy/sse.rs"]
pub(crate) mod sse;
#[path = "proxy/providers/streaming_codex_chat.rs"]
pub(crate) mod streaming_codex_chat;
#[path = "proxy/tool_media.rs"]
#[allow(dead_code)] // Keep upstream media helpers intact for converter compatibility.
pub(crate) mod tool_media;
#[path = "proxy/providers/transform_codex_chat.rs"]
pub(crate) mod transform_codex_chat;
/// The bridge intentionally uses conservative automatic reasoning defaults.
/// This shape is retained because the proven converter supports explicit
/// provider capability metadata, while the desktop catalog does not invent it.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub(crate) struct CodexChatReasoningConfig {
    #[serde(rename = "supportsThinking", skip_serializing_if = "Option::is_none")]
    pub(crate) supports_thinking: Option<bool>,
    #[serde(rename = "supportsEffort", skip_serializing_if = "Option::is_none")]
    pub(crate) supports_effort: Option<bool>,
    #[serde(rename = "thinkingParam", skip_serializing_if = "Option::is_none")]
    pub(crate) thinking_param: Option<String>,
    #[serde(rename = "effortParam", skip_serializing_if = "Option::is_none")]
    pub(crate) effort_param: Option<String>,
    #[serde(rename = "effortValueMode", skip_serializing_if = "Option::is_none")]
    pub(crate) effort_value_mode: Option<String>,
    #[serde(rename = "outputFormat", skip_serializing_if = "Option::is_none")]
    pub(crate) output_format: Option<String>,
    #[serde(skip)]
    pub(crate) effort_levels: Option<Vec<String>>,
}

pub(crate) mod transform {
    use serde_json::{json, Value};

    pub(crate) fn is_openai_o_series(model: &str) -> bool {
        model.len() > 1
            && model.starts_with('o')
            && model
                .as_bytes()
                .get(1)
                .is_some_and(|byte| byte.is_ascii_digit())
    }

    pub(crate) fn supports_reasoning_effort(model: &str) -> bool {
        super::claude_transform::supports_reasoning_effort(model)
    }

    pub(crate) fn inject_openai_stream_include_usage(result: &mut Value) {
        if result.get("stream").and_then(Value::as_bool) != Some(true) {
            return;
        }
        match result.get_mut("stream_options") {
            Some(Value::Object(options)) => {
                options.insert("include_usage".to_string(), json!(true));
            }
            _ => result["stream_options"] = json!({"include_usage": true}),
        }
    }
}

#[derive(Clone)]
struct BridgeAppState {
    client: reqwest::Client,
    history: Arc<Mutex<RouteHistories>>,
}

#[derive(Default)]
struct RouteHistories {
    credential: Option<tool_credentials::ToolCredential>,
    models: HashMap<String, Arc<codex_chat_history::CodexChatHistoryStore>>,
}

impl BridgeAppState {
    async fn history_for(
        &self,
        credential: &tool_credentials::ToolCredential,
        model: &str,
    ) -> Arc<codex_chat_history::CodexChatHistoryStore> {
        let mut histories = self.history.lock().await;
        // Full logical credential equality includes origin/group/key/protocol.
        // In-flight requests retain their old Arc; new generations cannot see it.
        if histories.credential.as_ref() != Some(credential) {
            histories.models.clear();
            histories.credential = Some(credential.clone());
        }
        histories.models.entry(model.into()).or_default().clone()
    }
}

fn upstream_client() -> Result<reqwest::Client, reqwest::Error> {
    reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .retry(reqwest::retry::never())
        .connect_timeout(Duration::from_secs(15))
        .read_timeout(Duration::from_secs(600))
        .build()
}

struct RunningBridge {
    shutdown: Option<oneshot::Sender<()>>,
    task: JoinHandle<()>,
}

impl Drop for RunningBridge {
    fn drop(&mut self) {
        self.task.abort();
    }
}

#[derive(Clone, Default)]
pub(crate) struct CodexBridgeRuntimeState {
    running: Arc<Mutex<Option<RunningBridge>>>,
}

impl CodexBridgeRuntimeState {
    /// Returns true only when this call started a new listener.
    pub(crate) async fn ensure_started(&self) -> Result<bool, ()> {
        let mut running = self.running.lock().await;
        if running
            .as_ref()
            .is_some_and(|bridge| !bridge.task.is_finished())
        {
            return Ok(false);
        }
        *running = None;

        let listener = TcpListener::bind(LISTEN_ADDRESS).await.map_err(|_| ())?;
        let client = upstream_client().map_err(|_| ())?;
        let state = BridgeAppState {
            client,
            history: Arc::new(Mutex::new(RouteHistories::default())),
        };
        let router = Router::new()
            .route("/yeschoy/health", get(health))
            .route("/yeschoy/v1/models", get(models))
            .route(
                "/yeschoy/v1/responses",
                post(responses).head(responses_head),
            )
            .with_state(state);
        let (shutdown, receive_shutdown) = oneshot::channel();
        let task = tokio::spawn(async move {
            let _ = axum::serve(listener, router)
                .with_graceful_shutdown(async move {
                    let _ = receive_shutdown.await;
                })
                .await;
        });
        *running = Some(RunningBridge {
            shutdown: Some(shutdown),
            task,
        });
        Ok(true)
    }

    pub(crate) async fn stop(&self) {
        let bridge = self.running.lock().await.take();
        if let Some(mut bridge) = bridge {
            if let Some(shutdown) = bridge.shutdown.take() {
                let _ = shutdown.send(());
            }
            if tokio::time::timeout(Duration::from_secs(2), &mut bridge.task)
                .await
                .is_err()
            {
                bridge.task.abort();
            }
        }
    }
}

pub(crate) async fn resume_if_configured(runtime: CodexBridgeRuntimeState) {
    let credential = tokio::task::spawn_blocking(|| tool_credentials::load("codex_desktop"))
        .await
        .ok()
        .and_then(Result::ok);
    if credential
        .as_ref()
        .and_then(|record| record.codex_transport.as_deref())
        == Some("chat_bridge")
        || credential
            .as_ref()
            .is_some_and(|record| record.has_model_set())
    {
        let _ = runtime.ensure_started().await;
    }
}

async fn health() -> impl IntoResponse {
    (StatusCode::OK, Json(json!({"status": "ready"})))
}

async fn responses_head() -> StatusCode {
    StatusCode::OK
}

async fn models(headers: HeaderMap) -> Response {
    let credential =
        match tokio::task::spawn_blocking(|| tool_credentials::load("codex_desktop")).await {
            Ok(Ok(value)) => value,
            _ => {
                return response_error(
                    StatusCode::SERVICE_UNAVAILABLE,
                    "Secure credential unavailable",
                )
            }
        };
    if credential.has_model_set() && !authorized(&headers, credential.client_token("codex_desktop"))
    {
        return response_error(StatusCode::UNAUTHORIZED, "Authentication failed");
    }
    if !credential.has_model_set() && credential.codex_transport.as_deref() != Some("chat_bridge") {
        return response_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "Compatibility connection is inactive",
        );
    }
    match crate::tool_adapters::codex_desktop::bridge_catalog_models(&credential.model_ids()) {
        Ok(value) => (StatusCode::OK, Json(value)).into_response(),
        Err(_) => response_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Model catalog unavailable",
        ),
    }
}

pub(crate) fn secure_equal(left: &str, right: &str) -> bool {
    let left = left.as_bytes();
    let right = right.as_bytes();
    let mut difference = left.len() ^ right.len();
    for index in 0..left.len().max(right.len()) {
        difference |= usize::from(
            left.get(index).copied().unwrap_or_default()
                ^ right.get(index).copied().unwrap_or_default(),
        );
    }
    difference == 0
}

fn response_error(status: StatusCode, message: &'static str) -> Response {
    (
        status,
        Json(json!({"error": {"type": "yeschoy_bridge_error", "message": message}})),
    )
        .into_response()
}

fn authorized(headers: &HeaderMap, token: &str) -> bool {
    let bearer = headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .unwrap_or("");
    !token.is_empty() && secure_equal(bearer, token)
}

fn route_snapshot(
    credential: &tool_credentials::ToolCredential,
    body: &Value,
) -> Result<(tool_credentials::ToolCredential, String), ()> {
    if !credential.has_model_set() {
        return Ok((credential.clone(), String::new()));
    }
    let model = body["model"]
        .as_str()
        .filter(|model| !model.is_empty())
        .ok_or(())?;
    let group = credential
        .models
        .iter()
        .find(|route| route.model_id == model)
        .ok_or(())?
        .billing_group
        .clone();
    credential
        .resolve_model(model)
        .map(|route| (route, group))
        .map_err(|_| ())
}

fn observed_error(
    context: &RequestContext,
    status: StatusCode,
    outcome: RequestOutcome,
) -> Response {
    context.record(outcome, status.as_u16());
    (status, Json(request_diagnostics::error_json(outcome))).into_response()
}

async fn decode_request_body(body: Body) -> Result<Value, Response> {
    let bytes = read_ai_request_body(body).await.map_err(|_| {
        (
            StatusCode::PAYLOAD_TOO_LARGE,
            Json(request_diagnostics::error_json(
                RequestOutcome::PayloadTooLarge,
            )),
        )
            .into_response()
    })?;
    serde_json::from_slice(&bytes)
        .map_err(|_| response_error(StatusCode::BAD_REQUEST, "Invalid request body"))
}

fn local_access_error(
    headers: &HeaderMap,
    credential: &tool_credentials::ToolCredential,
) -> Option<Response> {
    if !credential.has_model_set() && credential.codex_transport.as_deref() != Some("chat_bridge") {
        return Some(response_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "Compatibility connection is inactive",
        ));
    }
    if !authorized(headers, credential.client_token("codex_desktop")) {
        return Some(response_error(
            StatusCode::UNAUTHORIZED,
            "Authentication failed",
        ));
    }
    None
}

async fn responses(State(state): State<BridgeAppState>, request: Request<Body>) -> Response {
    let (parts, body) = request.into_parts();
    let credential =
        match tokio::task::spawn_blocking(|| tool_credentials::load("codex_desktop")).await {
            Ok(Ok(value)) => value,
            _ => {
                return response_error(
                    StatusCode::SERVICE_UNAVAILABLE,
                    "Secure credential unavailable",
                )
            }
        };
    // Authenticate before buffering a potentially large attachment request.
    if let Some(response) = local_access_error(&parts.headers, &credential) {
        return response;
    }
    let local_context = RequestContext {
        tool: "codex_desktop".into(),
        model: String::new(),
        group: String::new(),
        origin: credential.origin.clone(),
    };
    let _admission = match admit_ai_request(&parts.headers).await {
        Ok(permit) => permit,
        Err(AiRequestAdmissionError::PayloadTooLarge) => {
            return observed_error(
                &local_context,
                StatusCode::PAYLOAD_TOO_LARGE,
                RequestOutcome::PayloadTooLarge,
            )
        }
        Err(AiRequestAdmissionError::Busy) => {
            return observed_error(
                &local_context,
                StatusCode::TOO_MANY_REQUESTS,
                RequestOutcome::LocalBusy,
            )
        }
    };
    let body = match decode_request_body(body).await {
        Ok(body) => body,
        Err(response) => {
            if response.status() == StatusCode::PAYLOAD_TOO_LARGE {
                local_context.record(
                    RequestOutcome::PayloadTooLarge,
                    StatusCode::PAYLOAD_TOO_LARGE.as_u16(),
                );
            }
            return response;
        }
    };
    responses_with_authorized_credential(state, body, credential).await
}

#[cfg(test)]
async fn responses_with_credential(
    state: BridgeAppState,
    headers: HeaderMap,
    body: Value,
    configured: tool_credentials::ToolCredential,
) -> Response {
    if let Some(response) = local_access_error(&headers, &configured) {
        return response;
    }
    responses_with_authorized_credential(state, body, configured).await
}

async fn responses_with_authorized_credential(
    state: BridgeAppState,
    body: Value,
    configured: tool_credentials::ToolCredential,
) -> Response {
    let (credential, group) = match route_snapshot(&configured, &body) {
        Ok(route) => route,
        Err(_) => {
            request_diagnostics::record(
                "codex_desktop",
                "",
                "",
                &configured.origin,
                RequestOutcome::UnknownModel,
                0,
            );
            return response_error(
                StatusCode::BAD_REQUEST,
                request_diagnostics::curated_message(RequestOutcome::UnknownModel),
            );
        }
    };
    let context = RequestContext {
        tool: "codex_desktop".into(),
        model: credential.model_id.clone(),
        group,
        origin: credential.origin.clone(),
    };
    if !matches!(
        credential.origin.as_str(),
        "https://yeschoy.com" | "https://api.yeschoy.com"
    ) {
        return response_error(StatusCode::FORBIDDEN, "Upstream is not allowed");
    }
    forward_resolved(state, body, configured, credential, context).await
}

async fn forward_resolved(
    state: BridgeAppState,
    mut body: Value,
    configured: tool_credentials::ToolCredential,
    credential: tool_credentials::ToolCredential,
    context: RequestContext,
) -> Response {
    let Some(object) = body.as_object_mut() else {
        return observed_error(
            &context,
            StatusCode::BAD_REQUEST,
            RequestOutcome::InvalidResponse,
        );
    };
    object.insert("model".into(), Value::String(credential.model_id.clone()));
    let direct = credential.codex_transport.as_deref() == Some("direct_responses");
    if !direct && credential.codex_transport.as_deref() != Some("chat_bridge") {
        return observed_error(
            &context,
            StatusCode::BAD_REQUEST,
            RequestOutcome::InvalidResponse,
        );
    }
    let history = state.history_for(&configured, &credential.model_id).await;
    if !direct {
        history.enrich_request(&mut body).await;
    }
    let tool_context = transform_codex_chat::build_codex_tool_context_from_request(&body);
    let chat = match if direct {
        Ok(body)
    } else {
        transform_codex_chat::responses_to_chat_completions_with_reasoning(body, None)
    } {
        Ok(value) => value,
        Err(_) => {
            return observed_error(
                &context,
                StatusCode::UNPROCESSABLE_ENTITY,
                RequestOutcome::InvalidResponse,
            )
        }
    };
    let streaming = chat.get("stream").and_then(Value::as_bool) == Some(true);
    let upstream = match state
        .client
        .post(format!(
            "{}/v1/{}",
            credential.origin,
            if direct {
                "responses"
            } else {
                "chat/completions"
            }
        ))
        .bearer_auth(&credential.api_key)
        .header(
            header::ACCEPT,
            if streaming {
                "text/event-stream"
            } else {
                "application/json"
            },
        )
        .json(&chat)
        .send()
        .await
    {
        Ok(value) => value,
        Err(error) => {
            let outcome = request_diagnostics::transport_outcome(&error);
            context.record(outcome, 0);
            return (
                StatusCode::BAD_GATEWAY,
                Json(request_diagnostics::error_json(outcome)),
            )
                .into_response();
        }
    };
    let status =
        StatusCode::from_u16(upstream.status().as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
    if !status.is_success() {
        return observed_error(
            &context,
            status,
            request_diagnostics::outcome_for_status(status.as_u16()),
        );
    }
    if streaming {
        if direct {
            let observed = request_diagnostics::observed_sse(
                upstream.bytes_stream(),
                StreamProtocol::Responses,
                context,
                status.as_u16(),
            );
            return (
                [
                    (header::CONTENT_TYPE, "text/event-stream"),
                    (header::CACHE_CONTROL, "no-cache"),
                ],
                Body::from_stream(observed),
            )
                .into_response();
        }
        let converted = streaming_codex_chat::create_responses_sse_stream_from_chat_with_context(
            upstream.bytes_stream(),
            tool_context,
        );
        let observed = request_diagnostics::observed_sse(
            converted,
            StreamProtocol::Responses,
            context,
            status.as_u16(),
        );
        let recorded = codex_chat_history::record_responses_sse_stream(observed, history);
        return Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, "text/event-stream")
            .header(header::CACHE_CONTROL, "no-cache")
            .body(Body::from_stream(recorded))
            .unwrap_or_else(|_| {
                response_error(StatusCode::INTERNAL_SERVER_ERROR, "Response build failed")
            });
    }
    if upstream
        .content_length()
        .is_some_and(|length| length > MAX_RESPONSE_BYTES)
    {
        return observed_error(
            &context,
            StatusCode::BAD_GATEWAY,
            RequestOutcome::InvalidResponse,
        );
    }
    let chat_response = match to_bytes(
        Body::from_stream(upstream.bytes_stream()),
        MAX_RESPONSE_BYTES as usize,
    )
    .await
    {
        Ok(bytes) if bytes.len() as u64 <= MAX_RESPONSE_BYTES => {
            serde_json::from_slice::<Value>(&bytes).ok()
        }
        Err(error) if request_diagnostics::transport_outcome(&error) == RequestOutcome::Timeout => {
            return observed_error(&context, StatusCode::BAD_GATEWAY, RequestOutcome::Timeout);
        }
        _ => None,
    };
    let Some(chat_response) = chat_response else {
        return observed_error(
            &context,
            StatusCode::BAD_GATEWAY,
            RequestOutcome::InvalidResponse,
        );
    };
    if direct {
        if chat_response["object"] != "response"
            || !matches!(
                chat_response["status"].as_str(),
                Some("completed" | "incomplete")
            )
            || !chat_response["output"].is_array()
            || chat_response.get("error").is_some_and(|v| !v.is_null())
        {
            return observed_error(
                &context,
                StatusCode::BAD_GATEWAY,
                RequestOutcome::InvalidResponse,
            );
        }
        context.record(RequestOutcome::Ok, status.as_u16());
        return (StatusCode::OK, Json(chat_response)).into_response();
    }
    if !chat_response["choices"][0]["message"].is_object()
        || chat_response["choices"][0]["finish_reason"]
            .as_str()
            .is_none()
    {
        return observed_error(
            &context,
            StatusCode::BAD_GATEWAY,
            RequestOutcome::InvalidResponse,
        );
    }
    let converted = match transform_codex_chat::chat_completion_to_response_with_context(
        chat_response,
        &tool_context,
    ) {
        Ok(value) => value,
        Err(_) => {
            return observed_error(
                &context,
                StatusCode::BAD_GATEWAY,
                RequestOutcome::InvalidResponse,
            )
        }
    };
    history.record_response(&converted).await;
    context.record(RequestOutcome::Ok, status.as_u16());
    (StatusCode::OK, Json(converted)).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn synthetic_catalog(origin: &str) -> tool_credentials::ToolCredential {
        use tool_credentials::{ToolCredential, ToolModelRoute};
        let routes = vec![
            ToolModelRoute {
                model_id: "chat-a".into(),
                billing_group: "group-a".into(),
                api_key: "synthetic-key-a".into(),
                origin: origin.into(),
                claude_transport: None,
                codex_transport: Some("chat_bridge".into()),
            },
            ToolModelRoute {
                model_id: "responses-b".into(),
                billing_group: "group-b".into(),
                api_key: "synthetic-key-b".into(),
                origin: origin.into(),
                claude_transport: None,
                codex_transport: Some("direct_responses".into()),
            },
        ];
        ToolCredential {
            api_key: routes[0].api_key.clone(),
            origin: origin.into(),
            model_id: routes[0].model_id.clone(),
            local_gateway_token: Some(format!("ycg-{}", "d".repeat(64))),
            codex_transport: routes[0].codex_transport.clone(),
            claude_transport: None,
            models: routes,
        }
    }

    fn fixture_state() -> BridgeAppState {
        BridgeAppState {
            client: upstream_client().unwrap(),
            history: Arc::new(Mutex::new(RouteHistories::default())),
        }
    }

    #[tokio::test]
    async fn accepts_codex_json_above_the_legacy_two_megabyte_limit() {
        let attachment = "a".repeat(2 * 1024 * 1024 + 1);
        let body = Body::from(json!({"model":"chat-a","input":attachment}).to_string());
        let parsed = decode_request_body(body).await.unwrap();
        assert_eq!(
            parsed["input"].as_str().map(str::len),
            Some(2 * 1024 * 1024 + 1)
        );
    }

    #[tokio::test]
    async fn ru042_codex_routes_chat_and_responses_to_exact_keys() {
        use bytes::Bytes;
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let configured = synthetic_catalog(&format!("http://{}", listener.local_addr().unwrap()));
        let requests = Arc::new(Mutex::new(Vec::new()));
        let captured = requests.clone();
        let server = tokio::spawn(async move {
            axum::serve(listener, Router::new().fallback(axum::routing::any(move |uri: axum::http::Uri, headers: HeaderMap, bytes: Bytes| {
                let captured = captured.clone();
                async move {
                    let body: Value = serde_json::from_slice(&bytes).unwrap();
                    captured.lock().await.push(json!({"path":uri.path(),"auth":headers[header::AUTHORIZATION].to_str().unwrap(),"body":body}));
                    Json(if uri.path() == "/v1/responses" {
                        json!({"object":"response","id":"resp-b","model":body["model"],"status":"completed","output":[]})
                    } else { json!({"id":"chat-a","model":body["model"],"choices":[{"message":{"role":"assistant","content":"ok"},"finish_reason":"stop"}]}) })
                }
            }))).await.unwrap();
        });
        let state = fixture_state();
        for model in ["chat-a", "responses-b"] {
            let body = json!({"model":model,"input":"unchanged prompt","stream":false});
            let (credential, group) = route_snapshot(&configured, &body).unwrap();
            let context = RequestContext {
                tool: "codex_desktop".into(),
                model: model.into(),
                group,
                origin: credential.origin.clone(),
            };
            // Only the private transport stage uses loopback. The public entry
            // point still rejects every origin outside the two compiled lines.
            let response =
                forward_resolved(state.clone(), body, configured.clone(), credential, context)
                    .await;
            assert_eq!(response.status(), StatusCode::OK);
        }
        let captured = requests.lock().await;
        assert_eq!(captured.len(), 2);
        assert_eq!(captured[0]["path"], "/v1/chat/completions");
        assert_eq!(captured[0]["auth"], "Bearer synthetic-key-a");
        assert_eq!(captured[0]["body"]["model"], "chat-a");
        assert_eq!(captured[1]["path"], "/v1/responses");
        assert_eq!(captured[1]["auth"], "Bearer synthetic-key-b");
        assert_eq!(captured[1]["body"]["model"], "responses-b");
        assert_eq!(captured[1]["body"]["input"], "unchanged prompt");
        assert!(captured[1]["body"].get("messages").is_none());
        server.abort();
    }

    #[tokio::test]
    async fn ru042_codex_unknown_models_and_upstream_keys_never_pass_local_auth() {
        let configured = synthetic_catalog("https://yeschoy.com");
        for (token, body, status) in [
            (
                configured.api_key.as_str(),
                json!({"model":"chat-a"}),
                StatusCode::UNAUTHORIZED,
            ),
            (
                configured.client_token("codex_desktop"),
                json!({"model":"unknown-synthetic-key"}),
                StatusCode::BAD_REQUEST,
            ),
            (
                configured.client_token("codex_desktop"),
                json!({}),
                StatusCode::BAD_REQUEST,
            ),
        ] {
            let mut headers = HeaderMap::new();
            headers.insert(
                header::AUTHORIZATION,
                format!("Bearer {token}").parse().unwrap(),
            );
            let response =
                responses_with_credential(fixture_state(), headers, body, configured.clone()).await;
            assert_eq!(response.status(), status);
            let body = to_bytes(response.into_body(), 4096).await.unwrap();
            assert!(!String::from_utf8_lossy(&body).contains("synthetic-key"));
        }
    }

    #[tokio::test]
    async fn ru042_codex_history_isolated_by_model_group_key_and_generation() {
        let state = fixture_state();
        let configured = synthetic_catalog("https://yeschoy.com");
        let history_a = state.history_for(&configured, "chat-a").await;
        let history_b = state.history_for(&configured, "responses-b").await;
        assert!(!Arc::ptr_eq(&history_a, &history_b));
        assert!(Arc::ptr_eq(
            &history_a,
            &state.history_for(&configured, "chat-a").await
        ));
        for change in ["key", "group", "origin"] {
            let mut changed = configured.clone();
            match change {
                "key" => changed.models[0].api_key = "synthetic-new-key".into(),
                "group" => changed.models[0].billing_group = "other-group".into(),
                _ => changed.origin = "https://api.yeschoy.com".into(),
            }
            let new_history = state.history_for(&changed, "chat-a").await;
            assert!(!Arc::ptr_eq(&history_a, &new_history));
        }
        // The old Arc is alive for in-flight recording, but is no longer visible
        // to the next configured credential generation.
        assert_eq!(Arc::strong_count(&history_a), 1);
    }

    #[test]
    fn ru042_codex_legacy_model_pin_remains_usable() {
        let mut configured = synthetic_catalog("https://yeschoy.com");
        configured.models.clear();
        let (route, group) =
            route_snapshot(&configured, &json!({"model":"legacy-client-alias"})).unwrap();
        assert_eq!(route.model_id, "chat-a");
        assert!(group.is_empty());
        assert_eq!(configured.client_token("codex_desktop"), "synthetic-key-a");
    }

    #[test]
    fn secure_comparison_requires_exact_value() {
        assert!(secure_equal("sk-example", "sk-example"));
        assert!(!secure_equal("sk-example", "sk-other"));
        assert!(!secure_equal("", "sk-example"));
    }

    #[test]
    fn converter_handles_a_basic_responses_request() {
        let request = json!({"model":"gpt-5.5","input":"hello","stream":false});
        let chat = transform_codex_chat::responses_to_chat_completions(request).unwrap();
        assert_eq!(chat["model"], "gpt-5.5");
        assert_eq!(chat["messages"][0]["role"], "user");
    }

    #[tokio::test]
    async fn responses_health_is_public_and_bounded() {
        assert_eq!(responses_head().await, StatusCode::OK);
    }
}

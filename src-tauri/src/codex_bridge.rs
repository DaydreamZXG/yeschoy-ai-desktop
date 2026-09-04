use std::{sync::Arc, time::Duration};

use axum::{
    body::Body,
    extract::{DefaultBodyLimit, State},
    http::{header, HeaderMap, StatusCode},
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

use crate::tool_credentials;

pub(crate) const BASE_URL: &str = "http://127.0.0.1:15722/yeschoy/v1";
const LISTEN_ADDRESS: &str = "127.0.0.1:15722";
const MAX_REQUEST_BYTES: usize = 2 * 1024 * 1024;
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
pub(crate) mod error;
#[path = "proxy/json_canonical.rs"]
pub(crate) mod json_canonical;
#[path = "proxy/sse.rs"]
pub(crate) mod sse;
#[path = "proxy/providers/streaming_codex_chat.rs"]
pub(crate) mod streaming_codex_chat;
#[path = "proxy/tool_media.rs"]
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
        let normalized = model.to_lowercase();
        is_openai_o_series(&normalized)
            || normalized
                .strip_prefix("gpt-")
                .and_then(|rest| rest.chars().next())
                .is_some_and(|character| character.is_ascii_digit() && character >= '5')
            || normalized == "grok-4.5"
            || normalized.starts_with("grok-4.5-")
            || normalized.starts_with("grok-build-")
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
    history: Arc<codex_chat_history::CodexChatHistoryStore>,
}

struct RunningBridge {
    shutdown: Option<oneshot::Sender<()>>,
    task: JoinHandle<()>,
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
        let client = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(15))
            .build()
            .map_err(|_| ())?;
        let state = BridgeAppState {
            client,
            history: Arc::new(codex_chat_history::CodexChatHistoryStore::default()),
        };
        let router = Router::new()
            .route("/yeschoy/health", get(health))
            .route("/yeschoy/v1/models", get(models))
            .route(
                "/yeschoy/v1/responses",
                post(responses).head(responses_head),
            )
            .layer(DefaultBodyLimit::max(MAX_REQUEST_BYTES))
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
            let _ = tokio::time::timeout(Duration::from_secs(2), bridge.task).await;
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

async fn models() -> Response {
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
    if credential.codex_transport.as_deref() != Some("chat_bridge") {
        return response_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "Compatibility connection is inactive",
        );
    }
    match crate::tool_adapters::codex_desktop::bridge_catalog(&credential.model_id) {
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

async fn responses(
    State(state): State<BridgeAppState>,
    headers: HeaderMap,
    Json(mut body): Json<Value>,
) -> Response {
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
    if credential.codex_transport.as_deref() != Some("chat_bridge") {
        return response_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "Compatibility connection is inactive",
        );
    }
    let bearer = headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .unwrap_or("");
    if !secure_equal(bearer, &credential.api_key) {
        return response_error(StatusCode::UNAUTHORIZED, "Authentication failed");
    }
    if !matches!(
        credential.origin.as_str(),
        "https://yeschoy.com" | "https://api.yeschoy.com"
    ) {
        return response_error(StatusCode::FORBIDDEN, "Upstream is not allowed");
    }
    let Some(object) = body.as_object_mut() else {
        return response_error(StatusCode::BAD_REQUEST, "Invalid request body");
    };
    object.insert("model".into(), Value::String(credential.model_id.clone()));
    state.history.enrich_request(&mut body).await;
    let tool_context = transform_codex_chat::build_codex_tool_context_from_request(&body);
    let chat = match transform_codex_chat::responses_to_chat_completions_with_reasoning(body, None)
    {
        Ok(value) => value,
        Err(_) => {
            return response_error(
                StatusCode::UNPROCESSABLE_ENTITY,
                "Request conversion failed",
            )
        }
    };
    let streaming = chat.get("stream").and_then(Value::as_bool) == Some(true);
    let upstream = match state
        .client
        .post(format!("{}/v1/chat/completions", credential.origin))
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
        Err(_) => return response_error(StatusCode::BAD_GATEWAY, "Upstream request failed"),
    };
    let status =
        StatusCode::from_u16(upstream.status().as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
    if !status.is_success() {
        let value = if upstream.content_length().unwrap_or_default() <= MAX_RESPONSE_BYTES {
            upstream.json::<Value>().await.ok()
        } else {
            None
        };
        let converted = transform_codex_chat::chat_error_to_response_error(value.as_ref());
        return (status, Json(converted)).into_response();
    }
    if streaming {
        let converted = streaming_codex_chat::create_responses_sse_stream_from_chat_with_context(
            upstream.bytes_stream(),
            tool_context,
        );
        let recorded =
            codex_chat_history::record_responses_sse_stream(converted, state.history.clone());
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
        return response_error(StatusCode::BAD_GATEWAY, "Upstream response was too large");
    }
    let chat_response = match upstream.bytes().await {
        Ok(bytes) if bytes.len() as u64 <= MAX_RESPONSE_BYTES => {
            serde_json::from_slice::<Value>(&bytes).ok()
        }
        _ => None,
    };
    let Some(chat_response) = chat_response else {
        return response_error(StatusCode::BAD_GATEWAY, "Invalid upstream response");
    };
    let converted = match transform_codex_chat::chat_completion_to_response_with_context(
        chat_response,
        &tool_context,
    ) {
        Ok(value) => value,
        Err(_) => return response_error(StatusCode::BAD_GATEWAY, "Response conversion failed"),
    };
    state.history.record_response(&converted).await;
    (StatusCode::OK, Json(converted)).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

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

use std::{sync::Arc, time::Duration};

use axum::{
    body::{to_bytes, Body},
    extract::State,
    http::{header, Request, Response, StatusCode},
    routing::any,
    Router,
};
use bytes::Bytes;
use futures::{Stream, StreamExt};
use serde_json::{json, Value};
use tokio::{
    net::TcpListener,
    sync::{broadcast, oneshot, Mutex},
    task::JoinHandle,
};

use crate::{
    codex_bridge::{claude_streaming as streaming, claude_transform as transform, secure_equal},
    tool_adapters::AdapterFailure,
    tool_credentials::ToolCredential,
};

const MAX_BODY_BYTES: usize = 8 * 1024 * 1024;

fn upstream_client() -> Result<reqwest::Client, reqwest::Error> {
    reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(Duration::from_secs(15))
        .read_timeout(Duration::from_secs(60))
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

    fn from_credential(credential: &ToolCredential) -> Self {
        match credential.claude_transport.as_deref() {
            Some("chat_bridge") => Self::ChatBridge,
            _ => Self::DirectAnthropic,
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct VerificationEvent {
    pub(crate) model: String,
}

struct ProxyState {
    credential: ToolCredential,
    local_token: String,
    prefix: &'static str,
    client: reqwest::Client,
    events: broadcast::Sender<VerificationEvent>,
}

struct ProxyRuntime {
    shutdown: Option<oneshot::Sender<()>>,
    task: JoinHandle<()>,
    credential: ToolCredential,
    events: broadcast::Sender<VerificationEvent>,
}

#[derive(Clone)]
pub(crate) struct ClaudeBridgeRuntime {
    address: &'static str,
    prefix: &'static str,
    runtime: Arc<Mutex<Option<ProxyRuntime>>>,
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
        // Opening an already configured app must not interrupt its requests.
        // Keep check/start/store under the same lock as stop.
        let mut slot = self.runtime.lock().await;
        if let Some(runtime) = slot.as_ref() {
            if runtime.credential == credential && !runtime.task.is_finished() {
                return Ok(runtime.events.subscribe());
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
        let proxy_state = Arc::new(ProxyState {
            credential: credential.clone(),
            local_token,
            prefix: self.prefix,
            client: upstream_client().map_err(|_| AdapterFailure::LaunchFailed)?,
            events: events.clone(),
        });
        let router = Router::new()
            .fallback(any(proxy_request))
            .with_state(proxy_state);
        let (shutdown, shutdown_rx) = oneshot::channel::<()>();
        let task = tokio::spawn(async move {
            let _ = axum::serve(listener, router)
                .with_graceful_shutdown(async move {
                    let _ = shutdown_rx.await;
                })
                .await;
        });
        *slot = Some(ProxyRuntime {
            shutdown: Some(shutdown),
            task,
            credential,
            events,
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

fn response(status: StatusCode, content_type: &'static str, body: Body) -> Response<Body> {
    Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, content_type)
        .body(body)
        .unwrap_or_else(|_| Response::new(Body::empty()))
}

fn error_response(status: StatusCode, reason: &'static str) -> Response<Body> {
    let body = serde_json::to_vec(&json!({
        "type": "error",
        "error": {"type": "api_error", "message": reason}
    }))
    .unwrap_or_default();
    response(status, "application/json", Body::from(body))
}

fn locally_authorized(headers: &axum::http::HeaderMap, local_token: &str) -> bool {
    let bearer = format!("Bearer {local_token}");
    headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| secure_equal(value, &bearer))
        || headers
            .get("x-api-key")
            .and_then(|value| value.to_str().ok())
            .is_some_and(|value| secure_equal(value, local_token))
}

fn selected_model_body(bytes: &[u8], model: &str) -> Result<Value, ()> {
    let mut value: Value = serde_json::from_slice(bytes).map_err(|_| ())?;
    value
        .as_object_mut()
        .ok_or(())?
        .insert("model".into(), model.into());
    Ok(value)
}

fn preserve_reasoning_content(model: &str) -> bool {
    let normalized = model.to_ascii_lowercase();
    normalized.contains("deepseek") || normalized.contains("mimo")
}

fn chat_request(bytes: &[u8], model: &str) -> Result<(Vec<u8>, bool), ()> {
    let value = selected_model_body(bytes, model)?;
    let streaming = value
        .get("stream")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let mut converted = transform::anthropic_to_openai_with_reasoning_content(
        value,
        preserve_reasoning_content(model),
    )
    .map_err(|_| ())?;
    transform::inject_openai_stream_include_usage(&mut converted);
    serde_json::to_vec(&converted)
        .map(|bytes| (bytes, streaming))
        .map_err(|_| ())
}

async fn read_bounded(response: reqwest::Response) -> Result<Vec<u8>, ()> {
    to_bytes(Body::from_stream(response.bytes_stream()), MAX_BODY_BYTES)
        .await
        .map(|bytes| bytes.to_vec())
        .map_err(|_| ())
}

async fn proxy_request(
    State(state): State<Arc<ProxyState>>,
    request: Request<Body>,
) -> Response<Body> {
    if !locally_authorized(request.headers(), &state.local_token) {
        return error_response(StatusCode::UNAUTHORIZED, "unauthorized");
    }

    let path = request.uri().path().to_owned();
    let Some(upstream_path) = path.strip_prefix(state.prefix) else {
        return error_response(StatusCode::NOT_FOUND, "route not found");
    };
    if upstream_path != "/v1/messages" {
        return error_response(StatusCode::NOT_FOUND, "unsupported Claude endpoint");
    }
    let method = request.method().clone();
    let anthropic_version = request.headers().get("anthropic-version").cloned();
    let anthropic_beta = request.headers().get("anthropic-beta").cloned();
    let accept = request.headers().get(header::ACCEPT).cloned();
    let body = match to_bytes(request.into_body(), MAX_BODY_BYTES).await {
        Ok(bytes) => bytes,
        Err(_) => return error_response(StatusCode::PAYLOAD_TOO_LARGE, "request too large"),
    };

    let transport = ClaudeTransport::from_credential(&state.credential);
    let (outbound_path, outbound_body, streaming) = match transport {
        ClaudeTransport::DirectAnthropic => {
            let value = match selected_model_body(&body, &state.credential.model_id) {
                Ok(value) => value,
                Err(_) => return error_response(StatusCode::BAD_REQUEST, "invalid request"),
            };
            let streaming = value
                .get("stream")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            let bytes = match serde_json::to_vec(&value) {
                Ok(bytes) => bytes,
                Err(_) => return error_response(StatusCode::BAD_REQUEST, "invalid request"),
            };
            ("/v1/messages", bytes, streaming)
        }
        ClaudeTransport::ChatBridge => {
            let (bytes, streaming) = match chat_request(&body, &state.credential.model_id) {
                Ok(value) => value,
                Err(_) => return error_response(StatusCode::BAD_REQUEST, "invalid request"),
            };
            ("/v1/chat/completions", bytes, streaming)
        }
    };
    let url = format!(
        "{}{}",
        state.credential.origin.trim_end_matches('/'),
        outbound_path
    );
    let mut outbound = state
        .client
        .request(method, url)
        .header(header::CONTENT_TYPE, "application/json")
        .header(
            header::AUTHORIZATION,
            format!("Bearer {}", state.credential.api_key),
        )
        .body(outbound_body);
    if transport == ClaudeTransport::DirectAnthropic {
        outbound = outbound.header("x-api-key", &state.credential.api_key);
        if let Some(value) = anthropic_version {
            outbound = outbound.header("anthropic-version", value);
        }
        if let Some(value) = anthropic_beta {
            outbound = outbound.header("anthropic-beta", value);
        }
    }
    if let Some(value) = accept {
        outbound = outbound.header(header::ACCEPT, value);
    }
    let upstream = match outbound.send().await {
        Ok(response) => response,
        Err(_) => return error_response(StatusCode::BAD_GATEWAY, "provider unavailable"),
    };
    let status = upstream.status();
    if !status.is_success() {
        return match read_bounded(upstream).await {
            Ok(bytes) => response(status, "application/json", Body::from(bytes)),
            Err(_) => error_response(StatusCode::BAD_GATEWAY, "invalid provider response"),
        };
    }
    if transport == ClaudeTransport::DirectAnthropic {
        if streaming {
            return response(
                status,
                "text/event-stream",
                Body::from_stream(verified_stream(upstream.bytes_stream(), state.clone())),
            );
        }
        let bytes = match read_bounded(upstream).await {
            Ok(bytes) => bytes,
            Err(_) => return error_response(StatusCode::BAD_GATEWAY, "invalid provider response"),
        };
        if !serde_json::from_slice::<Value>(&bytes)
            .ok()
            .as_ref()
            .is_some_and(usable_message)
        {
            return error_response(StatusCode::BAD_GATEWAY, "invalid provider response");
        }
        let _ = state.events.send(VerificationEvent {
            model: state.credential.model_id.clone(),
        });
        return response(status, "application/json", Body::from(bytes));
    }
    if streaming {
        let stream = streaming::create_anthropic_sse_stream(upstream.bytes_stream());
        return response(
            status,
            "text/event-stream",
            Body::from_stream(verified_stream(stream, state.clone())),
        );
    }
    let bytes = match read_bounded(upstream).await {
        Ok(bytes) => bytes,
        Err(_) => return error_response(StatusCode::BAD_GATEWAY, "invalid provider response"),
    };
    let value = match serde_json::from_slice::<Value>(&bytes)
        .map_err(|_| ())
        .and_then(|value| transform::openai_to_anthropic(value).map_err(|_| ()))
    {
        Ok(value) if usable_message(&value) => value,
        Ok(_) => return error_response(StatusCode::BAD_GATEWAY, "invalid provider response"),
        Err(_) => return error_response(StatusCode::BAD_GATEWAY, "invalid provider response"),
    };
    match serde_json::to_vec(&value) {
        Ok(bytes) => {
            let _ = state.events.send(VerificationEvent {
                model: state.credential.model_id.clone(),
            });
            response(status, "application/json", Body::from(bytes))
        }
        Err(_) => error_response(StatusCode::BAD_GATEWAY, "invalid provider response"),
    }
}

fn usable_message(value: &Value) -> bool {
    value["type"] == "message"
        && value["role"] == "assistant"
        && value["stop_reason"].as_str().is_some()
        && value["content"].as_array().is_some_and(|content| {
            content.iter().any(|block| {
                block["text"]
                    .as_str()
                    .is_some_and(|text| !text.trim().is_empty())
                    || (block["type"] == "tool_use" && block["name"].as_str().is_some())
            })
        })
}

#[derive(Default)]
struct CompletionObserver {
    buffer: Vec<u8>,
    useful: bool,
    stopped: bool,
    failed: bool,
}

impl CompletionObserver {
    fn push(&mut self, bytes: &[u8]) -> bool {
        self.buffer.extend_from_slice(bytes);
        if self.buffer.len() > MAX_BODY_BYTES {
            self.failed = true;
            self.buffer.clear();
            return false;
        }
        while let Some(end) = self.buffer.iter().position(|b| *b == b'\n') {
            let line: Vec<u8> = self.buffer.drain(..=end).collect();
            let line = String::from_utf8_lossy(&line);
            let Some(data) = line.trim().strip_prefix("data:") else {
                continue;
            };
            let Ok(event) = serde_json::from_str::<Value>(data.trim()) else {
                continue;
            };
            if event["type"] == "error" {
                self.failed = true;
            }
            if event["type"] == "content_block_delta" {
                self.useful |= event["delta"]["text"]
                    .as_str()
                    .is_some_and(|text| !text.trim().is_empty());
            }
            if event["type"] == "content_block_start" {
                self.useful |= event["content_block"]["type"] == "tool_use"
                    || event["content_block"]["text"]
                        .as_str()
                        .is_some_and(|text| !text.trim().is_empty());
            }
            if event["type"] == "message_stop" {
                self.stopped = true;
            }
        }
        self.useful && self.stopped && !self.failed
    }
}

fn verified_stream<E: std::error::Error + Send + 'static>(
    stream: impl Stream<Item = Result<Bytes, E>> + Send + 'static,
    state: Arc<ProxyState>,
) -> impl Stream<Item = Result<Bytes, std::io::Error>> + Send {
    async_stream::stream! {
        futures::pin_mut!(stream);
        let mut observer = CompletionObserver::default();
        let mut notified = false;
        while let Some(chunk) = stream.next().await {
            let bytes = match chunk {
                Ok(bytes) => bytes,
                Err(_) => { yield Err(std::io::Error::other("provider stream interrupted")); break; }
            };
            let completed = observer.push(&bytes);
            yield Ok(bytes);
            if completed && !notified {
                notified = true;
                let _ = state.events.send(VerificationEvent { model: state.credential.model_id.clone() });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn repeated_open_keeps_live_claude_bridge_and_credential_change_restarts() {
        let runtime = ClaudeBridgeRuntime::new("127.0.0.1:0", "/test");
        let mut credential = ToolCredential {
            api_key: "synthetic-test-key-only".into(),
            origin: "https://yeschoy.com".into(),
            model_id: "test-model".into(),
            local_gateway_token: Some(format!("ycg-{}", "b".repeat(64))),
            codex_transport: None,
            claude_transport: None,
        };
        runtime.start(credential.clone()).await.unwrap();
        let first = runtime.runtime.lock().await.as_ref().unwrap().task.id();
        runtime.start(credential.clone()).await.unwrap();
        assert_eq!(
            runtime.runtime.lock().await.as_ref().unwrap().task.id(),
            first
        );
        credential.model_id = "other-model".into();
        runtime.start(credential).await.unwrap();
        assert_ne!(
            runtime.runtime.lock().await.as_ref().unwrap().task.id(),
            first
        );
        runtime.stop().await;
        assert!(runtime.runtime.lock().await.is_none());
    }

    #[tokio::test]
    async fn upstream_redirects_never_forward_even_synthetic_credentials() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let destination = TcpListener::bind("127.0.0.1:0").await.unwrap();
        for status in [301, 302, 307, 308] {
            let source = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let url = format!("http://{}/test", source.local_addr().unwrap());
            let location = format!(
                "http://{}/must-not-receive",
                destination.local_addr().unwrap()
            );
            let server = tokio::spawn(async move {
                let (mut socket, _) = source.accept().await.unwrap();
                let mut request = [0u8; 2048];
                let n = socket.read(&mut request).await.unwrap();
                assert!(String::from_utf8_lossy(&request[..n]).contains("synthetic-test-only"));
                socket.write_all(format!("HTTP/1.1 {status} Redirect\r\nLocation: {location}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").as_bytes()).await.unwrap();
            });
            let response = upstream_client()
                .unwrap()
                .get(url)
                .header("x-api-key", "synthetic-test-only")
                .send()
                .await
                .unwrap();
            assert_eq!(response.status().as_u16(), status);
            server.await.unwrap();
        }
        assert!(
            tokio::time::timeout(Duration::from_millis(30), destination.accept())
                .await
                .is_err()
        );
    }

    #[test]
    fn response_headers_empty_bodies_and_errors_are_not_verification() {
        assert!(!usable_message(
            &serde_json::json!({"type":"error","error":{"message":"failed"}})
        ));
        assert!(!usable_message(
            &serde_json::json!({"type":"message","role":"assistant","content":[],"stop_reason":"end_turn"})
        ));
        assert!(!usable_message(
            &serde_json::json!({"type":"message","role":"assistant","content":[{"type":"text","text":"hello"}],"stop_reason":null})
        ));
        assert!(usable_message(
            &serde_json::json!({"type":"message","role":"assistant","content":[{"type":"text","text":"hello"}],"stop_reason":"end_turn"})
        ));
    }

    #[test]
    fn stream_verification_requires_useful_complete_response_at_every_chunk_boundary() {
        let text=b"event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"delta\":{\"text\":\"YESCHOY_OK\"}}\n\n";
        let stop = b"event: message_stop\ndata: {\"type\":\"message_stop\"}\n\n";
        let payload = [text.as_slice(), stop.as_slice()].concat();
        for split in 0..payload.len() {
            let mut observer = CompletionObserver::default();
            observer.push(&payload[..split]);
            assert!(observer.push(&payload[split..]), "chunk split {split}");
        }
        let mut truncated = CompletionObserver::default();
        assert!(!truncated.push(text));
        let mut empty = CompletionObserver::default();
        assert!(!empty.push(stop));
        let mut failed = CompletionObserver::default();
        failed.push(text);
        failed.push(b"data: {\"type\":\"error\"}\n\n");
        assert!(!failed.push(stop));
    }

    #[test]
    fn local_auth_accepts_claude_code_and_desktop_header_shapes_only() {
        let token = format!("ycg-{}", "a".repeat(64));
        let mut headers = axum::http::HeaderMap::new();
        headers.insert("x-api-key", token.parse().unwrap());
        assert!(locally_authorized(&headers, &token));
        headers.clear();
        headers.insert(
            header::AUTHORIZATION,
            format!("Bearer {token}").parse().unwrap(),
        );
        assert!(locally_authorized(&headers, &token));
        headers.insert("x-api-key", "ycg-wrong".parse().unwrap());
        assert!(locally_authorized(&headers, &token));
        headers.clear();
        headers.insert("x-api-key", "ycg-wrong".parse().unwrap());
        assert!(!locally_authorized(&headers, &token));
    }

    #[test]
    fn chat_request_uses_selected_model_and_converts_tools() {
        let input = serde_json::to_vec(&json!({
            "model": "caller-controlled",
            "max_tokens": 512,
            "stream": true,
            "messages": [{"role":"user","content":"hello"}],
            "tools": [{"name":"read_file","description":"read","input_schema":{"type":"object"}}]
        }))
        .unwrap();
        let (bytes, streaming) = chat_request(&input, "claude-opus-5").unwrap();
        let value: Value = serde_json::from_slice(&bytes).unwrap();
        assert!(streaming);
        assert_eq!(value["model"], "claude-opus-5");
        assert_eq!(value["tools"][0]["type"], "function");
        assert_eq!(value["stream_options"]["include_usage"], true);
        assert!(!String::from_utf8(bytes)
            .unwrap()
            .contains("caller-controlled"));
    }

    #[test]
    fn non_stream_chat_response_converts_back_to_anthropic() {
        let value = transform::openai_to_anthropic(json!({
            "id":"chatcmpl-1",
            "model":"claude-opus-5",
            "choices":[{"message":{"role":"assistant","content":"ok"},"finish_reason":"stop"}],
            "usage":{"prompt_tokens":3,"completion_tokens":1}
        }))
        .unwrap();
        assert_eq!(value["type"], "message");
        assert_eq!(value["content"][0]["text"], "ok");
        assert_eq!(value["usage"]["input_tokens"], 3);
    }
}

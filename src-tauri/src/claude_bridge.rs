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
    sync::{broadcast, oneshot, Mutex, RwLock},
    task::JoinHandle,
};

use crate::{
    codex_bridge::{claude_streaming as streaming, claude_transform as transform, secure_equal},
    loopback_http::{admit_ai_request, read_ai_request_body, AiRequestAdmissionError},
    request_diagnostics::{self, RequestContext, RequestOutcome, StreamProtocol},
    tool_adapters::AdapterFailure,
    tool_credentials::ToolCredential,
};

const MAX_RESPONSE_BODY_BYTES: usize = 8 * 1024 * 1024;

fn upstream_client() -> Result<reqwest::Client, reqwest::Error> {
    reqwest::Client::builder()
        // Honor the user's standard outbound proxy environment. Loopback
        // callers use separate no-proxy clients, so local traffic stays local.
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
    state: Arc<RwLock<Arc<ProxyState>>>,
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
        if let Some(runtime) = slot.as_mut() {
            if !runtime.task.is_finished() {
                if runtime.credential != credential {
                    let previous = runtime.state.read().await.clone();
                    *runtime.state.write().await = Arc::new(ProxyState {
                        credential: credential.clone(),
                        local_token,
                        prefix: self.prefix,
                        client: previous.client.clone(),
                        events: runtime.events.clone(),
                    });
                    runtime.credential = credential;
                }
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
        let shared_state = Arc::new(RwLock::new(proxy_state));
        let router = Router::new()
            .fallback(any(proxy_dispatch))
            .with_state(shared_state.clone());
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
            state: shared_state,
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

async fn proxy_dispatch(
    State(state): State<Arc<RwLock<Arc<ProxyState>>>>,
    request: Request<Body>,
) -> Response<Body> {
    let snapshot = state.read().await.clone();
    if !matches!(
        snapshot.credential.origin.as_str(),
        "https://yeschoy.com" | "https://api.yeschoy.com"
    ) || snapshot
        .credential
        .models
        .iter()
        .any(|route| route.origin != snapshot.credential.origin)
    {
        return error_response(StatusCode::FORBIDDEN, "Upstream is not allowed");
    }
    proxy_request(State(snapshot), request).await
}

fn tool_id(prefix: &str) -> &'static str {
    if prefix.contains("desktop") {
        "claude_desktop"
    } else {
        "claude_code"
    }
}

fn route_snapshot(
    credential: &ToolCredential,
    prefix: &str,
    body: &[u8],
) -> Result<(ToolCredential, String), ()> {
    if !credential.has_model_set() {
        return Ok((credential.clone(), String::new()));
    }
    let value: Value = serde_json::from_slice(body).map_err(|_| ())?;
    let requested = value["model"]
        .as_str()
        .filter(|model| !model.is_empty())
        .ok_or(())?;
    let route = credential
        .models
        .iter()
        .find(|route| {
            if tool_id(prefix) == "claude_desktop" {
                crate::tool_model_profile::claude_gateway_route_matches(&route.model_id, requested)
            } else {
                route.model_id == requested
            }
        })
        .ok_or(())?;
    let group = route.billing_group.clone();
    credential
        .resolve_model(&route.model_id)
        .map(|route| (route, group))
        .map_err(|_| ())
}

fn observed_error(
    context: &RequestContext,
    status: StatusCode,
    outcome: RequestOutcome,
) -> Response<Body> {
    context.record(outcome, status.as_u16());
    error_response(status, request_diagnostics::curated_message(outcome))
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

async fn read_bounded(response: reqwest::Response) -> Result<Vec<u8>, RequestOutcome> {
    to_bytes(
        Body::from_stream(response.bytes_stream()),
        MAX_RESPONSE_BODY_BYTES,
    )
    .await
    .map(|bytes| bytes.to_vec())
    .map_err(|error| {
        if request_diagnostics::transport_outcome(&error) == RequestOutcome::Timeout {
            RequestOutcome::Timeout
        } else {
            RequestOutcome::InvalidResponse
        }
    })
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
    if upstream_path == "/v1/models" && request.method() == axum::http::Method::GET {
        let models = state.credential.model_ids();
        let desktop = tool_id(state.prefix) == "claude_desktop";
        let ids = models
            .iter()
            .map(|id| {
                if desktop {
                    crate::tool_model_profile::claude_gateway_route_id(id)
                } else {
                    id.clone()
                }
            })
            .collect::<Vec<_>>();
        let data: Vec<_> = ids
            .iter()
            .zip(models.iter())
            .map(|(route, model)| json!({"id":route,"type":"model","display_name":crate::tool_model_profile::display_name(model)}))
            .collect();
        let body =
            json!({"data":data,"has_more":false,"first_id":ids.first(),"last_id":ids.last()});
        return response(
            StatusCode::OK,
            "application/json",
            Body::from(body.to_string()),
        );
    }
    if upstream_path != "/v1/messages" {
        return error_response(StatusCode::NOT_FOUND, "unsupported Claude endpoint");
    }
    let local_context = RequestContext {
        tool: tool_id(state.prefix).into(),
        model: String::new(),
        group: String::new(),
        origin: state.credential.origin.clone(),
    };
    let _admission = match admit_ai_request(request.headers()).await {
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
    let method = request.method().clone();
    let anthropic_version = request.headers().get("anthropic-version").cloned();
    let anthropic_beta = request.headers().get("anthropic-beta").cloned();
    let accept = request.headers().get(header::ACCEPT).cloned();
    let body = match read_ai_request_body(request.into_body()).await {
        Ok(bytes) => bytes,
        Err(_) => {
            return observed_error(
                &local_context,
                StatusCode::PAYLOAD_TOO_LARGE,
                RequestOutcome::PayloadTooLarge,
            )
        }
    };

    let (credential, group) = match route_snapshot(&state.credential, state.prefix, &body) {
        Ok(route) => route,
        Err(_) => {
            request_diagnostics::record(
                tool_id(state.prefix),
                "",
                "",
                &state.credential.origin,
                RequestOutcome::UnknownModel,
                0,
            );
            return error_response(
                StatusCode::BAD_REQUEST,
                request_diagnostics::curated_message(RequestOutcome::UnknownModel),
            );
        }
    };
    let context = RequestContext {
        tool: tool_id(state.prefix).into(),
        model: credential.model_id.clone(),
        group,
        origin: credential.origin.clone(),
    };
    if state.credential.has_model_set()
        && !matches!(
            credential.claude_transport.as_deref(),
            Some("direct_anthropic" | "chat_bridge")
        )
    {
        return observed_error(
            &context,
            StatusCode::BAD_REQUEST,
            RequestOutcome::InvalidResponse,
        );
    }
    let transport = ClaudeTransport::from_credential(&credential);
    let (outbound_path, outbound_body, streaming) = match transport {
        ClaudeTransport::DirectAnthropic => {
            let value = match selected_model_body(&body, &credential.model_id) {
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
            let (bytes, streaming) = match chat_request(&body, &credential.model_id) {
                Ok(value) => value,
                Err(_) => return error_response(StatusCode::BAD_REQUEST, "invalid request"),
            };
            ("/v1/chat/completions", bytes, streaming)
        }
    };
    let url = format!(
        "{}{}",
        credential.origin.trim_end_matches('/'),
        outbound_path
    );
    let mut outbound = state
        .client
        .request(method, url)
        .header(header::CONTENT_TYPE, "application/json")
        .header(
            header::AUTHORIZATION,
            format!("Bearer {}", credential.api_key),
        )
        .body(outbound_body);
    if transport == ClaudeTransport::DirectAnthropic {
        outbound = outbound.header("x-api-key", &credential.api_key);
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
        Err(error) => {
            let outcome = request_diagnostics::transport_outcome(&error);
            context.record(outcome, 0);
            return error_response(
                StatusCode::BAD_GATEWAY,
                request_diagnostics::curated_message(outcome),
            );
        }
    };
    let status = upstream.status();
    if !status.is_success() {
        return observed_error(
            &context,
            status,
            request_diagnostics::outcome_for_status(status.as_u16()),
        );
    }
    if transport == ClaudeTransport::DirectAnthropic {
        if streaming {
            return response(
                status,
                "text/event-stream",
                Body::from_stream(verified_stream(
                    request_diagnostics::observed_sse(
                        upstream.bytes_stream(),
                        StreamProtocol::Anthropic,
                        context,
                        status.as_u16(),
                    ),
                    state.clone(),
                    credential.model_id.clone(),
                )),
            );
        }
        let bytes = match read_bounded(upstream).await {
            Ok(bytes) => bytes,
            Err(outcome) => return observed_error(&context, StatusCode::BAD_GATEWAY, outcome),
        };
        let value = match serde_json::from_slice::<Value>(&bytes) {
            Ok(value) if complete_message(&value) => value,
            _ => {
                return observed_error(
                    &context,
                    StatusCode::BAD_GATEWAY,
                    RequestOutcome::InvalidResponse,
                )
            }
        };
        if usable_message(&value) {
            let _ = state.events.send(VerificationEvent {
                model: credential.model_id.clone(),
            });
        }
        context.record(RequestOutcome::Ok, status.as_u16());
        return response(status, "application/json", Body::from(bytes));
    }
    if streaming {
        let stream = streaming::create_anthropic_sse_stream(upstream.bytes_stream());
        return response(
            status,
            "text/event-stream",
            Body::from_stream(verified_stream(
                request_diagnostics::observed_sse(
                    stream,
                    StreamProtocol::Anthropic,
                    context,
                    status.as_u16(),
                ),
                state.clone(),
                credential.model_id.clone(),
            )),
        );
    }
    let bytes = match read_bounded(upstream).await {
        Ok(bytes) => bytes,
        Err(outcome) => return observed_error(&context, StatusCode::BAD_GATEWAY, outcome),
    };
    let value = match serde_json::from_slice::<Value>(&bytes)
        .map_err(|_| ())
        .and_then(|value| {
            // The converter tolerates absent content, but a missing or non-object
            // message must not become a seemingly valid empty completion.
            if !value["choices"][0]["message"].is_object() {
                return Err(());
            }
            transform::openai_to_anthropic(value).map_err(|_| ())
        }) {
        Ok(value) if complete_message(&value) => value,
        Ok(_) => {
            return observed_error(
                &context,
                StatusCode::BAD_GATEWAY,
                RequestOutcome::InvalidResponse,
            )
        }
        Err(_) => {
            return observed_error(
                &context,
                StatusCode::BAD_GATEWAY,
                RequestOutcome::InvalidResponse,
            )
        }
    };
    match serde_json::to_vec(&value) {
        Ok(bytes) => {
            if usable_message(&value) {
                let _ = state.events.send(VerificationEvent {
                    model: credential.model_id.clone(),
                });
            }
            context.record(RequestOutcome::Ok, status.as_u16());
            response(status, "application/json", Body::from(bytes))
        }
        Err(_) => observed_error(
            &context,
            StatusCode::BAD_GATEWAY,
            RequestOutcome::InvalidResponse,
        ),
    }
}

// A completed protocol response need not contain visible text: Desktop's
// startup probe only requests one token, and Claude also supports empty turns
// and thinking-only output. Forward these without claiming useful activation.
fn complete_message(value: &Value) -> bool {
    value["type"] == "message"
        && value["role"] == "assistant"
        && value["stop_reason"]
            .as_str()
            .is_some_and(|reason| !reason.is_empty())
        && value["content"].as_array().is_some_and(|content| {
            content.iter().all(|block| match block["type"].as_str() {
                Some("text") => block["text"].is_string(),
                Some("thinking") => block["thinking"].is_string(),
                Some("redacted_thinking") => block["data"].is_string(),
                Some("tool_use") => block["name"].as_str().is_some_and(|name| !name.is_empty()),
                // Preserve additional protocol block types rather than tying
                // forwarding to a particular installed model or SDK version.
                Some(kind) => !kind.is_empty() && block.is_object(),
                None => false,
            })
        })
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
        if self.buffer.len() > MAX_RESPONSE_BODY_BYTES {
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
    model: String,
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
                let _ = state.events.send(VerificationEvent { model: model.clone() });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn claude_code_and_desktop_forward_requests_above_the_legacy_limit() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let forwarded_sizes = Arc::new(Mutex::new(Vec::new()));
        let captured = forwarded_sizes.clone();
        let server = tokio::spawn(async move {
            let router = Router::new().fallback(any(move |request: Request<Body>| {
                let captured = captured.clone();
                async move {
                    let body = crate::loopback_http::read_ai_request_body(request.into_body())
                        .await
                        .unwrap();
                    captured.lock().await.push(body.len());
                    response(
                        StatusCode::OK,
                        "application/json",
                        Body::from(
                            json!({
                                "type":"message",
                                "role":"assistant",
                                "model":"selected-model",
                                "content":[{"type":"text","text":"ok"}],
                                "stop_reason":"end_turn"
                            })
                            .to_string(),
                        ),
                    )
                }
            }));
            axum::serve(listener, router).await.unwrap();
        });
        let token = format!("ycg-{}", "a".repeat(64));
        let attachment = "a".repeat(8 * 1024 * 1024 + 1);
        for prefix in ["/claude-code", "/claude-desktop"] {
            let (events, _) = broadcast::channel(8);
            let state = Arc::new(ProxyState {
                credential: ToolCredential {
                    api_key: "synthetic-upstream-key".into(),
                    origin: origin.clone(),
                    model_id: "selected-model".into(),
                    local_gateway_token: Some(token.clone()),
                    codex_transport: None,
                    claude_transport: Some("direct_anthropic".into()),
                    models: vec![],
                },
                local_token: token.clone(),
                prefix,
                client: upstream_client().unwrap(),
                events,
            });
            let request = Request::builder()
                .method("POST")
                .uri(format!("{prefix}/v1/messages"))
                .header("x-api-key", &token)
                .body(Body::from(
                    json!({
                        "model":"selected-model",
                        "max_tokens":1,
                        "messages":[{"role":"user","content":attachment}]
                    })
                    .to_string(),
                ))
                .unwrap();
            assert_eq!(
                proxy_request(State(state), request).await.status(),
                StatusCode::OK
            );
        }
        let sizes = forwarded_sizes.lock().await;
        assert_eq!(sizes.len(), 2);
        assert!(sizes.iter().all(|size| *size > 8 * 1024 * 1024));
        server.abort();
    }

    #[tokio::test]
    async fn ru042_claude_exact_models_use_distinct_keys_and_protocols() {
        use crate::tool_credentials::ToolModelRoute;
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let requests = Arc::new(Mutex::new(Vec::new()));
        let captured = requests.clone();
        let server = tokio::spawn(async move {
            axum::serve(listener, Router::new().fallback(any(move |uri: axum::http::Uri, headers: axum::http::HeaderMap, body: Bytes| {
                let captured = captured.clone();
                async move {
                    let body: Value = serde_json::from_slice(&body).unwrap();
                    captured.lock().await.push(json!({"path":uri.path(), "auth":headers[header::AUTHORIZATION].to_str().unwrap(), "body":body}));
                    let value = if uri.path() == "/v1/messages" {
                        json!({"type":"message","role":"assistant","model":body["model"],"content":[{"type":"text","text":"ok"}],"stop_reason":"end_turn"})
                    } else { json!({"id":"synthetic","model":body["model"],"choices":[{"message":{"role":"assistant","content":"ok"},"finish_reason":"stop"}]}) };
                    response(StatusCode::OK, "application/json", Body::from(value.to_string()))
                }
            }))).await.unwrap();
        });
        let token = format!("ycg-{}", "a".repeat(64));
        let routes = vec![
            ToolModelRoute {
                model_id: "claude-test-a".into(),
                billing_group: "group-a".into(),
                api_key: "synthetic-key-a".into(),
                origin: origin.clone(),
                claude_transport: Some("direct_anthropic".into()),
                codex_transport: None,
            },
            ToolModelRoute {
                model_id: "gpt-5".into(),
                billing_group: "group-b".into(),
                api_key: "synthetic-key-b".into(),
                origin: origin.clone(),
                claude_transport: Some("chat_bridge".into()),
                codex_transport: None,
            },
        ];
        let configured = ToolCredential {
            api_key: routes[0].api_key.clone(),
            origin,
            model_id: routes[0].model_id.clone(),
            local_gateway_token: Some(token.clone()),
            codex_transport: None,
            claude_transport: routes[0].claude_transport.clone(),
            models: routes,
        };
        let (events, _) = broadcast::channel(8);
        let state = Arc::new(ProxyState {
            credential: configured.clone(),
            local_token: token.clone(),
            prefix: "/claude-desktop",
            client: upstream_client().unwrap(),
            events,
        });
        let catalog_request = Request::builder()
            .uri("/claude-desktop/v1/models")
            .header("x-api-key", &token)
            .body(Body::empty())
            .unwrap();
        let catalog = proxy_request(State(state.clone()), catalog_request).await;
        assert_eq!(catalog.status(), StatusCode::OK);
        let catalog: Value =
            serde_json::from_slice(&to_bytes(catalog.into_body(), 4096).await.unwrap()).unwrap();
        let first_alias = crate::tool_model_profile::claude_gateway_route_id("claude-test-a");
        let second_alias = crate::tool_model_profile::claude_gateway_route_id("gpt-5");
        assert_eq!(catalog["data"][0]["id"], first_alias);
        assert_eq!(catalog["data"][1]["display_name"], "gpt-5");
        assert_eq!(catalog["data"][1]["id"], second_alias);
        assert!(!catalog.to_string().contains("synthetic-key"));
        for model in [
            Some(first_alias.as_str()),
            Some(second_alias.as_str()),
            Some("unregistered"),
            None,
        ] {
            let mut body =
                json!({"max_tokens":1,"messages":[{"role":"user","content":"unchanged"}]});
            if let Some(model) = model {
                body["model"] = json!(model);
            }
            let request = Request::builder()
                .method("POST")
                .uri("/claude-desktop/v1/messages")
                .header("x-api-key", &token)
                .body(Body::from(body.to_string()))
                .unwrap();
            let reply = proxy_request(State(state.clone()), request).await;
            assert_eq!(
                reply.status(),
                if model.is_some_and(|model| model != "unregistered") {
                    StatusCode::OK
                } else {
                    StatusCode::BAD_REQUEST
                }
            );
        }
        let requests = requests.lock().await;
        assert_eq!(
            requests.len(),
            2,
            "unknown models must never reach an upstream or fallback"
        );
        assert_eq!(requests[0]["path"], "/v1/messages");
        assert_eq!(requests[1]["path"], "/v1/chat/completions");
        assert_eq!(requests[0]["auth"], "Bearer synthetic-key-a");
        assert_eq!(requests[1]["auth"], "Bearer synthetic-key-b");
        assert_eq!(requests[0]["body"]["model"], "claude-test-a");
        assert_eq!(requests[1]["body"]["model"], "gpt-5");
        assert_eq!(requests[1]["body"]["max_completion_tokens"], 1);
        assert_eq!(requests[1]["body"]["messages"][0]["content"], "unchanged");
        assert_eq!(
            route_snapshot(
                &configured,
                "/claude-desktop",
                format!(r#"{{"model":"{second_alias}"}}"#).as_bytes()
            )
            .unwrap()
            .1,
            "group-b"
        );
        assert!(route_snapshot(&configured, "/claude-desktop", br#"{"model":"gpt-5"}"#).is_err());
        assert_eq!(
            route_snapshot(&configured, "/claude-code", br#"{"model":"gpt-5"}"#)
                .unwrap()
                .1,
            "group-b"
        );
        server.abort();
    }

    #[test]
    fn ru054_claude_desktop_alias_routes_exact_models_and_rejects_unknown_aliases() {
        use crate::tool_credentials::ToolModelRoute;
        let routes = vec![
            ToolModelRoute {
                model_id: "deepseek-v4-flash".into(),
                billing_group: "domestic".into(),
                api_key: "synthetic-key-a".into(),
                origin: "https://yeschoy.com".into(),
                claude_transport: Some("chat_bridge".into()),
                codex_transport: None,
            },
            ToolModelRoute {
                model_id: "gpt-6-astra".into(),
                billing_group: "global".into(),
                api_key: "synthetic-key-b".into(),
                origin: "https://yeschoy.com".into(),
                claude_transport: Some("chat_bridge".into()),
                codex_transport: None,
            },
        ];
        let configured = ToolCredential {
            api_key: routes[0].api_key.clone(),
            origin: "https://yeschoy.com".into(),
            model_id: routes[0].model_id.clone(),
            local_gateway_token: Some(format!("ycg-{}", "a".repeat(64))),
            codex_transport: None,
            claude_transport: routes[0].claude_transport.clone(),
            models: routes,
        };
        for (model, group, key) in [
            ("deepseek-v4-flash", "domestic", "synthetic-key-a"),
            ("gpt-6-astra", "global", "synthetic-key-b"),
        ] {
            let alias = crate::tool_model_profile::claude_gateway_route_id(model);
            let body = json!({"model":alias}).to_string();
            let (selected, selected_group) =
                route_snapshot(&configured, "/claude-desktop", body.as_bytes()).unwrap();
            assert_eq!(selected.model_id, model);
            assert_eq!(selected.api_key, key);
            assert_eq!(selected_group, group);
        }
        let legacy_alias =
            crate::tool_model_profile::legacy_claude_gateway_route_id("deepseek-v4-flash");
        let legacy_body = json!({"model":legacy_alias}).to_string();
        let (legacy, legacy_group) =
            route_snapshot(&configured, "/claude-desktop", legacy_body.as_bytes()).unwrap();
        assert_eq!(legacy.model_id, "deepseek-v4-flash");
        assert_eq!(legacy_group, "domestic");
        assert!(route_snapshot(
            &configured,
            "/claude-desktop",
            br#"{"model":"anthropic/claude-router-unknown"}"#
        )
        .is_err());
        assert!(route_snapshot(
            &configured,
            "/claude-desktop",
            br#"{"model":"deepseek-v4-flash"}"#
        )
        .is_err());
        assert_eq!(
            route_snapshot(
                &configured,
                "/claude-code",
                br#"{"model":"deepseek-v4-flash"}"#
            )
            .unwrap()
            .0
            .model_id,
            "deepseek-v4-flash"
        );
    }

    #[tokio::test]
    async fn ru042_claude_chat_sse_error_is_not_lost_or_verified() {
        let body = concat!(
            "data: {\"id\":\"synthetic\",\"model\":\"selected-model\",\"choices\":[{\"delta\":{\"content\":\"partial\"}}]}\n\n",
            "event: error\ndata: {\"error\":{\"message\":\"synthetic-key secret raw upstream\"}}\n\n",
            "data: [DONE]\n\n"
        );
        let result = proxy_fixture(
            ClaudeTransport::ChatBridge,
            StatusCode::OK,
            "text/event-stream",
            body.as_bytes().to_vec(),
            true,
            true,
        )
        .await;
        let output = String::from_utf8(result.body).unwrap();
        assert!(output.contains("event: error"));
        assert!(!output.contains("synthetic-key"));
        assert!(!output.contains("message_stop"));
        assert!(result.verified_models.is_empty());
        assert_eq!(result.requests.len(), 1);
    }

    struct ProxyResult {
        status: StatusCode,
        body: Vec<u8>,
        verified_models: Vec<String>,
        requests: Vec<Value>,
    }

    // Only an ephemeral loopback server and synthetic credentials are used.
    // Exercise the real forwarding function, not just its response predicate.
    async fn proxy_fixture(
        transport: ClaudeTransport,
        status: StatusCode,
        content_type: &'static str,
        upstream_body: Vec<u8>,
        stream: bool,
        authorized: bool,
    ) -> ProxyResult {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let requests = Arc::new(Mutex::new(Vec::new()));
        let captured = requests.clone();
        let router = Router::new().fallback(any(
            move |uri: axum::http::Uri, headers: axum::http::HeaderMap, body: Bytes| {
                let captured = captured.clone();
                let upstream_body = upstream_body.clone();
                async move {
                    captured.lock().await.push(json!({
                        "path":uri.path(),
                        "body":serde_json::from_slice::<Value>(&body).unwrap(),
                        "upstreamAuth":headers.get(header::AUTHORIZATION).unwrap() == "Bearer synthetic-test-only",
                        "localAuthLeaked":headers.get("x-api-key").is_some_and(|v| v.to_str().unwrap().starts_with("ycg-"))
                    }));
                    response(status, content_type, Body::from(upstream_body))
                }
            },
        ));
        let server = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
        let (events, mut receiver) = broadcast::channel(8);
        let token = format!("ycg-{}", "c".repeat(64));
        let state = Arc::new(ProxyState {
            credential: ToolCredential {
                api_key: "synthetic-test-only".into(),
                origin,
                model_id: "selected-model".into(),
                local_gateway_token: Some(token.clone()),
                codex_transport: None,
                claude_transport: Some(transport.credential_value().into()),
                models: vec![],
            },
            local_token: token.clone(),
            prefix: "/claude-desktop",
            client: upstream_client().unwrap(),
            events,
        });
        // Exact Desktop startup-probe shape: max_tokens=1, '.' user input,
        // and the stream field absent. A caller model must not replace selection.
        let mut body = json!({"model":"claude-sonnet-4-6","max_tokens":1,
            "messages":[{"role":"user","content":"."}]});
        if stream {
            body["stream"] = json!(true);
        }
        let request = Request::builder()
            .method("POST")
            .uri("/claude-desktop/v1/messages")
            .header(
                "x-api-key",
                if authorized {
                    &token
                } else {
                    "wrong-synthetic-token"
                },
            )
            .header("anthropic-version", "2023-06-01")
            .body(Body::from(serde_json::to_vec(&body).unwrap()))
            .unwrap();
        let result = proxy_request(State(state), request).await;
        let status = result.status();
        let body = to_bytes(result.into_body(), MAX_RESPONSE_BODY_BYTES)
            .await
            .unwrap()
            .to_vec();
        let mut verified_models = Vec::new();
        while let Ok(event) = receiver.try_recv() {
            verified_models.push(event.model);
        }
        server.abort();
        let requests = requests.lock().await.clone();
        ProxyResult {
            status,
            body,
            verified_models,
            requests,
        }
    }

    fn assert_single_pinned_request(result: &ProxyResult, transport: ClaudeTransport) {
        assert_eq!(
            result.requests.len(),
            1,
            "no fallback or extra billed retry"
        );
        let request = &result.requests[0];
        assert_eq!(request["body"]["model"], "selected-model");
        assert_eq!(request["body"]["max_tokens"], 1);
        assert_eq!(request["upstreamAuth"], true);
        assert_eq!(request["localAuthLeaked"], false);
        assert_eq!(
            request["path"],
            match transport {
                ClaudeTransport::DirectAnthropic => "/v1/messages",
                ClaudeTransport::ChatBridge => "/v1/chat/completions",
            }
        );
    }

    #[tokio::test]
    async fn claude_startup_probe_forwards_valid_empty_messages_without_verifying_activation() {
        for content in [
            json!([]),
            json!([{"type":"text","text":""}]),
            json!([{"type":"text","text":" \n"}]),
            json!([{"type":"thinking","thinking":"synthetic thought","signature":"synthetic"}]),
            json!([{"type":"redacted_thinking","data":"synthetic"}]),
        ] {
            for reason in ["max_tokens", "end_turn"] {
                let value = json!({"id":"msg_test","type":"message","role":"assistant",
                    "model":"selected-model","content":content,"stop_reason":reason,
                    "usage":{"input_tokens":1,"output_tokens":1}});
                let bytes = serde_json::to_vec(&value).unwrap();
                let result = proxy_fixture(
                    ClaudeTransport::DirectAnthropic,
                    StatusCode::OK,
                    "application/json",
                    bytes.clone(),
                    false,
                    true,
                )
                .await;
                assert_eq!(result.status, StatusCode::OK);
                assert_eq!(
                    result.body, bytes,
                    "preserve the upstream message, do not fabricate text"
                );
                assert!(result.verified_models.is_empty());
                assert_single_pinned_request(&result, ClaudeTransport::DirectAnthropic);
                assert!(result.requests[0]["body"].get("stream").is_none());
            }
        }
    }

    #[tokio::test]
    async fn claude_chat_probe_accepts_empty_and_thinking_only_without_fake_verification() {
        for message in [
            json!({"role":"assistant","content":""}),
            json!({"role":"assistant","content":" "}),
            json!({"role":"assistant","content":null,"reasoning_content":"synthetic thought"}),
        ] {
            let bytes = serde_json::to_vec(&json!({"id":"chatcmpl_test","model":"selected-model",
                "choices":[{"message":message,"finish_reason":"length"}],
                "usage":{"prompt_tokens":1,"completion_tokens":1}}))
            .unwrap();
            let result = proxy_fixture(
                ClaudeTransport::ChatBridge,
                StatusCode::OK,
                "application/json",
                bytes,
                false,
                true,
            )
            .await;
            assert_eq!(result.status, StatusCode::OK);
            let value: Value = serde_json::from_slice(&result.body).unwrap();
            assert_eq!(value["type"], "message");
            assert_eq!(value["stop_reason"], "max_tokens");
            assert!(complete_message(&value));
            assert!(!usable_message(&value));
            assert!(result.verified_models.is_empty());
            assert_single_pinned_request(&result, ClaudeTransport::ChatBridge);
        }
    }

    #[tokio::test]
    async fn claude_proxy_preserves_useful_success_real_errors_and_auth_boundaries() {
        for transport in [
            ClaudeTransport::DirectAnthropic,
            ClaudeTransport::ChatBridge,
        ] {
            let value = match transport {
                ClaudeTransport::DirectAnthropic => json!({"type":"message","role":"assistant",
                    "content":[{"type":"text","text":"ok"}],"stop_reason":"end_turn"}),
                ClaudeTransport::ChatBridge => json!({"id":"test","model":"selected-model",
                    "choices":[{"message":{"role":"assistant","content":"ok"},"finish_reason":"stop"}]}),
            };
            let result = proxy_fixture(
                transport,
                StatusCode::OK,
                "application/json",
                serde_json::to_vec(&value).unwrap(),
                false,
                true,
            )
            .await;
            assert_eq!(result.status, StatusCode::OK);
            assert_eq!(result.verified_models, ["selected-model"]);
            assert_single_pinned_request(&result, transport);
            for status in [
                StatusCode::UNAUTHORIZED,
                StatusCode::TOO_MANY_REQUESTS,
                StatusCode::BAD_GATEWAY,
            ] {
                let bytes = br#"{"type":"error","error":{"type":"api_error","message":"synthetic failure"}}"#.to_vec();
                let result = proxy_fixture(
                    transport,
                    status,
                    "application/json",
                    bytes.clone(),
                    false,
                    true,
                )
                .await;
                assert_eq!(result.status, status);
                assert!(!String::from_utf8_lossy(&result.body).contains("synthetic failure"));
                assert_eq!(
                    serde_json::from_slice::<Value>(&result.body).unwrap()["type"],
                    "error"
                );
                assert!(result.verified_models.is_empty());
                assert_single_pinned_request(&result, transport);
            }
            for bytes in [
                b"not json".to_vec(),
                b"{}".to_vec(),
                br#"{"type":"error","error":{"message":"synthetic failure"}}"#.to_vec(),
                br#"{"choices":[{"message":null,"finish_reason":"stop"}]}"#.to_vec(),
                br#"{"choices":[{"message":"invalid","finish_reason":"stop"}]}"#.to_vec(),
                br#"{"choices":[{"message":[],"finish_reason":"stop"}]}"#.to_vec(),
                br#"{"choices":[{"finish_reason":"stop"}]}"#.to_vec(),
            ] {
                let result = proxy_fixture(
                    transport,
                    StatusCode::OK,
                    "application/json",
                    bytes,
                    false,
                    true,
                )
                .await;
                assert_eq!(result.status, StatusCode::BAD_GATEWAY);
                assert!(result.verified_models.is_empty());
            }
        }
        let unauthorized = proxy_fixture(
            ClaudeTransport::DirectAnthropic,
            StatusCode::OK,
            "application/json",
            b"{}".to_vec(),
            false,
            false,
        )
        .await;
        assert_eq!(unauthorized.status, StatusCode::UNAUTHORIZED);
        assert!(unauthorized.requests.is_empty());
        assert!(unauthorized.verified_models.is_empty());
        for content in [
            json!(null),
            json!("text"),
            json!([{"type":"text","text":12}]),
        ] {
            let value = json!({"type":"message","role":"assistant","content":content,"stop_reason":"end_turn"});
            assert!(!complete_message(&value));
        }
        assert!(!complete_message(
            &json!({"type":"message","role":"assistant","content":[],"stop_reason":null})
        ));
    }

    #[tokio::test]
    async fn claude_proxy_keeps_streaming_and_rejects_sse_for_a_nonstream_request() {
        let bytes = b"event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"delta\":{\"text\":\"ok\"}}\n\nevent: message_stop\ndata: {\"type\":\"message_stop\"}\n\n".to_vec();
        let result = proxy_fixture(
            ClaudeTransport::DirectAnthropic,
            StatusCode::OK,
            "text/event-stream",
            bytes.clone(),
            true,
            true,
        )
        .await;
        assert_eq!(result.status, StatusCode::OK);
        assert_eq!(result.body, bytes);
        assert_eq!(result.verified_models, ["selected-model"]);
        assert_single_pinned_request(&result, ClaudeTransport::DirectAnthropic);
        let result = proxy_fixture(
            ClaudeTransport::DirectAnthropic,
            StatusCode::OK,
            "text/event-stream",
            bytes,
            false,
            true,
        )
        .await;
        assert_eq!(result.status, StatusCode::BAD_GATEWAY);
        assert!(result.verified_models.is_empty());
    }

    #[tokio::test]
    async fn ru042_claude_route_update_keeps_listener_and_inflight_snapshot() {
        let runtime = ClaudeBridgeRuntime::new("127.0.0.1:0", "/test");
        let mut credential = ToolCredential {
            api_key: "synthetic-test-key-only".into(),
            origin: "https://yeschoy.com".into(),
            model_id: "test-model".into(),
            local_gateway_token: Some(format!("ycg-{}", "b".repeat(64))),
            codex_transport: None,
            claude_transport: None,
            models: vec![],
        };
        runtime.start(credential.clone()).await.unwrap();
        let first = runtime.runtime.lock().await.as_ref().unwrap().task.id();
        runtime.start(credential.clone()).await.unwrap();
        assert_eq!(
            runtime.runtime.lock().await.as_ref().unwrap().task.id(),
            first
        );
        credential.model_id = "other-model".into();
        let shared = runtime.runtime.lock().await.as_ref().unwrap().state.clone();
        let inflight = shared.read().await.clone();
        runtime.start(credential).await.unwrap();
        assert_eq!(
            runtime.runtime.lock().await.as_ref().unwrap().task.id(),
            first
        );
        assert_eq!(inflight.credential.model_id, "test-model");
        assert_eq!(shared.read().await.credential.model_id, "other-model");
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

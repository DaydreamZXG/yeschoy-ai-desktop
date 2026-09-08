//! Tool-authenticated loopback Chat gateways. A request pins one keyring
//! snapshot; updating a model set never interrupts an admitted request.
use std::{collections::HashSet, sync::Arc, time::Duration};

use axum::{
    body::Body,
    extract::State,
    http::{header, Method, Request, Response, StatusCode},
    routing::any,
    Router,
};
use futures::StreamExt;
use serde_json::{json, Value};
use tokio::{
    net::TcpListener,
    sync::{oneshot, Mutex},
    task::JoinHandle,
};

use crate::{
    codex_bridge::secure_equal,
    loopback_http::{admit_ai_request, read_ai_request_body, AiRequestAdmissionError},
    request_diagnostics::{self, RequestOutcome},
    tool_adapters::AdapterFailure,
    tool_credentials::{self, CredentialFailure, ToolCredential},
};

const ADDRESS: &str = "127.0.0.1:15730";
const TOOLS: [&str; 4] = ["pi", "hermes", "openclaw", "dsh_web"];
const MAX_RESPONSE_BODY_BYTES: usize = 8 * 1024 * 1024;
type Loader = Arc<dyn Fn(&str) -> Result<ToolCredential, CredentialFailure> + Send + Sync>;

pub(crate) fn base_url(tool: &str) -> Option<String> {
    TOOLS
        .contains(&tool)
        .then(|| format!("http://{ADDRESS}/{tool}"))
}

// Shared adapter predicates concern only public model IDs, never credentials.
pub(crate) fn validate_catalog(default: &str, ids: &[String]) -> Result<(), AdapterFailure> {
    let unique: HashSet<_> = ids.iter().collect();
    if ids.is_empty()
        || ids.len() > 200
        || unique.len() != ids.len()
        || !ids.iter().any(|id| id == default)
        || ids
            .iter()
            .any(|id| id.is_empty() || id.chars().count() > 200 || id.chars().any(char::is_control))
    {
        return Err(AdapterFailure::ConfigurationFailed(
            "configuration_parse_failed",
        ));
    }
    Ok(())
}

pub(crate) fn catalog_matches(value: &Value, field: Option<&str>, ids: &[String]) -> bool {
    value.as_array().is_some_and(|rows| {
        let actual: Option<HashSet<&str>> = rows
            .iter()
            .map(|row| field.map_or(row, |key| &row[key]).as_str())
            .collect();
        rows.len() == ids.len()
            && actual.is_some_and(|actual| {
                actual.len() == ids.len() && ids.iter().all(|id| actual.contains(id.as_str()))
            })
    })
}

pub(crate) fn default_matches(
    actual: Option<&str>,
    default: &str,
    ids: &[String],
    strict: bool,
) -> bool {
    actual.is_some_and(|actual| {
        if strict {
            actual == default
        } else {
            ids.iter().any(|id| id == actual)
        }
    })
}

#[derive(Clone)]
struct GatewayState {
    client: reqwest::Client,
    load: Loader,
    #[cfg(test)]
    upstream_override: Option<String>,
}

fn upstream_client() -> Result<reqwest::Client, reqwest::Error> {
    reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(Duration::from_secs(15))
        .read_timeout(Duration::from_secs(600))
        .build()
}

struct Running {
    shutdown: Option<oneshot::Sender<()>>,
    task: JoinHandle<()>,
}

impl Drop for Running {
    fn drop(&mut self) {
        // A shared shutdown deadline may cancel stop() while it owns this
        // handle. Dropping it must cancel the listener, never detach it.
        self.task.abort();
    }
}

#[derive(Clone, Default)]
pub(crate) struct ChatGatewayRuntimeState {
    running: Arc<Mutex<Option<Running>>>,
}

impl ChatGatewayRuntimeState {
    pub(crate) async fn ensure_started(&self) -> Result<(), AdapterFailure> {
        let mut slot = self.running.lock().await;
        if slot
            .as_ref()
            .is_some_and(|running| !running.task.is_finished())
        {
            return Ok(());
        }
        let listener = TcpListener::bind(ADDRESS)
            .await
            .map_err(|_| AdapterFailure::LaunchFailed)?;
        let state = GatewayState {
            client: upstream_client().map_err(|_| AdapterFailure::LaunchFailed)?,
            load: Arc::new(tool_credentials::load),
            #[cfg(test)]
            upstream_override: None,
        };
        let (shutdown, stopped) = oneshot::channel();
        let task = tokio::spawn(async move {
            let _ = axum::serve(listener, router(state))
                .with_graceful_shutdown(async {
                    let _ = stopped.await;
                })
                .await;
        });
        *slot = Some(Running {
            shutdown: Some(shutdown),
            task,
        });
        Ok(())
    }

    pub(crate) async fn stop(&self) {
        if let Some(mut running) = self.running.lock().await.take() {
            if let Some(shutdown) = running.shutdown.take() {
                let _ = shutdown.send(());
            }
            if tokio::time::timeout(Duration::from_secs(2), &mut running.task)
                .await
                .is_err()
            {
                running.task.abort();
                let _ = (&mut running.task).await;
            }
        }
    }
}

pub(crate) async fn resume_if_configured(state: ChatGatewayRuntimeState) {
    let configured = tokio::task::spawn_blocking(|| {
        TOOLS
            .iter()
            .any(|tool| tool_credentials::load(tool).is_ok_and(|record| record.has_model_set()))
    })
    .await
    .unwrap_or(false);
    if configured {
        let _ = state.ensure_started().await;
    }
}

fn router(state: GatewayState) -> Router {
    Router::new().fallback(any(request)).with_state(state)
}

fn response(status: StatusCode, content_type: &'static str, body: Body) -> Response<Body> {
    Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, content_type)
        .body(body)
        .expect("static gateway response")
}

fn safe_error(status: StatusCode, outcome: RequestOutcome) -> Response<Body> {
    response(
        status,
        "application/json",
        Body::from(request_diagnostics::error_json(outcome).to_string()),
    )
}

fn request_tool(path: &str) -> Option<(&'static str, &str)> {
    let (tool, endpoint) = path.strip_prefix('/')?.split_once('/')?;
    let tool = TOOLS.iter().copied().find(|candidate| *candidate == tool)?;
    Some((tool, endpoint))
}

#[derive(Clone)]
struct Route {
    tool: &'static str,
    model: String,
    group: String,
    credential: ToolCredential,
}

impl Route {
    fn observe(&self, outcome: RequestOutcome, status: u16) {
        request_diagnostics::record(
            self.tool,
            &self.model,
            &self.group,
            &self.credential.origin,
            outcome,
            status,
        );
    }
}

fn select_route(
    tool: &'static str,
    snapshot: &ToolCredential,
    model: &str,
) -> Result<Route, CredentialFailure> {
    let group = snapshot
        .models
        .iter()
        .find(|route| route.model_id == model)
        .ok_or(CredentialFailure::Invalid)?
        .billing_group
        .clone();
    Ok(Route {
        tool,
        model: model.to_owned(),
        group,
        credential: snapshot.resolve_model(model)?,
    })
}

async fn request(State(state): State<GatewayState>, request: Request<Body>) -> Response<Body> {
    let Some((tool, endpoint)) = request_tool(request.uri().path()) else {
        return safe_error(StatusCode::NOT_FOUND, RequestOutcome::InvalidResponse);
    };
    let models = request.method() == Method::GET && endpoint == "v1/models";
    if !(models || request.method() == Method::POST && endpoint == "v1/chat/completions") {
        return safe_error(StatusCode::NOT_FOUND, RequestOutcome::InvalidResponse);
    }
    let load = state.load.clone();
    let snapshot = match tokio::task::spawn_blocking(move || load(tool)).await {
        Ok(Ok(snapshot)) if snapshot.has_model_set() => snapshot,
        _ => {
            return safe_error(
                StatusCode::SERVICE_UNAVAILABLE,
                RequestOutcome::NetworkError,
            )
        }
    };
    let token = request
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|h| h.to_str().ok())
        .and_then(|h| h.strip_prefix("Bearer "))
        .unwrap_or("");
    if token.is_empty() || !secure_equal(token, snapshot.client_token(tool)) {
        return safe_error(StatusCode::UNAUTHORIZED, RequestOutcome::UpstreamError);
    }
    if models {
        return response(StatusCode::OK, "application/json", Body::from(json!({
            "object":"list", "data":snapshot.model_ids().into_iter().map(|id| json!({"id":id,"object":"model","owned_by":"yeschoy"})).collect::<Vec<_>>()
        }).to_string()));
    }
    let local_context = request_diagnostics::RequestContext {
        tool: tool.into(),
        model: String::new(),
        group: String::new(),
        origin: snapshot.origin.clone(),
    };
    let _admission = match admit_ai_request(request.headers()).await {
        Ok(permit) => permit,
        Err(AiRequestAdmissionError::PayloadTooLarge) => {
            local_context.record(
                RequestOutcome::PayloadTooLarge,
                StatusCode::PAYLOAD_TOO_LARGE.as_u16(),
            );
            return safe_error(
                StatusCode::PAYLOAD_TOO_LARGE,
                RequestOutcome::PayloadTooLarge,
            );
        }
        Err(AiRequestAdmissionError::Busy) => {
            local_context.record(
                RequestOutcome::LocalBusy,
                StatusCode::TOO_MANY_REQUESTS.as_u16(),
            );
            return safe_error(StatusCode::TOO_MANY_REQUESTS, RequestOutcome::LocalBusy);
        }
    };
    let bytes = match read_ai_request_body(request.into_body()).await {
        Ok(bytes) => bytes,
        Err(_) => {
            local_context.record(
                RequestOutcome::PayloadTooLarge,
                StatusCode::PAYLOAD_TOO_LARGE.as_u16(),
            );
            return safe_error(
                StatusCode::PAYLOAD_TOO_LARGE,
                RequestOutcome::PayloadTooLarge,
            );
        }
    };
    let mut body: Value = match serde_json::from_slice(&bytes) {
        Ok(body) => body,
        Err(_) => return safe_error(StatusCode::BAD_REQUEST, RequestOutcome::InvalidResponse),
    };
    let route = match body
        .get("model")
        .and_then(Value::as_str)
        .and_then(|id| select_route(tool, &snapshot, id).ok())
    {
        Some(route) => route,
        None => {
            // Never echo arbitrary unregistered model strings into diagnostics.
            request_diagnostics::record(
                tool,
                "",
                "",
                &snapshot.origin,
                RequestOutcome::UnknownModel,
                0,
            );
            return safe_error(StatusCode::BAD_REQUEST, RequestOutcome::UnknownModel);
        }
    };
    if !matches!(
        route.credential.origin.as_str(),
        "https://yeschoy.com" | "https://api.yeschoy.com"
    ) {
        route.observe(RequestOutcome::InvalidResponse, 0);
        return safe_error(StatusCode::FORBIDDEN, RequestOutcome::InvalidResponse);
    }
    let origin = route.credential.origin.as_str();
    let bytes = if crate::tool_model_profile::normalize_chat_reasoning(&mut body) {
        match serde_json::to_vec(&body) {
            Ok(bytes) => bytes.into(),
            Err(_) => return safe_error(StatusCode::BAD_REQUEST, RequestOutcome::InvalidResponse),
        }
    } else {
        bytes
    };
    #[cfg(test)]
    let origin = state.upstream_override.as_deref().unwrap_or(origin);
    let upstream = match state
        .client
        .post(format!(
            "{}/v1/chat/completions",
            origin.trim_end_matches('/')
        ))
        .bearer_auth(&route.credential.api_key)
        .header(header::CONTENT_TYPE, "application/json")
        .body(bytes)
        .send()
        .await
    {
        Ok(upstream) => upstream,
        Err(error) => {
            let outcome = if error.is_timeout() {
                RequestOutcome::Timeout
            } else {
                RequestOutcome::NetworkError
            };
            route.observe(outcome, 0);
            return safe_error(StatusCode::BAD_GATEWAY, outcome);
        }
    };
    let status = upstream.status();
    if !status.is_success() {
        let outcome = request_diagnostics::outcome_for_status(status.as_u16());
        route.observe(outcome, status.as_u16());
        return safe_error(status, outcome);
    }
    if body.get("stream").and_then(Value::as_bool) == Some(true) {
        let context = request_diagnostics::RequestContext {
            tool: tool.into(),
            model: route.model,
            group: route.group,
            origin: route.credential.origin,
        };
        return response(
            status,
            "text/event-stream",
            Body::from_stream(request_diagnostics::observed_sse(
                upstream.bytes_stream(),
                request_diagnostics::StreamProtocol::Chat,
                context,
                status.as_u16(),
            )),
        );
    }
    let mut stream = upstream.bytes_stream();
    let mut bytes = Vec::new();
    while let Some(chunk) = stream.next().await {
        match chunk {
            Ok(chunk) if bytes.len().saturating_add(chunk.len()) <= MAX_RESPONSE_BODY_BYTES => {
                bytes.extend_from_slice(&chunk)
            }
            Ok(_) => {
                route.observe(RequestOutcome::InvalidResponse, status.as_u16());
                return safe_error(StatusCode::BAD_GATEWAY, RequestOutcome::InvalidResponse);
            }
            Err(error) => {
                let outcome = request_diagnostics::transport_outcome(&error);
                route.observe(outcome, status.as_u16());
                return safe_error(StatusCode::BAD_GATEWAY, outcome);
            }
        }
    }
    let value: Value = match serde_json::from_slice(&bytes) {
        Ok(value) if complete_response(&value) => value,
        _ => {
            route.observe(RequestOutcome::InvalidResponse, status.as_u16());
            return safe_error(StatusCode::BAD_GATEWAY, RequestOutcome::InvalidResponse);
        }
    };
    let _ = value; // Preserve successful content without rewriting model identity or replies.
    route.observe(RequestOutcome::Ok, status.as_u16());
    response(status, "application/json", Body::from(bytes))
}

fn complete_response(value: &Value) -> bool {
    value.get("error").is_none_or(Value::is_null)
        && value["choices"].as_array().is_some_and(|choices| {
            !choices.is_empty()
                && choices.iter().all(|choice| {
                    choice["message"].is_object()
                        && choice["message"]["role"].as_str() == Some("assistant")
                        && choice["finish_reason"]
                            .as_str()
                            .is_some_and(|reason| !reason.is_empty())
                })
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tool_credentials::ToolModelRoute;
    use axum::{extract::State, http::HeaderMap, routing::post, Json};
    use std::sync::{
        atomic::{AtomicBool, Ordering},
        Mutex as StdMutex,
    };
    use tokio::sync::Notify;

    struct Server {
        url: String,
        task: JoinHandle<()>,
    }
    impl Drop for Server {
        fn drop(&mut self) {
            self.task.abort();
        }
    }
    async fn serve(app: Router) -> Server {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let task = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        Server { url, task }
    }

    fn fixture() -> ToolCredential {
        ToolCredential {
            api_key: "synthetic-group-a-key".into(),
            origin: "https://yeschoy.com".into(),
            model_id: "model-a".into(),
            local_gateway_token: Some("synthetic-local-tool-token".into()),
            claude_transport: None,
            codex_transport: None,
            models: [
                ("model-a", "group-a", "synthetic-group-a-key"),
                ("model-b", "group-b", "synthetic-group-b-key"),
            ]
            .into_iter()
            .map(|(model, group, key)| ToolModelRoute {
                model_id: model.into(),
                billing_group: group.into(),
                api_key: key.into(),
                origin: "https://yeschoy.com".into(),
                claude_transport: None,
                codex_transport: None,
            })
            .collect(),
        }
    }

    fn state(store: Arc<StdMutex<ToolCredential>>, upstream: &Server) -> GatewayState {
        GatewayState {
            client: upstream_client().unwrap(),
            load: Arc::new(move |tool| {
                assert!(TOOLS.contains(&tool));
                Ok(store.lock().unwrap().clone())
            }),
            upstream_override: Some(upstream.url.clone()),
        }
    }

    fn completed(model: &str) -> Value {
        json!({"model":model,"choices":[{"index":0,"message":{"role":"assistant","content":"fixture reply"},"finish_reason":"stop"}]})
    }

    #[tokio::test]
    async fn every_chat_gateway_client_forwards_requests_above_the_legacy_limit() {
        let forwarded_sizes = Arc::new(StdMutex::new(Vec::new()));
        let captured = forwarded_sizes.clone();
        let upstream = serve(Router::new().route(
            "/v1/chat/completions",
            post(move |request: Request<Body>| {
                let captured = captured.clone();
                async move {
                    let bytes = crate::loopback_http::read_ai_request_body(request.into_body())
                        .await
                        .unwrap();
                    captured.lock().unwrap().push(bytes.len());
                    let body: Value = serde_json::from_slice(&bytes).unwrap();
                    Json(completed(body["model"].as_str().unwrap()))
                }
            }),
        ))
        .await;
        let gateway = serve(router(state(Arc::new(StdMutex::new(fixture())), &upstream))).await;
        let client = upstream_client().unwrap();
        let attachment = "a".repeat(8 * 1024 * 1024 + 1);
        for tool in TOOLS {
            let response = client
                .post(format!("{}/{tool}/v1/chat/completions", gateway.url))
                .bearer_auth("synthetic-local-tool-token")
                .json(&json!({
                    "model":"model-a",
                    "messages":[{"role":"user","content":attachment}]
                }))
                .send()
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::OK, "{tool}");
        }
        let sizes = forwarded_sizes.lock().unwrap();
        assert_eq!(sizes.len(), TOOLS.len());
        assert!(sizes.iter().all(|size| *size > 8 * 1024 * 1024));
    }

    #[tokio::test]
    async fn ru043_chat_gateway_forwards_effort_and_keeps_model_group_and_budget() {
        let calls = Arc::new(StdMutex::new(Vec::<(String, Value)>::new()));
        let capture = calls.clone();
        let upstream = serve(Router::new().route(
            "/v1/chat/completions",
            post(move |headers: HeaderMap, Json(body): Json<Value>| {
                let capture = capture.clone();
                async move {
                    capture.lock().unwrap().push((
                        headers[header::AUTHORIZATION].to_str().unwrap().to_owned(),
                        body.clone(),
                    ));
                    Json(completed(body["model"].as_str().unwrap()))
                }
            }),
        ))
        .await;
        let mut credentials = fixture();
        credentials.model_id = "gpt-6-astra".into();
        credentials.models[0].model_id = "gpt-6-astra".into();
        credentials.models[1].model_id = "deepseek-v4-flash".into();
        let gateway = serve(router(state(
            Arc::new(StdMutex::new(credentials)),
            &upstream,
        )))
        .await;
        let client = upstream_client().unwrap();
        for tool in TOOLS {
            for effort in ["low", "medium", "high", "xhigh", "max"] {
                let mut body = json!({"model":"gpt-6-astra","messages":[{"role":"user","content":"synthetic prompt unchanged"}],"max_tokens":4096,"future_field":{"keep":true}});
                if tool == "hermes" {
                    body["reasoning"] = json!({"enabled":true,"effort":effort});
                } else {
                    body["reasoning_effort"] = json!(effort);
                }
                let response = client
                    .post(format!("{}/{tool}/v1/chat/completions", gateway.url))
                    .bearer_auth("synthetic-local-tool-token")
                    .json(&body)
                    .send()
                    .await
                    .unwrap();
                assert_eq!(response.status(), StatusCode::OK);
                let calls = calls.lock().unwrap();
                let (key, sent) = calls.last().unwrap();
                assert_eq!(key, "Bearer synthetic-group-a-key");
                assert_eq!(sent["model"], "gpt-6-astra");
                assert_eq!(sent["reasoning_effort"], effort);
                assert_eq!(sent["messages"], body["messages"]);
                assert_eq!(sent["max_tokens"], 4096);
                assert_eq!(sent["future_field"], body["future_field"]);
            }
            let response = client
                .post(format!("{}/{tool}/v1/chat/completions", gateway.url))
                .bearer_auth("synthetic-local-tool-token")
                .json(&json!({"model":"deepseek-v4-flash","reasoning_effort":"none","messages":[]}))
                .send()
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::OK);
            let calls = calls.lock().unwrap();
            let (key, sent) = calls.last().unwrap();
            assert_eq!(key, "Bearer synthetic-group-b-key");
            assert_eq!(sent["thinking"]["type"], "disabled");
            assert!(sent.get("reasoning_effort").is_none());
        }
        let response = client.post(format!("{}/dsh_web/v1/chat/completions", gateway.url)).bearer_auth("synthetic-local-tool-token").json(&json!({"model":"deepseek-v4-flash","thinking":{"type":"disabled"},"reasoning_effort":"high","messages":[]})).send().await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let calls = calls.lock().unwrap();
        let (key, sent) = calls.last().unwrap();
        assert_eq!(key, "Bearer synthetic-group-b-key");
        assert_eq!(sent["thinking"]["type"], "disabled");
        assert!(sent.get("reasoning_effort").is_none());
    }

    #[tokio::test]
    async fn ru042_gateway_cancelled_stop_aborts_owned_listener_task() {
        struct Dropped(Arc<AtomicBool>);
        impl Drop for Dropped {
            fn drop(&mut self) {
                self.0.store(true, Ordering::SeqCst);
            }
        }
        let dropped = Arc::new(AtomicBool::new(false));
        let signal = dropped.clone();
        let entered = Arc::new(Notify::new());
        let signal_entered = entered.clone();
        let task = tokio::spawn(async move {
            let _dropped = Dropped(signal);
            signal_entered.notify_one();
            std::future::pending::<()>().await;
        });
        entered.notified().await;
        let state = ChatGatewayRuntimeState {
            running: Arc::new(Mutex::new(Some(Running {
                shutdown: None,
                task,
            }))),
        };
        assert!(
            tokio::time::timeout(Duration::from_millis(10), state.stop())
                .await
                .is_err()
        );
        tokio::time::timeout(Duration::from_secs(1), async {
            while !dropped.load(Ordering::SeqCst) {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        assert!(state.running.lock().await.is_none());
    }

    #[derive(Clone)]
    struct Capture {
        calls: Arc<StdMutex<Vec<(String, String)>>>,
        first_a: Arc<AtomicBool>,
        entered: Arc<Notify>,
        release: Arc<Notify>,
    }

    async fn captured(
        State(capture): State<Capture>,
        headers: HeaderMap,
        Json(body): Json<Value>,
    ) -> Json<Value> {
        let model = body["model"].as_str().unwrap().to_owned();
        let key = headers[header::AUTHORIZATION].to_str().unwrap().to_owned();
        capture.calls.lock().unwrap().push((model.clone(), key));
        if model == "model-a" && capture.first_a.swap(false, Ordering::SeqCst) {
            capture.entered.notify_one();
            capture.release.notified().await;
        }
        Json(completed(&model))
    }

    #[tokio::test]
    async fn ru042_chat_gateway_routes_two_groups_and_pins_inflight_snapshot() {
        let capture = Capture {
            calls: Arc::default(),
            first_a: Arc::new(AtomicBool::new(true)),
            entered: Arc::new(Notify::new()),
            release: Arc::new(Notify::new()),
        };
        let upstream = serve(
            Router::new()
                .route("/v1/chat/completions", post(captured))
                .with_state(capture.clone()),
        )
        .await;
        let store = Arc::new(StdMutex::new(fixture()));
        let gateway = serve(router(state(store.clone(), &upstream))).await;
        let client = upstream_client().unwrap();
        let first = client
            .post(format!("{}/pi/v1/chat/completions", gateway.url))
            .bearer_auth("synthetic-local-tool-token")
            .json(&json!({"model":"model-a","messages":[]}));
        let first =
            tokio::spawn(async move { first.send().await.unwrap().json::<Value>().await.unwrap() });
        tokio::time::timeout(Duration::from_secs(3), capture.entered.notified())
            .await
            .unwrap();
        {
            let mut next = store.lock().unwrap();
            next.models[0].api_key = "synthetic-group-c-key".into();
            next.models[0].billing_group = "group-c".into();
        }
        let second = client
            .post(format!("{}/pi/v1/chat/completions", gateway.url))
            .bearer_auth("synthetic-local-tool-token")
            .json(&json!({"model":"model-b","messages":[]}))
            .send()
            .await
            .unwrap();
        assert_eq!(second.status(), StatusCode::OK);
        assert_eq!(second.json::<Value>().await.unwrap()["model"], "model-b");
        capture.release.notify_one();
        assert_eq!(first.await.unwrap()["model"], "model-a");
        let third = client
            .post(format!("{}/pi/v1/chat/completions", gateway.url))
            .bearer_auth("synthetic-local-tool-token")
            .json(&json!({"model":"model-a","messages":[]}))
            .send()
            .await
            .unwrap();
        assert_eq!(third.status(), StatusCode::OK);
        assert_eq!(
            *capture.calls.lock().unwrap(),
            vec![
                ("model-a".into(), "Bearer synthetic-group-a-key".into()),
                ("model-b".into(), "Bearer synthetic-group-b-key".into()),
                ("model-a".into(), "Bearer synthetic-group-c-key".into()),
            ]
        );
        assert_eq!(
            select_route("pi", &fixture(), "model-b").unwrap().group,
            "group-b"
        );
    }

    #[tokio::test]
    async fn ru042_chat_gateway_rejects_unknown_models_wrong_tool_auth_and_redirects() {
        let calls = Arc::new(StdMutex::new(0usize));
        let upstream_calls = calls.clone();
        let upstream = serve(Router::new().route(
            "/v1/chat/completions",
            post(move || {
                let calls = upstream_calls.clone();
                async move {
                    *calls.lock().unwrap() += 1;
                    Response::builder()
                        .status(StatusCode::TEMPORARY_REDIRECT)
                        .header(header::LOCATION, "https://untrusted.invalid/")
                        .body(Body::from("synthetic-secret-in-upstream-error"))
                        .unwrap()
                }
            }),
        ))
        .await;
        let gateway = serve(router(state(Arc::new(StdMutex::new(fixture())), &upstream))).await;
        let client = upstream_client().unwrap();
        for body in [
            json!({"model":"not-enrolled"}),
            json!({}),
            json!({"model":null}),
        ] {
            let result = client
                .post(format!("{}/openclaw/v1/chat/completions", gateway.url))
                .bearer_auth("synthetic-local-tool-token")
                .json(&body)
                .send()
                .await
                .unwrap();
            assert_eq!(result.status(), StatusCode::BAD_REQUEST);
            assert!(!result.text().await.unwrap().contains("not-enrolled"));
        }
        let result = client
            .post(format!("{}/hermes/v1/chat/completions", gateway.url))
            .bearer_auth("wrong-tool-token")
            .json(&json!({"model":"model-a"}))
            .send()
            .await
            .unwrap();
        assert_eq!(result.status(), StatusCode::UNAUTHORIZED);
        assert_eq!(*calls.lock().unwrap(), 0);
        let result = client
            .post(format!("{}/openclaw/v1/chat/completions", gateway.url))
            .bearer_auth("synthetic-local-tool-token")
            .json(&json!({"model":"model-a"}))
            .send()
            .await
            .unwrap();
        assert_eq!(result.status(), StatusCode::TEMPORARY_REDIRECT);
        assert!(result.headers().get(header::LOCATION).is_none());
        assert!(!result.text().await.unwrap().contains("synthetic-secret"));
        assert_eq!(*calls.lock().unwrap(), 1);
        assert!(base_url("claude_code").is_none());
        for tool in TOOLS {
            assert!(base_url(tool).unwrap().ends_with(tool));
        }
    }

    #[tokio::test]
    async fn ru042_chat_gateway_stream_errors_and_incomplete_json_never_succeed() {
        for raw in [
            "data: {\"error\":{\"message\":\"synthetic-key private-error\"}}\n\ndata: [DONE]\n\n",
            "data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"partial\"},\"finish_reason\":null}]}\n\n",
        ] {
            let upstream = serve(Router::new().route("/v1/chat/completions", post(move || async move {
                response(StatusCode::OK, "text/event-stream", Body::from(raw))
            }))).await;
            let gateway = serve(router(state(Arc::new(StdMutex::new(fixture())), &upstream))).await;
            let result = upstream_client().unwrap().post(format!("{}/dsh_web/v1/chat/completions", gateway.url))
                .bearer_auth("synthetic-local-tool-token").json(&json!({"model":"model-a","stream":true})).send().await.unwrap();
            let output = result.text().await.unwrap();
            assert!(output.contains("event: error"));
            assert!(!output.contains("synthetic-key"));
            assert!(!output.contains("[DONE]"));
        }
        assert!(!complete_response(
            &json!({"choices":[{"message":{},"finish_reason":null}]})
        ));
        assert!(!complete_response(
            &json!({"error":{"message":"private"},"choices":[]})
        ));
    }

    #[test]
    fn ru042_catalog_predicates_reject_duplicate_unknown_and_invalid_defaults() {
        let ids = vec!["model-a".into(), "供应商/model-b".into()];
        assert!(validate_catalog("model-a", &ids).is_ok());
        assert!(validate_catalog("model-c", &ids).is_err());
        assert!(validate_catalog("model-a", &["model-a".into(), "model-a".into()]).is_err());
        assert!(default_matches(
            Some("供应商/model-b"),
            "model-a",
            &ids,
            false
        ));
        assert!(!default_matches(
            Some("供应商/model-b"),
            "model-a",
            &ids,
            true
        ));
        assert!(!default_matches(
            Some("unregistered"),
            "model-a",
            &ids,
            false
        ));
        assert!(catalog_matches(
            &json!([{"id":"供应商/model-b"},{"id":"model-a"}]),
            Some("id"),
            &ids
        ));
        assert!(!catalog_matches(
            &json!([{"id":"model-a"},{"id":"model-a"}]),
            Some("id"),
            &ids
        ));
    }
}

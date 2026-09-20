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

/// 请求路径写进日志时用的标签。
///
/// 返回 `&'static str` 是这个函数存在的理由：请求路径是外部可控的，绝不能原样
/// 落进日志，而借用不到请求里的字节，类型就替我们保证了这一点 —— 能返回的只有
/// 下面这张表里的常量。
///
/// 需要认得它们，是因为桥只服务 `/v1/models` 和 `/v1/messages`，其余一律 404。
/// 实测 Claude Desktop 会在一秒内打来 9 个我们不认识的 POST；不知道那是什么，
/// 就分不清 404 掉的是无关紧要的东西，还是它用来做决定的东西
/// —— `count_tokens` 就是后者：量不到对话多大，也就无从判断何时该压缩。
fn log_label(method: &str, path: &str) -> &'static str {
    // 顺序有讲究：`find` 取第一个前缀命中，所以 `/v1/messages/count_tokens`
    // 和 `/v1/messages/batches` 必须排在 `/v1/messages` 前面，否则最想认出的
    // 那个会被它自己的前缀吃掉。
    const RECOGNISED: [&str; 9] = [
        "/v1/messages/count_tokens",
        "/v1/messages/batches",
        "/v1/messages",
        "/v1/complete",
        "/v1/files",
        "/v1/models",
        "/v1/organizations",
        "/v1/skills",
        "/v1/agents",
    ];
    match (method, path) {
        ("GET", "/v1/models") => "/v1/models",
        ("POST", "/v1/messages") => "/v1/messages",
        // 要么完全相等，要么后面紧跟 `/`。纯前缀匹配会把
        // `/v1/messages<垃圾>` 标成 `/v1/messages` —— 安全性不受影响（记下的
        // 仍是我们自己的常量），但标签会骗人，而这张日志的全部用处就是看清
        // 对方到底在要什么。
        _ => RECOGNISED
            .iter()
            .find(|candidate| {
                path == **candidate
                    || path
                        .strip_prefix(**candidate)
                        .is_some_and(|rest| rest.starts_with('/'))
            })
            .copied()
            .unwrap_or("<other>"),
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
    // 记一行「谁来过」。
    //
    // 之前只有 `/v1/models` 有日志，于是它零行时分不清两件完全不同的事：
    // Claude Desktop 不做模型发现，还是它压根没走这座桥。前者说明在
    // `/v1/models` 上报上下文窗口这条路走不通，得另想办法；后者是个大得多的
    // 问题。没有这一行，两者都只能猜。
    //
    // 只记方法和**已知**路径。未知路径来自请求 URL，是外部可控的，不能原样
    // 写进日志；请求头和请求体一个字节都不记。
    log::info!(
        "claude_bridge request surface={} method={} path={}",
        if state.prefix.contains("desktop") {
            "desktop"
        } else {
            "code"
        },
        request.method().as_str(),
        log_label(request.method().as_str(), path),
    );
    match (request.method().as_str(), path) {
        ("GET", "/v1/models") => models(&state),
        ("POST", "/v1/messages") => messages(&state, request).await,
        // 查询串不在 `path` 里，所以 SDK 的 beta 变体
        // （`/v1/messages/count_tokens?beta=true`，另带
        // `anthropic-beta: token-counting-2024-11-01`）命中的也是这一条。
        ("POST", "/v1/messages/count_tokens") => count_tokens(&state, request).await,
        _ => error(StatusCode::NOT_FOUND, "unsupported Claude endpoint"),
    }
}

/// Claude Desktop 会用这个端点做模型发现。返回 Anthropic 形状的列表，
/// `id` 是客户端写入 profile 的 route 别名，显示名保留账号里的真实模型。
fn models(state: &BridgeState) -> Response {
    let models = state.credential.model_ids();
    let desktop = state.prefix.contains("desktop");
    // Claude Desktop 的 profile schema 里没有上下文窗口字段，所以窗口只能在这个
    // 端点上说 —— 前提是它真的来做模型发现。它的 schema 写着会
    // （"the first model your endpoint returns under discovery"），但没有人验证过，
    // 而验证不了的话，「桌面版会不会自动压缩」就永远只能是推测。
    //
    // 所以记一行。只记：面向哪个客户端、供了几个模型、其中几个带得出窗口。
    // 不记模型 ID，不记请求里的任何东西 —— 这行日志是用来回答「它来过吗」，
    // 不是用来看内容的。
    log::info!(
        "claude_bridge model_discovery surface={} models={} with_window={}",
        if desktop { "desktop" } else { "code" },
        models.len(),
        models
            .iter()
            .filter(|id| crate::tool_model_profile::capability_profile(id)
                .and_then(|profile| profile.context_window)
                .is_some())
            .count(),
    );
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
            let mut entry = json!({
                "id": route,
                "type": "model",
                "display_name": crate::tool_model_profile::display_name(model),
                "supports1m": crate::tool_model_profile::supports_one_m_context(model),
            });
            let object = entry.as_object_mut().expect("json! built an object");
            // 我们就是那个网关，客户端从这里发现模型，所以窗口该在这里说清楚。
            // 不说的话，客户端只能拿它自己那张内置表去查 —— 而我们铸的路由 ID
            // 不在任何人的内置表里，于是它没有可对照的数，也就不知道什么时候
            // 该压缩对话。
            //
            // 三个键的出处，都不是猜的：
            //   max_input_tokens   Models API 表示上下文窗口的标准字段
            //   max_tokens         Models API 表示输出上限的标准字段
            //   max_output_tokens  Claude Desktop 自己的内置目录用的拼法
            //                      （它的 runtime 条目就是
            //                       {max_input_tokens, max_output_tokens}）
            // 输出上限写两种拼法，是因为这两边确实不一样，而多一个键对读不懂
            // 它的客户端没有任何影响。
            if let Some(profile) = crate::tool_model_profile::capability_profile(model) {
                if let Some(context) = profile.context_window {
                    object.insert("max_input_tokens".into(), json!(context));
                }
                if let Some(output) = profile.max_output_tokens {
                    object.insert("max_tokens".into(), json!(output));
                    object.insert("max_output_tokens".into(), json!(output));
                }
            }
            // 目录里没有的模型什么都不加 —— 报一个错的窗口，比不报更糟。
            entry
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
        // Claude offers the 1M option from the capability family it was given,
        // which can be 1M-capable while the real model behind it is not.
        Err(ResolveFailure::OneMUnavailable) => return invalid_request_error(
            "1M context is unavailable for this model. Select the model without the [1m] option.",
        ),
        Err(ResolveFailure::StaleRoute) => {
            log::warn!("claude_bridge stage=stale_route_alias");
            return invalid_request_error(
                "This profile points at a model your 野菜API account no longer provides. \
                 Open 野菜API and apply the connection again to refresh it.",
            );
        }
    };
    value["model"] = json!(model);
    // `context-1m-2025-08-07` is an Anthropic beta. Forwarding it on a
    // GPT/DeepSeek/Kimi route is meaningless at best, and strict relays reject
    // an unknown beta outright. The marker still selects the 1M-capable model.
    if one_m_context && crate::tool_model_profile::is_native_claude(&model) {
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
    let upstream = match send_upstream(state, &upstream_credential, &forwarded, &value).await {
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
        // Only the OpenAI-shaped message carries authoritative counts. Every
        // other overflow wording — Anthropic's own, a relay's paraphrase, a
        // localized message — still has to reach Claude as an overflow, or the
        // client reads it as an outage and never compacts the thread.
        return if is_context_overflow(&original.body) {
            log::info!(
                "claude_bridge stage=context_overflow counts=unparsed one_m={one_m_context}"
            );
            context_too_long_error()
        } else {
            original.into_response()
        };
    };
    // The relay, not the vendor spec, decides the real ceiling. A declared-1M
    // model whose relay serves less shows up here, and is the reason a 1M
    // conversation can still hit a wall.
    if one_m_context && overflow.maximum < 1_000_000 {
        log::warn!(
            "claude_bridge stage=one_m_declared_above_relay relay_maximum={}",
            overflow.maximum
        );
    }

    // The relay is the authority on tokenization. If the messages still fit
    // and only the requested completion crosses the boundary, preserve every
    // message and retry once with exactly the reported remaining capacity.
    let requested_max_tokens = value.get("max_tokens").and_then(Value::as_u64);
    if overflow.messages < overflow.maximum && requested_max_tokens == Some(overflow.completion) {
        let available = overflow.maximum - overflow.messages;
        if available > 0 && available < overflow.completion {
            value["max_tokens"] = json!(available);
            let retry = match send_upstream(state, &upstream_credential, &forwarded, &value).await {
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

/// 量一段对话有多大。
///
/// Claude Desktop 每轮都打这个端点 —— 实测一次对话之后连打二十个，十八毫秒内
/// 打完。之前桥不认它，二十个全 404，于是 Desktop 量不到尺寸，也就无从判断
/// 何时该自动压缩。「Claude 没办法自动压缩」就是这么来的。
///
/// 转发给中转站是死路：不带凭据探过，它对这个路径和对一个随手编的假路径返回
/// 逐字相同的 `404 Invalid URL`。所以数是本地估的，见 [`crate::token_estimate`]。
///
/// **故意不解析模型。** 估算与模型无关，而一次对话要打二十个请求 —— 让它们
/// 因为「档位过期」之类的原因一起报错，风险远大于给个数。模型解析失败该在
/// [`messages`] 里大声说，那里已经说了。
async fn count_tokens(state: &BridgeState, request: Request<Body>) -> Response {
    if !locally_authorized(request.headers(), &state.local_token) {
        return error(StatusCode::UNAUTHORIZED, "invalid local gateway token");
    }
    let body = match to_bytes(request.into_body(), MAX_REQUEST_BODY_BYTES).await {
        Ok(bytes) => bytes,
        Err(_) => return error(StatusCode::PAYLOAD_TOO_LARGE, "request body too large"),
    };
    let value: Value = match serde_json::from_slice(&body) {
        Ok(value) => value,
        Err(_) => return error(StatusCode::BAD_REQUEST, "invalid request"),
    };
    let estimate = crate::token_estimate::estimate_request(&value);
    // 只记数字。`messages` 和 `tools` 是用来回答「这二十个请求到底在数什么」的，
    // `tokens` 是用来跟上游报回的权威计数对照、校准余量系数的。
    // 请求体一个字节都不记。
    log::info!(
        "claude_bridge count_tokens surface={} messages={} tools={} system={} tokens={}",
        if state.prefix.contains("desktop") {
            "desktop"
        } else {
            "code"
        },
        estimate.messages,
        estimate.tools,
        estimate.has_system,
        estimate.tokens,
    );
    (
        StatusCode::OK,
        [(header::CONTENT_TYPE, "application/json")],
        Body::from(json!({ "input_tokens": estimate.tokens }).to_string()),
    )
        .into_response()
}

async fn send_upstream(
    state: &BridgeState,
    credential: &ToolCredential,
    forwarded: &[(header::HeaderName, String)],
    value: &Value,
) -> Result<reqwest::Response, reqwest::Error> {
    let url = format!("{}/v1/messages", credential.origin.trim_end_matches('/'));
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

/// Recognize an upstream context-limit rejection whose wording carries no
/// usable counts. Claude only compacts when the failure arrives as an
/// `invalid_request_error`; an unrecognized body is reported to the user as a
/// provider outage, which is why a 1M declaration that the relay does not
/// honor currently ends the conversation instead of shortening it.
fn is_context_overflow(body: &[u8]) -> bool {
    let text = String::from_utf8_lossy(body).to_lowercase();
    // Anthropic's own wording, OpenAI's error code, and the paraphrases relays
    // put in front of both. Each marker names the context limit explicitly so a
    // transient upstream failure is never rewritten into a prompt-size error.
    [
        "prompt is too long",
        "maximum context length",
        "context_length_exceeded",
        "context length exceeded",
        "context window exceeded",
        "exceeds the context window",
        "reduce the length of the messages",
        "上下文过长",
        "上下文长度",
        "超出最大上下文",
        "超过最大上下文",
    ]
    .iter()
    .any(|marker| text.contains(marker))
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
/// Why a requested model could not be resolved. The two reasons need
/// different words: one is a bad choice the user can change in the picker,
/// the other is a stale profile that only re-applying the connection fixes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ResolveFailure {
    OneMUnavailable,
    StaleRoute,
}

fn resolve_model(state: &BridgeState, requested: &str) -> Result<(String, bool), ResolveFailure> {
    let (base, one_m_context) = crate::tool_model_profile::split_one_m_context_marker(requested);
    let model = state.credential.model_ids().into_iter().find(|id| {
        id == base || crate::tool_model_profile::claude_gateway_route_matches(id, requested)
    });
    match model {
        Some(model) => {
            if one_m_context && !crate::tool_model_profile::supports_one_m_context(&model) {
                Err(ResolveFailure::OneMUnavailable)
            } else {
                Ok((model, one_m_context))
            }
        }
        None if one_m_context => Err(ResolveFailure::OneMUnavailable),
        // An unknown plain id is forwarded: the relay adds models faster than
        // this client ships, and refusing one we simply have not heard of
        // would make every new model unusable until the next release.
        //
        // A route alias is the opposite case. We mint it ourselves so Claude
        // Desktop's profile can name a model without carrying the real id, and
        // it means nothing upstream — forwarding one is a guaranteed 400 that
        // reaches the user as "Gateway rejected model anthropic/claude-router-
        // …", naming a string they have never seen and cannot act on. It only
        // gets here when the profile still points at a model the current
        // credential no longer enrolls, which re-applying the connection fixes.
        None if crate::tool_model_profile::is_claude_gateway_route(requested) => {
            Err(ResolveFailure::StaleRoute)
        }
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

/// A rejected request, as opposed to a failing provider. Claude retries and
/// reports an `api_error` as an outage; a bad selection has to arrive as an
/// invalid request so the user is told to change it instead of waiting.
fn invalid_request_error(message: &'static str) -> Response {
    (
        StatusCode::BAD_REQUEST,
        axum::Json(
            json!({"type": "error", "error": {"type": "invalid_request_error", "message": message}}),
        ),
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
    /// 日志里绝不能出现请求里的字节。返回类型已经保证了这一点，这条测试守的是
    /// 它不被改成 `&str` —— 那样借用请求路径就会编译通过，而路径是外部可控的。
    #[test]
    fn an_unrecognised_path_never_reaches_the_log() {
        for hostile in [
            "/v1/../../etc/passwd",
            "/v1/messages\u{0000}injected",
            "/v1/x?token=sk-secret",
            "/完全不认识的路径",
            "",
        ] {
            assert_eq!(
                super::log_label("POST", hostile),
                "<other>",
                "{hostile:?} 不该被原样记下来"
            );
        }
    }

    /// `/v1/messages/count_tokens` 不能被 `/v1/messages` 的前缀先吃掉 —— 那正是
    /// 我们最想认出来的那一个，混进 messages 里就白记了。
    #[test]
    fn the_longer_path_wins_over_its_own_prefix() {
        assert_eq!(
            super::log_label("POST", "/v1/messages/count_tokens"),
            "/v1/messages/count_tokens"
        );
        assert_eq!(super::log_label("POST", "/v1/messages"), "/v1/messages");
        assert_eq!(super::log_label("GET", "/v1/models"), "/v1/models");
        // 方法不对就不算服务得了的那两个，但仍要认出路径。
        assert_eq!(super::log_label("GET", "/v1/messages"), "/v1/messages");
    }

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

    /// 路由这一层必须**单独**测，而且必须过 `dispatch`。
    ///
    /// 这个仓库刚犯过一次相反的错（`36607b6b`）：处理逻辑写对了，却因为外层分支
    /// 条件互斥而一次都没被调到 —— 当时的测试之所以没挡住，正是因为它绕过路由
    /// 直接调了处理函数。本文件里 `/v1/messages` 的那几条端到端测试同样是直接调
    /// `messages()` 的，所以它们证明不了「这条路由接得上」。这条能。
    #[tokio::test]
    async fn the_count_tokens_route_is_reachable_through_dispatch() {
        let shared = Arc::new(RwLock::new(Arc::new(state())));
        let token = shared.read().await.local_token.clone();
        for uri in [
            "/claude-desktop/v1/messages/count_tokens",
            // SDK 的 beta 变体带查询串。`uri().path()` 不含查询串，所以它命中的
            // 必须是同一条路由 —— 这正是「不必为 beta 单开一条」的那个前提。
            "/claude-desktop/v1/messages/count_tokens?beta=true",
        ] {
            let request = Request::builder()
                .method("POST")
                .uri(uri)
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .body(Body::from(
                    json!({
                        "model": "claude-sonnet-5",
                        "messages": [{"role": "user", "content": "你好，数一下这段有多长"}],
                    })
                    .to_string(),
                ))
                .unwrap();
            let response = dispatch(State(shared.clone()), request).await;
            assert_eq!(response.status(), StatusCode::OK, "{uri}");
            let body = to_bytes(response.into_body(), 64 * 1024).await.unwrap();
            let value: Value = serde_json::from_slice(&body).unwrap();
            // Anthropic 的响应就这一个字段，Desktop 读的也是它。
            assert!(
                value["input_tokens"]
                    .as_u64()
                    .is_some_and(|count| count > 0),
                "{uri}: {value}"
            );
        }
    }

    /// 新端点照样是本地边界的一部分。它收请求体，没有本地令牌就不该收。
    #[tokio::test]
    async fn count_tokens_still_demands_the_local_token() {
        let shared = Arc::new(RwLock::new(Arc::new(state())));
        let request = Request::builder()
            .method("POST")
            .uri("/claude-desktop/v1/messages/count_tokens")
            .body(Body::from(r#"{"messages":[]}"#))
            .unwrap();
        let response = dispatch(State(shared), request).await;
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
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
    fn context_regression_countless_overflow_wordings_still_reach_claude() {
        for body in [
            br#"{"error":{"message":"prompt is too long: 250000 tokens > 200000 maximum"}}"#
                .as_slice(),
            br#"{"error":{"code":"context_length_exceeded"}}"#.as_slice(),
            "{\"error\":{\"message\":\"上下文过长，请压缩会话后重试\"}}".as_bytes(),
        ] {
            assert_eq!(
                parse_context_overflow(body),
                None,
                "no authoritative counts to retry with"
            );
            assert!(
                is_context_overflow(body),
                "{}",
                String::from_utf8_lossy(body)
            );
        }
        // A transient failure must never be rewritten into a prompt-size error:
        // Claude would compact a thread that was never too long.
        for body in [
            br#"{"error":{"message":"host_call_failed"}}"#.as_slice(),
            br#"{"error":{"message":"rate limit exceeded"}}"#.as_slice(),
            br#"{"error":{"message":"upstream temporarily unavailable"}}"#.as_slice(),
        ] {
            assert!(
                !is_context_overflow(body),
                "{}",
                String::from_utf8_lossy(body)
            );
        }
    }

    #[test]
    fn context_regression_one_m_beta_is_reserved_for_anthropic_native_models() {
        for id in ["claude-sonnet-5", "anthropic/claude-opus-5"] {
            assert!(crate::tool_model_profile::is_native_claude(id), "{id}");
        }
        // These are 1M-capable, but the Anthropic beta header means nothing to
        // their upstreams and strict relays reject an unknown beta value.
        for id in ["gpt-6-astra", "gpt-5.6-sol", "deepseek-v4-pro"] {
            assert!(!crate::tool_model_profile::is_native_claude(id), "{id}");
            assert!(
                crate::tool_model_profile::supports_one_m_context(id),
                "{id}"
            );
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
            assert_eq!(
                resolve_model(&state, requested),
                Err(ResolveFailure::OneMUnavailable)
            );
        }
        // A similar Claude role, or an alias enrolled in another profile,
        // cannot silently select one of this profile's models.
        let unregistered = crate::tool_model_profile::claude_gateway_route_id("claude-fable-5");
        let requested = format!("{unregistered}[1m]");
        assert_eq!(
            resolve_model(&state, &requested),
            Err(ResolveFailure::OneMUnavailable)
        );
    }

    #[test]
    fn a_stale_route_alias_is_refused_here_rather_than_sent_upstream() {
        // Claude Desktop's profile names models by an alias we mint. When it
        // still holds one for a model the current credential no longer
        // enrolls, forwarding it produced a relay 400 that reached the user as
        // `Gateway rejected model anthropic/claude-router-a983fd…` — a string
        // they have never seen, about a model they did not pick, with nothing
        // to do about it.
        let state = state();
        // Both alias shapes have to be caught. `claude-fable-5` maps to a
        // capability family, so it mints `<family>-v<96 digits>`; a model with
        // no family mints the opaque `anthropic/claude-router-<64 hex>`, which
        // is the form that actually reached a user as a relay 400.
        let stale = crate::tool_model_profile::claude_gateway_route_id("claude-fable-5");
        assert!(
            stale.contains("-v"),
            "expected the family alias shape: {stale}"
        );
        assert!(crate::tool_model_profile::is_claude_gateway_route(&stale));
        let opaque = crate::tool_model_profile::legacy_claude_gateway_route_id("claude-fable-5");
        assert!(opaque.starts_with("anthropic/claude-router-"));
        assert!(crate::tool_model_profile::is_claude_gateway_route(&opaque));
        assert_eq!(
            resolve_model(&state, &opaque),
            Err(ResolveFailure::StaleRoute)
        );
        assert_eq!(
            resolve_model(&state, &stale),
            Err(ResolveFailure::StaleRoute)
        );
        // An unknown *plain* id still passes through: the relay adds models
        // faster than this client ships, and refusing those would make every
        // new model unusable until the next release.
        assert_eq!(
            resolve_model(&state, "gemini-3.7-flash-high"),
            Ok(("gemini-3.7-flash-high".into(), false))
        );
        assert!(!crate::tool_model_profile::is_claude_gateway_route(
            "gemini-3.7-flash-high"
        ));
    }

    #[tokio::test]
    async fn models_endpoint_reports_the_context_window_it_knows() {
        // 客户端从这个端点发现模型。不把窗口说出来，它就只能拿自己的内置表去
        // 查我们铸的路由 ID —— 查不到，于是不知道何时该压缩。
        let response = models(&state());
        let bytes = to_bytes(response.into_body(), 64 * 1024).await.unwrap();
        let value: Value = serde_json::from_slice(&bytes).unwrap();
        let entry = &value["data"][0];
        let window = entry["max_input_tokens"].as_u64().unwrap();
        assert!(window > 0);
        // 输出上限两种拼法都给：Models API 用 max_tokens，Claude Desktop 自己
        // 的内置目录用 max_output_tokens。
        assert_eq!(entry["max_tokens"], entry["max_output_tokens"]);
        assert!(entry["max_tokens"].as_u64().unwrap() > 0);
        // 报出去的必须是目录里那个数，不能是别处拍的。
        let model = state().credential.model_ids()[0].clone();
        assert_eq!(
            Some(window),
            crate::tool_model_profile::capability_profile(&model).and_then(|p| p.context_window)
        );
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
    async fn claude_code_proxy_strips_one_m_suffix_without_adding_beta_for_non_claude() {
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
        assert!(headers.get("anthropic-beta").is_none());
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
        assert!(
            received.try_recv().is_err(),
            "the bridge must retry only once"
        );
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
        assert!(
            received.try_recv().is_err(),
            "input overflow must not retry"
        );
        server.abort();
        let _ = server.await;
    }
}

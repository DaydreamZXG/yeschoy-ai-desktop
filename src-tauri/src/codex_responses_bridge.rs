//! Codex 的 Responses ↔ Chat 转换：**野菜的那一层**。
//!
//! Codex 只发 `/v1/responses`（CLI 0.155.1 起 `wire_api = "chat"` 已被移除），
//! 而有些渠道的上游只有 `/v1/chat/completions`。转换本身用 vendored 的
//! cc-switch（`crate::proxy`），**这里只放野菜特有的东西**。
//!
//! # 为什么是「包着调用」而不是改那些文件
//!
//! 上一次 vendoring 把这些逻辑打进了 `transform_codex_chat.rs`，于是每次同步
//! 上游都要重打一遍补丁。现在改成：上游文件逐字节原样，野菜的行为写在这里，
//! 调用它公开的入口。再同步就是 `cp`，见 `proxy/VENDOR.md`。

use axum::{
    body::{to_bytes, Body},
    extract::State,
    http::{header, Request, StatusCode},
    response::{IntoResponse, Response},
    routing::any,
    Router,
};
use bytes::Bytes;
use futures::{Stream, StreamExt};
use serde_json::{json, Value};

use crate::{
    local_bridge::{locally_authorized, BridgeState, LocalBridgeRuntime, SharedState},
    tool_credentials::ToolCredential,
};

use crate::proxy::{
    error::ProxyError,
    providers::{
        streaming_codex_chat::create_responses_sse_stream_from_chat_with_context,
        transform_codex_chat::{
            build_codex_tool_context_from_request, chat_completion_to_response_with_context,
            responses_to_chat_completions_with_reasoning, CodexToolContext,
        },
    },
};

/// 一次 Responses 请求转换的结果：发给中转的 Chat 请求体 + 回程要用的工具上下文。
///
/// 两者绑在一个值里是**有意的**。Codex 的工具名带命名空间，转成 Chat 时被拍平，
/// 回程必须按同一份对照还原成 Codex 认得的形状。上下文只能来自**这一次**的原始
/// 请求体 —— 拆成两个参数各传各的，迟早会有人漏传，或者把上一轮的传进来，
/// 而那种错误的症状是「工具偶尔调不动」，最难查的一类。
pub(crate) struct Converted {
    /// 发给中转的 Chat Completions 请求体。
    pub(crate) chat: Value,
    /// 客户端要的是不是流式 —— 决定回程走下面哪个方法。
    pub(crate) stream: bool,
    tool_context: CodexToolContext,
}

impl Converted {
    /// 非流式回程：Chat 响应 → Responses 响应。
    pub(crate) fn response_from_chat(&self, chat: Value) -> Result<Value, ProxyError> {
        chat_completion_to_response_with_context(chat, &self.tool_context)
    }

    /// 流式回程：Chat 的 SSE → Responses 的 SSE。
    ///
    /// 消费 `self`：上下文要交给流一直用到结束，借不出去。
    pub(crate) fn response_stream_from_chat<E>(
        self,
        stream: impl Stream<Item = Result<Bytes, E>> + Send + 'static,
    ) -> impl Stream<Item = Result<Bytes, std::io::Error>> + Send
    where
        E: std::error::Error + Send + 'static,
    {
        create_responses_sse_stream_from_chat_with_context(stream, self.tool_context)
    }
}

/// 把 Codex 的 Responses 请求转成中转能吃的 Chat 请求。
///
/// `model` 是**还原后的真实模型 id**，不是写进 Codex 配置里的那个名字：
/// 档位映射要按真实模型查目录，拿别名查不到。
///
/// 转换交给上游；野菜只加一件事 —— 把 `reasoning.effort` 按**这个模型实际
/// 支持的档位**映射过去（`apply_chat_effort`）。各家的「思考」参数并不统一：
/// DeepSeek 要 `thinking: {type}`，OpenAI 系要 `reasoning_effort`，
/// 而且支持的档位各不相同。原样透传等于把不支持的档位发给上游。
pub(crate) fn responses_to_chat(mut body: Value, model: &str) -> Result<Converted, ProxyError> {
    // 必须在转换**之前**从原始 Responses 体里抽，转换会把工具名拍平。
    let tool_context = build_codex_tool_context_from_request(&body);
    let stream = body.get("stream").and_then(Value::as_bool).unwrap_or(false);
    let effort = body
        .pointer("/reasoning/effort")
        .and_then(Value::as_str)
        .map(str::to_owned);
    // 别名不能发给中转：中转不认识我们铸的名字。
    body["model"] = Value::String(model.to_owned());
    let mut chat = responses_to_chat_completions_with_reasoning(body, None)?;
    if let Some(effort) = effort {
        crate::tool_model_profile::apply_chat_effort(&mut chat, model, &effort);
    }
    normalize_openai_reasoning_params(&mut chat, model);
    Ok(Converted {
        chat,
        stream,
        tool_context,
    })
}

/// OpenAI 推理模型在 Chat Completions 上的三条硬性差异。
///
/// 这段是从 `23c12bd3` 删掉的 `normalize_chat_parameters` 里搬回来的，
/// **但只搬了上游不管、且 `apply_chat_effort` 也管不到的那部分**：
///
/// | 旧函数做的 | 现在谁做 |
/// | --- | --- |
/// | `xhigh` → `high`（gpt-5 / 5.1）、gpt-5-pro 锁 `high` | `apply_chat_effort`，按目录里各模型实际声明的档位，比手抄家族名单准 |
/// | 下面这三条 | 这里 |
///
/// 上游只对 `o` 系做 `max_completion_tokens` 改名（`is_openai_o_series` 要求
/// `o` + 数字），gpt-5/6 系拿到的是 `max_tokens` —— OpenAI 会回
/// `Unsupported parameter`。上游碰不到这个是因为它那条路直连 OpenAI 官方、
/// Codex 本来就不发这些字段；我们这条路后面是中转，什么都可能发过来。
///
/// **这是防御性的**：按设计，原生说 Responses 的模型直连、不进桥
/// （真 `encrypted_content` 转一圈是降级）。但「哪些模型走桥」由中转的上游
/// 决定，不由模型名决定 —— 万一某个渠道拿 Chat-only 的上游供 gpt-5，
/// 桥就得把它发对。漏了这段，症状是一个很难查的 400。
fn normalize_openai_reasoning_params(body: &mut Value, model: &str) {
    let model = model.to_ascii_lowercase();
    let o_series = crate::proxy::providers::transform::is_openai_o_series(&model);
    let gpt5_plus = model.starts_with("gpt-5") || model.starts_with("gpt-6");
    if !o_series && !gpt5_plus {
        return;
    }
    let Some(object) = body.as_object_mut() else {
        return;
    };

    // 上游已经对 `o` 系改过名了；这里补 gpt-5/6 系。
    // `or_insert`：显式写了 max_completion_tokens 就以它为准。
    if let Some(limit) = object.remove("max_tokens") {
        object.entry("max_completion_tokens").or_insert(limit);
    }

    // 采样参数与推理模型互斥。`none` 是「关掉思考」，那时它们仍然合法。
    let reasoning_active = object
        .get("reasoning_effort")
        .and_then(Value::as_str)
        .is_some_and(|value| value != "none");
    if reasoning_active {
        for key in ["temperature", "top_p", "logprobs", "top_logprobs"] {
            object.remove(key);
        }
    }

    // `stop` 在 o 系上不被接受；上游把它放在 passthrough 列表里原样带过来。
    if o_series {
        object.remove("stop");
    }
}

/// Codex 单次请求的上限。超过时拒绝，而不是截断。
const MAX_REQUEST_BODY_BYTES: usize = 32 * 1024 * 1024;
/// 非流式响应要整体读进来才能转换。流式的不经过这里。
const MAX_RESPONSE_BODY_BYTES: usize = 64 * 1024 * 1024;

/// 起一座 Codex 桥。骨架、起停、换凭据都在 `local_bridge`。
pub(crate) fn codex_runtime(address: &'static str, prefix: &'static str) -> LocalBridgeRuntime {
    LocalBridgeRuntime::new(address, prefix, router)
}

fn router(state: SharedState) -> Router {
    Router::new().fallback(any(dispatch)).with_state(state)
}

/// 请求路径写进日志时用的标签。
///
/// 返回 `&'static str` 是这个函数存在的理由，和 Claude 桥那边同一个道理：
/// 请求路径是外部可控的，绝不能原样落进日志，而借用不到请求里的字节，
/// 类型就替我们保证了这一点 —— 能返回的只有下面这张表里的常量。
fn log_label(method: &str, path: &str) -> &'static str {
    match (method, path) {
        ("POST", "/v1/responses") => "/v1/responses",
        ("GET", "/v1/models") => "/v1/models",
        _ => "<other>",
    }
}

async fn dispatch(State(state): State<SharedState>, request: Request<Body>) -> Response {
    let state = state.read().await.clone();
    let Some(path) = request.uri().path().strip_prefix(state.prefix) else {
        return error(StatusCode::NOT_FOUND, "unsupported Codex endpoint");
    };
    // 只记方法和**已知**路径；请求头和请求体一个字节都不记。
    // Claude 桥那边这行日志的价值已经验证过一次：`count_tokens` 被 404 掉
    // 这件事，除了它没有别的办法发现。
    log::info!(
        "codex_bridge request method={} path={}",
        request.method().as_str(),
        log_label(request.method().as_str(), path),
    );
    match (request.method().as_str(), path) {
        ("POST", "/v1/responses") => responses(&state, request).await,
        _ => error(StatusCode::NOT_FOUND, "unsupported Codex endpoint"),
    }
}

/// 这个模型要不要在本地转协议。
///
/// 信号来自凭据里**逐模型**的 `codex_transport`，接入时探测的结果落在那儿。
/// **默认是「不转」** —— 桥先表现得和今天的直连一模一样，转换按模型逐个打开。
/// 反过来（默认转、按模型关）会让接进桥这一步本身就可能弄坏现在能用的模型。
///
/// 原生说 Responses 的模型走这条分支原样透传，**一个字节都不碰**：它们带的是
/// 真的 `encrypted_content`，转成 Chat 再转回来等于拿我们伪造的令牌换掉真货。
pub(crate) fn needs_chat_conversion(credential: &ToolCredential, model: &str) -> bool {
    let transport = credential
        .models
        .iter()
        .find(|route| route.model_id == model)
        .and_then(|route| route.codex_transport.as_deref())
        .or(credential.codex_transport.as_deref());
    // 拿枚举问，不写字面量：这个字符串是接入流程写进凭据的
    // （`CodexTransport::credential_value`），两边各写一份迟早对不上，
    // 而对不上的症状是「探测明明判了走桥，桥却一直直连」—— 静默的。
    transport
        == Some(crate::tool_adapters::codex_desktop::CodexTransport::ChatBridge.credential_value())
}

/// 这座桥的门禁：本地令牌，**或**我们写进 Codex 配置的那把受限中转 key。
///
/// 为什么比 Claude 桥松一格：Claude Desktop 的 profile 里我们写的是本地令牌，
/// 中转 key 从不离开应用；而 Codex 的 `config.toml` 里写的一直是
/// `experimental_bearer_token = <受限中转 key>`。让桥认这把 key，现有用户的
/// 配置就不用动 —— 变的只有 `base_url`，不用换令牌、不用改凭据助手的契约、
/// 不用为已接入的用户写迁移。
///
/// 安全上不是让步：本机进程要是已经拿到了这把 key，它本来就能直接打中转，
/// 在环回口上认它并不多给任何权限。真正的门禁是**这把 key 本身是受限的**
/// —— 每个工具一把、可单独吊销。
fn codex_authorized(headers: &axum::http::HeaderMap, state: &BridgeState) -> bool {
    if locally_authorized(headers, &state.local_token) {
        return true;
    }
    let presented = headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "));
    let key = state.credential.upstream_key();
    presented.is_some_and(|value| !key.is_empty() && crate::codex_bridge::secure_equal(value, key))
}

async fn responses(state: &BridgeState, request: Request<Body>) -> Response {
    if !codex_authorized(request.headers(), state) {
        return error(StatusCode::UNAUTHORIZED, "local token required");
    }
    let accept_sse = request
        .headers()
        .get(header::ACCEPT)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.contains("text/event-stream"));
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
    let credential = match state.credential.resolve_model(&model) {
        Ok(credential) => credential,
        // 没有模型列表的旧凭据：默认路由就是历史行为。
        Err(_) if !state.credential.has_model_set() => state.credential.clone(),
        // 有列表、但要的模型不在里面。
        //
        // 这里原本回落到整份凭据，也就是**默认模型的 key**，理由是「中转认得的
        // 模型名由中转说了算，解析不出来就让中转自己回答」。那个理由对的是
        // 「这个模型存不存在」，而这里的问题不是存不存在 —— 每把 key 都按
        // `model_limits` 锁在自己那个计费分组的模型上，所以拿默认的 key 去问，
        // 换来的**必然**是中转的 403「This token has no access to model X」。
        // 那句话技术上正确，用户拿它什么也做不了：他既不知道是分组的事，
        // 也不知道该回哪儿改。既然结果是确定的，本地说清楚就是了。
        Err(_) => {
            log::warn!("codex_bridge stage=model_outside_key_scope");
            return error_owned(
                StatusCode::BAD_REQUEST,
                format!(
                    "{model} is not in the model list this app was connected with, and \
                     each key only covers the models in its own billing group. Open \
                     野菜API, add {model} to the list, and apply the connection again."
                ),
            );
        }
    };

    if !needs_chat_conversion(&credential, &model) {
        return forward_verbatim(state, &credential, bytes, accept_sse).await;
    }

    let converted = match responses_to_chat(body, &model) {
        Ok(converted) => converted,
        Err(failure) => return failure.into_response(),
    };
    let stream = converted.stream;
    let url = format!(
        "{}/v1/chat/completions",
        credential.origin.trim_end_matches('/')
    );
    let upstream = state
        .client
        .post(url)
        .bearer_auth(credential.upstream_key())
        .json(&converted.chat)
        .send()
        .await;
    let Ok(upstream) = upstream else {
        return error(StatusCode::BAD_GATEWAY, "upstream request failed");
    };
    let status = upstream.status();
    if !status.is_success() {
        // 中转的原话原样转达。它比我们更清楚为什么拒绝，替它改写只会让
        // 用户拿到一句更模糊的话。
        return passthrough_error(upstream).await;
    }
    let _ = state.events.send(crate::local_bridge::VerificationEvent);
    if stream {
        let chunks = upstream
            .bytes_stream()
            .map(|chunk| chunk.map_err(std::io::Error::other));
        let events = converted.response_stream_from_chat(chunks);
        return Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, "text/event-stream")
            .body(Body::from_stream(events))
            .unwrap_or_else(|_| error(StatusCode::INTERNAL_SERVER_ERROR, "response failed"));
    }
    let Ok(bytes) = to_bytes_from(upstream).await else {
        return error(StatusCode::BAD_GATEWAY, "upstream response too large");
    };
    let Ok(chat) = serde_json::from_slice::<Value>(&bytes) else {
        return error(StatusCode::BAD_GATEWAY, "upstream response was not JSON");
    };
    match converted.response_from_chat(chat) {
        Ok(value) => (StatusCode::OK, axum::Json(value)).into_response(),
        Err(failure) => failure.into_response(),
    }
}

/// 不需要转换的模型：请求体原样发给中转的 `/v1/responses`，响应原样回来。
///
/// 原样是重点 —— 反序列化再序列化一遍会重排字段、丢掉未知字段，而
/// `encrypted_content` 这类东西正是我们不认识也不该动的。
async fn forward_verbatim(
    state: &BridgeState,
    credential: &ToolCredential,
    body: Bytes,
    accept_sse: bool,
) -> Response {
    let url = format!("{}/v1/responses", credential.origin.trim_end_matches('/'));
    let upstream = state
        .client
        .post(url)
        .bearer_auth(credential.upstream_key())
        .header(header::CONTENT_TYPE, "application/json")
        .header(
            header::ACCEPT,
            if accept_sse {
                "text/event-stream"
            } else {
                "application/json"
            },
        )
        .body(body)
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
    let chunks = upstream
        .bytes_stream()
        .map(|chunk| chunk.map_err(std::io::Error::other));
    Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, content_type)
        .body(Body::from_stream(chunks))
        .unwrap_or_else(|_| error(StatusCode::INTERNAL_SERVER_ERROR, "response failed"))
}

async fn to_bytes_from(upstream: reqwest::Response) -> Result<Bytes, ()> {
    let mut chunks = upstream.bytes_stream();
    let mut collected = Vec::new();
    while let Some(chunk) = chunks.next().await {
        let chunk = chunk.map_err(|_| ())?;
        if collected.len() + chunk.len() > MAX_RESPONSE_BODY_BYTES {
            return Err(());
        }
        collected.extend_from_slice(&chunk);
    }
    Ok(Bytes::from(collected))
}

async fn passthrough_error(upstream: reqwest::Response) -> Response {
    let status =
        StatusCode::from_u16(upstream.status().as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
    match to_bytes_from(upstream).await {
        Ok(bytes) => Response::builder()
            .status(status)
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(bytes))
            .unwrap_or_else(|_| error(StatusCode::INTERNAL_SERVER_ERROR, "response failed")),
        Err(()) => error(status, "upstream error"),
    }
}

/// OpenAI 形状的错误体 —— Codex 认的是这个，不是 Anthropic 那个形状。
///
/// `message` 是 `&'static str`：这条路上的错误文案只能来自代码里的常量，
/// 借不到请求里的字节。
/// 同 `error`，但消息里要带模型名，所以收 `String`。
fn error_owned(status: StatusCode, message: String) -> Response {
    (
        status,
        axum::Json(json!({"error": {"message": message, "type": "yeschoy_bridge_error"}})),
    )
        .into_response()
}

fn error(status: StatusCode, message: &'static str) -> Response {
    (
        status,
        axum::Json(json!({"error": {"message": message, "type": "yeschoy_bridge_error"}})),
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        local_bridge::{upstream_client, VerificationEvent},
        tool_credentials::ToolModelRoute,
    };
    use axum::http::HeaderMap;
    use serde_json::json;
    use std::{sync::Arc, time::Duration};
    use tokio::{
        net::TcpListener,
        sync::{broadcast, RwLock},
    };

    /// `chat` 那个模型走桥转换，`direct` 那个原样透传 —— 两条分支各有一个夹具。
    fn state() -> BridgeState {
        let route = |model: &str, transport: Option<&str>| ToolModelRoute {
            model_id: model.into(),
            billing_group: "group-a".into(),
            api_key: "sk-synthetic-a".into(),
            origin: "https://yeschoy.com".into(),
            claude_transport: None,
            codex_transport: transport.map(str::to_owned),
        };
        let local_token = format!("ycg-{}", "a".repeat(64));
        BridgeState {
            credential: ToolCredential {
                api_key: "sk-synthetic-a".into(),
                origin: "https://yeschoy.com".into(),
                model_id: "kimi-k3".into(),
                local_gateway_token: Some(local_token.clone()),
                codex_transport: None,
                claude_transport: None,
                models: vec![
                    route("kimi-k3", Some("chat_bridge")),
                    route("gpt-5.6-sol", Some("direct_responses")),
                    route("glm-5.3", None),
                ],
            },
            local_token,
            prefix: "/codex",
            client: upstream_client().unwrap(),
            events: broadcast::channel(4).0,
        }
    }

    fn convert(model: &str, effort: &str, extra: Value) -> Value {
        let mut body = json!({
            "model": "irrelevant-alias",
            "input": "fixture",
            "reasoning": {"effort": effort},
        });
        if let (Some(target), Some(source)) = (body.as_object_mut(), extra.as_object()) {
            for (key, value) in source {
                target.insert(key.clone(), value.clone());
            }
        }
        responses_to_chat(body, model).expect("转换应当成功").chat
    }

    /// 从被删掉的本地补丁里搬回来的回归测试
    /// （原 `ru043_responses_reasoning_reaches_chat_without_silent_downgrade`）。
    ///
    /// 它守的是：Responses 的 `reasoning.effort` 到了 Chat 这边不能被静默降级
    /// 或丢掉。以前这条测试住在上游文件里，所以每次同步都要重打补丁；
    /// 现在住在野菜自己的文件里 —— **行为一样，维护成本没了**。
    #[test]
    fn responses_reasoning_reaches_chat_without_silent_downgrade() {
        // 五档全支持的模型：原样透传，并且 max_output_tokens 换成 Chat 的拼法。
        for effort in ["low", "medium", "high", "xhigh", "max"] {
            let converted = convert(
                "gpt-6-astra",
                effort,
                json!({"max_output_tokens": 300, "temperature": 0.7}),
            );
            assert_eq!(converted["reasoning_effort"], effort, "effort={effort}");
            assert_eq!(converted["model"], "gpt-6-astra");
            // 上游只给 `o` 系改名，gpt-6 要靠我们补这一手。
            assert_eq!(converted["max_completion_tokens"], 300, "effort={effort}");
            assert!(converted.get("max_tokens").is_none(), "effort={effort}");
            // 采样参数与推理模型互斥，发过去就是 400。
            assert!(converted.get("temperature").is_none(), "effort={effort}");
        }

        // DeepSeek 只有 off/low/high/max，且要额外的 `thinking` 开关。
        // medium 与 xhigh 都得落到 high —— 发原值上游不认。
        for (effort, expected) in [
            ("low", "low"),
            ("medium", "high"),
            ("high", "high"),
            ("xhigh", "high"),
            ("max", "max"),
        ] {
            let converted = convert(
                "deepseek-v4-pro",
                effort,
                json!({"max_output_tokens": 300, "temperature": 0.7}),
            );
            assert_eq!(converted["reasoning_effort"], expected, "effort={effort}");
            assert_eq!(converted["thinking"]["type"], "enabled");
            // 非 OpenAI 家族不碰：DeepSeek 要的就是 max_tokens，
            // 改成 max_completion_tokens 反而会把本来能用的模型发坏。
            assert_eq!(converted["max_tokens"], 300, "effort={effort}");
            assert_eq!(converted["temperature"], 0.7, "effort={effort}");
        }

        // 关掉思考是显式状态，不是「选最弱的档」。
        let off = convert("deepseek-v4-flash", "none", json!({}));
        assert_eq!(off["thinking"]["type"], "disabled");
        assert!(off.get("reasoning_effort").is_none());
    }

    /// 发给中转的必须是真实模型 id，不是我们铸给 Codex 的名字。
    ///
    /// 拿别名去发，中转会回 `model_not_found`；拿别名去查档位，
    /// 目录里也查不到，于是档位映射会静默失效。
    #[test]
    fn the_alias_never_reaches_the_relay() {
        let converted = convert("deepseek-v4-pro", "high", json!({}));
        assert_eq!(converted["model"], "deepseek-v4-pro");
        assert!(!converted.to_string().contains("irrelevant-alias"));
    }

    /// 没带 `reasoning` 的请求不该被我们凭空加上档位。
    #[test]
    fn a_request_without_reasoning_gains_no_effort() {
        let converted = responses_to_chat(
            json!({"model": "alias", "input": "fixture"}),
            "deepseek-v4-pro",
        )
        .expect("转换应当成功")
        .chat;
        assert!(converted.get("reasoning_effort").is_none());
        assert!(converted.get("thinking").is_none());
    }

    /// 这个文件里最值钱的一条：**工具名往返**。
    ///
    /// Codex 的命名空间工具在转 Chat 时被拍成一个扁平名
    /// （`mcp__codex_apps__gmail___search_emails`），回程必须拆回
    /// `namespace` + `name`。拆得对不对，取决于回程用的是不是**这一次**请求
    /// 抽出来的上下文 —— `Converted` 把两者绑在一起就是为了这个。
    ///
    /// 上游自己也测这条转换；我们这条测的是**我们的接线**：
    /// `responses_to_chat` 有没有在拍平之前把上下文抽出来并带到回程。
    /// 接线断了的症状是「工具偶尔调不动」，日志里什么都看不出来。
    #[test]
    fn a_namespaced_codex_tool_survives_the_round_trip() {
        let converted = responses_to_chat(
            json!({
                "model": "irrelevant-alias",
                "tools": [{"type": "tool_search"}],
                "input": [{
                    "type": "tool_search_output",
                    "call_id": "call_tool_search_1",
                    "status": "completed",
                    "execution": "client",
                    "tools": [{
                        "type": "namespace",
                        "name": "mcp__codex_apps__gmail",
                        "description": "Find and reference emails from your inbox.",
                        "tools": [{
                            "type": "function",
                            "name": "_search_emails",
                            "description": "Search Gmail for emails matching a query.",
                            "parameters": {"type": "object", "properties": {}}
                        }]
                    }]
                }]
            }),
            "kimi-k3",
        )
        .expect("转换应当成功");

        let restored = converted
            .response_from_chat(json!({
                "id": "chatcmpl_gmail",
                "object": "chat.completion",
                "created": 123,
                "model": "kimi-k3",
                "choices": [{
                    "message": {
                        "role": "assistant",
                        "tool_calls": [{
                            "id": "call_gmail",
                            "type": "function",
                            "function": {
                                "name": "mcp__codex_apps__gmail___search_emails",
                                "arguments": "{}"
                            }
                        }]
                    },
                    "finish_reason": "tool_calls"
                }]
            }))
            .expect("回程应当成功");

        assert_eq!(restored["output"][0]["type"], "function_call");
        assert_eq!(restored["output"][0]["call_id"], "call_gmail");
        assert_eq!(restored["output"][0]["namespace"], "mcp__codex_apps__gmail");
        assert_eq!(restored["output"][0]["name"], "_search_emails");
    }

    /// `stream` 要如实带过去 —— 回程走非流式还是流式由它决定。
    /// 判错的后果是整条响应形状不对，而不是少个字段。
    #[test]
    fn the_stream_flag_is_carried_from_the_request() {
        let ask = |body: Value| {
            responses_to_chat(body, "kimi-k3")
                .expect("转换应当成功")
                .stream
        };
        assert!(ask(json!({"model": "a", "input": "x", "stream": true})));
        assert!(!ask(json!({"model": "a", "input": "x", "stream": false})));
        // Codex 总是显式写 stream，但没写时按非流式处理才是安全的默认：
        // 把非流式响应当 SSE 发，客户端会一直等到超时。
        assert!(!ask(json!({"model": "a", "input": "x"})));
    }

    /// 转不转由**逐模型**的 `codex_transport` 决定，而且**默认不转**。
    ///
    /// 默认这个方向是有意选的：桥接进来的第一天要和今天的直连一模一样，
    /// 转换按模型逐个打开。反过来（默认转、按模型关）会让「接进桥」这一步
    /// 本身就可能弄坏现在能用的模型。
    #[test]
    fn conversion_is_opt_in_per_model() {
        let state = state();
        assert!(needs_chat_conversion(&state.credential, "kimi-k3"));
        assert!(!needs_chat_conversion(&state.credential, "gpt-5.6-sol"));
        // 没标过的模型：不转。
        assert!(!needs_chat_conversion(&state.credential, "glm-5.3"));
        // 账号里有、目录和路由表里都还没有的模型：也不转。
        assert!(!needs_chat_conversion(&state.credential, "brand-new-model"));
    }

    /// 日志里绝不能出现请求里的字节。返回类型已经保证了这一点，这条测试守的是
    /// 它不被改成 `&str` —— 那样借用请求路径就会编译通过，而路径是外部可控的。
    #[test]
    fn an_unrecognised_path_never_reaches_the_log() {
        for hostile in [
            "/v1/../../etc/passwd",
            "/v1/responses\u{0000}injected",
            "/v1/x?token=sk-secret",
            "/v1/responses/extra",
        ] {
            assert_eq!(log_label("POST", hostile), "<other>", "{hostile}");
        }
        assert_eq!(log_label("POST", "/v1/responses"), "/v1/responses");
    }

    fn signed(uri: &str, token: &str, body: Value) -> Request<Body> {
        Request::builder()
            .method("POST")
            .uri(uri)
            .header(header::AUTHORIZATION, format!("Bearer {token}"))
            .body(Body::from(body.to_string()))
            .unwrap()
    }

    /// 桥监听在 `127.0.0.1` 上，本机任何进程都能连 —— 门禁必须长在**路由上**，
    /// 不是长在某个处理函数里。所以这条走 `dispatch`。
    #[tokio::test]
    async fn the_route_refuses_anything_without_a_known_token() {
        let shared = Arc::new(RwLock::new(Arc::new(state())));
        let body = json!({"model": "kimi-k3", "input": "probe"});
        for token in ["", "sk-wrong", "ycg-not-the-one"] {
            let request = signed("/codex/v1/responses", token, body.clone());
            let response = dispatch(State(shared.clone()), request).await;
            assert_eq!(
                response.status(),
                StatusCode::UNAUTHORIZED,
                "token={token:?}"
            );
        }
        // 前缀不对的一律 404，不能泄漏「这个路径存在」。
        for uri in ["/v1/responses", "/codexx/v1/responses", "/codex/v1/chat"] {
            let token = shared.read().await.local_token.clone();
            let response = dispatch(State(shared.clone()), signed(uri, &token, body.clone())).await;
            assert_eq!(response.status(), StatusCode::NOT_FOUND, "{uri}");
        }
    }

    /// 两把令牌都收：本地令牌，以及我们写进 Codex 配置的那把受限中转 key。
    /// 后者是「现有用户零迁移」的全部依据 —— 它要是不收，所有已接入的 Codex
    /// 在升级后会一起 401。
    #[test]
    fn both_the_local_token_and_the_scoped_relay_key_open_the_door() {
        let state = state();
        let bearer = |value: &str| {
            let mut headers = HeaderMap::new();
            headers.insert(
                header::AUTHORIZATION,
                format!("Bearer {value}").parse().unwrap(),
            );
            headers
        };
        assert!(codex_authorized(&bearer(&state.local_token), &state));
        assert!(codex_authorized(&bearer("sk-synthetic-a"), &state));
        assert!(!codex_authorized(&bearer("sk-synthetic-b"), &state));
        assert!(!codex_authorized(&HeaderMap::new(), &state));
    }

    /// 起一个桩上游，记下它收到的路径与**原始请求字节**。
    ///
    /// 收 `Bytes` 而不是 `Json<Value>` 是有意的：透传那条路要守的是
    /// 「一个字节都没动」，而桩要是自己先解析一遍，把原样转发换成
    /// 「解析再序列化」这种改动它就看不出来了。
    async fn stub_upstream(
        content_type: &'static str,
        reply: &'static str,
    ) -> (
        String,
        tokio::sync::mpsc::UnboundedReceiver<(String, Bytes)>,
    ) {
        stub_upstream_with_status(StatusCode::OK, content_type, reply).await
    }

    async fn stub_upstream_with_status(
        status: StatusCode,
        content_type: &'static str,
        reply: &'static str,
    ) -> (
        String,
        tokio::sync::mpsc::UnboundedReceiver<(String, Bytes)>,
    ) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let (sent, received) = tokio::sync::mpsc::unbounded_channel();
        let record = move |path: &'static str| {
            let sent = sent.clone();
            move |body: Bytes| {
                let sent = sent.clone();
                async move {
                    sent.send((path.to_owned(), body)).unwrap();
                    (status, [(header::CONTENT_TYPE, content_type)], reply)
                }
            }
        };
        let router = Router::new()
            .route(
                "/v1/chat/completions",
                axum::routing::post(record("/v1/chat/completions")),
            )
            .route(
                "/v1/responses",
                axum::routing::post(record("/v1/responses")),
            );
        tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
        (origin, received)
    }

    fn pointed_at(origin: &str) -> BridgeState {
        let mut state = state();
        state.credential.origin = origin.to_owned();
        for route in &mut state.credential.models {
            route.origin = origin.to_owned();
        }
        state.client = reqwest::Client::builder()
            .no_proxy()
            .timeout(Duration::from_secs(5))
            .build()
            .unwrap();
        state
    }

    /// 只记 Authorization 的桩 —— 这两条测的是「发出去的是谁的 key」。
    async fn stub_recording_authorization() -> (String, tokio::sync::mpsc::UnboundedReceiver<String>)
    {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let (sent, received) = tokio::sync::mpsc::unbounded_channel();
        // 两条路各回各自形状的响应，好让转换分支和透传分支都能走到底 ——
        // 只回一种的话，走桥那个模型会在转换时 422，测不到它的 key。
        let reply = move |body: &'static str| {
            let sent = sent.clone();
            move |headers: HeaderMap| {
                let sent = sent.clone();
                async move {
                    sent.send(
                        headers
                            .get(header::AUTHORIZATION)
                            .and_then(|v| v.to_str().ok())
                            .unwrap_or_default()
                            .to_owned(),
                    )
                    .unwrap();
                    (
                        StatusCode::OK,
                        [(header::CONTENT_TYPE, "application/json")],
                        body,
                    )
                }
            }
        };
        let router = Router::new()
            .route(
                "/v1/responses",
                axum::routing::post(reply(
                    r#"{"id":"resp_1","object":"response","status":"completed","output":[]}"#,
                )),
            )
            .route(
                "/v1/chat/completions",
                axum::routing::post(reply(
                    r#"{"id":"chatcmpl-1","object":"chat.completion","created":1,"model":"kimi-k3","choices":[{"message":{"role":"assistant","content":"ok"},"finish_reason":"stop"}]}"#,
                )),
            );
        tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
        (origin, received)
    }

    /// 每个模型必须带**它自己计费分组**的 key 出去。
    ///
    /// 中转给每把 key 设了 `model_limits`，锁死在该分组的模型上。用户在应用里
    /// 换到别的分组的模型时若还发默认那把，中转必然回
    /// 403 `This token has no access to model …` —— 线上就是这么炸的
    /// （0.4.23 的 Codex 直连中转，配置里只放得下一个 bearer token，
    /// 没有这座桥来按模型换 key）。
    #[tokio::test]
    async fn each_model_goes_upstream_with_its_own_billing_groups_key() {
        let (origin, mut seen) = stub_recording_authorization().await;
        let mut state = pointed_at(&origin);
        // 第二个模型换到另一个分组、另一把 key —— 线上的常用模型列表就是这样。
        state.credential.models[1].billing_group = "group-b".into();
        state.credential.models[1].api_key = "sk-synthetic-b".into();
        let token = state.local_token.clone();
        let shared = Arc::new(RwLock::new(Arc::new(state)));

        for (model, expected) in [
            ("kimi-k3", "Bearer sk-synthetic-a"),
            ("gpt-5.6-sol", "Bearer sk-synthetic-b"),
        ] {
            let response = dispatch(
                State(shared.clone()),
                signed(
                    "/codex/v1/responses",
                    &token,
                    json!({"model": model, "input": "fixture"}),
                ),
            )
            .await;
            assert_eq!(response.status(), StatusCode::OK);
            assert_eq!(seen.recv().await.unwrap(), expected, "model {model}");
        }
    }

    /// 要的模型不在列表里：本地说清楚，而不是拿默认那把 key 去换一个
    /// 一定会失败、且用户看不懂的 403。
    #[tokio::test]
    async fn a_model_outside_the_key_scope_is_refused_here_with_something_to_act_on() {
        let (origin, mut seen) = stub_recording_authorization().await;
        let state = pointed_at(&origin);
        let token = state.local_token.clone();
        let shared = Arc::new(RwLock::new(Arc::new(state)));

        let response = dispatch(
            State(shared),
            signed(
                "/codex/v1/responses",
                &token,
                json!({"model": "deepseek-v4.1-flash", "input": "fixture"}),
            ),
        )
        .await;

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let body = to_bytes(response.into_body(), MAX_REQUEST_BODY_BYTES)
            .await
            .unwrap();
        let text = String::from_utf8_lossy(&body);
        // 点名是哪个模型，并说清回哪儿改 —— 中转那句「This token has no access」
        // 技术上正确，但用户拿它什么也做不了。
        assert!(text.contains("deepseek-v4.1-flash"), "{text}");
        assert!(text.contains("野菜API"), "{text}");
        // 而且根本没往上游发 —— 那一趟的结果是确定的。
        assert!(seen.try_recv().is_err());
    }

    /// 标了 `chat_bridge` 的模型：请求去 `/v1/chat/completions`，
    /// 回来的 Chat 响应被转回 Responses 形状。整条路过 `dispatch`。
    #[tokio::test]
    async fn a_chat_bridge_model_is_converted_in_both_directions() {
        let (origin, mut received) = stub_upstream(
            "application/json",
            r#"{"id":"chatcmpl-1","object":"chat.completion","created":1,"model":"kimi-k3","choices":[{"message":{"role":"assistant","content":"你好"},"finish_reason":"stop"}]}"#,
        )
        .await;
        let state = pointed_at(&origin);
        let token = state.local_token.clone();
        let mut events = state.events.subscribe();
        let shared = Arc::new(RwLock::new(Arc::new(state)));

        let response = dispatch(
            State(shared.clone()),
            signed(
                "/codex/v1/responses",
                &token,
                json!({"model": "kimi-k3", "input": "你好", "reasoning": {"effort": "high"}}),
            ),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);

        let (path, sent) = received.recv().await.unwrap();
        let sent: Value = serde_json::from_slice(&sent).unwrap();
        assert_eq!(path, "/v1/chat/completions");
        // 发出去的是 Chat 形状，模型是真实 id。
        assert_eq!(sent["model"], "kimi-k3");
        assert!(sent.get("messages").is_some(), "{sent}");
        assert!(sent.get("input").is_none(), "{sent}");

        // 回来的是 Responses 形状 —— Codex 只认这个。
        let body = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
        let value: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(value["object"], "response");
        assert_eq!(value["output"][0]["type"], "message");

        // 成功一次就该报「首次使用已验证」。
        assert!(matches!(events.try_recv(), Ok(VerificationEvent)));
    }

    /// 没标的模型：原样发到 `/v1/responses`，请求体**逐字节**不变。
    ///
    /// 「逐字节」是重点，不是修辞。原生说 Responses 的模型带的是**真的**
    /// `encrypted_content`，转成 Chat 再转回来等于拿我们伪造的令牌换掉真货；
    /// 连反序列化再序列化一遍都不行 —— 那会重排字段、丢掉我们不认识的字段，
    /// 而「我们不认识」正是这条路上最该保留的东西。所以这里比的是字节。
    #[tokio::test]
    async fn a_direct_model_is_forwarded_byte_for_byte() {
        let (origin, mut received) = stub_upstream(
            "application/json",
            r#"{"object":"response","output":[],"status":"completed"}"#,
        )
        .await;
        let state = pointed_at(&origin);
        let token = state.local_token.clone();
        let shared = Arc::new(RwLock::new(Arc::new(state)));

        // 手写的原始文本，**故意不规范**：键序不是字典序、缩进不统一。
        // 用 `json!` 造就验不出问题了 —— serde 的 Map 默认按键排序，
        // 「解析再序列化」的结果会和 `json!` 的输出一模一样，测试照样绿。
        // 我第一版就是这么写的，变异测试当场证明它什么都没守住。
        let sent_body = concat!(
            "{\"model\":\"gpt-5.6-sol\",  \"an_unknown_field_we_must_not_drop\":{\"nested\":[1,2,3]},",
            "\"input\":[{\"type\":\"reasoning\",\"encrypted_content\":\"gAAAAAB-opaque-do-not-touch\",",
            "\"summary\":[]}],\"reasoning\":{\"effort\":\"xhigh\"}}"
        );
        let response = dispatch(
            State(shared.clone()),
            Request::builder()
                .method("POST")
                .uri("/codex/v1/responses")
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .body(Body::from(sent_body))
                .unwrap(),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);

        let (path, arrived) = received.recv().await.unwrap();
        assert_eq!(path, "/v1/responses");
        assert_eq!(std::str::from_utf8(&arrived).unwrap(), sent_body);
    }
    /// Codex 真实会话里的 Chat SSE：先吐正文，再吐一个带命名空间的工具调用。
    ///
    /// 用真形状而不是最小形状，是因为增量拼接（工具名在一个 chunk、参数在
    /// 下一个）正是这类转换器最容易出错的地方。
    const CHAT_SSE: &str = concat!(
        r#"data: {"id":"chatcmpl_gmail","model":"kimi-k3","choices":[{"delta":{"content":"查一下"}}]}"#,
        "\n\n",
        r#"data: {"id":"chatcmpl_gmail","model":"kimi-k3","choices":[{"delta":{"tool_calls":[{"index":0,"id":"call_gmail","type":"function","function":{"name":"mcp__codex_apps__gmail___search_emails"}}]}}]}"#,
        "\n\n",
        r#"data: {"id":"chatcmpl_gmail","model":"kimi-k3","choices":[{"delta":{"tool_calls":[{"index":0,"function":{"arguments":"{\"query\":\"in:inbox\"}"}}]},"finish_reason":"tool_calls"}]}"#,
        "\n\n",
        "data: [DONE]\n\n",
    );

    /// 流式整条路过 `dispatch`。
    ///
    /// 上游自己测过 SSE 转换本身；这条测的是**我们的接线**，四件非流式那条
    /// 测不到的事：
    ///
    /// 1. 路由真的选了流式分支（依据是请求里的 `stream`，判错整条响应形状就错）；
    /// 2. `stream: true` 有转发给中转 —— 不转发的话中转回的是一个 JSON，
    ///    而我们在等 SSE，客户端就一直挂到超时；
    /// 3. 回给 Codex 的 `Content-Type` 是 `text/event-stream`；
    /// 4. 工具上下文到达了**流式**那条消费路径 —— 它和非流式那条是两个入口，
    ///    断一个另一个照样绿。
    #[tokio::test]
    async fn a_streamed_chat_response_becomes_responses_events_through_dispatch() {
        let (origin, mut received) = stub_upstream("text/event-stream", CHAT_SSE).await;
        let state = pointed_at(&origin);
        let token = state.local_token.clone();
        let shared = Arc::new(RwLock::new(Arc::new(state)));

        let response = dispatch(
            State(shared.clone()),
            signed(
                "/codex/v1/responses",
                &token,
                json!({
                    "model": "kimi-k3",
                    "stream": true,
                    "tools": [{"type": "tool_search"}],
                    "input": [{
                        "type": "tool_search_output",
                        "call_id": "call_tool_search_1",
                        "tools": [{
                            "type": "namespace",
                            "name": "mcp__codex_apps__gmail",
                            "tools": [{
                                "type": "function",
                                "name": "_search_emails",
                                "description": "Search Gmail.",
                                "parameters": {"type": "object"}
                            }]
                        }]
                    }]
                }),
            ),
        )
        .await;

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response
                .headers()
                .get(header::CONTENT_TYPE)
                .and_then(|value| value.to_str().ok()),
            Some("text/event-stream"),
        );

        let (path, sent) = received.recv().await.unwrap();
        assert_eq!(path, "/v1/chat/completions");
        let sent: Value = serde_json::from_slice(&sent).unwrap();
        // 不转发 stream，中转回的就是一个 JSON，而我们在等 SSE —— 客户端挂死。
        assert_eq!(sent["stream"], true, "{sent}");

        let body = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
        let text = std::str::from_utf8(&body).unwrap();
        let events: Vec<Value> = text
            .split("\n\n")
            .filter_map(|block| block.lines().find_map(|line| line.strip_prefix("data: ")))
            .filter_map(|data| serde_json::from_str(data).ok())
            .collect();
        let kinds: Vec<&str> = events
            .iter()
            .filter_map(|event| event["type"].as_str())
            .collect();

        // Codex 靠这条链拿到工具调用；缺一环它就不会执行工具。
        for expected in [
            "response.created",
            "response.output_text.delta",
            "response.function_call_arguments.delta",
            "response.function_call_arguments.done",
            "response.output_item.done",
            "response.completed",
        ] {
            assert!(kinds.contains(&expected), "缺 {expected}：{kinds:?}");
        }

        // 工具名在流里也要拆回 namespace + name。拍平的名字发回去 Codex 不认。
        let call = events
            .iter()
            .filter_map(|event| event.get("item"))
            .find(|item| item["type"] == "function_call")
            .unwrap_or_else(|| panic!("流里没有 function_call：{text}"));
        assert_eq!(call["call_id"], "call_gmail");
        assert_eq!(call["namespace"], "mcp__codex_apps__gmail");
        assert_eq!(call["name"], "_search_emails");
    }

    /// 中转拒绝时，把它的原话原样交给 Codex —— 状态码和响应体都不改写。
    ///
    /// 中转比我们清楚它为什么拒绝（`分组 default 下无可用渠道`、
    /// `unknown provider for model gi/…` 这类）。替它换成一句我们自己的话，
    /// 用户和排查的人就都少了唯一那条线索。
    #[tokio::test]
    async fn the_relays_own_refusal_reaches_codex_unchanged() {
        const REFUSAL: &str =
            r#"{"error":{"message":"分组 default 下无可用渠道","type":"new_api_error"}}"#;
        let (origin, _received) =
            stub_upstream_with_status(StatusCode::BAD_REQUEST, "application/json", REFUSAL).await;
        let state = pointed_at(&origin);
        let token = state.local_token.clone();
        let shared = Arc::new(RwLock::new(Arc::new(state)));

        let response = dispatch(
            State(shared.clone()),
            signed(
                "/codex/v1/responses",
                &token,
                json!({"model": "kimi-k3", "input": "probe"}),
            ),
        )
        .await;

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let body = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
        assert_eq!(std::str::from_utf8(&body).unwrap(), REFUSAL);
    }

    /// # 真机验收夹具（第一层）：真 codex CLI → 真桥 → 桩中转
    ///
    /// 默认不跑。`cargo test --lib -- --ignored real_codex_cli` 手动触发，
    /// 需要机器上装着 `codex`，且端口 15731 空闲（关掉野菜API）。
    ///
    /// **为什么必须有这一条**：上面所有断言都是拿我们自己挑的字符串去比我们
    /// 自己的输出 —— 真客户端解析不了的话，它们一条都不会红。这条用真的
    /// codex 0.155.1 解析我们吐的 SSE，并且能看到 **Codex 第二轮回传
    /// reasoning / `encrypted_content` 时我们的桥转不转得动** ——
    /// 那是整个设计里最大的未知，二进制里该符号出现 38 次。
    ///
    /// **绝不碰用户的 `~/.codex`**：给 CLI 单独的 `CODEX_HOME`（它只读那里的
    /// `config.toml`）外加 `--ephemeral`（不落会话文件）；写配置前还断言
    /// `config_dir` 落在临时目录内（它读的是登录 shell 的 `CODEX_HOME`，
    /// 那是个随时会变的环境事实，不能让测试依赖它）。
    #[tokio::test]
    #[ignore = "需要本机安装 codex CLI 且端口 15731 空闲；跑法见函数文档"]
    async fn real_codex_cli_drives_the_bridge_end_to_end() {
        use crate::tool_adapters::{codex_desktop, common};
        use std::path::PathBuf;

        // ---- 前置守卫：不满足就带明确信息失败，不静默跳过 ----
        let codex = std::env::var("YESCHOY_CODEX_BIN")
            .ok()
            .map(PathBuf::from)
            .or_else(|| {
                std::env::var_os("PATH").and_then(|paths| {
                    std::env::split_paths(&paths)
                        .map(|dir| dir.join("codex"))
                        .find(|path| path.is_file())
                })
            })
            .expect("找不到 codex 可执行文件；装上它，或用 YESCHOY_CODEX_BIN 指路");
        assert!(
            TcpListener::bind(codex_desktop::PROXY_ADDRESS)
                .await
                .is_ok(),
            "{} 被占用了。桥的地址是常量，夹具换不了端口 —— 关掉野菜API 再跑。",
            codex_desktop::PROXY_ADDRESS
        );

        let home = common::temporary_working_directory("codex-real-device").unwrap();
        let codex_home = codex_desktop::config_dir(&home).expect("解析 CODEX_HOME 失败");
        // 登录 shell 里设了 CODEX_HOME 的话，写配置会落到用户真实目录里去。
        assert!(
            codex_home.starts_with(&home),
            "CODEX_HOME 把配置目录指到了临时目录之外（{}）—— \
         再跑下去就会覆盖用户真实的 Codex 配置。先 unset CODEX_HOME。",
            codex_home.display()
        );

        // ---- 桩中转：读请求再决定回什么 ----
        let (origin, seen) = codex_stub_relay().await;

        // ---- 配置用我们自己的写入函数造，不手写 TOML ----
        let key = format!("sk-{}", "d".repeat(48));
        let model = "kimi-k3";
        let mut prepared = codex_desktop::prepare_catalog_with_provider_hint(
            &home,
            &origin,
            model,
            codex_desktop::CodexTransport::DirectResponses,
            Some(&key),
            &[model.to_owned()],
            None,
        )
        .expect("生成 Codex 配置失败");
        prepared.commit().expect("写入 Codex 配置失败");

        // ---- 起真桥，让这个模型走转换那条路 ----
        let local_token = format!("ycg-{}", "e".repeat(64));
        let credential = ToolCredential {
            api_key: key.clone(),
            origin: origin.clone(),
            model_id: model.into(),
            local_gateway_token: Some(local_token),
            codex_transport: Some("chat_bridge".into()),
            claude_transport: None,
            models: vec![ToolModelRoute {
                model_id: model.into(),
                billing_group: "cheap".into(),
                api_key: key.clone(),
                origin: origin.clone(),
                claude_transport: None,
                codex_transport: Some("chat_bridge".into()),
            }],
        };
        let runtime = codex_runtime(codex_desktop::PROXY_ADDRESS, "/codex");
        runtime.start(credential).await.expect("桥没起来");

        // ---- 跑真 CLI ----
        // 输出重定向到文件而不是管道：超时被杀掉时管道里的东西就拿不到了，
        // 而那正是最需要看的 —— Codex 自己会说它卡在哪。
        let log_path = home.join("codex-exec.log");
        let log = std::fs::File::create(&log_path).unwrap();
        let output = tokio::time::timeout(
            Duration::from_secs(120),
            tokio::process::Command::new(&codex)
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::from(log.try_clone().unwrap()))
                .stderr(std::process::Stdio::from(log))
                .args([
                    "exec",
                    // **不要**加 `--ignore-user-config`：它的意思是「不加载
                    // $CODEX_HOME/config.toml」，会把我们刚写的那份一起忽略，
                    // Codex 于是退回默认 provider，请求根本不会到桥这儿来。
                    // 隔离靠的是下面那个 `CODEX_HOME` —— 指到临时目录，
                    // 用户的 ~/.codex/config.toml 自然读不到也写不着。
                    "--ephemeral",
                    "--skip-git-repo-check",
                    "-s",
                    "read-only",
                    "-C",
                ])
                .arg(&home)
                .args([
                    "-m",
                    model,
                    "请运行 echo yeschoy-bridge-ok，然后告诉我结果。",
                ])
                .env("CODEX_HOME", &codex_home)
                .kill_on_drop(true)
                // `.status()` 而不是 `.output()`：后者会把 stdout/stderr
                // 强制换成管道，把上面那两行文件重定向直接覆盖掉，
                // 于是超时时什么都读不到（踩过一次）。
                .status(),
        )
        .await;

        let requests = seen.lock().unwrap().clone();
        let transcript = home.join("bridge-transcript.txt");
        let _ = std::fs::write(
            &transcript,
            requests
                .iter()
                .enumerate()
                .map(|(index, (path, body))| {
                    format!(
                        "=== #{index} {path} ===\n{}\n",
                        String::from_utf8_lossy(body)
                    )
                })
                .collect::<String>(),
        );
        let stdout = std::fs::read_to_string(&log_path).unwrap_or_default();
        let tail: String = {
            let chars: Vec<char> = stdout.chars().collect();
            chars[chars.len().saturating_sub(1800)..].iter().collect()
        };
        let context = format!(
            "桩收到 {} 个请求；请求全文 {}；codex 输出 {}\n--- codex 说（末尾）---\n{tail}",
            requests.len(),
            transcript.display(),
            log_path.display(),
        );

        let status = output
            .unwrap_or_else(|_| panic!("codex 超时没退出。{context}"))
            .expect("启动 codex 失败");
        assert!(
            status.success(),
            "codex 退出码 {:?}。{context}",
            status.code()
        );

        // ---- 断言 ----
        assert!(
            requests.len() >= 2,
            "只看到 {} 个请求，第二轮没发生。{context}",
            requests.len()
        );
        for (path, body) in &requests {
            assert_eq!(path, "/v1/chat/completions", "{context}");
            let value: Value = serde_json::from_slice(body).expect("请求不是 JSON");
            assert!(value.get("messages").is_some(), "不是 Chat 形状：{value}");
            assert!(
                value.get("input").is_none(),
                "Responses 的字段漏过去了：{value}"
            );
            assert_eq!(value["stream"], true, "{context}");
            assert_eq!(value["model"], model, "{context}");
        }

        let first: Value = serde_json::from_slice(&requests[0].1).unwrap();
        assert!(
            first["tools"]
                .as_array()
                .is_some_and(|tools| !tools.is_empty()),
            "第一轮没带工具定义，请求侧转换把它们丢了：{first}"
        );

        let second: Value = serde_json::from_slice(&requests[1].1).unwrap();
        let has_tool_result = second["messages"]
            .as_array()
            .is_some_and(|messages| messages.iter().any(|m| m["role"] == "tool"));
        assert!(
            has_tool_result,
            "第二轮里没有工具结果 —— Codex 没执行我们转出的工具调用。{context}"
        );

        assert!(
            stdout.contains(ECHO_MARKER),
            "最终文本没到用户眼前。{context}\n--- stdout ---\n{stdout}"
        );

        // 跑过的人要能看到到底发生了什么，而不只是一个绿点。
        println!(
            "真机验收通过：{} 个请求过桥；第一轮 {} 条消息 / {} 个工具；\
             第二轮带回了工具结果（即 Codex 解析了我们转出的事件并执行了工具，\
             它回传的那一轮我们也转换掉了）。",
            requests.len(),
            first["messages"].as_array().map(Vec::len).unwrap_or(0),
            first["tools"].as_array().map(Vec::len).unwrap_or(0),
        );

        runtime.stop().await;
        let _ = std::fs::remove_dir_all(&home);
    }

    /// 桩中转：记下每个请求的原始字节，并**按请求内容**决定回什么。
    ///
    /// 读请求而不是写死回复，是因为 Codex 发什么工具由它自己决定。写死一个
    /// 名字就是在赌一个我们没验证过的假设，而赌输的表现是「工具没被调用」，
    /// 看起来像我们的桥坏了。
    type SeenRequests = Arc<std::sync::Mutex<Vec<(String, Bytes)>>>;

    /// 桩让 Codex 去执行的命令，以及要在最终输出里找到的记号。
    const ECHO_MARKER: &str = "yeschoy-bridge-ok";
    const ECHO_MARKER_COMMAND: &str = "echo yeschoy-bridge-ok";

    async fn codex_stub_relay() -> (String, SeenRequests) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let seen: SeenRequests = Arc::new(std::sync::Mutex::new(Vec::new()));
        let recorder = seen.clone();
        let router = Router::new().route(
            "/v1/chat/completions",
            axum::routing::post(move |body: Bytes| {
                let recorder = recorder.clone();
                async move {
                    recorder
                        .lock()
                        .unwrap()
                        .push(("/v1/chat/completions".to_owned(), body.clone()));
                    let request: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
                    let already_ran_a_tool = request["messages"]
                        .as_array()
                        .is_some_and(|messages| messages.iter().any(|m| m["role"] == "tool"));
                    let reply = if already_ran_a_tool {
                        chat_sse_final_answer()
                    } else {
                        chat_sse_tool_call(&request)
                    };
                    ([(header::CONTENT_TYPE, "text/event-stream")], reply)
                }
            }),
        );
        tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
        (origin, seen)
    }

    fn chat_sse_chunk(delta: Value, finish: Option<&str>) -> String {
        let mut choice = json!({"index": 0, "delta": delta});
        if let Some(finish) = finish {
            choice["finish_reason"] = json!(finish);
        }
        format!(
            "data: {}\n\n",
            json!({
                "id": "chatcmpl-real-device",
                "object": "chat.completion.chunk",
                "created": 1,
                "model": "kimi-k3",
                "choices": [choice],
            })
        )
    }

    /// 第一轮：挑一个 Codex 真的发过来的工具，调它。
    fn chat_sse_tool_call(request: &Value) -> String {
        let tools = request["tools"].as_array().cloned().unwrap_or_default();
        let name_of = |tool: &Value| {
            tool["function"]["name"]
                .as_str()
                .or_else(|| tool["name"].as_str())
                .unwrap_or_default()
                .to_owned()
        };
        // 优先挑 shell 一类的工具，它的入参我们给得出合法值；否则退回第一个。
        let shell = tools.iter().find(|tool| {
            let name = name_of(tool);
            name == "shell" || name.contains("shell") || name.contains("exec")
        });
        let (name, arguments) = match shell.or_else(|| tools.first()) {
            Some(tool) if !name_of(tool).is_empty() => {
                let name = name_of(tool);
                // 参数按**工具自己声明的 schema** 填，不按我们以为的形状填。
                // 第一版写死 `{"command": [...]}`，而 Codex 0.155.1 的
                // `exec_command` 要的是字符串 `cmd` —— 真机上换来一句
                // `failed to parse function arguments: missing field cmd`。
                // 那不是桥的问题，是桩在赌一个没验证过的形状。
                let properties = tool["function"]["parameters"]["properties"]
                    .as_object()
                    .or_else(|| tool["parameters"]["properties"].as_object());
                let arguments = match properties {
                    Some(properties) if properties.contains_key("cmd") => {
                        json!({ "cmd": ECHO_MARKER_COMMAND }).to_string()
                    }
                    Some(properties) if properties.contains_key("command") => {
                        // 有的工具要数组，有的要字符串 —— 按它声明的类型给。
                        if properties["command"]["type"] == "array" {
                            json!({"command": ["echo", ECHO_MARKER]}).to_string()
                        } else {
                            json!({ "command": ECHO_MARKER_COMMAND }).to_string()
                        }
                    }
                    _ => "{}".to_owned(),
                };
                (name, arguments)
            }
            // Codex 一个工具都没发：直接给最终答案，测试会在工具断言那里说清楚。
            _ => return chat_sse_final_answer(),
        };
        [
            chat_sse_chunk(json!({"role": "assistant", "content": "这就跑。"}), None),
            chat_sse_chunk(
                json!({"tool_calls": [{
                    "index": 0,
                    "id": "call_real_device",
                    "type": "function",
                    "function": {"name": name, "arguments": ""}
                }]}),
                None,
            ),
            chat_sse_chunk(
                json!({"tool_calls": [{
                    "index": 0,
                    "function": {"arguments": arguments}
                }]}),
                Some("tool_calls"),
            ),
            "data: [DONE]\n\n".to_owned(),
        ]
        .concat()
    }

    /// 第二轮：工具结果回来了，给最终文本。
    fn chat_sse_final_answer() -> String {
        [
            chat_sse_chunk(
                json!({"role": "assistant", "content": format!("结果是 {ECHO_MARKER}，桥通了。")}),
                None,
            ),
            chat_sse_chunk(json!({}), Some("stop")),
            "data: [DONE]\n\n".to_owned(),
        ]
        .concat()
    }
}

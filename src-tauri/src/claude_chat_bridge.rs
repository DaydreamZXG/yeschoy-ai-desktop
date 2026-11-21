//! Claude 客户端 ↔ 只会说 OpenAI Chat 格式的中转站。
//!
//! [`super::messages`] 平时把 Anthropic 请求原样转给站点的 `/v1/messages`。
//! 有的中转站没有这个端点，只有 `/v1/chat/completions`。接入时探出这种站
//! （`claude_transport = chat_bridge`）之后，请求改从这里走：把 Anthropic 请求
//! 翻成 Chat 请求发出去，再把回复翻回 Anthropic 格式，Claude 客户端感觉不到区别。
//!
//! 翻译本身用的是 `crate::proxy` 里来自 CC Switch 的协议转换（见
//! `src/proxy/VENDOR.md`）；这个文件只负责收发和把错误说成 Claude 听得懂的样子。

use axum::{
    body::Body,
    http::{header, StatusCode},
    response::{IntoResponse, Response},
};
use futures::StreamExt;
use serde_json::{json, Value};

use super::{buffer_upstream, context_too_long_error, error, is_context_overflow};
use crate::{
    codex_responses_bridge::normalize_openai_reasoning_params,
    local_bridge::BridgeState,
    proxy::providers::{
        streaming::create_anthropic_sse_stream,
        transform::{
            anthropic_to_openai_with_reasoning_content, inject_openai_stream_include_usage,
            openai_to_anthropic, resolve_reasoning_effort,
        },
    },
    tool_credentials::ToolCredential,
    tool_model_profile::{apply_chat_effort, is_deepseek},
};

/// 非流式回复整段读进内存再翻译，给个上限。
const MAX_RESPONSE_BODY_BYTES: usize = 32 * 1024 * 1024;

pub(super) fn applies(credential: &ToolCredential) -> bool {
    credential.claude_transport.as_deref()
        == Some(super::ClaudeTransport::ChatBridge.credential_value())
}

/// Anthropic 请求 → 这个模型真正收得下的 Chat 请求。
///
/// 结构转换交给上游；这里补三件它不管、而各家又不一样的事，和 Codex 那座桥
/// 用的是同一套规则：
///
/// * DeepSeek 开着思考做工具调用时，续轮必须把上一步的思考原样带回去
///   （`reasoning_content`），丢了它整轮工具调用就被拒；
/// * 思考开关和档位：DeepSeek 要 `thinking: {type}` 且只有 low/high/max，
///   OpenAI 系按模型实际支持的档位落；
/// * gpt-5/6 不收 `max_tokens`，要 `max_completion_tokens`。
fn to_chat(value: Value) -> Option<Value> {
    let model = value.get("model").and_then(Value::as_str)?.to_owned();
    let deepseek = is_deepseek(&model);
    // 关掉思考是显式状态，优先于界面上残留的档位。
    let asked = value
        .pointer("/output_config/effort")
        .and_then(Value::as_str);
    let effort = match value.pointer("/thinking/type").and_then(Value::as_str) {
        Some("disabled") => Some("none"),
        // 上游把 Claude 的 max 说成 OpenAI 的 xhigh；DeepSeek 自己就有 max，
        // 经 xhigh 转一道会落到 high，用户选不到最高档。
        _ if deepseek && asked == Some("max") => Some("max"),
        _ => resolve_reasoning_effort(&value),
    };
    let mut chat = anthropic_to_openai_with_reasoning_content(value, deepseek).ok()?;
    if let Some(effort) = effort {
        // 只动「确实有思考档位」的模型：DeepSeek，或上游已经判定该带档位的。
        // 别的模型多一个 `reasoning_effort` 字段，严格的后端会直接拒。
        if deepseek || chat.get("reasoning_effort").is_some() {
            apply_chat_effort(&mut chat, &model, effort);
        }
    }
    normalize_openai_reasoning_params(&mut chat, &model);
    inject_openai_stream_include_usage(&mut chat);
    Some(chat)
}

pub(super) async fn forward(
    state: &BridgeState,
    credential: &ToolCredential,
    forwarded: &[(header::HeaderName, String)],
    value: Value,
) -> Response {
    let streaming = value.get("stream").and_then(Value::as_bool) == Some(true);
    let Some(chat) = to_chat(value) else {
        return anthropic_error(
            StatusCode::BAD_REQUEST,
            "invalid_request_error",
            "This request could not be converted for a Chat Completions relay.".into(),
        );
    };

    let url = format!(
        "{}/chat/completions",
        crate::provider_presets::openai_base(&credential.origin)
    );
    // 应用自报的名字照原样带过去。`anthropic-*` 那些头不带：对面是 Chat 接口，
    // 用不上。
    let mut request = state.client.post(url);
    if let Some((name, value)) = forwarded
        .iter()
        .find(|(name, _)| name == header::USER_AGENT)
    {
        request = request.header(name, value);
    }
    // 厂商要的会话标记：应用带了就带它的，没带就补一个。
    if let Some(wanted) = crate::provider_presets::session_header(&credential.origin) {
        let sent = forwarded
            .iter()
            .find(|(name, _)| name.as_str().eq_ignore_ascii_case(wanted));
        request = match sent {
            Some((name, value)) => request.header(name, value),
            None => match crate::local_bridge::session_mark(
                &credential.origin,
                forwarded
                    .iter()
                    .map(|(name, value)| (name.as_str(), value.as_str())),
                &chat,
            ) {
                Some((name, mark)) => request.header(name, mark),
                None => request,
            },
        };
    }
    let upstream = match request
        .bearer_auth(credential.upstream_key())
        .json(&chat)
        .send()
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

    if !upstream.status().is_success() {
        let Ok(failed) = buffer_upstream(upstream).await else {
            return error(
                StatusCode::BAD_GATEWAY,
                "relay returned an oversized error response",
            );
        };
        // 和直连那条路同一个理由：Claude 只有收到「请求太长」才会去压缩对话。
        if is_context_overflow(&failed.body) {
            return context_too_long_error();
        }
        return anthropic_error(
            failed.status,
            error_type(failed.status),
            upstream_message(&failed.body),
        );
    }

    if streaming {
        return Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, "text/event-stream")
            .header(header::CACHE_CONTROL, "no-cache")
            .body(Body::from_stream(create_anthropic_sse_stream(
                surface_stream_errors(upstream.bytes_stream()),
            )))
            .unwrap_or_else(|_| error(StatusCode::INTERNAL_SERVER_ERROR, "response failed"));
    }

    let mut body = Vec::new();
    let mut stream = upstream.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let Ok(chunk) = chunk else {
            return error(StatusCode::BAD_GATEWAY, "relay response was interrupted");
        };
        if body.len() + chunk.len() > MAX_RESPONSE_BODY_BYTES {
            return error(StatusCode::BAD_GATEWAY, "relay response was too large");
        }
        body.extend_from_slice(&chunk);
    }
    match serde_json::from_slice::<Value>(&body)
        .ok()
        .and_then(|reply| openai_to_anthropic(reply).ok())
    {
        Some(reply) => (StatusCode::OK, axum::Json(reply)).into_response(),
        None => error(
            StatusCode::BAD_GATEWAY,
            "relay returned a reply that is not a Chat Completions response",
        ),
    }
}

/// 把「200 的流里夹着一个错误对象」变成流错误。
///
/// 有的中转站在已经回了 200 之后才发现上游失败，于是在流里发一条
/// `data: {"error": {...}}` 再收尾。上游的转换器只认 `choices`，这一条会被
/// 当成空块跳过，Claude 收到的是一次「成功但什么也没说」的回复。这里逐行看，
/// 遇到它就以错误结束这条流，转换器会把它说成 Anthropic 的 `error` 事件。
/// 其余字节按原样、按行放行。
fn surface_stream_errors<E: Send + 'static>(
    upstream: impl futures::Stream<Item = Result<bytes::Bytes, E>> + Send + 'static,
) -> impl futures::Stream<Item = Result<bytes::Bytes, std::io::Error>> + Send {
    async_stream::stream! {
        futures::pin_mut!(upstream);
        let mut pending: Vec<u8> = Vec::new();
        while let Some(chunk) = upstream.next().await {
            let chunk = match chunk {
                Ok(chunk) => chunk,
                Err(_) => {
                    yield Err(std::io::Error::other("the relay closed the connection"));
                    return;
                }
            };
            pending.extend_from_slice(&chunk);
            while let Some(end) = pending.iter().position(|byte| *byte == b'\n') {
                let line: Vec<u8> = pending.drain(..=end).collect();
                if let Some(message) = stream_error_message(&line) {
                    yield Err(std::io::Error::other(message));
                    return;
                }
                yield Ok(bytes::Bytes::from(line));
            }
        }
        if !pending.is_empty() {
            if let Some(message) = stream_error_message(&pending) {
                yield Err(std::io::Error::other(message));
                return;
            }
            yield Ok(bytes::Bytes::from(pending));
        }
    }
}

/// 一行 SSE 是不是「只有错误、没有内容」的数据行；是的话给出站点的原话。
fn stream_error_message(line: &[u8]) -> Option<String> {
    let data = line.strip_prefix(b"data:")?;
    let value = serde_json::from_slice::<Value>(data).ok()?;
    let failed = value.get("error").is_some_and(|error| !error.is_null())
        && value
            .get("choices")
            .and_then(Value::as_array)
            .is_none_or(Vec::is_empty);
    failed.then(|| upstream_message(data))
}

/// Anthropic 的错误类型决定 Claude 是重试、等一等，还是让用户去处理。
fn error_type(status: StatusCode) -> &'static str {
    match status.as_u16() {
        401 => "authentication_error",
        403 => "permission_error",
        404 => "not_found_error",
        413 => "request_too_large",
        429 => "rate_limit_error",
        400..=499 => "invalid_request_error",
        529 => "overloaded_error",
        _ => "api_error",
    }
}

/// 站点自己的错误原话。OpenAI 形状是 `{"error":{"message":...}}`，
/// 也有站点直接给 `{"message":...}` 或 `{"error":"..."}`。
fn upstream_message(body: &[u8]) -> String {
    let value = serde_json::from_slice::<Value>(body).unwrap_or(Value::Null);
    let message = value
        .pointer("/error/message")
        .or_else(|| value.get("message"))
        .or_else(|| value.get("error"))
        .and_then(Value::as_str)
        .unwrap_or("The relay rejected this request.");
    message
        .chars()
        .filter(|c| !c.is_control())
        .take(600)
        .collect()
}

fn anthropic_error(status: StatusCode, kind: &'static str, message: String) -> Response {
    (
        status,
        axum::Json(json!({"type": "error", "error": {"type": kind, "message": message}})),
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gpt5_and_newer_get_max_completion_tokens() {
        let chat = to_chat(json!({
            "model": "gpt-5.4", "max_tokens": 512, "temperature": 0.3,
            "messages": [{"role": "user", "content": "hi"}]
        }))
        .unwrap();
        assert_eq!(chat["max_completion_tokens"], 512);
        assert!(chat.get("max_tokens").is_none());
        // 别的家族要的就是 max_tokens，不能跟着改。
        let chat = to_chat(json!({
            "model": "deepseek-v4-pro", "max_tokens": 512,
            "messages": [{"role": "user", "content": "hi"}]
        }))
        .unwrap();
        assert_eq!(chat["max_tokens"], 512);
    }

    #[test]
    fn deepseek_keeps_its_reasoning_across_a_tool_call_and_obeys_the_thinking_switch() {
        let request = |extra: Value| {
            let mut body = json!({
                "model": "deepseek-v4-pro", "max_tokens": 64,
                "messages": [
                    {"role": "user", "content": "list files"},
                    {"role": "assistant", "content": [
                        {"type": "thinking", "thinking": "need ls", "signature": ""},
                        {"type": "tool_use", "id": "t1", "name": "ls", "input": {}}
                    ]},
                    {"role": "user", "content": [
                        {"type": "tool_result", "tool_use_id": "t1", "content": "a.txt"}
                    ]}
                ]
            });
            body.as_object_mut()
                .unwrap()
                .extend(extra.as_object().unwrap().clone());
            to_chat(body).unwrap()
        };

        let chat = request(json!({"output_config": {"effort": "medium"}}));
        let assistant = chat["messages"]
            .as_array()
            .unwrap()
            .iter()
            .find(|message| message["role"] == "assistant")
            .unwrap();
        assert_eq!(assistant["reasoning_content"], "need ls");
        assert_eq!(chat["thinking"]["type"], "enabled");
        // DeepSeek 没有 medium，落到 high。
        assert_eq!(chat["reasoning_effort"], "high");

        // max 是 DeepSeek 自己有的档，不能被降成 high。
        let max = request(json!({"output_config": {"effort": "max"}}));
        assert_eq!(max["reasoning_effort"], "max");
        assert_eq!(max["thinking"]["type"], "enabled");

        let off = request(json!({
            "thinking": {"type": "disabled"},
            "output_config": {"effort": "high"}
        }));
        assert_eq!(off["thinking"]["type"], "disabled");
        assert!(off.get("reasoning_effort").is_none());

        // 没说要不要思考：不替用户加开关。
        let silent = request(json!({}));
        assert!(silent.get("thinking").is_none());
        assert!(silent.get("reasoning_effort").is_none());
    }

    #[test]
    fn models_without_reasoning_levels_never_gain_a_reasoning_field() {
        let chat = to_chat(json!({
            "model": "some-relay-model", "max_tokens": 64,
            "thinking": {"type": "enabled", "budget_tokens": 8000},
            "messages": [{"role": "user", "content": "hi"}]
        }))
        .unwrap();
        assert!(chat.get("reasoning_effort").is_none());
        assert!(chat.get("thinking").is_none());
        assert_eq!(chat["max_tokens"], 64);
    }

    #[tokio::test]
    async fn an_error_object_inside_a_successful_stream_ends_it_as_an_error() {
        let upstream = futures::stream::iter([
            Ok::<_, std::io::Error>(bytes::Bytes::from_static(
                b"data: {\"choices\":[{\"delta\":{\"content\":\"hi\"}}]}\n\ndata: {\"err",
            )),
            Ok(bytes::Bytes::from_static(
                b"or\":{\"message\":\"upstream overloaded\"}}\n\ndata: [DONE]\n\n",
            )),
        ]);
        let items: Vec<_> = surface_stream_errors(upstream).collect().await;
        let passed: Vec<u8> = items
            .iter()
            .filter_map(|item| item.as_ref().ok())
            .flat_map(|bytes| bytes.to_vec())
            .collect();
        assert_eq!(
            passed,
            b"data: {\"choices\":[{\"delta\":{\"content\":\"hi\"}}]}\n\n"
        );
        let failure = items.last().unwrap().as_ref().unwrap_err();
        assert_eq!(failure.to_string(), "upstream overloaded");

        // 正常的流逐字节不变，包括跨块切开的行。
        let upstream = futures::stream::iter([
            Ok::<_, std::io::Error>(bytes::Bytes::from_static(b"data: {\"choices\":[")),
            Ok(bytes::Bytes::from_static(b"]}\n\ndata: [DONE]\n\n")),
        ]);
        let passed: Vec<u8> = surface_stream_errors(upstream)
            .filter_map(|item| async move { item.ok() })
            .flat_map(|bytes| futures::stream::iter(bytes.to_vec()))
            .collect()
            .await;
        assert_eq!(passed, b"data: {\"choices\":[]}\n\ndata: [DONE]\n\n");
    }

    #[test]
    fn upstream_errors_keep_the_stations_words_and_a_claude_error_type() {
        assert_eq!(error_type(StatusCode::UNAUTHORIZED), "authentication_error");
        assert_eq!(
            error_type(StatusCode::TOO_MANY_REQUESTS),
            "rate_limit_error"
        );
        assert_eq!(
            error_type(StatusCode::PAYMENT_REQUIRED),
            "invalid_request_error"
        );
        assert_eq!(error_type(StatusCode::BAD_GATEWAY), "api_error");
        assert_eq!(
            upstream_message(br#"{"error":{"message":"quota exceeded"}}"#),
            "quota exceeded"
        );
        assert_eq!(
            upstream_message(br#"{"message":"no channel"}"#),
            "no channel"
        );
        assert_eq!(upstream_message(br#"{"error":"bad key"}"#), "bad key");
        assert_eq!(
            upstream_message(b"<html>"),
            "The relay rejected this request."
        );
    }
}

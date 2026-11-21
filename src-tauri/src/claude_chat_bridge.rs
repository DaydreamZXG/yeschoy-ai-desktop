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
    local_bridge::BridgeState,
    proxy::providers::{
        streaming::create_anthropic_sse_stream,
        transform::{anthropic_to_openai, inject_openai_stream_include_usage, openai_to_anthropic},
    },
    tool_credentials::ToolCredential,
};

/// 非流式回复整段读进内存再翻译，给个上限。
const MAX_RESPONSE_BODY_BYTES: usize = 32 * 1024 * 1024;

pub(super) fn applies(credential: &ToolCredential) -> bool {
    credential.claude_transport.as_deref()
        == Some(super::ClaudeTransport::ChatBridge.credential_value())
}

pub(super) async fn forward(
    state: &BridgeState,
    credential: &ToolCredential,
    value: Value,
) -> Response {
    let streaming = value.get("stream").and_then(Value::as_bool) == Some(true);
    let mut chat = match anthropic_to_openai(value) {
        Ok(chat) => chat,
        Err(_) => {
            return anthropic_error(
                StatusCode::BAD_REQUEST,
                "invalid_request_error",
                "This request could not be converted for a Chat Completions relay.".into(),
            )
        }
    };
    inject_openai_stream_include_usage(&mut chat);

    let url = format!(
        "{}/v1/chat/completions",
        credential.origin.trim_end_matches('/')
    );
    let upstream = match state
        .client
        .post(url)
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
                upstream.bytes_stream(),
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

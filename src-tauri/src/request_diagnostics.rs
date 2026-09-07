//! Bounded, volatile observations. Never retain request bodies, keys or upstream errors.
use std::{
    collections::HashMap,
    sync::{Mutex, OnceLock},
    time::{SystemTime, UNIX_EPOCH},
};

use bytes::Bytes;
use futures::{Stream, StreamExt};
use serde::Serialize;
use serde_json::{json, Value};

const TOOLS: [&str; 7] = [
    "claude_code",
    "claude_desktop",
    "codex_desktop",
    "pi",
    "dsh_web",
    "hermes",
    "openclaw",
];
pub(crate) const MAX_EVENT_BYTES: usize = 16 * 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum RequestOutcome {
    Ok,
    Timeout,
    NetworkError,
    UpstreamError,
    InvalidResponse,
    StreamInterrupted,
    UnknownModel,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RequestObservation {
    pub(crate) model_id: String,
    pub(crate) billing_group: String,
    pub(crate) line_id: String,
    pub(crate) outcome: RequestOutcome,
    pub(crate) http_status: u16,
    pub(crate) observed_at_epoch_ms: u64,
}

fn observations() -> &'static Mutex<HashMap<String, RequestObservation>> {
    static OBSERVATIONS: OnceLock<Mutex<HashMap<String, RequestObservation>>> = OnceLock::new();
    OBSERVATIONS.get_or_init(|| Mutex::new(HashMap::new()))
}

pub(crate) fn record(
    tool: &str,
    model: &str,
    group: &str,
    origin: &str,
    outcome: RequestOutcome,
    http_status: u16,
) {
    let line = match origin {
        "https://yeschoy.com" => "mainland_optimized",
        "https://api.yeschoy.com" => "global_accelerated",
        _ => return,
    };
    if !TOOLS.contains(&tool)
        || model.chars().count() > 200
        || group.chars().count() > 128
        || model.chars().chain(group.chars()).any(char::is_control)
        || http_status > 599
    {
        return;
    }
    let observation = RequestObservation {
        model_id: model.into(),
        billing_group: group.into(),
        line_id: line.into(),
        outcome,
        http_status,
        observed_at_epoch_ms: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis()
            .min(u128::from(u64::MAX)) as u64,
    };
    if let Ok(mut values) = observations().lock() {
        values.insert(tool.into(), observation);
    }
}

pub(crate) fn latest(tool: &str) -> Option<RequestObservation> {
    observations().lock().ok()?.get(tool).cloned()
}

pub(crate) fn clear(tool: &str) {
    if let Ok(mut values) = observations().lock() {
        values.remove(tool);
    }
}

pub(crate) fn curated_message(outcome: RequestOutcome) -> &'static str {
    match outcome {
        RequestOutcome::Ok => "Request completed",
        RequestOutcome::Timeout => "The provider took too long to respond. Try again.",
        RequestOutcome::NetworkError => "The provider could not be reached. Check the connection and try again.",
        RequestOutcome::UpstreamError => "The provider could not complete this request. Check the selected model and account, then try again.",
        RequestOutcome::InvalidResponse => "The provider returned an unreadable response. Try again.",
        RequestOutcome::StreamInterrupted => "The response was interrupted before completion. Try again.",
        RequestOutcome::UnknownModel => "This model is not configured. Select a model from the configured list.",
    }
}

pub(crate) fn outcome_for_status(status: u16) -> RequestOutcome {
    if status == 408 || status == 504 {
        RequestOutcome::Timeout
    } else {
        RequestOutcome::UpstreamError
    }
}

pub(crate) fn transport_outcome(error: &(dyn std::error::Error + 'static)) -> RequestOutcome {
    let mut current = Some(error);
    while let Some(error) = current {
        if error
            .downcast_ref::<reqwest::Error>()
            .is_some_and(reqwest::Error::is_timeout)
            || error
                .downcast_ref::<std::io::Error>()
                .is_some_and(|e| e.kind() == std::io::ErrorKind::TimedOut)
        {
            return RequestOutcome::Timeout;
        }
        current = error.source();
    }
    RequestOutcome::NetworkError
}

pub(crate) fn error_json(outcome: RequestOutcome) -> Value {
    json!({"error":{"type":"yeschoy_bridge_error", "code":outcome, "message":curated_message(outcome)}})
}

#[derive(Clone)]
pub(crate) struct RequestContext {
    pub(crate) tool: String,
    pub(crate) model: String,
    pub(crate) group: String,
    pub(crate) origin: String,
}
impl RequestContext {
    pub(crate) fn record(&self, outcome: RequestOutcome, status: u16) {
        record(
            &self.tool,
            &self.model,
            &self.group,
            &self.origin,
            outcome,
            status,
        );
    }
}

#[derive(Clone, Copy)]
pub(crate) enum StreamProtocol {
    Anthropic,
    Responses,
    Chat,
}

pub(crate) fn sse_error(protocol: StreamProtocol, outcome: RequestOutcome) -> Bytes {
    let error = error_json(outcome)["error"].clone();
    let (name, body) = match protocol {
        StreamProtocol::Anthropic => {
            let mut error = error;
            error["type"] = json!("api_error");
            ("error", json!({"type":"error","error":error}))
        }
        StreamProtocol::Chat => ("error", json!({"error":error})),
        StreamProtocol::Responses => (
            "response.failed",
            json!({"type":"response.failed","response":{"id":"resp_yeschoy_error","object":"response","created_at":SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs(),"model":"","status":"failed","output":[],"usage":null,"error":error}}),
        ),
    };
    Bytes::from(format!("event: {name}\ndata: {body}\n\n"))
}

// Validate/sanitize the protocol the client actually receives. Error messages and
// transport details never cross the local gateway. EOF without a terminal fails.
pub(crate) fn observed_sse<E: std::error::Error + Send + 'static>(
    stream: impl Stream<Item = Result<Bytes, E>> + Send + 'static,
    protocol: StreamProtocol,
    context: RequestContext,
    status: u16,
) -> impl Stream<Item = Result<Bytes, std::io::Error>> + Send {
    async_stream::stream! {
        let mut buffer = String::new();
        let mut utf8_remainder = Vec::new();
        let mut chat_finished = false;
        futures::pin_mut!(stream);
        while let Some(chunk) = stream.next().await {
            let bytes = match chunk {
                Ok(bytes) => bytes,
                Err(error) => {
                    let outcome = if transport_outcome(&error) == RequestOutcome::Timeout { RequestOutcome::Timeout } else { RequestOutcome::StreamInterrupted };
                    context.record(outcome, status);
                    yield Ok(sse_error(protocol, outcome));
                    return;
                }
            };
            crate::codex_bridge::sse::append_utf8_safe(&mut buffer, &mut utf8_remainder, &bytes);
            if buffer.len() > MAX_EVENT_BYTES {
                context.record(RequestOutcome::InvalidResponse, status);
                yield Ok(sse_error(protocol, RequestOutcome::InvalidResponse));
                return;
            }
            while let Some(block) = crate::codex_bridge::sse::take_sse_block(&mut buffer) {
                let mut name = "";
                let mut data = Vec::new();
                for line in block.lines() {
                    if let Some(value) = crate::codex_bridge::sse::strip_sse_field(line, "event") { name = value.trim(); }
                    if let Some(value) = crate::codex_bridge::sse::strip_sse_field(line, "data") { data.push(value); }
                }
                if data.is_empty() { yield Ok(Bytes::from(format!("{block}\n\n"))); continue; }
                let data = data.join("\n");
                if matches!(protocol, StreamProtocol::Chat) && data.trim() == "[DONE]" {
                    if chat_finished {
                        context.record(RequestOutcome::Ok, status);
                        yield Ok(Bytes::from("data: [DONE]\n\n"));
                    } else {
                        context.record(RequestOutcome::StreamInterrupted, status);
                        yield Ok(sse_error(protocol, RequestOutcome::StreamInterrupted));
                    }
                    return;
                }
                let value = match serde_json::from_str::<Value>(&data) {
                    Ok(value) if value.is_object() => value,
                    _ => {
                        context.record(RequestOutcome::InvalidResponse, status);
                        yield Ok(sse_error(protocol, RequestOutcome::InvalidResponse));
                        return;
                    }
                };
                let event_type = value["type"].as_str().unwrap_or(name);
                if name == "error" || event_type == "error" || event_type == "response.failed" || value.get("error").is_some_and(|v| !v.is_null()) || value.pointer("/response/error").is_some_and(|v| !v.is_null()) {
                    let code = value.pointer("/error/code").or_else(|| value.pointer("/response/error/code")).and_then(Value::as_str);
                    let outcome = match code { Some("timeout") => RequestOutcome::Timeout, Some("stream_interrupted") => RequestOutcome::StreamInterrupted, Some("invalid_response") => RequestOutcome::InvalidResponse, _ => RequestOutcome::UpstreamError };
                    context.record(outcome, status);
                    yield Ok(sse_error(protocol, outcome));
                    return;
                }
                let terminal = match protocol {
                    StreamProtocol::Anthropic => event_type == "message_stop",
                    StreamProtocol::Responses => {
                        if event_type == "response.completed" || event_type == "response.incomplete" {
                            if !matches!(value.pointer("/response/status").and_then(Value::as_str), Some("completed" | "incomplete")) || !value["response"]["output"].is_array() {
                                context.record(RequestOutcome::InvalidResponse, status);
                                yield Ok(sse_error(protocol, RequestOutcome::InvalidResponse));
                                return;
                            }
                            true
                        } else { false }
                    },
                    StreamProtocol::Chat => {
                        chat_finished |= value["choices"].as_array().is_some_and(|choices| choices.iter().any(|c| c["finish_reason"].as_str().is_some_and(|r| !r.is_empty())));
                        false
                    }
                };
                if terminal { context.record(RequestOutcome::Ok, status); }
                yield Ok(Bytes::from(format!("{block}\n\n")));
                if terminal { return; }
            }
        }
        // Chat's finish_reason is also a protocol completion when a compatible
        // server closes directly, but a partial JSON/SSE frame is never accepted.
        if matches!(protocol, StreamProtocol::Chat) && chat_finished && buffer.trim().is_empty() && utf8_remainder.is_empty() {
            context.record(RequestOutcome::Ok, status);
        } else {
            context.record(RequestOutcome::StreamInterrupted, status);
            yield Ok(sse_error(protocol, RequestOutcome::StreamInterrupted));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ru042_observations_are_bounded_and_secret_free() {
        clear("pi");
        record(
            "pi",
            "model-a",
            "group-a",
            "https://yeschoy.com",
            RequestOutcome::Timeout,
            0,
        );
        let value = serde_json::to_value(latest("pi").unwrap()).unwrap();
        assert_eq!(value.as_object().unwrap().len(), 6);
        assert_eq!(value["httpStatus"], 0);
        assert_eq!(value["lineId"], "mainland_optimized");
        assert_eq!(value["outcome"], "timeout");
        record(
            "pi",
            "model-b",
            "group-b",
            "https://api.yeschoy.com",
            RequestOutcome::Ok,
            200,
        );
        assert_eq!(latest("pi").unwrap().model_id, "model-b");
        record(
            "not-a-tool",
            "a",
            "b",
            "https://yeschoy.com",
            RequestOutcome::Ok,
            200,
        );
        assert!(latest("not-a-tool").is_none());
        clear("pi");
        assert!(latest("pi").is_none());
    }

    #[tokio::test]
    async fn ru042_stream_errors_and_truncation_are_curated_not_success() {
        for (protocol, raw) in [
            (StreamProtocol::Anthropic, "event: error\ndata: {\"type\":\"error\",\"error\":{\"message\":\"secret-key raw upstream\"}}\n\n"),
            (StreamProtocol::Responses, "event: response.failed\ndata: {\"type\":\"response.failed\",\"response\":{\"error\":{\"message\":\"secret-key raw upstream\"}}}\n\n"),
            (StreamProtocol::Chat, "data: [DONE]\n\n"),
        ] {
            let stream = futures::stream::iter(vec![Ok::<_, std::io::Error>(Bytes::from(raw))]);
            let context = RequestContext { tool: "hermes".into(), model: "model-a".into(), group: "g".into(), origin: "https://yeschoy.com".into() };
            let values: Vec<_> = observed_sse(stream, protocol, context, 200).collect().await;
            let output: Vec<u8> = values.into_iter().flat_map(Result::unwrap).collect();
            let output = String::from_utf8(output).unwrap();
            assert!(!output.contains("secret-key"));
            assert!(!output.contains("message_stop"));
            assert!(!output.contains("response.completed"));
            assert_ne!(latest("hermes").unwrap().outcome, RequestOutcome::Ok);
        }
    }

    #[tokio::test]
    async fn ru042_read_timeout_is_distinct_and_does_not_expose_transport_detail() {
        let stream = futures::stream::iter(vec![Err::<Bytes, _>(std::io::Error::new(
            std::io::ErrorKind::TimedOut,
            "synthetic-secret-url",
        ))]);
        let context = RequestContext {
            tool: "dsh_web".into(),
            model: "model".into(),
            group: "group".into(),
            origin: "https://yeschoy.com".into(),
        };
        let chunks: Vec<_> = observed_sse(stream, StreamProtocol::Chat, context, 200)
            .collect()
            .await;
        let output: Vec<u8> = chunks.into_iter().flat_map(Result::unwrap).collect();
        let output = String::from_utf8(output).unwrap();
        assert!(output.contains("timeout"));
        assert!(!output.contains("synthetic-secret-url"));
        assert_eq!(latest("dsh_web").unwrap().outcome, RequestOutcome::Timeout);
    }
}

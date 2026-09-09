//! Codex/Claude protocol helpers vendored from CC Switch.
//!
//! This module used to host a loopback gateway on 127.0.0.1:15722. The relay
//! serves the Responses and Anthropic protocols natively, so the gateway and
//! its watchdog were removed; what remains are the conversion helpers that
//! `claude_bridge` and the catalog writers still use.
use serde::{Deserialize, Serialize};

pub(crate) const BASE_URL: &str = "http://127.0.0.1:15722/yeschoy/v1";

#[path = "proxy/providers/streaming.rs"]
pub(crate) mod claude_streaming;
#[path = "proxy/providers/transform.rs"]
pub(crate) mod claude_transform;
#[path = "proxy/providers/codex_chat_common.rs"]
pub(crate) mod codex_chat_common;
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

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
}

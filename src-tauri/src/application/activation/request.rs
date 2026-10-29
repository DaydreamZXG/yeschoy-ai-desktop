//! Activation request identity and shape (application boundary): no Tauri,
//! transport, tool format, process or filesystem access.
use serde::Deserialize;

use crate::connectivity_core::request_id_is_valid;
use crate::domain::model::{bounded_plain_text, valid_model_bindings, ModelBinding};
use crate::station_connect::connect_origin_is_acceptable;

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ToolActivationRequest {
    pub(crate) request_id: String,
    pub(crate) line_id: String,
    pub(crate) tool_id: String,
    pub(crate) model_id: String,
    pub(crate) installation_id: String,
    pub(crate) billing_group: String,
    #[serde(default)]
    pub(crate) models: Option<Vec<ModelBinding>>,
    #[serde(default)]
    pub(crate) installation_job_id: Option<String>,
    #[serde(default)]
    pub(crate) restart_running_app: bool,
    /// Separate from `restart_running_app`: consenting to a restart is not
    /// consenting to move the user's claude.ai login (ru056).
    #[serde(default)]
    pub(crate) displace_claude_login: bool,
    /// Whether this activation may probe the relay's `/v1/responses` and
    /// `/v1/chat/completions` with a minimal real request to decide the Codex
    /// wire format. Off unless the user turned it on in Settings: a successful
    /// probe spends real tokens, and the product promises it sends no paid
    /// test requests. Without it every model keeps the direct Responses route,
    /// which is also where an inconclusive probe ends up.
    #[serde(default)]
    pub(crate) probe_codex_transport: bool,
}

impl ToolActivationRequest {
    pub(crate) fn bindings(&self) -> Vec<ModelBinding> {
        self.models.clone().unwrap_or_else(|| {
            vec![ModelBinding {
                model_id: self.model_id.clone(),
                billing_group: self.billing_group.clone(),
            }]
        })
    }
}

fn activation_line_id_is_valid(line_id: &str) -> bool {
    matches!(line_id, "mainland_optimized" | "global_accelerated")
        || crate::account_v2::station_connect_origin_from_line_id(line_id)
            .is_some_and(|origin| connect_origin_is_acceptable(origin).is_some())
}

pub(crate) fn request_is_valid(request: &ToolActivationRequest) -> bool {
    request_id_is_valid(&request.request_id)
        && activation_line_id_is_valid(&request.line_id)
        && crate::tool_adapters::tool_ids().any(|id| id == request.tool_id)
        && bounded_plain_text(&request.model_id, 200)
        && bounded_plain_text(&request.billing_group, 128)
        && request.billing_group != "auto"
        && valid_model_bindings(
            &request.bindings(),
            &request.model_id,
            &request.billing_group,
        )
        && request.installation_id.len() <= 128
        && request
            .installation_job_id
            .as_ref()
            .is_none_or(|id| request_id_is_valid(id))
        && request
            .installation_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
        && (!request.restart_running_app
            || crate::tool_adapters::desktop_lifecycle::requires_reload(&request.tool_id))
}

#[cfg(test)]
#[path = "request_tests.rs"]
mod tests;

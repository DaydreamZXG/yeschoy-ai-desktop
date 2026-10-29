//! 接入相关 Tauri 命令门面：只负责 Tauri 参数注入、错误字符串转换和调用用例。
//! 扫描、配置与连接恢复的具体逻辑分别在 application/activation 与
//! application/connections；工具适配器与站点实现的差异不在这里判断。
use serde::{Deserialize, Serialize};

use crate::{
    account_v2::AccountV2State,
    connectivity_core::request_id_is_valid,
    tool_adapters::{self, claude_code, claude_desktop, codex_desktop, dsh_web},
};

pub(crate) use crate::application::activation::request::ToolActivationRequest;
pub(crate) use crate::application::activation::result::{SkippedBinding, ToolActivationProjection};
pub(crate) use crate::application::activation::session::ActivationOperationState;
pub(crate) use crate::application::activation::tokens::tool_for_token_name;
pub(crate) use crate::application::connections::{
    restore_connection_for_exit, ConnectionRequest, ConnectionResponse,
};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ActivationTargetScanRequest {
    request_id: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActivationTargetScanResponse {
    request_id: String,
    schema_version: u8,
    platform: &'static str,
    targets: Vec<tool_adapters::TargetProjection>,
}

#[tauri::command]
pub async fn scan_activation_targets_v1(
    request: ActivationTargetScanRequest,
) -> Result<ActivationTargetScanResponse, String> {
    if !request_id_is_valid(&request.request_id) {
        return Err("invalid_activation_target_scan".into());
    }
    Ok(ActivationTargetScanResponse {
        request_id: request.request_id,
        schema_version: 1,
        platform: crate::tool_discovery::platform_name(),
        targets: tool_adapters::scan_targets().await,
    })
}

#[tauri::command]
#[allow(clippy::too_many_arguments)] // Tauri injects the independently owned runtime states.
pub async fn configure_desktop_tool_v2(
    app: tauri::AppHandle,
    activation_state: tauri::State<'_, ActivationOperationState>,
    account_state: tauri::State<'_, AccountV2State>,
    installation_state: tauri::State<'_, crate::app_installation::AppInstallationState>,
    claude_code_runtime: tauri::State<'_, claude_code::ClaudeCodeRuntimeState>,
    claude_runtime: tauri::State<'_, claude_desktop::ClaudeDesktopRuntimeState>,
    codex_runtime: tauri::State<'_, codex_desktop::CodexRuntimeState>,
    dsh_runtime: tauri::State<'_, dsh_web::DshRuntimeState>,
    request: ToolActivationRequest,
) -> Result<ToolActivationProjection, String> {
    crate::application::activation::service::configure_desktop_tool(
        &crate::application::activation::service::ActivationContext {
            app: &app,
            activation_state: &activation_state,
            account_state: &account_state,
            installation_state: &installation_state,
            claude_code_runtime: &claude_code_runtime,
            claude_runtime: &claude_runtime,
            codex_runtime: &codex_runtime,
            dsh_runtime: &dsh_runtime,
        },
        request,
    )
    .await
}

/// 连接状态读取与恢复的命令入口：用例本身（回执、凭据、登录态与历史）
/// 在 application/connections，门面只做参数注入和错误字符串转换。
#[tauri::command]
pub async fn manage_tool_connections_v1(
    account_state: tauri::State<'_, AccountV2State>,
    claude_code_runtime: tauri::State<'_, claude_code::ClaudeCodeRuntimeState>,
    claude_runtime: tauri::State<'_, claude_desktop::ClaudeDesktopRuntimeState>,
    codex_runtime: tauri::State<'_, codex_desktop::CodexRuntimeState>,
    dsh_runtime: tauri::State<'_, dsh_web::DshRuntimeState>,
    request: ConnectionRequest,
) -> Result<ConnectionResponse, String> {
    crate::application::connections::manage_tool_connections(
        &crate::application::connections::ConnectionContext {
            account_state: &account_state,
            claude_code_runtime: &claude_code_runtime,
            claude_runtime: &claude_runtime,
            codex_runtime: &codex_runtime,
            dsh_runtime: &dsh_runtime,
        },
        request,
    )
    .await
}

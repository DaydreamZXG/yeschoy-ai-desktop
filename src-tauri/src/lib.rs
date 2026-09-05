mod account_finance;
mod account_v2;
mod claude_bridge;
mod codex_bridge;
mod connection_recovery;
mod connectivity;
mod connectivity_core;
mod desktop_app_discovery;
mod desktop_app_discovery_core;
mod open_connection;
mod service_catalog;
mod service_catalog_core;
mod tool_activation;
mod tool_adapters;
mod tool_credentials;
mod tool_discovery;
mod tool_discovery_core;
mod tool_discovery_v2;
mod tool_selection_core;
mod window_appearance;

pub use tool_credentials::credential_helper_exit_code;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let claude_code_runtime = tool_adapters::claude_code::ClaudeCodeRuntimeState::default();
    let resume_claude_code_runtime = claude_code_runtime.clone();
    let claude_runtime = tool_adapters::claude_desktop::ClaudeDesktopRuntimeState::default();
    let resume_claude_runtime = claude_runtime.clone();
    let codex_bridge = codex_bridge::CodexBridgeRuntimeState::default();
    let resume_codex_bridge = codex_bridge.clone();
    tauri::Builder::default()
        .manage(account_v2::AccountV2State::default())
        .manage(claude_code_runtime)
        .manage(claude_runtime)
        .manage(codex_bridge)
        .manage(tool_adapters::dsh_web::DshRuntimeState::default())
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                // Keep local gateways alive. The explicit Quit action below
                // stops owned runtimes; minimizing is not a hidden exit.
                api.prevent_close();
                let _ = window.minimize();
            }
        })
        .setup(move |app| {
            window_appearance::initialize(app)?;
            tauri::async_runtime::spawn(async move {
                let _guard = tool_activation::ACTIVATION_LOCK.lock().await;
                tool_adapters::claude_code::resume_if_configured(resume_claude_code_runtime).await;
                tool_adapters::claude_desktop::resume_if_configured(resume_claude_runtime).await;
                codex_bridge::resume_if_configured(resume_codex_bridge).await;
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            desktop_app_discovery::scan_desktop_apps_read_only,
            account_v2::account_inspect_v2,
            account_v2::account_begin_authorization_v2,
            account_v2::account_poll_authorization_v2,
            account_v2::account_cancel_authorization_v2,
            account_v2::account_logout_v2,
            account_v2::account_open_wallet_v2,
            tool_activation::scan_activation_targets_v1,
            tool_activation::configure_desktop_tool_v2,
            tool_activation::manage_tool_connections_v1,
            open_connection::open_tool_connection_v1,
            quit_desktop_assistant,
            tool_discovery::scan_tools_read_only,
            tool_discovery_v2::scan_tools_read_only_v2,
            window_appearance::set_window_appearance,
            connectivity::check_line_connectivity_read_only,
            service_catalog::read_public_service_catalog
        ])
        .run(tauri::generate_context!())
        .expect("failed to run the 野菜API desktop shell");
}

#[tauri::command]
async fn quit_desktop_assistant(
    app: tauri::AppHandle,
    claude_code: tauri::State<'_, tool_adapters::claude_code::ClaudeCodeRuntimeState>,
    claude: tauri::State<'_, tool_adapters::claude_desktop::ClaudeDesktopRuntimeState>,
    codex: tauri::State<'_, codex_bridge::CodexBridgeRuntimeState>,
    dsh: tauri::State<'_, tool_adapters::dsh_web::DshRuntimeState>,
) -> Result<(), String> {
    let _guard = tool_activation::ACTIVATION_LOCK.lock().await;
    claude_code.stop().await;
    claude.stop().await;
    codex.stop().await;
    dsh.stop().await;
    app.exit(0);
    Ok(())
}

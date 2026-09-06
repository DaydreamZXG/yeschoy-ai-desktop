mod account_finance;
mod account_v2;
mod app_installation;
mod chat_gateway;
mod claude_bridge;
mod codex_bridge;
mod connection_recovery;
mod connectivity;
mod connectivity_core;
mod desktop_app_discovery;
mod desktop_app_discovery_core;
mod open_connection;
mod request_diagnostics;
mod service_catalog;
mod service_catalog_core;
mod shutdown_coordinator;
mod tool_activation;
mod tool_adapters;
mod tool_credentials;
mod tool_discovery;
mod tool_discovery_core;
mod tool_discovery_v2;
mod tool_model_profile;
mod tool_selection_core;
mod window_appearance;

pub use tool_credentials::credential_helper_exit_code;

use serde::Serialize;
use tauri::{Emitter, Manager};

use shutdown_coordinator::{
    global as shutdown, DrainOutcome, ShutdownProgress, FINISHING_NOTICE_AFTER, RUNTIME_STOP_GRACE,
};

const CLOSE_CHOICE_EVENT: &str = "yeschoy://close-choice";
const EXIT_PROGRESS_EVENT: &str = "yeschoy://exit-progress";

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let claude_code_runtime = tool_adapters::claude_code::ClaudeCodeRuntimeState::default();
    let resume_claude_code_runtime = claude_code_runtime.clone();
    let claude_runtime = tool_adapters::claude_desktop::ClaudeDesktopRuntimeState::default();
    let resume_claude_runtime = claude_runtime.clone();
    let codex_bridge = codex_bridge::CodexBridgeRuntimeState::default();
    let resume_codex_bridge = codex_bridge.clone();
    let chat_gateway = chat_gateway::ChatGatewayRuntimeState::default();
    let resume_chat_gateway = chat_gateway.clone();
    let app = tauri::Builder::default()
        .manage(account_v2::AccountV2State::default())
        .manage(app_installation::AppInstallationState::default())
        .manage(claude_code_runtime)
        .manage(claude_runtime)
        .manage(codex_bridge)
        .manage(chat_gateway)
        .manage(tool_adapters::dsh_web::DshRuntimeState::default())
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                // Closing asks; neither X nor dismissing the choice cancels
                // an operation. Only explicit confirmation starts shutdown.
                api.prevent_close();
                request_close_choice(window.app_handle());
            }
        })
        .setup(move |app| {
            window_appearance::initialize(app)?;
            register_runtime_stops(app.handle())?;
            tauri::async_runtime::spawn(async move {
                let Ok(permit) = shutdown().admit_operation() else {
                    return;
                };
                let Ok(_guard) = permit
                    .cancel_safe(tool_activation::ACTIVATION_LOCK.lock())
                    .await
                else {
                    return;
                };
                if permit.is_cancelled() {
                    return;
                }
                tool_adapters::claude_code::resume_if_configured(resume_claude_code_runtime).await;
                if permit.is_cancelled() {
                    return;
                }
                tool_adapters::claude_desktop::resume_if_configured(resume_claude_runtime).await;
                if permit.is_cancelled() {
                    return;
                }
                codex_bridge::resume_if_configured(resume_codex_bridge).await;
                if !permit.is_cancelled() {
                    chat_gateway::resume_if_configured(resume_chat_gateway).await;
                }
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            app_installation::manage_app_installation_v2,
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
            read_desktop_exit_state,
            dismiss_desktop_exit_prompt,
            background_desktop_assistant,
            tool_discovery::scan_tools_read_only,
            tool_discovery_v2::scan_tools_read_only_v2,
            window_appearance::set_window_appearance,
            connectivity::check_line_connectivity_read_only,
            service_catalog::read_public_service_catalog
        ])
        .build(tauri::generate_context!())
        .expect("failed to run the 野菜API desktop shell");
    app.run(|app, event| {
        if let tauri::RunEvent::ExitRequested { api, .. } = event {
            if shutdown().progress() != ShutdownProgress::ExitRequested {
                // Native menu Quit must use the same confirmation and cleanup
                // path. AppHandle::exit below is allowed only after safe drain.
                api.prevent_exit();
                request_close_choice(app);
            }
        }
    });
}

fn register_runtime_stops(app: &tauri::AppHandle) -> Result<(), std::io::Error> {
    // Only closures for helper-owned runtimes are registered. There is no
    // process ID/name supplied by the renderer or third-party app termination.
    macro_rules! register {
        ($id:literal, $state:ty) => {{
            let app = app.clone();
            shutdown()
                .register_stop($id, move || async move {
                    app.state::<$state>().stop().await;
                })
                .map_err(|_| std::io::Error::other("shutdown_registration_failed"))?;
        }};
    }
    register!(
        "claude_code",
        tool_adapters::claude_code::ClaudeCodeRuntimeState
    );
    register!(
        "claude_desktop",
        tool_adapters::claude_desktop::ClaudeDesktopRuntimeState
    );
    register!("codex_bridge", codex_bridge::CodexBridgeRuntimeState);
    register!("dsh_web", tool_adapters::dsh_web::DshRuntimeState);
    register!("chat_gateway", chat_gateway::ChatGatewayRuntimeState);
    Ok(())
}

fn request_close_choice(app: &tauri::AppHandle) {
    // Persist the intent in memory until acknowledged. A click before the
    // renderer listener is ready is read back when the global host mounts.
    shutdown().request_close_choice();
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
        if window.emit(CLOSE_CHOICE_EVENT, ()).is_err() {
            log::warn!("desktop_shutdown stage=close_choice_emit_failed");
        }
    }
}

#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
enum ExitStatus {
    Exiting,
    FinishingOperation,
    RetryableError,
}

#[derive(Clone, Copy, Serialize)]
struct ExitResponse {
    status: ExitStatus,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ExitState {
    close_requested: bool,
    shutdown: Option<ExitResponse>,
}

fn exit_response() -> ExitResponse {
    ExitResponse {
        status: if shutdown().progress() == ShutdownProgress::FinishingOperation {
            ExitStatus::FinishingOperation
        } else {
            ExitStatus::Exiting
        },
    }
}

#[tauri::command]
fn read_desktop_exit_state() -> ExitState {
    ExitState {
        close_requested: shutdown().close_choice_requested(),
        shutdown: shutdown().is_shutting_down().then(exit_response),
    }
}

#[tauri::command]
fn dismiss_desktop_exit_prompt() {
    shutdown().dismiss_close_choice();
}

#[tauri::command]
fn background_desktop_assistant(app: tauri::AppHandle) -> Result<(), String> {
    let window = app.get_webview_window("main").ok_or("window_unavailable")?;
    window.minimize().map_err(|_| "window_minimize_failed")?;
    shutdown().dismiss_close_choice();
    Ok(())
}

fn emit_exit_progress(app: &tauri::AppHandle, status: ExitStatus) {
    if app
        .emit_to("main", EXIT_PROGRESS_EVENT, ExitResponse { status })
        .is_err()
    {
        log::warn!("desktop_shutdown stage=progress_emit_failed");
    }
}

#[tauri::command]
async fn quit_desktop_assistant(app: tauri::AppHandle) -> ExitResponse {
    if shutdown().request_shutdown() {
        tauri::async_runtime::spawn(async move {
            let mut finishing_notified = false;
            while shutdown()
                .wait_quiescent(tokio::time::Instant::now() + FINISHING_NOTICE_AFTER)
                .await
                == DrainOutcome::FinishingOperation
            {
                if !finishing_notified {
                    emit_exit_progress(&app, ExitStatus::FinishingOperation);
                    log::info!("desktop_shutdown stage=finishing_operation");
                    finishing_notified = true;
                }
            }
            emit_exit_progress(&app, ExitStatus::Exiting);
            let report = shutdown()
                .stop_registered(tokio::time::Instant::now() + RUNTIME_STOP_GRACE)
                .await;
            for stage in &report.cancelled {
                log::warn!("desktop_shutdown stage=runtime_grace_expired runtime={stage}");
            }
            for stage in &report.failed {
                log::warn!("desktop_shutdown stage=runtime_stop_failed runtime={stage}");
            }
            if report.ready_to_exit {
                log::info!("desktop_shutdown stage=exit_requested");
                app.exit(0);
            } else {
                emit_exit_progress(&app, ExitStatus::RetryableError);
            }
        });
    }
    exit_response()
}

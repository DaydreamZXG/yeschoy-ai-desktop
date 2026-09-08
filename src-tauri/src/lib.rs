#[cfg(all(not(debug_assertions), not(feature = "custom-protocol")))]
compile_error!("release builds require the embedded frontend");

#[cfg(feature = "custom-protocol")]
const FRONTEND_MODE: &str = "embedded-custom-protocol";
#[cfg(not(feature = "custom-protocol"))]
const FRONTEND_MODE: &str = "dev-server";
#[cfg(feature = "custom-protocol")]
const FRONTEND_MODE_MARKER: &str = "desktop_shell frontend_mode=embedded-custom-protocol";
#[cfg(not(feature = "custom-protocol"))]
const FRONTEND_MODE_MARKER: &str = "desktop_shell frontend_mode=dev-server";

mod account_finance;
mod account_v2;
mod app_installation;
mod app_update;
mod chat_gateway;
mod claude_bridge;
mod codex_bridge;
mod connection_recovery;
mod connectivity;
mod connectivity_core;
mod desktop_app_discovery;
mod desktop_app_discovery_core;
mod loopback_http;
mod open_connection;
// Client-first OAuth preparation. Deliberately dormant until the server and
// local-bridge bearer/billing contract are deployed and integration-tested.
#[allow(dead_code)]
mod oauth_pkce;
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

#[cfg(not(target_os = "windows"))]
const CLOSE_CHOICE_EVENT: &str = "yeschoy://close-choice";
const EXIT_PROGRESS_EVENT: &str = "yeschoy://exit-progress";

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    log::info!("{FRONTEND_MODE_MARKER} selected={FRONTEND_MODE}");
    let claude_code_runtime = tool_adapters::claude_code::ClaudeCodeRuntimeState::default();
    let resume_claude_code_runtime = claude_code_runtime.clone();
    let claude_runtime = tool_adapters::claude_desktop::ClaudeDesktopRuntimeState::default();
    let resume_claude_runtime = claude_runtime.clone();
    let codex_bridge = codex_bridge::CodexBridgeRuntimeState::default();
    let resume_codex_bridge = codex_bridge.clone();
    let chat_gateway = chat_gateway::ChatGatewayRuntimeState::default();
    let resume_chat_gateway = chat_gateway.clone();
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_updater::Builder::new().build())
        .manage(account_v2::AccountV2State::default())
        .manage(app_installation::AppInstallationState::default())
        .manage(app_update::AppUpdateState::default())
        .manage(tool_activation::ActivationOperationState::default())
        .manage(claude_code_runtime)
        .manage(claude_runtime)
        .manage(codex_bridge)
        .manage(chat_gateway)
        .manage(tool_adapters::dsh_web::DshRuntimeState::default())
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                if shutdown().is_shutting_down() {
                    return;
                }
                #[cfg(target_os = "windows")]
                handle_windows_close(window);
                #[cfg(not(target_os = "windows"))]
                {
                    // Closing asks; neither X nor dismissing the choice cancels
                    // an operation. Only explicit confirmation starts shutdown.
                    request_close_choice(window.app_handle());
                }
            }
        })
        .setup(move |app| {
            window_appearance::initialize(app)?;
            register_runtime_stops(app.handle())?;
            #[cfg(target_os = "windows")]
            register_installer_shutdown_listener(app.handle().clone())?;
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
                // Each helper owns an independent listener. Starting them in
                // sequence allowed one stale keychain/runtime lookup to keep
                // Codex's already-configured loopback gateway offline and its
                // UI stuck on the logo screen.
                tokio::join!(
                    codex_bridge::resume_if_configured(resume_codex_bridge),
                    chat_gateway::resume_if_configured(resume_chat_gateway),
                    tool_adapters::claude_code::resume_if_configured(resume_claude_code_runtime),
                    tool_adapters::claude_desktop::resume_if_configured(resume_claude_runtime),
                );
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            app_update::manage_desktop_update_v1,
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
            tool_activation::cancel_tool_activation_v1,
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
                api.prevent_exit();
                #[cfg(target_os = "windows")]
                {
                    // Windows system-level quit must remain usable even when
                    // WebView2 could not render the application UI.
                    begin_desktop_shutdown(app.clone());
                }
                #[cfg(not(target_os = "windows"))]
                {
                    // Native menu Quit uses the same confirmation and cleanup
                    // path. AppHandle::exit below is allowed only after drain.
                    request_close_choice(app);
                }
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

#[cfg(target_os = "windows")]
fn register_installer_shutdown_listener<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
) -> Result<(), std::io::Error> {
    use std::ffi::c_void;
    use windows::core::w;
    use windows::Win32::{
        Foundation::{CloseHandle, HANDLE, WAIT_OBJECT_0},
        System::Threading::{CreateEventW, WaitForSingleObject, INFINITE},
    };

    // The installer opens this per-session event instead of sending WM_CLOSE.
    // WM_CLOSE is intentionally interactive in the desktop app and therefore
    // cannot mean "install an update now" without a separate trusted signal.
    let event = unsafe {
        CreateEventW(
            None,
            false,
            false,
            w!("Local\\YesChoyDesktopInstallerShutdown_v1"),
        )
    }
    .map_err(|error| {
        std::io::Error::other(format!(
            "could not create the Windows installer shutdown event: {error}"
        ))
    })?;
    // HANDLE wraps a raw pointer and is not Send. The operating-system handle
    // value itself is process-wide, so move its integer representation into the
    // dedicated waiter and reconstruct it there for the single close.
    let event_value = event.0 as usize;
    std::thread::Builder::new()
        .name("yeschoy-installer-shutdown".into())
        .spawn(move || {
            let event = HANDLE(event_value as *mut c_void);
            let signalled = unsafe { WaitForSingleObject(event, INFINITE) } == WAIT_OBJECT_0;
            let _ = unsafe { CloseHandle(event) };
            if !signalled {
                log::warn!("desktop_shutdown stage=installer_event_wait_failed");
                return;
            }
            log::info!("desktop_shutdown stage=installer_requested");
            let shutdown_app = app.clone();
            if app
                .run_on_main_thread(move || {
                    begin_desktop_shutdown(shutdown_app);
                })
                .is_err()
            {
                log::warn!("desktop_shutdown stage=installer_dispatch_failed");
            }
        })
        .map(|_| ())
}

#[cfg(not(target_os = "windows"))]
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

fn background_desktop_window<R: tauri::Runtime>(window: &tauri::Window<R>) -> Result<(), String> {
    window.minimize().map_err(|_| "window_minimize_failed")?;
    shutdown().dismiss_close_choice();
    Ok(())
}

#[tauri::command]
fn background_desktop_assistant(app: tauri::AppHandle) -> Result<(), String> {
    let webview = app.get_webview_window("main").ok_or("window_unavailable")?;
    background_desktop_window(&webview.as_ref().window())
}

fn emit_exit_progress<R: tauri::Runtime>(app: &tauri::AppHandle<R>, status: ExitStatus) {
    if app
        .emit_to("main", EXIT_PROGRESS_EVENT, ExitResponse { status })
        .is_err()
    {
        log::warn!("desktop_shutdown stage=progress_emit_failed");
    }
}

fn begin_desktop_shutdown<R: tauri::Runtime>(app: tauri::AppHandle<R>) -> ExitResponse {
    if shutdown().request_shutdown() {
        tauri::async_runtime::spawn(async move {
            if drain_desktop_runtimes(&app).await {
                log::info!("desktop_shutdown stage=exit_requested");
                app.exit(0);
            } else {
                emit_exit_progress(&app, ExitStatus::RetryableError);
            }
        });
    }
    exit_response()
}

pub(crate) async fn drain_desktop_runtimes<R: tauri::Runtime>(app: &tauri::AppHandle<R>) -> bool {
    let mut finishing_notified = false;
    while shutdown()
        .wait_quiescent(tokio::time::Instant::now() + FINISHING_NOTICE_AFTER)
        .await
        == DrainOutcome::FinishingOperation
    {
        if !finishing_notified {
            emit_exit_progress(app, ExitStatus::FinishingOperation);
            log::info!("desktop_shutdown stage=finishing_operation");
            finishing_notified = true;
        }
    }
    emit_exit_progress(app, ExitStatus::Exiting);
    let report = shutdown()
        .stop_registered(tokio::time::Instant::now() + RUNTIME_STOP_GRACE)
        .await;
    for stage in &report.cancelled {
        log::warn!("desktop_shutdown stage=runtime_grace_expired runtime={stage}");
    }
    for stage in &report.failed {
        log::warn!("desktop_shutdown stage=runtime_stop_failed runtime={stage}");
    }
    report.ready_to_exit
}

#[tauri::command]
async fn quit_desktop_assistant(app: tauri::AppHandle) -> ExitResponse {
    begin_desktop_shutdown(app)
}

#[cfg(target_os = "windows")]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum NativeCloseChoice {
    Exit,
    Background,
    Cancel,
}

#[cfg(target_os = "windows")]
fn windows_close_choice<R: tauri::Runtime>(window: &tauri::Window<R>) -> NativeCloseChoice {
    use windows::core::w;
    use windows::Win32::UI::WindowsAndMessaging::{
        MessageBoxW, IDNO, IDYES, MB_ICONQUESTION, MB_SETFOREGROUND, MB_YESNOCANCEL,
    };

    let owner = window.hwnd().ok();
    // The choice is native by design: it remains usable when WebView2 or the
    // embedded renderer fails before JavaScript can mount an exit dialog.
    let result = unsafe {
        MessageBoxW(
            owner,
            w!("选择“是”完全退出；选择“否”最小化到后台；选择“取消”继续使用。"),
            w!("关闭野菜API？"),
            MB_YESNOCANCEL | MB_ICONQUESTION | MB_SETFOREGROUND,
        )
    };
    if result == IDYES {
        NativeCloseChoice::Exit
    } else if result == IDNO {
        NativeCloseChoice::Background
    } else {
        NativeCloseChoice::Cancel
    }
}

#[cfg(target_os = "windows")]
fn handle_windows_close<R: tauri::Runtime>(window: &tauri::Window<R>) {
    match windows_close_choice(window) {
        NativeCloseChoice::Exit => {
            begin_desktop_shutdown(window.app_handle().clone());
        }
        NativeCloseChoice::Background => {
            if let Err(error) = background_desktop_window(window) {
                log::warn!("desktop_shutdown stage=native_close_failed error={error}");
            }
        }
        NativeCloseChoice::Cancel => {}
    }
}

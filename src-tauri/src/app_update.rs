use std::{
    fs,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Mutex, MutexGuard,
    },
    time::{Duration, Instant},
};

use serde::{Deserialize, Serialize};
use tauri::{Emitter, Manager, State};
use tauri_plugin_updater::{Error as UpdaterError, Update, UpdaterExt};

use crate::{drain_desktop_runtimes, shutdown_coordinator::global as shutdown};

const UPDATE_PROGRESS_EVENT: &str = "yeschoy://update-progress";
const CHECK_TIMEOUT: Duration = Duration::from_secs(12);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(4);
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(30 * 60);
const PROGRESS_INTERVAL: Duration = Duration::from_millis(250);
const RECOVERY_MARKER: &str = "update-install-recovery-v1";

/// 查更新、下载更新这次走哪条线路；`None` 是直连。两件事都排在 `gate` 后面，不会同时改它。
static ROUTE: Mutex<Option<reqwest::Url>> = Mutex::new(None);

fn set_route(route: Option<reqwest::Url>) {
    *ROUTE.lock().unwrap_or_else(|e| e.into_inner()) = route;
}

fn current_route() -> Option<reqwest::Url> {
    ROUTE.lock().unwrap_or_else(|e| e.into_inner()).clone()
}

fn routed_client(builder: reqwest_updater::ClientBuilder) -> reqwest_updater::ClientBuilder {
    let builder = builder.connect_timeout(CONNECT_TIMEOUT);
    match current_route().map(|proxy| reqwest_updater::Proxy::all(proxy.as_str())) {
        Some(Ok(proxy)) => builder.proxy(proxy),
        Some(Err(_)) | None => builder.no_proxy(),
    }
}

fn network_error(error: &UpdaterError) -> bool {
    matches!(
        error,
        UpdaterError::Reqwest(_) | UpdaterError::Network(_) | UpdaterError::ReleaseNotFound
    )
}

/// 跟着系统代理查更新；当前线路连不上就换另一条再查一次。
async fn check_on_routes(
    app: &tauri::AppHandle,
    endpoint: &str,
    public_key: &str,
) -> Result<Option<Update>, UpdaterError> {
    let endpoint: reqwest::Url = endpoint.parse()?;
    let routes = crate::net_route::routes_for(&endpoint);
    let last = routes.len() - 1;
    for (index, route) in routes.into_iter().enumerate() {
        let via_proxy = route.is_some();
        set_route(route);
        let checked = app
            .updater_builder()
            .timeout(CHECK_TIMEOUT)
            .pubkey(public_key)
            .configure_client(routed_client)
            .endpoints(vec![endpoint.clone()])?
            .build()?
            .check()
            .await;
        match checked {
            Err(error) if index < last && network_error(&error) => {
                crate::net_route::route_failed(&endpoint, via_proxy);
            }
            other => return other,
        }
    }
    unreachable!("routes_for always returns at least one route")
}

#[derive(Default)]
pub(crate) struct AppUpdateState {
    gate: tokio::sync::Mutex<()>,
    available: Mutex<Option<Update>>,
}

impl AppUpdateState {
    fn available(&self) -> MutexGuard<'_, Option<Update>> {
        self.available
            .lock()
            .unwrap_or_else(|error| error.into_inner())
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
enum UpdateAction {
    Check,
    Install,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct AppUpdateRequest {
    request_id: String,
    action: UpdateAction,
    expected_version: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
enum UpdatePhase {
    Checking,
    Current,
    Available,
    Downloading,
    Restarting,
    Unavailable,
    Failed,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AppUpdateResponse {
    schema_version: u8,
    request_id: String,
    phase: UpdatePhase,
    current_version: String,
    available_version: String,
    notes: String,
    downloaded_bytes: u64,
    total_bytes: u64,
    reason_code: &'static str,
}

impl AppUpdateResponse {
    fn new(
        request_id: &str,
        phase: UpdatePhase,
        current_version: &str,
        reason_code: &'static str,
    ) -> Self {
        Self {
            schema_version: 1,
            request_id: request_id.to_string(),
            phase,
            current_version: current_version.to_string(),
            available_version: String::new(),
            notes: String::new(),
            downloaded_bytes: 0,
            total_bytes: 0,
            reason_code,
        }
    }

    fn with_available_version(mut self, available_version: &str) -> Self {
        self.available_version = available_version.to_string();
        self
    }

    fn with_notes(mut self, notes: &str) -> Self {
        self.notes = bounded_notes(notes);
        self
    }

    fn with_progress(mut self, downloaded_bytes: u64, total_bytes: u64) -> Self {
        self.downloaded_bytes = downloaded_bytes;
        self.total_bytes = total_bytes;
        self
    }
}

#[tauri::command]
pub(crate) async fn manage_desktop_update_v1(
    app: tauri::AppHandle,
    state: State<'_, AppUpdateState>,
    request: AppUpdateRequest,
) -> Result<AppUpdateResponse, String> {
    let current_version = app.package_info().version.to_string();
    if !valid_request_id(&request.request_id)
        || !valid_expected_version(request.action, &request.expected_version)
    {
        return Ok(AppUpdateResponse::new(
            &request.request_id,
            UpdatePhase::Failed,
            &current_version,
            "invalid_request",
        ));
    }

    Ok(match request.action {
        UpdateAction::Check => check_update(&app, &state, &request.request_id).await,
        UpdateAction::Install => {
            install_update(&app, &state, &request.request_id, &request.expected_version).await
        }
    })
}

async fn check_update(
    app: &tauri::AppHandle,
    state: &AppUpdateState,
    request_id: &str,
) -> AppUpdateResponse {
    let current_version = app.package_info().version.to_string();
    if take_recovery_marker(app) {
        let response = AppUpdateResponse::new(
            request_id,
            UpdatePhase::Failed,
            &current_version,
            "install_failed_restarted",
        );
        emit_progress(app, &response);
        return response;
    }

    let Ok(permit) = shutdown().admit_operation() else {
        return AppUpdateResponse::new(
            request_id,
            UpdatePhase::Failed,
            &current_version,
            "shutting_down",
        );
    };
    let Ok(gate) = permit.cancel_safe(state.gate.lock()).await else {
        return AppUpdateResponse::new(
            request_id,
            UpdatePhase::Failed,
            &current_version,
            "shutting_down",
        );
    };

    let checking = AppUpdateResponse::new(
        request_id,
        UpdatePhase::Checking,
        &current_version,
        "checking",
    );
    emit_progress(app, &checking);

    let channel = match crate::update_channel::compiled_channel() {
        Ok(channel) => channel,
        Err(reason) => {
            *state.available() = None;
            return AppUpdateResponse::new(
                request_id,
                UpdatePhase::Unavailable,
                &current_version,
                if reason == "updater_not_configured" {
                    "updater_not_configured"
                } else {
                    "manifest_invalid"
                },
            );
        }
    };
    let checked = permit
        .cancel_safe(check_on_routes(app, &channel.endpoint, &channel.public_key))
        .await;
    let response = match checked {
        Err(_) => AppUpdateResponse::new(
            request_id,
            UpdatePhase::Failed,
            &current_version,
            "shutting_down",
        ),
        Ok(Err(error)) => {
            *state.available() = None;
            unavailable_response(request_id, &current_version, &error)
        }
        Ok(Ok(None)) => {
            *state.available() = None;
            AppUpdateResponse::new(
                request_id,
                UpdatePhase::Current,
                &current_version,
                "no_update",
            )
        }
        Ok(Ok(Some(update))) => {
            if !channel.accepts(
                &update.raw_json,
                update.download_url.as_str(),
                &update.version,
            ) || update.signature.trim().is_empty()
            {
                *state.available() = None;
                AppUpdateResponse::new(
                    request_id,
                    UpdatePhase::Unavailable,
                    &current_version,
                    "manifest_invalid",
                )
            } else {
                let notes = update.body.as_deref().unwrap_or_default();
                let response = AppUpdateResponse::new(
                    request_id,
                    UpdatePhase::Available,
                    &current_version,
                    "update_available",
                )
                .with_available_version(&update.version)
                .with_notes(notes);
                *state.available() = Some(update);
                response
            }
        }
    };
    drop(gate);
    emit_progress(app, &response);
    response
}

async fn install_update(
    app: &tauri::AppHandle,
    state: &AppUpdateState,
    request_id: &str,
    expected_version: &str,
) -> AppUpdateResponse {
    let current_version = app.package_info().version.to_string();
    let Ok(permit) = shutdown().admit_operation() else {
        return AppUpdateResponse::new(
            request_id,
            UpdatePhase::Failed,
            &current_version,
            "shutting_down",
        );
    };
    let Ok(gate) = permit.cancel_safe(state.gate.lock()).await else {
        return AppUpdateResponse::new(
            request_id,
            UpdatePhase::Failed,
            &current_version,
            "shutting_down",
        );
    };

    let update = state.available().clone();
    let Some(mut update) = update else {
        return AppUpdateResponse::new(
            request_id,
            UpdatePhase::Failed,
            &current_version,
            "update_not_checked",
        );
    };
    if update.version != expected_version {
        return AppUpdateResponse::new(
            request_id,
            UpdatePhase::Failed,
            &current_version,
            "stale_version",
        )
        .with_available_version(&update.version);
    }

    update.timeout = Some(DOWNLOAD_TIMEOUT);
    let downloaded = Arc::new(AtomicU64::new(0));
    let total = Arc::new(AtomicU64::new(0));
    let callback_downloaded = downloaded.clone();
    let callback_total = total.clone();
    let callback_app = app.clone();
    let callback_request_id = request_id.to_string();
    let callback_version = update.version.clone();
    let callback_current = current_version.clone();
    let mut last_emitted = Instant::now() - PROGRESS_INTERVAL;
    let initial = AppUpdateResponse::new(
        request_id,
        UpdatePhase::Downloading,
        &current_version,
        "downloading",
    )
    .with_available_version(&update.version)
    .with_notes(update.body.as_deref().unwrap_or_default());
    emit_progress(app, &initial);

    let mut on_chunk = move |chunk: usize, announced_total: Option<u64>| {
        let observed =
            callback_downloaded.fetch_add(chunk as u64, Ordering::Relaxed) + chunk as u64;
        let announced = announced_total.unwrap_or(0);
        callback_total.store(announced, Ordering::Relaxed);
        if last_emitted.elapsed() >= PROGRESS_INTERVAL {
            last_emitted = Instant::now();
            emit_progress(
                &callback_app,
                &AppUpdateResponse::new(
                    &callback_request_id,
                    UpdatePhase::Downloading,
                    &callback_current,
                    "downloading",
                )
                .with_available_version(&callback_version)
                .with_progress(observed, announced),
            );
        }
    };
    let routes = crate::net_route::routes_for(&update.download_url);
    set_route(routes.first().cloned().flatten());
    let mut downloaded_bytes = permit
        .cancel_safe(update.download(&mut on_chunk, || {}))
        .await;
    if let Ok(Err(error)) = &downloaded_bytes {
        let current = current_route();
        if network_error(error) {
            if let Some(other) = routes.into_iter().find(|route| *route != current) {
                crate::net_route::route_failed(&update.download_url, current.is_some());
                set_route(other);
                downloaded.store(0, Ordering::Relaxed);
                downloaded_bytes = permit
                    .cancel_safe(update.download(&mut on_chunk, || {}))
                    .await;
            }
        }
    }
    let bytes = match downloaded_bytes {
        Err(_) => {
            return AppUpdateResponse::new(
                request_id,
                UpdatePhase::Failed,
                &current_version,
                "shutting_down",
            )
            .with_available_version(&update.version)
            .with_progress(
                downloaded.load(Ordering::Relaxed),
                total.load(Ordering::Relaxed),
            );
        }
        Ok(Err(error)) => {
            let response = AppUpdateResponse::new(
                request_id,
                UpdatePhase::Failed,
                &current_version,
                download_reason(&error),
            )
            .with_available_version(&update.version)
            .with_progress(
                downloaded.load(Ordering::Relaxed),
                total.load(Ordering::Relaxed),
            );
            emit_progress(app, &response);
            return response;
        }
        Ok(Ok(bytes)) => bytes,
    };

    let restarting = AppUpdateResponse::new(
        request_id,
        UpdatePhase::Restarting,
        &current_version,
        "preparing_restart",
    )
    .with_available_version(&update.version)
    .with_progress(
        downloaded.load(Ordering::Relaxed),
        total.load(Ordering::Relaxed),
    );
    emit_progress(app, &restarting);

    // Verified bytes are now process-owned. Release both updater admission and
    // its private gate before requesting shared shutdown, otherwise the drain
    // would wait on the updater itself.
    drop(gate);
    drop(permit);
    if !shutdown().request_shutdown() {
        return AppUpdateResponse::new(
            request_id,
            UpdatePhase::Failed,
            &current_version,
            "shutting_down",
        )
        .with_available_version(&update.version)
        .with_progress(
            downloaded.load(Ordering::Relaxed),
            total.load(Ordering::Relaxed),
        );
    }
    if !drain_desktop_runtimes(app).await {
        let response = AppUpdateResponse::new(
            request_id,
            UpdatePhase::Failed,
            &current_version,
            "shutdown_failed",
        )
        .with_available_version(&update.version)
        .with_progress(
            downloaded.load(Ordering::Relaxed),
            total.load(Ordering::Relaxed),
        );
        emit_progress(app, &response);
        write_recovery_marker(app);
        app.request_restart();
        return response;
    }

    if update.install(&bytes).is_err() {
        log::error!("desktop_update stage=install_failed");
        let response = AppUpdateResponse::new(
            request_id,
            UpdatePhase::Failed,
            &current_version,
            "install_failed",
        )
        .with_available_version(&update.version)
        .with_progress(
            downloaded.load(Ordering::Relaxed),
            total.load(Ordering::Relaxed),
        );
        emit_progress(app, &response);
        write_recovery_marker(app);
        app.request_restart();
        return response;
    }

    // Windows' updater exits while launching the installer. macOS reaches this
    // line after replacing the signed app bundle and must request a restart.
    app.request_restart();
    restarting
}

fn emit_progress(app: &tauri::AppHandle, response: &AppUpdateResponse) {
    if app
        .emit_to("main", UPDATE_PROGRESS_EVENT, response)
        .is_err()
    {
        log::warn!("desktop_update stage=progress_emit_failed");
    }
}

fn unavailable_response(
    request_id: &str,
    current_version: &str,
    error: &UpdaterError,
) -> AppUpdateResponse {
    AppUpdateResponse::new(
        request_id,
        UpdatePhase::Unavailable,
        current_version,
        check_reason(error),
    )
}

fn check_reason(error: &UpdaterError) -> &'static str {
    match error {
        UpdaterError::EmptyEndpoints => "updater_not_configured",
        UpdaterError::UnsupportedArch | UpdaterError::UnsupportedOs => "platform_unsupported",
        UpdaterError::InsecureTransportProtocol => "endpoint_invalid",
        UpdaterError::Serialization(_)
        | UpdaterError::TargetNotFound(_)
        | UpdaterError::UrlParse(_) => "manifest_invalid",
        UpdaterError::Reqwest(_) | UpdaterError::Network(_) | UpdaterError::ReleaseNotFound => {
            "channel_unavailable"
        }
        _ => "check_failed",
    }
}

fn download_reason(error: &UpdaterError) -> &'static str {
    match error {
        UpdaterError::Minisign(_) | UpdaterError::Base64(_) | UpdaterError::SignatureUtf8(_) => {
            "signature_invalid"
        }
        UpdaterError::Reqwest(_) | UpdaterError::Network(_) => "download_failed",
        _ => "artifact_invalid",
    }
}

fn valid_request_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
}

fn valid_expected_version(action: UpdateAction, value: &str) -> bool {
    if action == UpdateAction::Check {
        return value.is_empty();
    }
    !value.is_empty()
        && value.len() <= 40
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'+' | b'-'))
}

fn bounded_notes(value: &str) -> String {
    value.chars().take(600).collect()
}

fn recovery_marker_path(app: &tauri::AppHandle) -> Option<std::path::PathBuf> {
    app.path()
        .app_data_dir()
        .ok()
        .map(|directory| directory.join(RECOVERY_MARKER))
}

fn take_recovery_marker(app: &tauri::AppHandle) -> bool {
    recovery_marker_path(app).is_some_and(|path| {
        if path.is_file() {
            let _ = fs::remove_file(path);
            true
        } else {
            false
        }
    })
}

fn write_recovery_marker(app: &tauri::AppHandle) {
    let Some(path) = recovery_marker_path(app) else {
        return;
    };
    let Some(parent) = path.parent() else {
        return;
    };
    if fs::create_dir_all(parent).is_err()
        || fs::write(path, b"install_failed_restarted\n").is_err()
    {
        log::warn!("desktop_update stage=recovery_marker_failed");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_ids_and_expected_versions_are_closed() {
        assert!(valid_request_id("update-123_A"));
        assert!(!valid_request_id(""));
        assert!(!valid_request_id("../update"));
        assert!(valid_expected_version(UpdateAction::Check, ""));
        assert!(!valid_expected_version(UpdateAction::Check, "0.4.14"));
        assert!(valid_expected_version(UpdateAction::Install, "0.4.14"));
        assert!(!valid_expected_version(UpdateAction::Install, ""));
        assert!(!valid_expected_version(
            UpdateAction::Install,
            "https://example.invalid/update"
        ));
    }

    #[test]
    fn notes_are_unicode_safe_and_bounded() {
        let notes = "菜".repeat(700);
        let bounded = bounded_notes(&notes);
        assert_eq!(bounded.chars().count(), 600);
        assert!(bounded.is_char_boundary(bounded.len()));
    }

    #[test]
    fn updater_state_has_an_independent_gate_and_empty_candidate() {
        let state = AppUpdateState::default();
        assert!(state.gate.try_lock().is_ok());
        assert!(state.available().is_none());
    }
}

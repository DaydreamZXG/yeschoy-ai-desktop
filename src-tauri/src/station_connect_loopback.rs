//! Loopback OAuth + PKCE fallback for station-connect lines (T7).
//!
//! Runs parallel to deep-link connect; does not replace device-code on catalog lines.

use std::{
    sync::Mutex,
    time::{Duration, Instant},
};

use reqwest::header::CONTENT_TYPE;
use serde::Serialize;
use tauri::{AppHandle, Manager, Runtime};

use crate::account_v2::{
    account_project_line_after_connect, fetch_desktop_bootstrap, install_station_connect_session,
    line_id_for_station_connect_origin, loopback_oauth_enabled_for_bootstrap, open_system_browser,
    station_connect_origin_from_line_id, station_connect_projection_failure_reason, AccountV2State,
};
use crate::oauth_pkce::{
    CallbackOutcome, DeviceMetadata, PkceFailure, PreparedAuthorization, SensitiveFormRequest,
};
use crate::station_connect::{
    connect_origin_is_acceptable, emit_station_connect_progress, station_connect_progress_event,
};
const CALLBACK_POLL: Duration = Duration::from_secs(1);

#[derive(Default)]
pub struct StationConnectLoopbackState {
    active: Mutex<Option<ActiveLoopback>>,
}

struct ActiveLoopback {
    prepared: PreparedAuthorization,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StationConnectLoopbackCapabilities {
    available: bool,
    reason_code: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StationConnectLoopbackBeginResult {
    started: bool,
    reason_code: String,
}

#[tauri::command]
pub async fn station_connect_loopback_capabilities_v1(
    line_id: String,
) -> Result<StationConnectLoopbackCapabilities, String> {
    let (available, reason) = loopback_capabilities(&line_id).await;
    Ok(StationConnectLoopbackCapabilities {
        available,
        reason_code: reason.to_owned(),
    })
}

#[tauri::command]
pub async fn station_connect_begin_loopback_oauth_v1<R: Runtime>(
    app: AppHandle<R>,
    line_id: String,
) -> Result<StationConnectLoopbackBeginResult, String> {
    let (available, reason) = loopback_capabilities(&line_id).await;
    if !available {
        return Ok(StationConnectLoopbackBeginResult {
            started: false,
            reason_code: reason.to_owned(),
        });
    }
    let origin = station_connect_origin_from_line_id(&line_id)
        .and_then(connect_origin_is_acceptable)
        .ok_or_else(|| "connect_origin_not_acceptable".to_owned())?;
    let loopback_state = app.state::<StationConnectLoopbackState>();
    let prepared = match PreparedAuthorization::prepare_for_loopback_origin(&origin).await {
        Ok(value) => value,
        Err(error) => {
            return Ok(StationConnectLoopbackBeginResult {
                started: false,
                reason_code: pkce_failure_reason_code(error).to_owned(),
            });
        }
    };
    let browser_url = prepared.browser_url().to_owned();
    {
        let mut active = loopback_state
            .inner()
            .active
            .lock()
            .map_err(|_| "connect_loopback_unavailable".to_owned())?;
        if let Some(previous) = active.take() {
            previous.prepared.cancel();
        }
        *active = Some(ActiveLoopback { prepared });
    }
    if open_system_browser(&browser_url).is_err() {
        let _ = cancel_active_loopback(loopback_state.inner());
        return Ok(StationConnectLoopbackBeginResult {
            started: false,
            reason_code: "connect_browser_open_failed".to_owned(),
        });
    }
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        run_loopback_oauth_flow(app, line_id, origin).await;
    });
    Ok(StationConnectLoopbackBeginResult {
        started: true,
        reason_code: String::new(),
    })
}

#[tauri::command]
pub fn station_connect_cancel_loopback_oauth_v1<R: Runtime>(
    app: AppHandle<R>,
) -> Result<(), String> {
    let state = app.state::<StationConnectLoopbackState>();
    cancel_active_loopback(state.inner()).map_err(|code| code.to_owned())
}

async fn loopback_capabilities(line_id: &str) -> (bool, &'static str) {
    if station_connect_origin_from_line_id(line_id).is_none() {
        return (false, "connect_loopback_unavailable");
    }
    let origin = match account_line_origin_for_loopback(line_id) {
        Ok(value) => value,
        Err(reason) => return (false, reason),
    };
    if loopback_oauth_endpoints_check(&origin).is_err() {
        return (false, "connect_origin_not_acceptable");
    }
    let bootstrap = match fetch_desktop_bootstrap(line_id).await {
        Ok(value) => value,
        Err(_) => return (false, "backend_unavailable"),
    };
    if !loopback_oauth_enabled_for_bootstrap(&bootstrap) {
        return (false, "connect_loopback_unavailable");
    }
    (true, "")
}

fn account_line_origin_for_loopback(line_id: &str) -> Result<String, &'static str> {
    let raw = station_connect_origin_from_line_id(line_id).ok_or("connect_loopback_unavailable")?;
    connect_origin_is_acceptable(raw).ok_or("connect_origin_not_acceptable")
}

fn loopback_oauth_endpoints_check(origin: &str) -> Result<(), PkceFailure> {
    crate::oauth_pkce::loopback_oauth_endpoints(origin).map(|_| ())
}

fn cancel_active_loopback(state: &StationConnectLoopbackState) -> Result<(), &'static str> {
    let mut active = state
        .active
        .lock()
        .map_err(|_| "connect_loopback_unavailable")?;
    if let Some(entry) = active.take() {
        entry.prepared.cancel();
    }
    Ok(())
}

async fn run_loopback_oauth_flow<R: Runtime>(app: AppHandle<R>, line_id: String, origin: String) {
    emit_station_connect_progress(
        &app,
        station_connect_progress_event("connecting", Some(origin.clone()), None, None, false),
    );
    let prepared = {
        let state = app.state::<StationConnectLoopbackState>();
        let mut active = match state.inner().active.lock() {
            Ok(value) => value,
            Err(_) => {
                emit_failure(&app, &origin, None, "connect_loopback_unavailable", false);
                return;
            }
        };
        match active.take() {
            Some(entry) => entry.prepared,
            None => {
                emit_failure(&app, &origin, None, "connect_loopback_unavailable", true);
                return;
            }
        }
    };

    let deadline = Instant::now() + Duration::from_secs(600);
    let metadata = match DeviceMetadata::generate(None) {
        Ok(value) => value,
        Err(error) => {
            emit_failure(&app, &origin, None, pkce_failure_reason_code(error), true);
            return;
        }
    };

    let authorization_code = loop {
        if Instant::now() >= deadline {
            prepared.cancel();
            emit_failure(&app, &origin, None, "connect_loopback_expired", true);
            return;
        }
        match prepared.accept_one(CALLBACK_POLL).await {
            Ok(CallbackOutcome::AuthorizationCode(code)) => break code,
            Ok(CallbackOutcome::NoRequest) => continue,
            Ok(CallbackOutcome::AccessDenied) => {
                emit_failure(&app, &origin, None, "connect_oauth_denied", true);
                return;
            }
            Ok(CallbackOutcome::Cancelled) => {
                emit_failure(&app, &origin, None, "connect_loopback_cancelled", true);
                return;
            }
            Ok(CallbackOutcome::Expired) => {
                emit_failure(&app, &origin, None, "connect_loopback_expired", true);
                return;
            }
            Ok(CallbackOutcome::AlreadyHandled) => {
                emit_failure(&app, &origin, None, "connect_loopback_busy", true);
                return;
            }
            Ok(CallbackOutcome::Rejected(_)) => continue,
            Err(error) => {
                emit_failure(&app, &origin, None, pkce_failure_reason_code(error), true);
                return;
            }
        }
    };

    let exchange = match prepared.build_code_exchange_request(authorization_code, &metadata) {
        Ok(value) => value,
        Err(error) => {
            emit_failure(&app, &origin, None, pkce_failure_reason_code(error), true);
            return;
        }
    };

    match post_oauth_token(&exchange).await {
        Ok(bundle) => install_and_project(&app, &line_id, &origin, bundle).await,
        Err(reason) => emit_failure(&app, &origin, None, reason, true),
    }
}

async fn install_and_project<R: Runtime>(
    app: &AppHandle<R>,
    line_id: &str,
    origin: &str,
    bundle: crate::account_v2::StationConnectAuthBundle,
) {
    let account_state = app.state::<AccountV2State>();
    let synthetic_line = line_id_for_station_connect_origin(origin);
    let line_id = if line_id.is_empty() {
        synthetic_line.as_str()
    } else {
        line_id
    };
    match install_station_connect_session(&account_state, line_id, bundle) {
        Ok(()) => match account_project_line_after_connect(&account_state, line_id).await {
            Ok(projection) if projection.is_signed_in() => {
                emit_station_connect_progress(
                    app,
                    station_connect_progress_event(
                        "signed_in",
                        Some(origin.to_owned()),
                        Some(line_id.to_owned()),
                        None,
                        false,
                    ),
                );
            }
            Ok(projection) => {
                emit_failure(app, origin, Some(line_id), projection.reason_code(), true);
            }
            Err(failure) => {
                let (reason, recoverable) = station_connect_projection_failure_reason(failure);
                emit_failure(app, origin, Some(line_id), reason, recoverable);
            }
        },
        Err(reason) => {
            let recoverable = reason != "connect_auth_contract";
            emit_failure(app, origin, None, reason, recoverable);
        }
    }
}

fn emit_failure<R: Runtime>(
    app: &AppHandle<R>,
    origin: &str,
    line_id: Option<&str>,
    reason: &str,
    recoverable: bool,
) {
    emit_station_connect_progress(
        app,
        station_connect_progress_event(
            "failed",
            Some(origin.to_owned()),
            line_id.map(str::to_owned),
            Some(reason.to_owned()),
            recoverable,
        ),
    );
}

async fn post_oauth_token(
    request: &SensitiveFormRequest,
) -> Result<crate::account_v2::StationConnectAuthBundle, &'static str> {
    let client = crate::account_v2::shared_http_client().map_err(|_| "connect_network_error")?;
    let response = client
        .post(request.endpoint())
        .header(CONTENT_TYPE, "application/x-www-form-urlencoded")
        .body(request.body().to_owned())
        .send()
        .await
        .map_err(map_reqwest_to_reason)?;
    let status = response.status().as_u16();
    let content_type = response
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
        .to_owned();
    let body = response
        .bytes()
        .await
        .map_err(|_| "connect_network_error")?;
    if body.len() > 2 * 1024 * 1024 {
        return Err("connect_response_too_large");
    }
    let oauth = crate::oauth_pkce::parse_token_response(status, &content_type, &body)
        .map_err(map_pkce_token_failure)?;
    oauth_bundle_to_station_connect(&oauth)
}

fn oauth_bundle_to_station_connect(
    bundle: &crate::oauth_pkce::OAuthTokenBundle,
) -> Result<crate::account_v2::StationConnectAuthBundle, &'static str> {
    let refresh = bundle.refresh_token().ok_or("connect_auth_contract")?;
    let refresh_expires = bundle.refresh_expires_in().ok_or("connect_auth_contract")?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        .min(i64::MAX as u64) as i64;
    let access_expires_at = now.saturating_add(bundle.expires_in().min(i64::MAX as u64) as i64);
    let refresh_expires_at = now.saturating_add(refresh_expires.min(i64::MAX as u64) as i64);
    if access_expires_at <= now || refresh_expires_at <= access_expires_at {
        return Err("connect_auth_contract");
    }
    Ok(crate::account_v2::StationConnectAuthBundle {
        access_token: bundle.access_token().to_owned(),
        token_type: "Bearer".to_owned(),
        access_expires_at,
        refresh_token: refresh.to_owned(),
        refresh_expires_at,
        session_id: bundle.session_id().to_owned(),
    })
}

fn map_pkce_token_failure(error: PkceFailure) -> &'static str {
    match error {
        PkceFailure::ResponseTooLarge => "connect_response_too_large",
        PkceFailure::InvalidCredential => "connect_server_rejected",
        PkceFailure::InvalidResponse => "connect_invalid_response",
        other => pkce_failure_reason_code(other),
    }
}

fn map_reqwest_to_reason(error: reqwest::Error) -> &'static str {
    if error.is_timeout() {
        "connect_network_timeout"
    } else {
        "connect_network_error"
    }
}

pub(crate) fn pkce_failure_reason_code(error: PkceFailure) -> &'static str {
    match error {
        PkceFailure::InvalidOrigin => "connect_origin_not_acceptable",
        PkceFailure::ListenerUnavailable => "connect_loopback_unavailable",
        PkceFailure::InvalidRedirect | PkceFailure::InvalidAuthorizationInput => {
            "connect_invalid_response"
        }
        PkceFailure::InvalidDeviceMetadata => "connect_auth_contract",
        PkceFailure::InvalidCredential => "connect_server_rejected",
        PkceFailure::InvalidResponse => "connect_invalid_response",
        PkceFailure::ResponseTooLarge => "connect_response_too_large",
        PkceFailure::RandomUnavailable => "connect_loopback_unavailable",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pkce_failures_map_to_connect_reason_codes() {
        assert_eq!(
            pkce_failure_reason_code(PkceFailure::InvalidOrigin),
            "connect_origin_not_acceptable"
        );
        assert_eq!(
            pkce_failure_reason_code(PkceFailure::ListenerUnavailable),
            "connect_loopback_unavailable"
        );
    }
}

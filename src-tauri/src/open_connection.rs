//! Launch-only reuse. No account HTTP client, config writer or token issuance.
use serde::{Deserialize, Serialize};

use crate::{
    claude_bridge::ClaudeTransport,
    connection_recovery::{self, Store},
    tool_activation::ACTIVATION_LOCK,
    tool_adapters::{
        self, claude_code, claude_desktop, codex_desktop, desktop_lifecycle, dsh_web, pi,
        terminal_launch, workbuddy, AdapterFailure,
    },
    tool_credentials::{self, CredentialFailure, ToolCredential},
};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OpenRequest {
    request_id: String,
    tool_id: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenResponse {
    request_id: String,
    schema_version: u8,
    tool_id: String,
    status: &'static str,
    /// Why `status` came out the way it did. For `launch_failed` this is the
    /// adapter's own `LaunchError` code, which used to be discarded by
    /// `map_err(|_| "launch_failed")` — leaving every launch failure, from a
    /// missing workspace to a terminal that will not spawn, sharing one
    /// sentence. For every other status it repeats the status.
    reason_code: &'static str,
}

/// `(status, reason_code)`. Statuses that carry no finer cause use `st`.
type OpenFailure = (&'static str, &'static str);

fn st(status: &'static str) -> OpenFailure {
    (status, status)
}

/// Keep the adapter's launch reason; fall back to the status when it has none.
fn launch_failure(error: AdapterFailure) -> OpenFailure {
    match error {
        AdapterFailure::LaunchError(reason) => ("launch_failed", reason),
        _ => st("launch_failed"),
    }
}

fn valid_request(request: &OpenRequest) -> bool {
    !request.request_id.is_empty()
        && request.request_id.len() <= 100
        && request
            .request_id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_'))
        && matches!(
            request.tool_id.as_str(),
            "claude_code" | "claude_desktop" | "codex_desktop" | "pi" | "dsh_web" | "workbuddy"
        )
}

pub(crate) fn validate_settings(
    home: &std::path::Path,
    tool: &str,
    credential: &ToolCredential,
) -> Result<(), AdapterFailure> {
    if credential.has_model_set() {
        let models = credential.model_ids();
        // Each adapter owns its endpoint choice. Claude Code's catalog path
        // resolves to its local compatibility pass-through inside prepare.
        let origin = credential.origin.clone();
        let local = credential.local_gateway_token.as_deref();
        let ct = if credential.claude_transport.as_deref() == Some("chat_bridge") {
            ClaudeTransport::ChatBridge
        } else {
            ClaudeTransport::DirectAnthropic
        };
        let xt = if credential.codex_transport.as_deref() == Some("chat_bridge") {
            codex_desktop::CodexTransport::ChatBridge
        } else {
            codex_desktop::CodexTransport::DirectResponses
        };
        return match tool {
            "claude_code" => claude_code::prepare_catalog(
                home,
                &origin,
                &credential.model_id,
                ct,
                local,
                &models,
            )?
            .validate_existing(),
            "claude_desktop" => {
                claude_desktop::prepare_catalog(home, &credential.model_id, local, &models)?
                    .validate_existing()
            }
            "codex_desktop" => codex_desktop::prepare_catalog(
                home,
                &origin,
                &credential.model_id,
                xt,
                Some(credential.upstream_key()),
                &models,
            )?
            .validate_existing(),
            "pi" => pi::prepare_catalog(home, &origin, &credential.model_id, &models)?
                .validate_existing(),
            "dsh_web" => dsh_web::prepare_catalog(home, &origin, &credential.model_id, &models)?
                .validate_existing(),
            "workbuddy" => workbuddy::prepare_catalog(home, credential)?.validate_existing(),
            _ => Err(AdapterFailure::UnsupportedProfile),
        };
    }
    // Preparation stages bytes in memory only. Validation is read-only and
    // shared with post-write readback, so it tolerates unrelated user settings.
    match tool {
        "claude_code" => {
            let transport = if credential.claude_transport.as_deref() == Some("chat_bridge") {
                if credential.local_gateway_token.is_none() {
                    return Err(AdapterFailure::SecureStorageUnavailable);
                }
                ClaudeTransport::ChatBridge
            } else {
                ClaudeTransport::DirectAnthropic
            };
            claude_code::prepare(
                home,
                &credential.origin,
                &credential.model_id,
                transport,
                credential.local_gateway_token.as_deref(),
            )?
            .validate_existing()
        }
        "pi" => pi::prepare(home, &credential.origin, &credential.model_id)?.validate_existing(),
        "claude_desktop" => {
            if credential.local_gateway_token.is_none() {
                return Err(AdapterFailure::SecureStorageUnavailable);
            }
            claude_desktop::prepare(
                home,
                &credential.model_id,
                credential.local_gateway_token.as_deref(),
            )?
            .validate_existing()
        }
        "codex_desktop" => {
            let transport = if credential.codex_transport.as_deref() == Some("chat_bridge") {
                codex_desktop::CodexTransport::ChatBridge
            } else {
                codex_desktop::CodexTransport::DirectResponses
            };
            codex_desktop::prepare(home, &credential.origin, &credential.model_id, transport)?
                .validate_existing()
        }
        "dsh_web" => {
            dsh_web::prepare(home, &credential.origin, &credential.model_id)?.validate_existing()
        }
        "workbuddy" => workbuddy::prepare_catalog(home, credential)?.validate_existing(),
        _ => Err(AdapterFailure::UnsupportedProfile),
    }
}

fn existing_credential(tool: &str) -> Result<ToolCredential, &'static str> {
    let store = Store::open(false).map_err(|_| "secure_storage_unavailable")?;
    if let Some(store) = store {
        if store
            .load(tool)
            .map_err(|_| "secure_storage_unavailable")?
            .is_some_and(|r| r.pending)
        {
            return Err("recovery_pending");
        }
    }
    tool_credentials::load(tool).map_err(|error| match error {
        CredentialFailure::Missing => "not_connected",
        _ => "secure_storage_unavailable",
    })
}

#[tauri::command]
pub async fn open_tool_connection_v1(
    claude_code: tauri::State<'_, claude_code::ClaudeCodeRuntimeState>,
    claude: tauri::State<'_, claude_desktop::ClaudeDesktopRuntimeState>,
    codex: tauri::State<'_, codex_desktop::CodexRuntimeState>,
    dsh: tauri::State<'_, dsh_web::DshRuntimeState>,
    request: OpenRequest,
) -> Result<OpenResponse, String> {
    if !valid_request(&request) {
        return Err("invalid_open_request".into());
    }
    let result = async {
        let permit = crate::shutdown_coordinator::global()
            .admit_operation()
            .map_err(|_| st("busy"))?;
        let _guard = ACTIVATION_LOCK.try_lock().map_err(|_| st("busy"))?;
        let _process_guard = connection_recovery::operation_lock().map_err(|_| st("busy"))?;
        let credential = existing_credential(&request.tool_id).map_err(st)?;
        let home = tool_adapters::user_home().ok_or(st("settings_changed"))?;
        let installation = permit
            .cancel_safe(tool_adapters::resolve_preferred_installation(
                &request.tool_id,
            ))
            .await
            .map_err(|_| st("busy"))?
            .map_err(|_| st("tool_not_found"))?;
        validate_settings(&home, &request.tool_id, &credential)
            .map_err(|_| st("settings_changed"))?;
        if permit.is_cancelled() {
            return Err(st("busy"));
        }
        // Start helper-owned runtimes before launching their clients so a
        // first request cannot race the loopback listener.
        let launched = permit
            .cancel_safe(async {
                match request.tool_id.as_str() {
                    "claude_desktop" => {
                        claude.start(credential).await.map_err(launch_failure)?;
                        if permit.is_cancelled() {
                            return Err(st("busy"));
                        }
                        Ok(claude_desktop::launch(&installation.path))
                    }
                    "codex_desktop" => {
                        // Codex's config.toml points at `127.0.0.1:15731`; the
                        // bridge must be listening before the app is opened,
                        // exactly as activation does in `start_local_adapter`.
                        // Opening used to skip this, so a Codex opened from
                        // here after an app restart had nothing to talk to.
                        codex.start(credential).await.map_err(launch_failure)?;
                        if permit.is_cancelled() {
                            return Err(st("busy"));
                        }
                        Ok(codex_desktop::launch(&installation.path))
                    }
                    "dsh_web" => {
                        Ok(
                            dsh_web::open_existing(&dsh, &installation, credential.upstream_key())
                                .await,
                        )
                    }
                    "claude_code" => {
                        claude_code
                            .start(credential)
                            .await
                            .map_err(launch_failure)?;
                        if permit.is_cancelled() {
                            return Err(st("busy"));
                        }
                        Ok(
                            terminal_launch::launch_async(&installation, &request.tool_id, &home)
                                .await,
                        )
                    }
                    "pi" => {
                        if permit.is_cancelled() {
                            return Err(st("busy"));
                        }
                        Ok(
                            terminal_launch::launch_async(&installation, &request.tool_id, &home)
                                .await,
                        )
                    }
                    "workbuddy" => Ok(desktop_lifecycle::open_unless_running(
                        "workbuddy",
                        &installation.path,
                    )
                    .await),
                    _ => unreachable!(),
                }
            })
            .await
            .map_err(|_| st("busy"))??;
        launched.map_err(launch_failure)?;
        Ok::<_, OpenFailure>("opened")
    }
    .await;
    let (status, reason_code) = match result {
        Ok(status) => (status, status),
        Err(failure) => failure,
    };
    Ok(OpenResponse {
        request_id: request.request_id,
        schema_version: 2,
        tool_id: request.tool_id,
        status,
        reason_code,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_boundary_rejects_commands_paths_and_secret_fields() {
        let valid = r#"{"requestId":"open-1","toolId":"dsh_web"}"#;
        assert!(valid_request(&serde_json::from_str(valid).unwrap()));
        for tool in [
            "../../app",
            "https://example.com",
            "dsh_web;id",
            "powershell",
            "terminal",
        ] {
            assert!(!valid_request(&OpenRequest {
                request_id: "open-1".into(),
                tool_id: tool.into()
            }));
        }
        assert!(serde_json::from_str::<OpenRequest>(
            r#"{"requestId":"open-1","toolId":"dsh_web","apiKey":"secret"}"#
        )
        .is_err());
        let response = serde_json::to_value(OpenResponse {
            request_id: "open-1".into(),
            schema_version: 2,
            tool_id: "dsh_web".into(),
            status: "opened",
            reason_code: "opened",
        })
        .unwrap();
        // Still a closed shape: exactly the five declared fields, nothing the
        // credential or the installation path could ride out on.
        let object = response.as_object().unwrap();
        assert_eq!(object.len(), 5);
        assert_eq!(object["reasonCode"], "opened");
    }

    /// Every launch failure used to arrive as the bare status `launch_failed`,
    /// because `map_err(|_| "launch_failed")` dropped the adapter's own code.
    /// The renderer could then only ever say one sentence — "请确认应用可以手动
    /// 打开" — whether the workspace directory had gone missing, the terminal
    /// refused to spawn, or the app would not exit. These are the codes that
    /// have to survive the trip.
    #[test]
    fn launch_failures_keep_the_adapter_reason() {
        for reason in [
            "terminal_launch_failed",
            "terminal_unavailable",
            "workspace_unavailable",
            "invalid_launch_target",
            "launch_target_missing",
            "desktop_launch_exit_failed",
            "desktop_launch_wait_failed",
        ] {
            assert_eq!(
                launch_failure(AdapterFailure::LaunchError(reason)),
                ("launch_failed", reason),
            );
        }
        // An adapter failure with no reason of its own still reports honestly
        // rather than inventing one.
        assert_eq!(
            launch_failure(AdapterFailure::LaunchFailed),
            ("launch_failed", "launch_failed"),
        );
        // Statuses that genuinely have no finer cause repeat themselves, so the
        // renderer never has to treat `reasonCode` as optional.
        assert_eq!(st("busy"), ("busy", "busy"));
        assert_eq!(st("not_connected"), ("not_connected", "not_connected"));
    }

    #[test]
    fn all_six_open_targets_use_only_the_closed_native_request() {
        for tool in [
            "claude_code",
            "claude_desktop",
            "codex_desktop",
            "pi",
            "dsh_web",
            "workbuddy",
        ] {
            assert!(valid_request(&OpenRequest {
                request_id: "open-fixture".into(),
                tool_id: tool.into()
            }));
            for field in ["command", "path", "args", "url", "apiKey"] {
                let mut value = serde_json::json!({"requestId":"open-fixture","toolId":tool});
                value[field] = serde_json::json!("never-accepted");
                assert!(serde_json::from_value::<OpenRequest>(value).is_err());
            }
        }
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn launch_preflight_is_read_only_and_preserves_unrelated_settings() {
        let home = tool_adapters::common::temporary_working_directory("open-preflight").unwrap();
        let credential = ToolCredential {
            api_key: "synthetic-test-key-only".into(),
            origin: "https://yeschoy.com".into(),
            model_id: "test-model".into(),
            local_gateway_token: Some(format!("ycg-{}", "a".repeat(64))),
            codex_transport: None,
            claude_transport: None,
            models: vec![],
        };
        let mut prepared = claude_desktop::prepare(
            &home,
            &credential.model_id,
            credential.local_gateway_token.as_deref(),
        )
        .unwrap();
        prepared.commit().unwrap();
        let (normal, _, _, _) = claude_desktop::current_paths(&home).unwrap();
        let mut config: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&normal).unwrap()).unwrap();
        config["theme"] = "my-preference".into();
        let changed = serde_json::to_vec_pretty(&config).unwrap();
        std::fs::write(&normal, &changed).unwrap();
        assert!(validate_settings(&home, "claude_desktop", &credential).is_ok());
        assert_eq!(std::fs::read(&normal).unwrap(), changed);
        config["deploymentMode"] = "consumer".into();
        let other_provider = serde_json::to_vec(&config).unwrap();
        std::fs::write(&normal, &other_provider).unwrap();
        assert!(validate_settings(&home, "claude_desktop", &credential).is_err());
        assert_eq!(std::fs::read(&normal).unwrap(), other_provider);
        std::fs::remove_dir_all(home).unwrap();
    }
}

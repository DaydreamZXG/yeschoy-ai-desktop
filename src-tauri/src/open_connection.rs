//! Launch-only reuse. No account HTTP client, config writer or token issuance.
use serde::{Deserialize, Serialize};

use crate::{
    codex_bridge::CodexBridgeRuntimeState,
    connection_recovery::{self, Store},
    tool_activation::ACTIVATION_LOCK,
    tool_adapters::{self, claude_desktop, codex_desktop, dsh_web, AdapterFailure},
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
            "claude_desktop" | "codex_desktop" | "dsh_web"
        )
}

fn validate_settings(
    home: &std::path::Path,
    tool: &str,
    credential: &ToolCredential,
) -> Result<(), AdapterFailure> {
    // Preparation stages bytes in memory only. Validation is read-only and
    // shared with post-write readback, so it tolerates unrelated user settings.
    match tool {
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
    claude: tauri::State<'_, claude_desktop::ClaudeDesktopRuntimeState>,
    codex: tauri::State<'_, CodexBridgeRuntimeState>,
    dsh: tauri::State<'_, dsh_web::DshRuntimeState>,
    request: OpenRequest,
) -> Result<OpenResponse, String> {
    if !valid_request(&request) {
        return Err("invalid_open_request".into());
    }
    let result = async {
        let _guard = ACTIVATION_LOCK.try_lock().map_err(|_| "busy")?;
        let _process_guard = connection_recovery::operation_lock().map_err(|_| "busy")?;
        let credential = existing_credential(&request.tool_id)?;
        let home = tool_adapters::user_home().ok_or("settings_changed")?;
        let installation = tool_adapters::resolve_preferred_installation(&request.tool_id)
            .await
            .map_err(|_| "tool_not_found")?;
        validate_settings(&home, &request.tool_id, &credential).map_err(|_| "settings_changed")?;
        let launched = match request.tool_id.as_str() {
            "claude_desktop" => {
                claude
                    .start(credential)
                    .await
                    .map_err(|_| "launch_failed")?;
                claude_desktop::launch(&installation.path)
            }
            "codex_desktop" => {
                if credential.codex_transport.as_deref() == Some("chat_bridge") {
                    codex.ensure_started().await.map_err(|_| "launch_failed")?;
                }
                codex_desktop::launch(&installation.path)
            }
            "dsh_web" => dsh_web::open_existing(&dsh, &installation, &credential.api_key).await,
            _ => unreachable!(),
        };
        launched.map_err(|_| "launch_failed")?;
        Ok::<_, &'static str>("opened")
    }
    .await;
    Ok(OpenResponse {
        request_id: request.request_id,
        schema_version: 1,
        tool_id: request.tool_id,
        status: result.unwrap_or_else(|status| status),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_boundary_rejects_commands_paths_and_secret_fields() {
        let valid = r#"{"requestId":"open-1","toolId":"dsh_web"}"#;
        assert!(valid_request(&serde_json::from_str(valid).unwrap()));
        for tool in ["pi", "../../app", "https://example.com", "dsh_web;id"] {
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
            schema_version: 1,
            tool_id: "dsh_web".into(),
            status: "opened",
        })
        .unwrap();
        assert_eq!(response.as_object().unwrap().len(), 4);
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

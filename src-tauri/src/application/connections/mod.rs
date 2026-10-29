//! 本地连接状态与恢复用例。只读检查汇总每个工具的回执、凭据与当前配置是否一致；
//! 恢复顺序为「回执标 pending → 还原配置 → 凭据/登录态/历史 → 清理回执」，
//! 任一步失败都保留可重试的 pending 回执。阻塞存储全部在阻塞线程池执行，并与
//! 激活共用同一把写锁，避免交叉写入。依赖以 `ConnectionContext` 显式传入。
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::account_v2::{native_session_access, native_session_epoch, AccountV2State};
use crate::application::activation::lifecycle::{stop_helper_runtime, HelperRuntimes};
use crate::application::activation::session::activation_lock;
use crate::application::activation::tokens::{needs_background, revoke_owned_tool_tokens};
use crate::connection_recovery::{self, Receipt, Store};
use crate::connectivity_core::request_id_is_valid;
use crate::domain::model::ModelBinding;
use crate::shutdown_coordinator;
use crate::tool_adapters::{
    self, claude_code, claude_desktop, codex_desktop, desktop_lifecycle, dsh_web, AdapterFailure,
};
use crate::tool_credentials::{self, CredentialFailure};

const CONNECTION_LOCK_WAIT_TIMEOUT: Duration = Duration::from_secs(8);
// Match the renderer's existing inspection budget, including lock wait and I/O.
const CONNECTION_INSPECTION_TIMEOUT: Duration = Duration::from_secs(15);

const CONNECTION_TOOLS: [&str; 8] = [
    "claude_code",
    "claude_desktop",
    "codex_desktop",
    "pi",
    "opencode",
    "dsh_web",
    "workbuddy",
    "dsh_desktop",
];

/// Called only by the exit owner after admission closes and active operations
/// drain. Blocking storage work stays off the UI/async executor. No remote keys
/// are revoked, and missing originals fail visibly instead of inventing them.
pub(crate) async fn restore_connection_for_exit(tool: &'static str) -> Result<(), ()> {
    let _guard = tokio::time::timeout(CONNECTION_LOCK_WAIT_TIMEOUT, activation_lock().lock())
        .await
        .map_err(|_| ())?;
    let needs_restore = tokio::time::timeout(
        CONNECTION_INSPECTION_TIMEOUT,
        tokio::task::spawn_blocking(move || {
            let store = Store::open(false).map_err(|_| ())?;
            let projection = inspect_connection(tool, Ok(store.as_ref()));
            match projection.state {
                "not_connected" => Ok(false),
                "unavailable" | "legacy" => Err(()),
                _ if projection.restore_mode != "original" => Err(()),
                _ => Ok(true),
            }
        }),
    )
    .await
    .map_err(|_| ())?
    .map_err(|_| ())??;
    if !needs_restore {
        return Ok(());
    }
    crate::exit_restore::after_normal_close(
        async {
            if desktop_lifecycle::requires_reload(tool) {
                // Multiple installations are ambiguous: never guess which to close.
                let installation = tool_adapters::resolve_installation(tool, "")
                    .await
                    .map_err(|_| ())?;
                desktop_lifecycle::quit_for_exit_restore(tool, &installation.path)
                    .await
                    .map_err(|_| ())?;
            }
            Ok(())
        },
        || async {
            tokio::task::spawn_blocking(move || {
                let _process_guard = connection_recovery::operation_lock().map_err(|_| ())?;
                let store = Store::open(false).map_err(|_| ())?.ok_or(())?;
                let Some(mut record) = store.load(tool).map_err(|_| ())? else {
                    return Ok(());
                };
                if !record.original_known {
                    return Err(());
                }
                record.pending = true;
                store.save(&record).map_err(|_| ())?;
                connection_recovery::restore_files(&record).map_err(|_| ())?;
                if tool == "codex_desktop" {
                    let home = tool_adapters::user_home().ok_or(())?;
                    crate::codex_history_takeover::restore(&home).map_err(|_| ())?;
                }
                // 和 `manage_tool_connections_v1` 的 restore 分支同款：接入时挪走
                // 的 claude.ai 登录态一起搬回去。回执里不记「挪没挪」，而没备份
                // 时 `restore` 本来就什么都不做，所以对 claude_code 一律调。
                if tool == "claude_code" {
                    crate::claude_login_takeover::restore().map_err(|_| ())?;
                }
                tool_credentials::restore(tool, None).map_err(|_| ())?;
                store.remove(tool).map_err(|_| ())?;
                crate::request_diagnostics::clear(tool);
                Ok(())
            })
            .await
            .map_err(|_| ())?
        },
    )
    .await
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ConnectionRequest {
    request_id: String,
    operation: String,
    tool_id: String,
    /// PRD 9.1: deleting a tool's local configuration must separately ask
    /// whether its dedicated keys are revoked too. Older frontends omit the
    /// field and keep the historical revoke-by-default behavior.
    revoke_tokens: Option<bool>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionProjection {
    tool_id: String,
    state: &'static str,
    model_id: String,
    line_id: String,
    billing_group: String,
    updated_at_epoch_ms: u64,
    restore_mode: &'static str,
    requires_background: bool,
    reason_code: &'static str,
    models: Vec<ModelBinding>,
    #[serde(skip_serializing_if = "Option::is_none")]
    last_request: Option<crate::request_diagnostics::RequestObservation>,
}

impl ConnectionProjection {
    fn empty(tool: &str) -> Self {
        Self {
            tool_id: tool.into(),
            state: "not_connected",
            model_id: String::new(),
            line_id: String::new(),
            billing_group: String::new(),
            updated_at_epoch_ms: 0,
            restore_mode: "none",
            requires_background: false,
            reason_code: "not_connected",
            models: vec![],
            last_request: None,
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionResponse {
    request_id: String,
    schema_version: u8,
    status: &'static str,
    connections: Vec<ConnectionProjection>,
    reason_code: &'static str,
}

fn inspect_connection(
    tool: &str,
    store: std::result::Result<Option<&Store>, ()>,
) -> ConnectionProjection {
    // Keychain/Credential Manager reads can block. Reuse one read per tool for
    // both legacy detection and the model projection.
    let credential = tool_credentials::load(tool);
    let mut projection = ConnectionProjection::empty(tool);
    let stored = match store {
        Ok(Some(store)) => store.load(tool),
        Ok(None) => Ok(None),
        Err(()) => Err(connection_recovery::Failure::Storage),
    };
    match stored {
        Ok(Some(record)) => {
            projection.state = if record.pending {
                "recovery_pending"
            } else if connection_recovery::configuration_matches(&record)
                && !connection_recovery::requires_gateway_migration(tool, &record)
            {
                "connected"
            } else {
                "changed"
            };
            projection.restore_mode = if record.original_known {
                "original"
            } else {
                "remove_yeschoy"
            };
            projection.model_id = record.receipt.model_id;
            projection.line_id = record.receipt.line_id;
            projection.billing_group = record.receipt.billing_group;
            projection.updated_at_epoch_ms = record.receipt.updated_at_epoch_ms;
            projection.requires_background = record.receipt.requires_background;
            projection.reason_code = projection.state;
        }
        Err(_) => {
            projection.state = "unavailable";
            projection.reason_code = "recovery_storage_unavailable";
        }
        Ok(None) => match &credential {
            Ok(credential) => {
                projection.state = "legacy";
                projection.restore_mode = "remove_yeschoy";
                projection.model_id = credential.model_id.clone();
                projection.line_id = if credential.origin == "https://api.yeschoy.com" {
                    "global_accelerated"
                } else {
                    "mainland_optimized"
                }
                .into();
                projection.requires_background = needs_background(tool, credential);
                projection.reason_code = "original_settings_unavailable";
            }
            Err(CredentialFailure::Missing) => {}
            Err(_) => {
                projection.state = "unavailable";
                projection.reason_code = "secure_storage_unavailable";
            }
        },
    }
    if let Ok(credential) = &credential {
        projection.models = credential
            .models
            .iter()
            .map(|m| ModelBinding {
                model_id: m.model_id.clone(),
                billing_group: m.billing_group.clone(),
            })
            .collect();
        if projection.models.is_empty()
            && !projection.model_id.is_empty()
            && !projection.billing_group.is_empty()
        {
            projection.models.push(ModelBinding {
                model_id: projection.model_id.clone(),
                billing_group: projection.billing_group.clone(),
            });
        }
        if projection.state == "changed"
            && credential.has_model_set()
            && tool_adapters::user_home().is_some_and(|home| {
                crate::open_connection::validate_settings(&home, tool, credential).is_ok()
            })
        {
            projection.state = "connected";
            projection.reason_code = "connected";
        }
        projection.requires_background = needs_background(tool, credential);
    }
    projection.last_request = crate::request_diagnostics::latest(tool);
    projection
}

pub(crate) async fn inspect_on_worker<T: Send + 'static>(
    lock: &'static tokio::sync::Mutex<()>,
    deadline: Duration,
    inspect: impl FnOnce() -> T + Send + 'static,
) -> Result<T, &'static str> {
    tokio::time::timeout(deadline, async move {
        let guard = tokio::time::timeout(CONNECTION_LOCK_WAIT_TIMEOUT, lock.lock())
            .await
            .map_err(|_| "connection_operation_busy")?;
        // Same blocking-worker boundary as the existing provider commands. Keep
        // the write lock in the worker even if the caller times out: an OS read
        // cannot be cancelled safely, and must not overlap configuration writes.
        tauri::async_runtime::spawn_blocking(move || {
            let _guard = guard;
            inspect()
        })
        .await
        .map_err(|_| "connection_inspection_failed")
    })
    .await
    .map_err(|_| "connection_inspect_timed_out")?
}

fn inspect_connections(store: Result<Option<&Store>, ()>) -> Vec<ConnectionProjection> {
    inspect_connections_with(|tool| inspect_connection(tool, store))
}

fn inspect_connections_with(
    mut inspect: impl FnMut(&str) -> ConnectionProjection,
) -> Vec<ConnectionProjection> {
    CONNECTION_TOOLS
        .iter()
        .map(|tool| {
            // A single adapter must not discard the other six local results.
            // No writes/recovery take place here; preserve an unknown state
            // for the failed tool and never expose the raw panic payload.
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| inspect(tool))).unwrap_or_else(
                |_| {
                    let mut projection = ConnectionProjection::empty(tool);
                    projection.state = "unavailable";
                    projection.reason_code = "connection_inspection_failed";
                    projection
                },
            )
        })
        .collect()
}

fn legacy_paths(tool: &str) -> Result<Vec<std::path::PathBuf>, AdapterFailure> {
    let home = tool_adapters::user_home()
        .ok_or(AdapterFailure::ConfigurationFailed("home_unavailable"))?;
    Ok(match tool {
        "claude_code" => vec![home.join(".claude/settings.json")],
        "codex_desktop" => {
            let config_dir = codex_desktop::config_dir(&home)?;
            vec![
                config_dir.join("config.toml"),
                config_dir.join("yeschoy-model-catalog.json"),
            ]
        }
        "claude_desktop" => {
            let (a, b, c, d) = claude_desktop::current_paths(&home)?;
            vec![a, b, c, d]
        }
        "pi" => vec![
            home.join(".pi/agent/models.json"),
            home.join(".pi/agent/settings.json"),
        ],
        // DSH 0.2 moves `settings.yaml` into the web profile's patch and
        // renames it, so a connection from before the journal may now live
        // there instead. Only files that exist are taken.
        "dsh_web" => {
            let root = dsh_web::dsh_home(&home, std::env::var_os("DSH_HOME"))?;
            vec![
                root.join("settings.yaml"),
                root.join("profiles/web/cordis.patch.yml"),
            ]
        }
        "workbuddy" => vec![home.join(".workbuddy/models.json")],
        // Support began with the recovery journal: nothing legacy.
        "opencode" => Vec::new(),
        // Desktop support began with the recovery journal: nothing legacy.
        "dsh_desktop" => Vec::new(),
        _ => return Err(AdapterFailure::UnsupportedProfile),
    })
}

fn legacy_recovery(store: &Store, tool: &str) -> Result<Option<connection_recovery::Record>, ()> {
    let credential = match tool_credentials::load(tool) {
        Ok(value) => value,
        Err(CredentialFailure::Missing) => return Ok(None),
        Err(_) => return Err(()),
    };
    let mut changes = Vec::new();
    for path in legacy_paths(tool).map_err(|_| ())? {
        let before = tool_adapters::common::snapshot(&path).map_err(|_| ())?;
        if let Some(bytes) = &before {
            let transaction = tool_adapters::common::FileTransaction::stage_with_snapshot(
                path,
                before.clone(),
                bytes.clone(),
            )
            .map_err(|_| ())?;
            changes.extend_from_slice(transaction.changes());
        }
    }
    if changes.is_empty() {
        return Ok(None);
    }
    let receipt = Receipt {
        tool_id: tool.into(),
        model_id: credential.model_id.clone(),
        line_id: if credential.origin == "https://api.yeschoy.com" {
            "global_accelerated"
        } else {
            "mainland_optimized"
        }
        .into(),
        billing_group: String::new(),
        updated_at_epoch_ms: 0,
        requires_background: needs_background(tool, &credential),
    };
    store
        .begin(receipt, &changes, Some(&credential))
        .map(Some)
        .map_err(|_| ())
}

/// 连接状态用例可用的原生依赖，全部以引用传入。
pub(crate) struct ConnectionContext<'a> {
    pub(crate) account_state: &'a AccountV2State,
    pub(crate) claude_code_runtime: &'a claude_code::ClaudeCodeRuntimeState,
    pub(crate) claude_runtime: &'a claude_desktop::ClaudeDesktopRuntimeState,
    pub(crate) codex_runtime: &'a codex_desktop::CodexRuntimeState,
    pub(crate) dsh_runtime: &'a dsh_web::DshRuntimeState,
}

pub(crate) async fn manage_tool_connections(
    context: &ConnectionContext<'_>,
    request: ConnectionRequest,
) -> Result<ConnectionResponse, String> {
    let account_state = context.account_state;
    let claude_code_runtime = context.claude_code_runtime;
    let claude_runtime = context.claude_runtime;
    let codex_runtime = context.codex_runtime;
    let dsh_runtime = context.dsh_runtime;
    if !request_id_is_valid(&request.request_id)
        || !matches!(request.operation.as_str(), "inspect" | "restore")
        || !(CONNECTION_TOOLS.contains(&request.tool_id.as_str())
            || (request.operation == "inspect" && request.tool_id.is_empty()))
    {
        return Err("invalid_connection_request".into());
    }
    let permit = shutdown_coordinator::global()
        .admit_operation()
        .map_err(|_| "assistant_shutting_down")?;
    if request.operation == "inspect" {
        let started = std::time::Instant::now();
        let result = permit
            .cancel_safe(inspect_on_worker(
                activation_lock(),
                CONNECTION_INSPECTION_TIMEOUT,
                || {
                    let store = Store::open(false);
                    inspect_connections(match &store {
                        Ok(store) => Ok(store.as_ref()),
                        Err(_) => Err(()),
                    })
                },
            ))
            .await
            .unwrap_or(Err("assistant_shutting_down"));
        let connections = result.map_err(|code| {
            // Never log raw storage errors, paths, account data or credentials.
            log::warn!(
                "connection_inspection request_id={} code={} elapsed_ms={}",
                request.request_id,
                code,
                started.elapsed().as_millis()
            );
            code.to_string()
        })?;
        for connection in connections.iter().filter(|c| c.state == "unavailable") {
            log::warn!(
                "connection_inspection request_id={} code=connection_partial_unavailable tool_id={} reason={}",
                request.request_id,
                connection.tool_id,
                connection.reason_code
            );
        }
        return Ok(ConnectionResponse {
            request_id: request.request_id,
            schema_version: 2,
            status: "ok",
            connections,
            reason_code: "local_state",
        });
    }
    let _guard = tokio::time::timeout(
        CONNECTION_LOCK_WAIT_TIMEOUT,
        permit.cancel_safe(activation_lock().lock()),
    )
    .await
    .map_err(|_| "connection_operation_busy")?
    .map_err(|_| "assistant_shutting_down")?;
    let _process_guard = if request.operation == "restore" {
        Some(connection_recovery::operation_lock().map_err(|_| "connection_operation_busy")?)
    } else {
        None
    };
    let store = Store::open(request.operation == "restore");
    let mut status = "ok";
    let mut reason = "local_state";
    if request.operation == "restore" {
        let credential_for_cleanup = tool_credentials::load(&request.tool_id).ok();
        let session_epoch = native_session_epoch(account_state).ok();
        let result = (|| -> Result<bool, ()> {
            let store = store.as_ref().map_err(|_| ())?.as_ref().ok_or(())?;
            let record = store.load(&request.tool_id).map_err(|_| ())?;
            let mut record = match record {
                Some(record) => Some(record),
                None => legacy_recovery(store, &request.tool_id)?,
            };
            let mut kept = false;
            if let Some(record) = &mut record {
                record.pending = true;
                store.save(record).map_err(|_| ())?;
                kept = connection_recovery::restore_files(record).map_err(|_| ())?;
                if request.tool_id == "codex_desktop" {
                    let home = tool_adapters::user_home().ok_or(())?;
                    crate::codex_history_takeover::restore(&home).map_err(|_| ())?;
                }
                // 接入时挪走的 claude.ai 登录态搬回来。没接管过就什么都不做。
                if request.tool_id == "claude_code" {
                    crate::claude_login_takeover::restore().map_err(|_| ())?;
                }
            }
            Ok(kept)
        })();
        if let Ok(kept) = result {
            // Stop only this helper-owned runtime, never the third-party app or
            // another provider. A running app may need to reopen its settings.
            let _ = permit
                .cancel_safe(async {
                    stop_helper_runtime(
                        &request.tool_id,
                        &HelperRuntimes {
                            claude_code: claude_code_runtime,
                            claude: claude_runtime,
                            codex: codex_runtime,
                            dsh: dsh_runtime,
                        },
                    )
                    .await;
                })
                .await;
            // Remote key handling comes before the local credential cleanup:
            // when revocation is requested but cannot complete, the credential
            // and the pending receipt (already saved above) stay in place so
            // the restore action remains available as the retry entry.
            let revoke_tokens = request.revoke_tokens.unwrap_or(true);
            let token_cleanup_complete = if !revoke_tokens {
                true
            } else {
                match (credential_for_cleanup.as_ref(), session_epoch) {
                    (Some(credential), Some(epoch)) => {
                        match native_session_access(
                            account_state,
                            if credential.origin == "https://api.yeschoy.com" {
                                "global_accelerated"
                            } else {
                                "mainland_optimized"
                            },
                            epoch,
                        )
                        .await
                        {
                            Ok((origin, access_token)) => {
                                revoke_owned_tool_tokens(
                                    &origin,
                                    &access_token,
                                    &request.tool_id,
                                    credential,
                                )
                                .await
                            }
                            Err(_) => false,
                        }
                    }
                    (None, _) => true,
                    _ => false,
                }
            };
            if revoke_tokens && !token_cleanup_complete {
                status = if kept {
                    "restored_with_changes"
                } else {
                    "restored"
                };
                reason = "local_settings_restored_token_cleanup_pending";
            } else {
                // Complete the local file/key transaction.
                let cleaned = tool_credentials::restore(&request.tool_id, None).is_ok()
                    && store
                        .as_ref()
                        .ok()
                        .and_then(|s| s.as_ref())
                        .is_some_and(|s| s.remove(&request.tool_id).is_ok());
                crate::request_diagnostics::clear(&request.tool_id);
                if cleaned {
                    status = if kept {
                        "restored_with_changes"
                    } else {
                        "restored"
                    };
                    reason = if !revoke_tokens {
                        "local_settings_restored_token_kept"
                    } else if kept {
                        "later_changes_preserved"
                    } else {
                        "local_settings_restored"
                    };
                } else {
                    status = "recovery_failed";
                    reason = "recovery_cleanup_failed";
                }
            }
        } else {
            status = "recovery_failed";
            reason = "recovery_not_completed";
        }
    }
    let connections = inspect_connections(match &store {
        Ok(store) => Ok(store.as_ref()),
        Err(_) => Err(()),
    });
    Ok(ConnectionResponse {
        request_id: request.request_id,
        schema_version: 2,
        status,
        connections,
        reason_code: reason,
    })
}

#[cfg(test)]
#[path = "tests.rs"]
mod tests;

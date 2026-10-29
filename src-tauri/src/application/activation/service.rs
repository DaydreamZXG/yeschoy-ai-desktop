//! 一键接入用例：验证请求 → 定位应用 → 处理未完成事务 → 取会话与模型能力 →
//! 铸受限密钥 → 补偿点 → 写配置并读回 → 启动并观测 → 提交或整体回滚。
//!
//! 这个模块不持有 Tauri 状态本身：依赖以 `ActivationContext` 显式传入，因此接入顺序
//! 和失败出口能在测试里按需替换，而不是只能通过真实命令触发。
use super::capability::{apply_codex_probes, probe_requested, validate_models};
use super::error::ActivationFailure;
use super::lifecycle::{
    codex_history_on_worker, commit_codex_history_on_worker, finalize_after_start,
    needs_claude_login_consent, open_configured_adapter, prepare_adapter, recover_after_stop,
    restart_previous_runtime, restore_displaced_login, restore_files_and_credential,
    start_local_adapter, stop_helper_runtime, DesktopReloadGuard, HelperRuntimes,
};
use super::request::{request_is_valid, ToolActivationRequest};
use super::result::{SkippedBinding, ToolActivationProjection};
use super::session::{activation_lock, emit_activation_progress, ActivationOperationState};
use super::tokens::{
    acquire_token, credential_for_models, delete_created_tokens, group_hash, needs_background,
    now_epoch_ms, retire_superseded_tokens, surviving_bindings, token_failure_is_global,
};
use crate::account_v2::{
    ensure_session_epoch, native_session_access, native_session_epoch, AccountV2State,
    NativeSessionFailure,
};
use crate::connection_recovery::{self, Receipt, Store};
use crate::shutdown_coordinator;
use crate::tool_adapters::{
    self, claude_code, claude_desktop, codex_desktop, desktop_lifecycle, dsh_web, AdapterFailure,
};
use crate::tool_credentials::{self, CredentialFailure, ToolCredential};

/// 一次接入可用的原生依赖。全部以引用传入，服务本身不读取全局状态。
///
/// `R` 是 Tauri 运行时：生产走 `Wry`，端到端回归走 `tauri::test::MockRuntime`。
/// 只有把运行时变成参数，编排体的同一份代码才能被测试完整执行到，而不是
/// 只能在测试里把它拆成一片片单独验证。
pub(crate) struct ActivationContext<'a, R: tauri::Runtime = tauri::Wry> {
    pub(crate) app: &'a tauri::AppHandle<R>,
    pub(crate) activation_state: &'a ActivationOperationState,
    pub(crate) account_state: &'a AccountV2State,
    pub(crate) installation_state: &'a crate::app_installation::AppInstallationState,
    pub(crate) claude_code_runtime: &'a claude_code::ClaudeCodeRuntimeState,
    pub(crate) claude_runtime: &'a claude_desktop::ClaudeDesktopRuntimeState,
    pub(crate) codex_runtime: &'a codex_desktop::CodexRuntimeState,
    pub(crate) dsh_runtime: &'a dsh_web::DshRuntimeState,
}

pub(crate) async fn configure_desktop_tool<R: tauri::Runtime>(
    context: &ActivationContext<'_, R>,
    request: ToolActivationRequest,
) -> Result<ToolActivationProjection, String> {
    let app = context.app;
    let activation_state = context.activation_state;
    let account_state = context.account_state;
    let installation_state = context.installation_state;
    let claude_code_runtime = context.claude_code_runtime;
    let claude_runtime = context.claude_runtime;
    let codex_runtime = context.codex_runtime;
    let dsh_runtime = context.dsh_runtime;
    if !request_is_valid(&request) {
        return Err("invalid_tool_activation_request".into());
    }
    let registration = activation_state
        .begin(&request.request_id)
        .map_err(|_| "activation_already_running")?;
    let cancelled = || {
        ActivationFailure::ConfigurationFailed(if registration.cancellation.is_requested() {
            "activation_cancelled"
        } else {
            "assistant_shutting_down"
        })
        .projection(&request)
    };
    emit_activation_progress(app, &request, "queued", 0);
    // 排障时间线：只记录阶段与耗时，不记录请求内容、密钥或路径。
    let started = std::time::Instant::now();
    macro_rules! stage {
        ($name:literal) => {
            log::info!(
                "activation stage={} tool={} restart={} elapsed_ms={}",
                $name,
                request.tool_id,
                request.restart_running_app,
                started.elapsed().as_millis()
            );
        };
    }
    stage!("enter");
    let permit = match shutdown_coordinator::global().admit_operation() {
        Ok(p) => p,
        Err(_) => return Ok(cancelled()),
    };
    let is_cancelled = || permit.is_cancelled() || registration.cancellation.is_requested();
    let session_epoch = if let Some(id) = request.installation_job_id.as_deref() {
        let intent = crate::app_installation::Intent {
            line_id: request.line_id.clone(),
            model_id: request.model_id.clone(),
            billing_group: request.billing_group.clone(),
            models: request.bindings(),
        };
        match installation_state.claim(
            id,
            &request.tool_id,
            &request.installation_id,
            &intent,
            account_state,
        ) {
            Ok(epoch) => epoch,
            Err(reason) => {
                return Ok(ActivationFailure::ConfigurationFailed(reason).projection(&request))
            }
        }
    } else {
        match native_session_epoch(account_state) {
            Ok(epoch) => epoch,
            Err(_) => return Ok(ActivationFailure::ServerUnavailable.projection(&request)),
        }
    };
    let _activation_guard = match permit.cancel_safe(activation_lock().lock()).await {
        Ok(g) => g,
        Err(_) => return Ok(cancelled()),
    };
    stage!("lock");
    if is_cancelled() {
        return Ok(cancelled());
    }
    emit_activation_progress(app, &request, "checking_application", 1);
    let _process_guard = match connection_recovery::operation_lock() {
        Ok(g) => g,
        Err(_) => {
            return Ok(
                ActivationFailure::ConfigurationFailed("recovery_pending").projection(&request)
            )
        }
    };
    let installation = match permit
        .cancel_safe(tool_adapters::resolve_installation(
            &request.tool_id,
            &request.installation_id,
        ))
        .await
    {
        Ok(Ok(v)) => v,
        Ok(Err(e)) => return Ok(ActivationFailure::Adapter(e).projection(&request)),
        Err(_) => return Ok(cancelled()),
    };
    // claude.ai 的登录态存在钥匙串里，写配置文件碰不到它。两份凭据并存时
    // Claude Code 用 claude.ai 那份，中转从头到尾没被用上 —— 野菜显示
    // 「设置已完成」，用户在 Claude Code 里看到的却是账号被限制。
    //
    // 探测不弹窗（只查属性不查数据），所以没登录 claude.ai 的人不会平白
    // 收到任何东西。真要挪它才会弹系统授权框，而那个框必须先有解释，
    // 所以这里先回一个待确认状态，由界面把话说清楚。
    if needs_claude_login_consent(
        &request.tool_id,
        request.displace_claude_login,
        crate::claude_login_takeover::present,
    ) {
        return Ok(ToolActivationProjection::new(
            &request,
            "application_running",
            "claude_login_conflict",
        ));
    }
    if desktop_lifecycle::requires_reload(&request.tool_id) {
        match desktop_lifecycle::is_running(&request.tool_id, &installation.path).await {
            Ok(true) if !request.restart_running_app => {
                return Ok(ToolActivationProjection::new(
                    &request,
                    "application_running",
                    "save_work_before_restart",
                ))
            }
            Ok(_) => {}
            Err(error) => return Ok(ActivationFailure::Adapter(error).projection(&request)),
        }
    }
    let mut reload_guard = DesktopReloadGuard::new(&request, &installation);
    let mut recovery = match Store::open(false) {
        Ok(store) => store,
        Err(_) => {
            return Ok(
                ActivationFailure::ConfigurationFailed("recovery_storage_unavailable")
                    .projection(&request),
            )
        }
    };
    if let Some(store) = recovery.as_ref() {
        let pending = match store.load(&request.tool_id) {
            Ok(record) => record.filter(|record| record.pending),
            Err(_) => {
                return Ok(
                    ActivationFailure::ConfigurationFailed("recovery_storage_unavailable")
                        .projection(&request),
                )
            }
        };
        if let Some(pending) = pending {
            // A pending record may restore application files. Desktop apps
            // must be stopped before that local mutation, using the same
            // explicit save-work consent as a normal update.
            if desktop_lifecycle::requires_reload(&request.tool_id) {
                if request.restart_running_app {
                    match permit
                        .cancel_safe(desktop_lifecycle::quit_for_reconfigure(
                            &request.tool_id,
                            &installation.path,
                        ))
                        .await
                    {
                        Ok(Ok(closed)) => reload_guard.record_normal_quit(closed),
                        Ok(Err(AdapterFailure::LaunchError("graceful_restart_required"))) => {
                            return Ok(ToolActivationProjection::new(
                                &request,
                                "application_running",
                                "graceful_restart_required",
                            ));
                        }
                        Ok(Err(error)) => {
                            return Ok(ActivationFailure::Adapter(error).projection(&request));
                        }
                        Err(_) => return Ok(cancelled()),
                    }
                } else {
                    match desktop_lifecycle::is_running(&request.tool_id, &installation.path).await
                    {
                        Ok(true) => {
                            return Ok(ToolActivationProjection::new(
                                &request,
                                "application_running",
                                "save_work_before_restart",
                            ));
                        }
                        Ok(false) => {}
                        Err(error) => {
                            return Ok(ActivationFailure::Adapter(error).projection(&request));
                        }
                    }
                }
            }
            let reopen_after_recovery = reload_guard.closed;
            reload_guard.disarm();
            if permit
                .cancel_safe(stop_helper_runtime(
                    &request.tool_id,
                    &HelperRuntimes {
                        claude_code: claude_code_runtime,
                        claude: claude_runtime,
                        codex: codex_runtime,
                        dsh: dsh_runtime,
                    },
                ))
                .await
                .is_err()
            {
                return Ok(cancelled());
            }
            let previous = pending.rollback_credential().cloned();
            match store.recover_pending(&request.tool_id, || match previous.as_ref() {
                Some(credential) => tool_credentials::store(&request.tool_id, credential).is_ok(),
                None => tool_credentials::restore(&request.tool_id, None).is_ok(),
            }) {
                Ok(true) => {
                    if request.tool_id == "codex_desktop" {
                        let Some(home) = tool_adapters::user_home() else {
                            return Ok(ActivationFailure::ConfigurationFailed(
                                "codex_history_takeover_recovery_failed",
                            )
                            .projection(&request));
                        };
                        if crate::codex_history_takeover::restore(&home).is_err() {
                            return Ok(ActivationFailure::ConfigurationFailed(
                                "codex_history_takeover_recovery_failed",
                            )
                            .projection(&request));
                        }
                    }
                    crate::request_diagnostics::clear(&request.tool_id);
                    if request.tool_id == "claude_code" {
                        if let Some(previous) =
                            previous.clone().filter(ToolCredential::has_model_set)
                        {
                            if claude_code_runtime.start(previous).await.is_err() {
                                reload_guard.disarm();
                                return Ok(ActivationFailure::ConfigurationFailed(
                                    "previous_connection_runtime_failed",
                                )
                                .projection(&request));
                            }
                        }
                    } else if request.tool_id == "claude_desktop" {
                        if let Some(previous) = previous {
                            if claude_runtime.start(previous).await.is_err() {
                                reload_guard.disarm();
                                return Ok(ActivationFailure::ConfigurationFailed(
                                    "previous_connection_runtime_failed",
                                )
                                .projection(&request));
                            }
                        }
                    }
                    reload_guard.record_normal_quit(reopen_after_recovery);
                }
                Ok(false) => {}
                Err(failure) => {
                    use connection_recovery::PendingRecoveryFailure as RecoveryFailure;
                    let reason = match failure {
                        RecoveryFailure::Load => "recovery_storage_unavailable",
                        RecoveryFailure::Files => "configuration_rollback_failed",
                        RecoveryFailure::Credential => "credential_restore_failed",
                        RecoveryFailure::Receipt => "recovery_receipt_failed",
                    };
                    return Ok(ActivationFailure::ConfigurationFailed(reason).projection(&request));
                }
            }
        }
    }
    // Session refresh and token issuance may mutate the account. Never drop
    // these futures on shutdown; finish the bounded request then clean up.
    if is_cancelled() {
        return Ok(cancelled());
    }
    emit_activation_progress(app, &request, "authenticating", 2);
    let (origin, access_token) =
        match native_session_access(account_state, &request.line_id, session_epoch).await {
            Ok(v) => v,
            Err(NativeSessionFailure::SignedOut) => {
                return Ok(ActivationFailure::SignedOut.projection(&request))
            }
            Err(NativeSessionFailure::ServerUnavailable) => {
                return Ok(ActivationFailure::ServerUnavailable.projection(&request))
            }
            Err(NativeSessionFailure::AccountChanged) => {
                return Ok(
                    ActivationFailure::ConfigurationFailed("account_changed").projection(&request)
                )
            }
        };
    // A previous run may have minted a key and died before its local commit.
    // Release only keys this device's ledger recorded and the relay still lists;
    // best-effort, and never on the critical path of this activation.
    super::tokens::release_abandoned_issuances(&origin, &access_token).await;
    let bindings = request.bindings();
    if is_cancelled() {
        return Ok(cancelled());
    }
    emit_activation_progress(app, &request, "checking_models", 3);
    let transports = match permit
        .cancel_safe(validate_models(
            &origin,
            &access_token,
            &bindings,
            &request.tool_id,
        ))
        .await
    {
        Ok(Ok(v)) => v,
        Ok(Err(e)) => return Ok(e.projection(&request)),
        Err(_) => return Ok(cancelled()),
    };
    let credential_before = match tool_credentials::snapshot(&request.tool_id) {
        Ok(v) => v,
        Err(_) => {
            return Ok(
                ActivationFailure::Adapter(AdapterFailure::SecureStorageUnavailable)
                    .projection(&request),
            )
        }
    };
    let previous_record = match tool_credentials::load(&request.tool_id) {
        Ok(v) => Some(v),
        Err(CredentialFailure::Missing) if credential_before.is_none() => None,
        Err(_) => {
            return Ok(
                ActivationFailure::Adapter(AdapterFailure::SecureStorageUnavailable)
                    .projection(&request),
            )
        }
    };
    stage!("credentials");
    let mut leases = Vec::new();
    // 铸不出密钥的分组。**不再拖垮整单** —— 以前第一个失败的分组就让整次接入
    // 回滚，用户只看到一句「暂时无法从野菜API获取接入信息」，而那句话背后有
    // 十七个不同的原因，日志里也没有任何一行说是哪个分组。于是重试一百次还是
    // 同一个分组失败，人就卡在这儿了。
    let mut skipped: Vec<SkippedBinding> = Vec::new();
    emit_activation_progress(app, &request, "securing_access", 4);
    for binding in &bindings {
        if leases
            .iter()
            .any(|(group, _)| group == &binding.billing_group)
            || skipped
                .iter()
                .any(|s| s.billing_group == binding.billing_group)
        {
            continue;
        }
        if is_cancelled() || ensure_session_epoch(account_state, session_epoch).is_err() {
            delete_created_tokens(&origin, &access_token, &leases).await;
            return Ok(if is_cancelled() {
                cancelled()
            } else {
                ActivationFailure::ConfigurationFailed("account_changed").projection(&request)
            });
        }
        let mut group_model_ids = bindings
            .iter()
            .filter(|candidate| candidate.billing_group == binding.billing_group)
            .map(|candidate| candidate.model_id.clone())
            .collect::<Vec<_>>();
        group_model_ids.sort_unstable();
        match acquire_token(
            &origin,
            &access_token,
            &request.tool_id,
            &binding.billing_group,
            &group_model_ids,
        )
        .await
        {
            Ok(lease) => leases.push((binding.billing_group.clone(), lease)),
            // 会话没了 / 助手在退出：不是这个分组的事，停下来，把已经铸出的
            // 密钥删掉，按整单失败报 —— 而不是记成一个「跳过」。
            Err(e) if token_failure_is_global(&e) => {
                delete_created_tokens(&origin, &access_token, &leases).await;
                return Ok(e.projection(&request));
            }
            Err(e) => {
                // 记下来，其余分组照常。分组名是中转返回的数据，不进日志；
                // 哈希（密钥名里本来就带这一截）足够把日志和后台的密钥对上，
                // 模型 id 与 `codex_probe` 那几行的口径一致。
                let reason = e.projection(&request).reason_code;
                log::warn!(
                    "activation stage=token_group_skipped tool={} group_hash={:08x} models={} reason={reason}",
                    request.tool_id,
                    group_hash(&binding.billing_group),
                    group_model_ids.join(","),
                );
                skipped.extend(
                    bindings
                        .iter()
                        .filter(|candidate| candidate.billing_group == binding.billing_group)
                        .map(|candidate| SkippedBinding {
                            model_id: candidate.model_id.clone(),
                            billing_group: candidate.billing_group.clone(),
                            reason_code: reason,
                        }),
                );
            }
        }
    }
    let leased_groups = leases
        .iter()
        .map(|(group, _)| group.clone())
        .collect::<Vec<_>>();
    let (bindings, transports) = match surviving_bindings(
        bindings,
        transports,
        &leased_groups,
        &skipped,
        &request.model_id,
    ) {
        Ok(pair) => pair,
        Err(blocker) => {
            delete_created_tokens(&origin, &access_token, &leases).await;
            return Ok(ToolActivationProjection::new(
                &request,
                "server_unavailable",
                blocker.reason_code,
            )
            .with_skipped(skipped));
        }
    };
    let mut transports = transports;
    if is_cancelled() || ensure_session_epoch(account_state, session_epoch).is_err() {
        delete_created_tokens(&origin, &access_token, &leases).await;
        return Ok(if is_cancelled() {
            cancelled()
        } else {
            ActivationFailure::ConfigurationFailed("account_changed").projection(&request)
        });
    }
    if probe_requested(&request.tool_id, request.probe_codex_transport) {
        // 接入时探一次真实请求，把结论落进凭据 —— 桥之后按模型读它。
        // 放在这里是因为要用 lease 铸出来的受限 key：`/v1/*` 不收账户令牌，
        // 而 lease 到上一步才存在。
        //
        // 只在用户于设置里显式打开时才探：探测成功的那次会真的生成十几个
        // token，而产品承诺「不发收费的测试请求」。关掉时每个模型停在直连
        // ——也就是探测拿不到结论时的同一个终点，功能不降级，只是桥接路由
        // 对这个模型不再自动判定。
        //
        // 不另发进度事件：这一步用的就是刚铸出来的 key，本来就属于
        // `securing_access`（上一条已经发过）。为它复用 `checking_models`
        // 会让界面从「获取访问权限」倒退回「检查模型」，看着像出错了。
        if permit
            .cancel_safe(apply_codex_probes(
                &origin,
                &bindings,
                &leases,
                &mut transports,
            ))
            .await
            .is_err()
        {
            delete_created_tokens(&origin, &access_token, &leases).await;
            return Ok(cancelled());
        }
    }
    stage!("tokens_start");
    let credential = match credential_for_models(
        &request,
        &origin,
        &bindings,
        &transports,
        &leases,
        previous_record.as_ref(),
    ) {
        Ok(v) => v,
        Err(e) => {
            delete_created_tokens(&origin, &access_token, &leases).await;
            return Ok(e.projection(&request));
        }
    };
    stage!("tokens_ready");
    let default_transport = bindings
        .iter()
        .position(|m| m.model_id == request.model_id)
        .and_then(|i| transports[i]);
    let recovery = match recovery.take() {
        Some(store) => store,
        None => match Store::open(true) {
            Ok(Some(store)) => store,
            _ => {
                delete_created_tokens(&origin, &access_token, &leases).await;
                return Ok(
                    ActivationFailure::ConfigurationFailed("recovery_storage_unavailable")
                        .projection(&request),
                );
            }
        },
    };
    if desktop_lifecycle::requires_reload(&request.tool_id) {
        if request.restart_running_app {
            match permit
                .cancel_safe(desktop_lifecycle::quit_for_reconfigure(
                    &request.tool_id,
                    &installation.path,
                ))
                .await
            {
                Ok(Ok(closed)) => reload_guard.record_normal_quit(closed),
                Ok(Err(AdapterFailure::LaunchError("graceful_restart_required"))) => {
                    delete_created_tokens(&origin, &access_token, &leases).await;
                    return Ok(ToolActivationProjection::new(
                        &request,
                        "application_running",
                        "graceful_restart_required",
                    ));
                }
                Ok(Err(error)) => {
                    delete_created_tokens(&origin, &access_token, &leases).await;
                    return Ok(ActivationFailure::Adapter(error).projection(&request));
                }
                Err(_) => {
                    delete_created_tokens(&origin, &access_token, &leases).await;
                    return Ok(cancelled());
                }
            }
        } else {
            match desktop_lifecycle::is_running(&request.tool_id, &installation.path).await {
                Ok(true) => {
                    delete_created_tokens(&origin, &access_token, &leases).await;
                    return Ok(ToolActivationProjection::new(
                        &request,
                        "application_running",
                        "save_work_before_restart",
                    ));
                }
                Ok(false) => {}
                Err(error) => {
                    delete_created_tokens(&origin, &access_token, &leases).await;
                    return Ok(ActivationFailure::Adapter(error).projection(&request));
                }
            }
        }
    }
    stage!("lifecycle");
    // Desktop apps can flush their own config while closing. Snapshot only
    // after the graceful shutdown has completed; otherwise the transaction
    // mistakes that legitimate final write for a competing configuration
    // tool and sends the user into a false "higher precedence" failure.
    emit_activation_progress(app, &request, "preparing_settings", 5);
    // A pre-0.4.19 receipt may currently show `yeschoy`, while its encrypted
    // original snapshot still records the previous provider identifier. Reuse
    // that identifier so upgrading users get the compatibility fix without
    // first restoring or touching config.toml themselves.
    let codex_provider_hint = if request.tool_id == "codex_desktop" {
        match recovery.load(&request.tool_id) {
            Ok(Some(record)) if record.original_known && !record.pending => record
                .files
                .iter()
                .find(|file| {
                    file.path
                        .file_name()
                        .is_some_and(|name| name == "config.toml")
                })
                .and_then(|file| codex_desktop::provider_id_from_snapshot(file.before.as_deref())),
            Ok(_) => None,
            Err(_) => {
                delete_created_tokens(&origin, &access_token, &leases).await;
                return Ok(
                    ActivationFailure::ConfigurationFailed("recovery_storage_unavailable")
                        .projection(&request),
                );
            }
        }
    } else {
        None
    };
    let mut prepared = match prepare_adapter(
        &request,
        &credential,
        default_transport,
        codex_provider_hint.as_deref(),
    ) {
        Ok(v) => v,
        Err(e) => {
            delete_created_tokens(&origin, &access_token, &leases).await;
            return Ok(ActivationFailure::Adapter(e).projection(&request));
        }
    };
    stage!("prepared");
    let receipt = Receipt {
        tool_id: request.tool_id.clone(),
        model_id: request.model_id.clone(),
        line_id: request.line_id.clone(),
        billing_group: request.billing_group.clone(),
        updated_at_epoch_ms: now_epoch_ms(),
        requires_background: needs_background(&request.tool_id, &credential),
    };
    let mut recovery_record =
        match recovery.begin(receipt, prepared.changes(), previous_record.as_ref()) {
            Ok(v) => v,
            Err(e) => {
                delete_created_tokens(&origin, &access_token, &leases).await;
                return Ok(ActivationFailure::ConfigurationFailed(
                    if e == connection_recovery::Failure::Changed {
                        "recovery_pending"
                    } else {
                        "recovery_storage_unavailable"
                    },
                )
                .projection(&request));
            }
        };
    // 同意已经拿到了（`displace_claude_login`），这里才真的动钥匙串 —— 系统
    // 会弹一次授权框。挪走而不是删掉，`manage_tool_connections_v1` 的 restore
    // 分支会把它搬回去；在那之前若接入失败或被取消，回滚时就得搬回去
    // （`restore_displaced_login`），所以记住这次到底有没有挪。
    let mut login_displaced = false;
    if request.tool_id == "claude_code" && request.displace_claude_login {
        match tokio::task::spawn_blocking(crate::claude_login_takeover::take_over).await {
            Ok(Ok(outcome)) => {
                login_displaced = outcome == crate::claude_login_takeover::Outcome::Displaced;
            }
            // 用户在系统框上点了拒绝。不能当成成功继续 —— 那样会写完配置宣布
            // 「设置已完成」，而 Claude Code 依旧走 claude.ai，正是这次要修的
            // 那个不报错的错误。
            Ok(Err(crate::claude_login_takeover::Failure::NotPermitted)) => {
                let _ = recovery.abandon(&recovery_record);
                delete_created_tokens(&origin, &access_token, &leases).await;
                return Ok(
                    ActivationFailure::ConfigurationFailed("claude_login_not_released")
                        .projection(&request),
                );
            }
            Ok(Err(_)) | Err(_) => {
                let _ = recovery.abandon(&recovery_record);
                delete_created_tokens(&origin, &access_token, &leases).await;
                return Ok(
                    ActivationFailure::ConfigurationFailed("claude_login_takeover_failed")
                        .projection(&request),
                );
            }
        }
    }
    let mut codex_history = if request.tool_id == "codex_desktop" {
        let Some(home) = tool_adapters::user_home() else {
            let _ = recovery.abandon(&recovery_record);
            delete_created_tokens(&origin, &access_token, &leases).await;
            return Ok(
                ActivationFailure::ConfigurationFailed("home_unavailable").projection(&request)
            );
        };
        let Some(provider_id) = prepared.codex_provider_id() else {
            let _ = recovery.abandon(&recovery_record);
            delete_created_tokens(&origin, &access_token, &leases).await;
            return Ok(
                ActivationFailure::ConfigurationFailed("codex_history_takeover_failed")
                    .projection(&request),
            );
        };
        let provider_id = provider_id.to_owned();
        match codex_history_on_worker(move || {
            crate::codex_history_takeover::Takeover::begin(&home, &provider_id)
        })
        .await
        {
            Ok(history) => Some(history),
            Err(error) => {
                let _ = recovery.abandon(&recovery_record);
                delete_created_tokens(&origin, &access_token, &leases).await;
                return Ok(ActivationFailure::Adapter(error).projection(&request));
            }
        }
    } else {
        None
    };
    // Helper-owned runtimes are started by `start_local_adapter` only after
    // both the credential and settings commits have succeeded.
    if is_cancelled() {
        if let Some(history) = codex_history.as_mut() {
            let _ = history.rollback();
        }
        let _ = recovery.abandon(&recovery_record);
        restore_displaced_login(login_displaced).await;
        delete_created_tokens(&origin, &access_token, &leases).await;
        return Ok(cancelled());
    }
    // No cancellation point between credential publication and local file commit.
    let reopen_previous = reload_guard.closed;
    // From here failures require an explicit successful rollback, not a Drop
    // that could reopen a partially-written or unrecoverable configuration.
    reload_guard.disarm();
    emit_activation_progress(app, &request, "applying_settings", 6);
    let local_result = tool_credentials::store(&request.tool_id, &credential)
        .map_err(|_| AdapterFailure::SecureStorageUnavailable)
        .and_then(|()| prepared.commit());
    stage!("committed");
    log::info!(
        "activation stage=local_commit tool={} ok={}",
        request.tool_id,
        local_result.is_ok()
    );
    let configured = match local_result {
        Err(e) => Err(e),
        Ok(()) => match permit
            .cancel_safe(start_local_adapter(
                &request,
                &credential,
                &HelperRuntimes {
                    claude_code: claude_code_runtime,
                    claude: claude_runtime,
                    codex: codex_runtime,
                    dsh: dsh_runtime,
                },
            ))
            .await
        {
            Ok(result) => result,
            Err(_) => Err(AdapterFailure::ConfigurationFailed(
                "assistant_shutting_down",
            )),
        },
    };
    let mut desktop_open_attempted = false;
    let result = finalize_after_start(
        configured,
        commit_codex_history_on_worker(&mut codex_history),
        async {
            if desktop_lifecycle::reloads_installation(&request.tool_id, &installation.path) {
                if is_cancelled() {
                    return Err(AdapterFailure::ConfigurationFailed("activation_cancelled"));
                }
                desktop_open_attempted = true;
                emit_activation_progress(app, &request, "opening_application", 7);
                emit_activation_progress(app, &request, "checking_application_started", 7);
                desktop_lifecycle::open_and_wait(&request.tool_id, &installation.path).await?;
            }
            Ok(())
        },
        || {
            if is_cancelled() {
                return Err(AdapterFailure::ConfigurationFailed(
                    if registration.cancellation.is_requested() {
                        "activation_cancelled"
                    } else {
                        "assistant_shutting_down"
                    },
                ));
            }
            if ensure_session_epoch(account_state, session_epoch).is_err() {
                return Err(AdapterFailure::ConfigurationFailed("account_changed"));
            }
            recovery
                .finish(&mut recovery_record)
                .map_err(|_| AdapterFailure::ConfigurationFailed("recovery_receipt_failed"))?;
            Ok(())
        },
    )
    .await;
    if result.is_ok() {
        if let Some(history) = codex_history.as_mut() {
            history.disarm();
        }
    }
    if let Err(error) = result {
        log::warn!(
            "activation stage=rollback tool={} reason={:?}",
            request.tool_id,
            error
        );
        emit_activation_progress(app, &request, "restoring_settings", 6);
        let cleanup = recover_after_stop(
            async {
                if desktop_open_attempted {
                    desktop_lifecycle::quit_for_reconfigure(&request.tool_id, &installation.path)
                        .await?;
                }
                Ok(())
            },
            || async {
                restore_files_and_credential(
                    &request,
                    &mut prepared,
                    &recovery_record,
                    credential_before.as_deref(),
                    login_displaced,
                    &mut codex_history,
                )
                .ok()?;
                restart_previous_runtime(
                    &request,
                    previous_record,
                    HelperRuntimes {
                        claude_code: claude_code_runtime,
                        claude: claude_runtime,
                        codex: codex_runtime,
                        dsh: dsh_runtime,
                    },
                    &permit,
                    login_displaced,
                )
                .await
            },
        )
        .await;
        if let Some(failure) = cleanup {
            return Ok(ActivationFailure::Adapter(failure).projection(&request));
        }
        if recovery.abandon(&recovery_record).is_err() {
            return Ok(
                ActivationFailure::ConfigurationFailed("recovery_receipt_failed")
                    .projection(&request),
            );
        }
        if (reopen_previous || desktop_open_attempted)
            && !shutdown_coordinator::global().is_shutting_down()
            && desktop_lifecycle::open_and_wait(&request.tool_id, &installation.path)
                .await
                .is_err()
        {
            return Ok(
                ActivationFailure::ConfigurationFailed("previous_app_reopen_failed")
                    .projection(&request),
            );
        }
        return Ok(if desktop_open_attempted {
            let reason = if matches!(error, AdapterFailure::LaunchError(_)) {
                "desktop_start_failed_restored"
            } else {
                "desktop_change_failed_restored"
            };
            ActivationFailure::ConfigurationFailed(reason).projection(&request)
        } else {
            ActivationFailure::Adapter(error).projection(&request)
        });
    }
    let open_result =
        if desktop_lifecycle::reloads_installation(&request.tool_id, &installation.path) {
            Ok(()) // Already opened and observed before the transaction finished.
        } else {
            emit_activation_progress(app, &request, "opening_application", 7);
            open_configured_adapter(
                &request,
                &installation,
                &credential,
                &HelperRuntimes {
                    claude_code: claude_code_runtime,
                    claude: claude_runtime,
                    codex: codex_runtime,
                    dsh: dsh_runtime,
                },
            )
            .await
        };
    stage!("opened");
    log::info!(
        "activation stage=open tool={} ok={}",
        request.tool_id,
        open_result.is_ok()
    );
    // The committed configuration now references only scoped keys. Broad or
    // stale helper-owned predecessors can be retired without risking rollback
    // to a key that was deleted mid-transaction. This cleanup is deliberately
    // scheduled only after the user-facing application open has been tried.
    retire_superseded_tokens(&origin, &access_token, &leases);
    if let Err(error) = open_result {
        let reason = match error {
            AdapterFailure::LaunchError(reason) => reason,
            _ => "configuration_ready_open_failed",
        };
        return Ok(
            ToolActivationProjection::new(&request, "launch_failed", reason).with_skipped(skipped),
        );
    }
    emit_activation_progress(app, &request, "complete", 7);
    // 这两条是「设置已经写进去了」的出口，所以必须带上 `skipped`：其余返回点
    // 都已经回滚，跳过了谁不再有意义。
    Ok(ToolActivationProjection::new(
        &request,
        "ready",
        if desktop_lifecycle::reloads_installation(&request.tool_id, &installation.path) {
            "desktop_start_observed"
        } else {
            "configuration_ready"
        },
    )
    .with_skipped(skipped))
}

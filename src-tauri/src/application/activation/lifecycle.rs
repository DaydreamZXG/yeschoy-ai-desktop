//! Tool lifecycle: one table of per-tool differences, shared stop/start and
//! desktop-reload steps, and the explicit failure compensation order. Adapters
//! own their configuration format; nothing here writes a tool config file.
use std::path::PathBuf;

use crate::application::activation::capability::ModelTransport;
use crate::application::activation::error::AdapterFailure;
use crate::application::activation::request::ToolActivationRequest;

use crate::connection_recovery;
use crate::shutdown_coordinator;
use crate::tool_adapters::{
    self, claude_code, claude_desktop, codex_desktop, desktop_lifecycle, dsh_web, opencode, pi,
    workbuddy, ResolvedInstallation,
};
use crate::tool_credentials::{self, ToolCredential};

/// Independently owned helper runtimes an activation may start or stop.
pub(crate) struct HelperRuntimes<'a> {
    pub(crate) claude_code: &'a claude_code::ClaudeCodeRuntimeState,
    pub(crate) claude: &'a claude_desktop::ClaudeDesktopRuntimeState,
    pub(crate) codex: &'a codex_desktop::CodexRuntimeState,
    pub(crate) dsh: &'a dsh_web::DshRuntimeState,
}

pub(crate) enum PreparedAdapter {
    ClaudeCode(claude_code::Prepared),
    ClaudeDesktop(claude_desktop::Prepared),
    CodexDesktop(codex_desktop::Prepared),
    Pi(pi::Prepared),
    OpenCode(opencode::Prepared),
    /// Both DSH profiles: `dsh_web` and `dsh_desktop`.
    Dsh(dsh_web::Prepared),
    WorkBuddy(workbuddy::Prepared),
}

impl PreparedAdapter {
    pub(crate) fn changes(&self) -> &[tool_adapters::common::FileChange] {
        match self {
            Self::ClaudeCode(v) => v.changes(),
            Self::ClaudeDesktop(v) => v.changes(),
            Self::CodexDesktop(v) => v.changes(),
            Self::Pi(v) => v.changes(),
            Self::OpenCode(v) => v.changes(),
            Self::Dsh(v) => v.changes(),
            Self::WorkBuddy(v) => v.changes(),
        }
    }

    pub(crate) fn codex_provider_id(&self) -> Option<&str> {
        match self {
            Self::CodexDesktop(value) => Some(value.provider_id()),
            _ => None,
        }
    }

    /// Commit every plan through the application port: the shared transaction
    /// writes and reads back, then the adapter verifies the fields it owns.
    pub(crate) fn commit(&mut self) -> Result<(), AdapterFailure> {
        match self {
            Self::ClaudeCode(value) => value.commit(),
            Self::ClaudeDesktop(value) => value.commit(),
            Self::CodexDesktop(value) => value.commit(),
            Self::Pi(value) => value.commit(),
            Self::OpenCode(value) => value.commit(),
            Self::Dsh(value) => value.commit(),
            Self::WorkBuddy(value) => value.commit(),
        }
    }

    pub(crate) fn rollback(&mut self) -> Result<(), AdapterFailure> {
        match self {
            Self::ClaudeCode(value) => value.rollback(),
            Self::ClaudeDesktop(value) => value.rollback(),
            Self::CodexDesktop(value) => value.rollback(),
            Self::Pi(value) => value.rollback(),
            Self::OpenCode(value) => value.rollback(),
            Self::Dsh(value) => value.rollback(),
            Self::WorkBuddy(value) => value.rollback(),
        }
    }
}

/// 是否要先向用户要一次「挪走 claude.ai 登录态」的同意。
///
/// 抽成函数是因为它曾经被写死在 `if desktop_lifecycle::requires_reload(..)`
/// 里面：那个条件只对 `claude_desktop` / `codex_desktop` 为真，里面再判
/// `claude_code` 是两个互斥条件相与 —— 整段代码一次都没执行过，而当时的测试
/// 直接 mock 了 `configure_desktop_tool_v2` 的返回，走的是界面契约，根本没到
/// 这里。判定独立出来才有东西可测。
///
/// `present` 是惰性的：探测要 shell 出去查钥匙串，只有 claude_code 且尚未
/// 同意时才值得付这个代价。
pub(crate) fn prepare_adapter(
    request: &ToolActivationRequest,
    credential: &ToolCredential,
    model_transport: Option<ModelTransport>,
    codex_provider_hint: Option<&str>,
) -> Result<PreparedAdapter, AdapterFailure> {
    let home = crate::tool_adapters::user_home()
        .filter(|path| path.is_absolute() && path.is_dir())
        .ok_or(AdapterFailure::ConfigurationFailed("home_unavailable"))?;
    let models = credential.model_ids();
    let local = credential.local_gateway_token.as_deref();
    // Each adapter owns its endpoint choice. Claude Code's catalog adapter and
    // Claude Desktop use local pass-throughs; the remaining surfaces point at
    // the relay directly.
    let origin = credential.origin.clone();
    let provider_key = credential.upstream_key();
    match request.tool_id.as_str() {
        "claude_code" => claude_code::prepare_catalog(
            &home,
            &origin,
            &request.model_id,
            match model_transport {
                Some(ModelTransport::Claude(v)) => v,
                _ => return Err(AdapterFailure::ConfigurationFailed("invalid_request")),
            },
            local,
            &models,
        )
        .map(PreparedAdapter::ClaudeCode),
        "claude_desktop" => {
            claude_desktop::prepare_catalog(&home, &request.model_id, local, &models)
                .map(PreparedAdapter::ClaudeDesktop)
        }
        "codex_desktop" => codex_desktop::prepare_catalog_with_provider_hint(
            &home,
            &origin,
            &request.model_id,
            match model_transport {
                Some(ModelTransport::Codex(v)) => v,
                _ => return Err(AdapterFailure::ConfigurationFailed("invalid_request")),
            },
            Some(provider_key),
            &models,
            codex_provider_hint,
        )
        .map(PreparedAdapter::CodexDesktop),
        "pi" => {
            pi::prepare_catalog(&home, &origin, &request.model_id, &models).map(PreparedAdapter::Pi)
        }
        // 只写本地令牌：中转站 key 留在钥匙串里，由 opencode_bridge 转发时取。
        "opencode" => opencode::prepare_catalog(&home, &request.model_id, &models, local)
            .map(PreparedAdapter::OpenCode),
        // One DSH provider per billing group, each with its own scoped key.
        "dsh_web" => dsh_web::prepare_catalog(
            &home,
            &origin,
            &request.model_id,
            &dsh_web::routes(credential),
        )
        .map(PreparedAdapter::Dsh),
        "dsh_desktop" => dsh_web::prepare_desktop_catalog(
            &home,
            &origin,
            &request.model_id,
            &dsh_web::routes(credential),
        )
        .map(PreparedAdapter::Dsh),
        "workbuddy" => {
            workbuddy::prepare_catalog(&home, credential).map(PreparedAdapter::WorkBuddy)
        }
        _ => Err(AdapterFailure::ConfigurationFailed("invalid_request")),
    }
}

pub(crate) struct DesktopReloadGuard {
    tool_id: String,
    path: PathBuf,
    pub(crate) closed: bool,
}

impl DesktopReloadGuard {
    pub(crate) fn new(
        request: &ToolActivationRequest,
        installation: &ResolvedInstallation,
    ) -> Self {
        Self {
            tool_id: request.tool_id.clone(),
            path: installation.path.clone(),
            closed: false,
        }
    }

    pub(crate) fn record_normal_quit(&mut self, closed: bool) {
        self.closed |= closed;
    }

    pub(crate) fn disarm(&mut self) {
        self.closed = false;
    }
}

impl Drop for DesktopReloadGuard {
    fn drop(&mut self) {
        if !self.closed {
            return;
        }
        let result = match self.tool_id.as_str() {
            "claude_desktop" => claude_desktop::launch(&self.path),
            "codex_desktop" => codex_desktop::launch(&self.path),
            _ => Ok(()),
        };
        if result.is_err() {
            log::warn!("desktop_reload stage=rollback_reopen result=failed");
        }
    }
}

pub(crate) async fn stop_helper_runtime(tool: &str, runtime: &HelperRuntimes<'_>) {
    match tool {
        "claude_code" => runtime.claude_code.stop().await,
        "claude_desktop" => runtime.claude.stop().await,
        "dsh_web" => runtime.dsh.stop().await,
        "codex_desktop" => runtime.codex.stop().await,
        "opencode" => crate::opencode_bridge::global().stop().await,
        // Pi connects straight to the relay origin.
        _ => {}
    }
}

pub(crate) async fn codex_history_on_worker<T: Send + 'static>(
    operation: impl FnOnce() -> Result<T, AdapterFailure> + Send + 'static,
) -> Result<T, AdapterFailure> {
    tokio::task::spawn_blocking(operation)
        .await
        .map_err(|_| AdapterFailure::ConfigurationFailed("codex_history_takeover_failed"))?
}

/// 把接入时挪走的 claude.ai 登录态还回去，作为回滚的一部分。
///
/// 接管发生在凭据和配置落盘之前；之后每一条失败或取消的出口都会把文件和
/// 凭据还原，唯独钥匙串里那份以前没人管 —— 用户在 Claude Code 里发现自己
/// 被登出了，而野菜显示「未接入」，也没有任何入口能把它搬回来（restore
/// 分支要有回执，而回执已经被 abandon 了）。
///
/// 还不回去只记一行 warn，不改变回滚的结论：登录态仍在备份位，下一次接入
/// 或 restore 还能搬。真钥匙串没法在单元测试里模拟，所以这里只是把「什么
/// 时候调」收成一个点，调用方按 `Outcome::Displaced` 传 `displaced`。
pub(crate) async fn restore_displaced_login(displaced: bool) {
    if !displaced {
        return;
    }
    match tokio::task::spawn_blocking(crate::claude_login_takeover::restore).await {
        Ok(Ok(())) => log::info!("activation stage=claude_login_restored"),
        Ok(Err(failure)) => {
            log::warn!("activation stage=claude_login_restore_failed reason={failure:?}")
        }
        Err(_) => log::warn!("activation stage=claude_login_restore_failed reason=worker_failed"),
    }
}

#[allow(clippy::too_many_arguments)] // Explicit transaction/runtime inputs; no hidden mutable global rollback context.
pub(crate) fn restore_files_and_credential(
    request: &ToolActivationRequest,
    prepared: &mut PreparedAdapter,
    recovery_record: &connection_recovery::Record,
    credential_before: Option<&str>,
    _login_displaced: bool,
    codex_history: &mut Option<crate::codex_history_takeover::Takeover>,
) -> Result<(), AdapterFailure> {
    let rollback_failed = if desktop_lifecycle::requires_reload(&request.tool_id) {
        connection_recovery::restore_attempt(recovery_record).is_err()
    } else {
        prepared.rollback().is_err()
    };
    if rollback_failed {
        // The live files may still reference the new key. Keep both keys and
        // the encrypted pending checkpoint until recovery really succeeds.
        return Err(AdapterFailure::ConfigurationFailed(
            "configuration_rollback_failed",
        ));
    }
    if let Some(history) = codex_history.as_mut() {
        if history.rollback().is_err() {
            return Err(AdapterFailure::ConfigurationFailed(
                "configuration_rollback_failed",
            ));
        }
    }
    if tool_credentials::restore(&request.tool_id, credential_before).is_err() {
        return Err(AdapterFailure::ConfigurationFailed(
            "credential_restore_failed",
        ));
    }
    Ok(())
}

/// Restart the previous helper runtime after files and credentials are back.
/// The login restore happens afterwards so a pending receipt can still restore
/// it if these steps fail.
pub(crate) async fn restart_previous_runtime(
    request: &ToolActivationRequest,
    previous_record: Option<ToolCredential>,
    runtimes: HelperRuntimes<'_>,
    permit: &shutdown_coordinator::OperationPermit,
    login_displaced: bool,
) -> Option<AdapterFailure> {
    restore_displaced_login(login_displaced).await;
    let runtime_result = permit
        .cancel_safe(async {
            if request.tool_id == "dsh_web" {
                runtimes.dsh.stop().await;
            }
            if request.tool_id == "claude_code" {
                runtimes.claude_code.stop().await;
                if let Some(previous) = previous_record.clone().filter(|credential| {
                    credential.has_model_set() && !shutdown_coordinator::global().is_shutting_down()
                }) {
                    runtimes.claude_code.start(previous).await?;
                }
            }
            if request.tool_id == "codex_desktop" {
                runtimes.codex.stop().await;
                if let Some(previous) = previous_record
                    .clone()
                    .filter(|_| !shutdown_coordinator::global().is_shutting_down())
                {
                    runtimes.codex.start(previous).await?;
                }
            }
            if request.tool_id == "claude_desktop" {
                runtimes.claude.stop().await;
                if let Some(previous) = previous_record
                    .clone()
                    .filter(|_| !shutdown_coordinator::global().is_shutting_down())
                {
                    runtimes.claude.start(previous).await?;
                }
            }
            Ok::<(), AdapterFailure>(())
        })
        .await;
    if matches!(runtime_result, Ok(Err(_))) {
        return Some(AdapterFailure::ConfigurationFailed(
            "previous_connection_runtime_failed",
        ));
    }
    None
}

#[cfg(test)]
#[path = "lifecycle_tests.rs"]
mod tests;

// Codex history must be committed after its provider config but before Codex
// starts and reads the old provider bucket. The durable receipt still follows
// startup observation. Kept separate so fixtures exercise the exact ordering.
pub(crate) async fn finalize_after_start<BeforeStart, Start>(
    configured: Result<(), AdapterFailure>,
    before_start: BeforeStart,
    start: Start,
    finish: impl FnOnce() -> Result<(), AdapterFailure>,
) -> Result<(), AdapterFailure>
where
    BeforeStart: std::future::Future<Output = Result<(), AdapterFailure>>,
    Start: std::future::Future<Output = Result<(), AdapterFailure>>,
{
    configured?;
    before_start.await?;
    start.await?;
    finish()
}

pub(crate) async fn commit_codex_history_on_worker(
    history: &mut Option<crate::codex_history_takeover::Takeover>,
) -> Result<(), AdapterFailure> {
    let Some(mut takeover) = history.take() else {
        return Ok(());
    };
    let (takeover, result) = codex_history_on_worker(move || {
        let result = takeover.commit();
        Ok((takeover, result))
    })
    .await?;
    *history = Some(takeover);
    result
}

// Never restore files or revoke credentials while an attempted launch might
// still be using them. A refused/unknown stop leaves the checkpoint intact.
pub(crate) async fn recover_after_stop<Stop, Restore, RestoreFuture>(
    stopped: Stop,
    restore: Restore,
) -> Option<AdapterFailure>
where
    Stop: std::future::Future<Output = Result<(), AdapterFailure>>,
    Restore: FnOnce() -> RestoreFuture,
    RestoreFuture: std::future::Future<Output = Option<AdapterFailure>>,
{
    if stopped.await.is_err() {
        return Some(AdapterFailure::ConfigurationFailed(
            "desktop_recovery_waiting_for_exit",
        ));
    }
    restore().await
}

// A failed restoration happens after writes. Keep it distinct from an initial
// secure-storage failure so the UI cannot claim the settings were untouched.
#[cfg(test)]
pub(crate) fn restoration_failure(
    files_failed: bool,
    credential_failed: bool,
) -> Option<AdapterFailure> {
    if files_failed {
        Some(AdapterFailure::ConfigurationFailed(
            "configuration_rollback_failed",
        ))
    } else if credential_failed {
        Some(AdapterFailure::ConfigurationFailed(
            "credential_restore_failed",
        ))
    } else {
        None
    }
}

/// 是否要先向用户要一次「挪走 claude.ai 登录态」的同意。
///
/// 抽成函数是因为它曾经被写死在 `if desktop_lifecycle::requires_reload(..)`
/// 里面：那个条件只对 `claude_desktop` / `codex_desktop` 为真，里面再判
/// `claude_code` 是两个互斥条件相与 —— 整段代码一次都没执行过，而当时的测试
/// 直接 mock 了 `configure_desktop_tool_v2` 的返回，走的是界面契约，根本没到
/// 这里。判定独立出来才有东西可测。
///
/// `present` 是惰性的：探测要 shell 出去查钥匙串，只有 claude_code 且尚未
/// 同意时才值得付这个代价。
pub(crate) fn needs_claude_login_consent(
    tool_id: &str,
    already_consented: bool,
    present: impl FnOnce() -> bool,
) -> bool {
    tool_id == "claude_code" && !already_consented && present()
}

pub(crate) async fn start_local_adapter(
    request: &ToolActivationRequest,
    credential: &ToolCredential,
    runtime: &HelperRuntimes<'_>,
) -> Result<(), AdapterFailure> {
    match request.tool_id.as_str() {
        "claude_code" => runtime.claude_code.start(credential.clone()).await,
        "claude_desktop" => runtime.claude.start(credential.clone()).await.map(|_| ()),
        "codex_desktop" => {
            // 桥要先在监听，Codex 才有地方可发 —— 它的 config.toml 现在指向
            // `127.0.0.1:15731/codex/v1`。
            runtime.codex.start(credential.clone()).await?;
            codex_desktop::ensure_credential_ready(credential).await
        }
        "opencode" => crate::opencode_bridge::global()
            .start(credential.clone())
            .await
            .map(|_| ()),
        "pi" | "dsh_web" | "workbuddy" | "dsh_desktop" => Ok(()),
        _ => Err(AdapterFailure::ConfigurationFailed("invalid_request")),
    }
}

pub(crate) async fn open_configured_adapter(
    request: &ToolActivationRequest,
    installation: &ResolvedInstallation,
    credential: &ToolCredential,
    runtime: &HelperRuntimes<'_>,
) -> Result<(), AdapterFailure> {
    match request.tool_id.as_str() {
        "claude_desktop" => claude_desktop::launch(&installation.path),
        "codex_desktop" if codex_desktop::is_cli_installation(&installation.path) => {
            let home = crate::tool_adapters::user_home()
                .ok_or(AdapterFailure::ConfigurationFailed("home_unavailable"))?;
            crate::tool_adapters::terminal_launch::launch_async(installation, "codex_cli", &home)
                .await
        }
        "codex_desktop" => codex_desktop::launch(&installation.path),
        "dsh_web" => dsh_web::open_existing(runtime.dsh, installation, credential).await,
        "workbuddy" => {
            desktop_lifecycle::open_unless_running("workbuddy", &installation.path).await
        }
        "dsh_desktop" => {
            desktop_lifecycle::open_unless_running("dsh_desktop", &installation.path).await
        }
        "claude_code" | "pi" | "opencode" => {
            let home = crate::tool_adapters::user_home()
                .ok_or(AdapterFailure::ConfigurationFailed("home_unavailable"))?;
            crate::tool_adapters::terminal_launch::launch_async(
                installation,
                &request.tool_id,
                &home,
            )
            .await
        }
        _ => Err(AdapterFailure::ConfigurationFailed("invalid_request")),
    }
}

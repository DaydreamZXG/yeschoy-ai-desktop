#![cfg(test)]
//! 端到端编排回归：把 `configure_desktop_tool` 本身完整跑一遍。
//!
//! 之前每个子模块都有自己的单元测试，但「先验请求 → 定位应用 → 拿操作锁 →
//! 取会话 → … → 启动 → 提交或回滚」这套**顺序和失败出口**只能靠读代码确认。
//! 这里用真实的 `tauri::test::mock_app()` AppHandle 和真实的状态结构，让同一份
//! 编排代码自己跑到应该停下的位置，并检查它沿途发出的每个进度事件。
//!
//! ## 为什么停在这里
//!
//! 续上后半段（铸密钥、写配置、启动应用）要先过 `native_session_access`：它要
//! 用账户令牌通过 HTTPS 访问中转站，没有可替换的发送端。为了在测试里续上而在
//! 生产代码里开一条测试专用的传输旁路，等于让测试拥有生产不存在的网络边界，
//! 这个代价不能付。所以这里断言的是**进入网络之前**的出口与进度事件 —— 恰恰是
//! 改动顺序时最容易被打碎、又最难被单个模块测试发现的部分。
//!
//! ## 夹具前提（很重要）
//!
//! - **不碰真实 HOME 下的文件。** 平常这些用例都会停在会话之前，不会写用户
//!   目录。唯一例外是装了 pi 的机器：CLI 发现走 PATH 与登录 shell，不受 HOME
//!   影响，编排会继续到「恢复存储」这一步。恢复存储的密钥放在系统钥匙串里，
//!   而测试二进制跑在终端里时读不到默认钥匙串（`ERR_SEC_AUTH_FAILED` 一类），
//!   于是它在那里停下 —— 同样是**只读**、不写任何文件。
//! - **不碰网络。** 停在会话之前。
//! - **不改 `HOME`。** 改环境变量是进程级的，而 macOS 的钥匙串默认项会按
//!   `HOME` 缓存；在测试里切换 `HOME` 会把同一进程内之后的钥匙串访问一起弄坏
//!   （表现是 `A default keychain could not be found`），连累别的用例。
//! - 因此用例**不依赖**机器上装没装 pi：是否装了只影响「停在第几步」，两个
//!   答案都在下面被显式接受。
use super::request::ToolActivationRequest;
use super::result::ToolActivationProjection;
use super::service::{configure_desktop_tool, ActivationContext};
use super::session::{emit_activation_progress, ActivationOperationState};
use crate::account_v2::AccountV2State;
use crate::app_installation::AppInstallationState;
use crate::tool_adapters::{claude_code, claude_desktop, codex_desktop, dsh_web};
use tauri::Listener;

const PROGRESS_EVENT: &str = "yeschoy://activation-progress";

/// 事件载荷 → 渲染层看到的形状。
fn decode_progress(payload: &str) -> Option<serde_json::Value> {
    let value: serde_json::Value = serde_json::from_str(payload).ok()?;
    value.is_object().then_some(value)
}

fn field<'a>(value: &'a serde_json::Value, key: &str) -> &'a str {
    value
        .get(key)
        .and_then(serde_json::Value::as_str)
        .unwrap_or("")
}

/// 一个假请求：假令牌由各状态自己持有，这里只剩选择本身。
fn request(tool: &str, installation: &str) -> ToolActivationRequest {
    serde_json::from_value(serde_json::json!({
        "requestId": "activation-e2e-fixture",
        "lineId": "mainland_optimized",
        "toolId": tool,
        "modelId": "glm-5.3",
        "billingGroup": "default",
        "installationId": installation
    }))
    .unwrap()
}

struct Fixture {
    app: tauri::App<tauri::test::MockRuntime>,
    activation_state: ActivationOperationState,
    account_state: AccountV2State,
    installation_state: AppInstallationState,
}

impl Fixture {
    fn new() -> Self {
        Self {
            app: tauri::test::mock_app(),
            activation_state: ActivationOperationState::default(),
            account_state: AccountV2State::default(),
            installation_state: AppInstallationState::default(),
        }
    }

    /// 跑一次接入，同时收集这段时间里的进度事件。
    fn attempt(&self, request: ToolActivationRequest) -> Run {
        let progress = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let sink = progress.clone();
        self.app.listen(PROGRESS_EVENT, move |event| {
            if let Some(decoded) = decode_progress(event.payload()) {
                sink.lock().unwrap().push(decoded);
            }
        });
        let claude_code_runtime = claude_code::ClaudeCodeRuntimeState::default();
        let claude_runtime = claude_desktop::ClaudeDesktopRuntimeState::default();
        let codex_runtime = codex_desktop::CodexRuntimeState::default();
        let dsh_runtime = dsh_web::DshRuntimeState::default();
        let result = futures::executor::block_on(configure_desktop_tool(
            &ActivationContext {
                app: self.app.handle(),
                activation_state: &self.activation_state,
                account_state: &self.account_state,
                installation_state: &self.installation_state,
                claude_code_runtime: &claude_code_runtime,
                claude_runtime: &claude_runtime,
                codex_runtime: &codex_runtime,
                dsh_runtime: &dsh_runtime,
            },
            request,
        ));
        let progress = progress.lock().unwrap().clone();
        Run { result, progress }
    }
}

/// 一次接入尝试的结果加上它实际发出的进度事件。
struct Run {
    result: Result<ToolActivationProjection, String>,
    progress: Vec<serde_json::Value>,
}

/// 结果投影的 JSON 形状（渲染层看到的那一份），用于断原因码。
fn projection_json(result: &Result<ToolActivationProjection, String>) -> serde_json::Value {
    serde_json::to_value(result.as_ref().unwrap()).unwrap()
}

/// 进度事件必须步数一致、单调不减，并且只报这一个工具、这一个请求。
fn assert_progress_is_well_formed(run: &Run) {
    let mut last = 0;
    for event in &run.progress {
        let step = event
            .get("completedSteps")
            .and_then(serde_json::Value::as_u64)
            .expect("completedSteps");
        assert_eq!(event["totalSteps"], 7, "{event:?}");
        assert!(step >= last, "progress went backwards");
        assert_eq!(field(event, "toolId"), "pi");
        assert_eq!(field(event, "requestId"), "activation-e2e-fixture");
        last = step;
    }
}

fn stages(run: &Run) -> Vec<String> {
    run.progress
        .iter()
        .map(|p| field(p, "stage").to_owned())
        .collect()
}

#[test]
fn an_activation_stops_at_the_first_failing_stage_and_says_why() {
    let fixture = Fixture::new();
    // CLI 发现走 PATH 与登录 shell，装了 pi 的机器上这一步一定能过，没装的
    // 机器上必须停在定位应用。两种都是正确行为，夹具先问清楚是哪一种。
    let installed = futures::executor::block_on(crate::tool_adapters::scan_targets())
        .into_iter()
        .any(|target| target.tool_id == "pi");

    let run = fixture.attempt(request("pi", ""));
    assert!(run.result.is_ok(), "failures come back as a projection");
    let projection = projection_json(&run.result);
    assert_eq!(field(&projection, "toolId"), "pi");
    assert_eq!(
        stages(&run),
        ["queued", "checking_application"],
        "must stop before any later stage"
    );
    assert_progress_is_well_formed(&run);
    if installed {
        // 定位到了应用，然后停在操作锁 / 恢复存储。两个原因码都合法：
        // 恢复存储（`.yeschoy/connection-recovery`）下已有未完成的接入基线时
        // 先回 `recovery_pending`，没有时走到建基线、钥匙串在测试进程里读不到，
        // 回 `recovery_storage_unavailable`。取决于这台机器上此前有没有留下
        // 未完成的事务，测试不能假定其中一种。
        //
        // 关键是**不是**会话或模型校验那些原因码 —— 那说明编排跑到了网络那
        // 一步，而这个夹具不该碰网络。
        assert_eq!(field(&projection, "status"), "configuration_failed");
        assert!(
            matches!(
                field(&projection, "reasonCode"),
                "recovery_pending" | "recovery_storage_unavailable"
            ),
            "unexpected stop: {projection}"
        );
    } else {
        assert_eq!(field(&projection, "status"), "tool_not_found");
        assert_eq!(field(&projection, "reasonCode"), "tool_not_found");
    }
}

#[test]
fn an_invalid_request_never_enters_the_pipeline() {
    let fixture = Fixture::new();
    let mut invalid = request("pi", "");
    invalid.model_id.clear();
    let run = fixture.attempt(invalid);
    assert_eq!(
        run.result.as_ref().unwrap_err(),
        "invalid_tool_activation_request"
    );
    // 连 `queued` 都不许发：界面收不到任何一步，也就不会显示一条假的进度。
    assert!(run.progress.is_empty(), "an invalid request emits nothing");
}

#[test]
fn a_second_activation_is_refused_while_one_is_running() {
    let fixture = Fixture::new();
    let registration = fixture
        .activation_state
        .begin("activation-e2e-fixture")
        .unwrap();
    let second = fixture.attempt(request("pi", ""));
    assert_eq!(
        second.result.as_ref().unwrap_err(),
        "activation_already_running"
    );
    assert!(
        second.progress.is_empty(),
        "a refused request emits nothing"
    );
    drop(registration);

    // 名额释放后同一请求能被正常受理 —— 说明「被拒」只与在途名额有关，
    // 而不是请求本身有问题。
    let third = fixture.attempt(request("pi", ""));
    assert!(third.result.is_ok());
    assert_progress_is_well_formed(&third);
    assert!(!third.progress.is_empty(), "an accepted request reports");
}

#[test]
fn progress_events_are_delivered_verbatim() {
    let fixture = Fixture::new();
    let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let sink = seen.clone();
    fixture.app.listen(PROGRESS_EVENT, move |event| {
        if let Some(decoded) = decode_progress(event.payload()) {
            sink.lock().unwrap().push(decoded);
        }
    });
    emit_activation_progress(fixture.app.handle(), &request("pi", ""), "queued", 0);
    let seen = seen.lock().unwrap();
    assert_eq!(seen.len(), 1);
    // 渲染层按 camelCase 解码；字段名一漂移，界面就会静默失去进度。
    for key in [
        "requestId",
        "toolId",
        "stage",
        "completedSteps",
        "totalSteps",
    ] {
        assert!(seen[0].get(key).is_some(), "missing {key} in {}", seen[0]);
    }
    assert_eq!(seen[0]["completedSteps"], 0);
    assert_eq!(field(&seen[0], "stage"), "queued");
}

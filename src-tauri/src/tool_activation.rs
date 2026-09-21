use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex as StdMutex, OnceLock,
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use reqwest::{Method, Url};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tauri::Emitter;

use crate::{
    account_v2::{
        ensure_session_epoch, native_account_json, native_session_access, native_session_epoch,
        AccountV2State, NativeSessionFailure,
    },
    claude_bridge::ClaudeTransport,
    connection_recovery::{self, Receipt, Store},
    connectivity_core::request_id_is_valid,
    shutdown_coordinator,
    tool_adapters::{
        self, claude_code, claude_desktop, codex_desktop, desktop_lifecycle, dsh_web, pi,
        workbuddy, AdapterFailure, ResolvedInstallation,
    },
    tool_credentials::{self, CredentialFailure, ToolCredential, ToolModelRoute},
};

/// 中转列表接口一页最多给 100 条（`common.GetPageQuery` 封顶），要多也没用。
const TOKEN_PAGE_SIZE: usize = 100;
/// 最多翻这么多页。中转默认每用户 1000 把密钥，20 页绰绰有余；真有账户超过
/// 这个数，后面的就当没有 —— 宁可多铸一把，也不在接入路径上无限翻页。
const TOKEN_PAGE_LIMIT: usize = 20;
pub(crate) static ACTIVATION_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
const ACTIVATION_PROGRESS_EVENT: &str = "yeschoy://activation-progress";
const ACTIVATION_PROGRESS_TOTAL: u8 = 7;
const CONNECTION_LOCK_WAIT_TIMEOUT: Duration = Duration::from_secs(8);
// Match the renderer's existing inspection budget, including lock wait and I/O.
const CONNECTION_INSPECTION_TIMEOUT: Duration = Duration::from_secs(15);

#[derive(Default)]
pub(crate) struct ActivationOperationState {
    active: StdMutex<HashMap<String, Arc<ActivationCancellation>>>,
}

#[derive(Default)]
struct ActivationCancellation {
    requested: AtomicBool,
}

impl ActivationCancellation {
    fn request(&self) {
        self.requested.store(true, Ordering::Release);
    }

    fn is_requested(&self) -> bool {
        self.requested.load(Ordering::Acquire)
    }
}

struct ActivationRegistration<'a> {
    request_id: String,
    cancellation: Arc<ActivationCancellation>,
    state: &'a ActivationOperationState,
}

impl Drop for ActivationRegistration<'_> {
    fn drop(&mut self) {
        if let Ok(mut active) = self.state.active.lock() {
            let current = active.get(&self.request_id);
            if current.is_some_and(|value| Arc::ptr_eq(value, &self.cancellation)) {
                active.remove(&self.request_id);
            }
        }
    }
}

impl ActivationOperationState {
    fn begin(&self, request_id: &str) -> Result<ActivationRegistration<'_>, ()> {
        let cancellation = Arc::new(ActivationCancellation::default());
        let mut active = self.active.lock().map_err(|_| ())?;
        // There is one shared activation/configuration transaction. Reject a
        // second click immediately instead of queueing it behind the native
        // lock and making the renderer look frozen.
        if !active.is_empty() {
            return Err(());
        }
        active.insert(request_id.to_owned(), cancellation.clone());
        Ok(ActivationRegistration {
            request_id: request_id.to_owned(),
            cancellation,
            state: self,
        })
    }

    fn cancel(&self, request_id: &str) -> Result<bool, ()> {
        let active = self.active.lock().map_err(|_| ())?;
        Ok(active.get(request_id).is_some_and(|value| {
            value.request();
            true
        }))
    }
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ActivationProgress {
    request_id: String,
    tool_id: String,
    stage: &'static str,
    completed_steps: u8,
    total_steps: u8,
}

fn emit_activation_progress(
    app: &tauri::AppHandle,
    request: &ToolActivationRequest,
    stage: &'static str,
    completed_steps: u8,
) {
    if app
        .emit_to(
            "main",
            ACTIVATION_PROGRESS_EVENT,
            ActivationProgress {
                request_id: request.request_id.clone(),
                tool_id: request.tool_id.clone(),
                stage,
                completed_steps,
                total_steps: ACTIVATION_PROGRESS_TOTAL,
            },
        )
        .is_err()
    {
        log::warn!("tool_activation stage=progress_emit_failed");
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ActivationCancelRequest {
    request_id: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActivationCancelResponse {
    request_id: String,
    status: &'static str,
}

#[tauri::command]
pub fn cancel_tool_activation_v1(
    state: tauri::State<'_, ActivationOperationState>,
    request: ActivationCancelRequest,
) -> Result<ActivationCancelResponse, String> {
    if !request_id_is_valid(&request.request_id) {
        return Err("invalid_activation_cancel_request".into());
    }
    let found = state
        .cancel(&request.request_id)
        .map_err(|_| "activation_state_unavailable")?;
    Ok(ActivationCancelResponse {
        request_id: request.request_id,
        status: if found {
            "cancel_requested"
        } else {
            "not_found"
        },
    })
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ToolActivationRequest {
    request_id: String,
    line_id: String,
    tool_id: String,
    model_id: String,
    installation_id: String,
    billing_group: String,
    #[serde(default)]
    models: Option<Vec<ModelBinding>>,
    #[serde(default)]
    installation_job_id: Option<String>,
    #[serde(default)]
    restart_running_app: bool,
    /// 单独一个标志，不复用 `restart_running_app`。
    ///
    /// 同意「重启应用」和同意「挪走我的 claude.ai 登录态」是两件事。合成一个
    /// 的话，用户为了换模型点了重启，就顺带默许了动他的账号凭据 —— 这个仓库
    /// 里有专门的测试防止这种隐式同意（ru056），不能在这里破例。
    #[serde(default)]
    displace_claude_login: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ModelBinding {
    pub(crate) model_id: String,
    pub(crate) billing_group: String,
}

impl ToolActivationRequest {
    fn bindings(&self) -> Vec<ModelBinding> {
        self.models.clone().unwrap_or_else(|| {
            vec![ModelBinding {
                model_id: self.model_id.clone(),
                billing_group: self.billing_group.clone(),
            }]
        })
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolActivationProjection {
    request_id: String,
    schema_version: u8,
    status: &'static str,
    tool_id: String,
    model_id: String,
    billing_group: String,
    observed_at_epoch_ms: u64,
    reason_code: &'static str,
    models: Vec<ModelBinding>,
    /// 这次没写进应用的模型，以及每个是因为什么。
    ///
    /// 一个计费分组铸不出密钥时不再拖垮整单（见 `securing_access` 那个循环），
    /// 但界面必须说得出少了谁 —— 否则用户看到「接入完成」，而列表里那个模型
    /// 其实不能用，比直接失败还糟。
    skipped: Vec<SkippedBinding>,
}

/// 被跳过的一个绑定，带上它自己的原因码。
///
/// 原因码是常量，和失败投影里的 `reason_code` 同一套，所以界面能复用既有文案。
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkippedBinding {
    model_id: String,
    billing_group: String,
    reason_code: &'static str,
}

impl ToolActivationProjection {
    fn new(
        request: &ToolActivationRequest,
        status: &'static str,
        reason_code: &'static str,
    ) -> Self {
        Self {
            request_id: request.request_id.clone(),
            // 5：新增 `skipped`。渲染层的 `exactKeys` 要求键集完全一致，所以
            // 加字段必须升版本，不能在 4 上悄悄多带一个键。
            schema_version: 5,
            status,
            tool_id: request.tool_id.clone(),
            model_id: request.model_id.clone(),
            billing_group: request.billing_group.clone(),
            observed_at_epoch_ms: now_epoch_ms(),
            reason_code,
            models: request.bindings(),
            skipped: Vec::new(),
        }
    }

    /// 挡住整单的那个绑定，通过 `skipped` 传出去，**不动 `model_id`**。
    ///
    /// 第一版是把投影的 `model_id`/`billing_group` 改写成出错的那个绑定 ——
    /// 看着直接，但渲染层有一条反伪造检查：`result.modelId !== input.modelId`
    /// 就整个拒收。于是用户拿到的不是「是 glm-5.3-flash 挡住了」，而是更糟的
    /// 「暂时无法确认接入结果」。投影的 `model_id` 是「这次请求的主语」，是契约，
    /// 不能借用来表达别的意思。界面自己从 `skipped` 里找那一个。
    fn with_skipped(mut self, skipped: Vec<SkippedBinding>) -> Self {
        self.skipped = skipped;
        self
    }
}

#[derive(Clone, Copy, Debug)]
enum ActivationFailure {
    SignedOut,
    /// Nothing produces these two any more. Both gates were withdrawn -- the
    /// model one in `1b3fb135`, the billing group one here -- because both
    /// judged a model unusable from an absent or unreliable declaration. The
    /// variants stay only so the renderer keeps its recovery copy for an older
    /// native binary; do not reach for them to add a gate back without a
    /// signal you can trust. See src/configuration/modelCompatibility.ts.
    UnsupportedModel,
    ServerUnavailable,
    Adapter(AdapterFailure),
    ConfigurationFailed(&'static str),
}

impl ActivationFailure {
    fn projection(self, request: &ToolActivationRequest) -> ToolActivationProjection {
        match self {
            Self::SignedOut => ToolActivationProjection::new(request, "signed_out", "signed_out"),
            Self::UnsupportedModel => {
                ToolActivationProjection::new(request, "unsupported_model", "model_not_available")
            }
            Self::ServerUnavailable => {
                ToolActivationProjection::new(request, "server_unavailable", "server_unavailable")
            }
            Self::Adapter(error) => match error {
                AdapterFailure::ToolNotFound => {
                    ToolActivationProjection::new(request, "tool_not_found", "tool_not_found")
                }
                AdapterFailure::MultipleInstallations => ToolActivationProjection::new(
                    request,
                    "multiple_installations",
                    "installation_selection_required",
                ),
                AdapterFailure::MissingRuntime => ToolActivationProjection::new(
                    request,
                    "missing_runtime",
                    "required_runtime_not_found",
                ),
                AdapterFailure::UnsupportedProfile => ToolActivationProjection::new(
                    request,
                    "unsupported_profile",
                    "profile_not_supported",
                ),
                AdapterFailure::ExternalOverride => ToolActivationProjection::new(
                    request,
                    "external_override",
                    "higher_precedence_override",
                ),
                AdapterFailure::SecureStorageUnavailable => ToolActivationProjection::new(
                    request,
                    "secure_storage_unavailable",
                    "secure_storage_unavailable",
                ),
                AdapterFailure::ConfigurationFailed(reason) => {
                    ToolActivationProjection::new(request, "configuration_failed", reason)
                }
                AdapterFailure::LaunchFailed => {
                    ToolActivationProjection::new(request, "launch_failed", "tool_launch_failed")
                }
                AdapterFailure::LaunchError(reason) => {
                    ToolActivationProjection::new(request, "launch_failed", reason)
                }
            },
            Self::ConfigurationFailed(reason) => {
                ToolActivationProjection::new(request, "configuration_failed", reason)
            }
        }
    }
}

struct TokenLease {
    id: u64,
    key: String,
    created: bool,
    retire_after_commit: Vec<u64>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ModelTransport {
    Claude(ClaudeTransport),
    Codex(codex_desktop::CodexTransport),
}

fn now_epoch_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u64::MAX as u128) as u64
}

fn bounded_plain_text(value: &str, maximum: usize) -> bool {
    !value.is_empty()
        && value.chars().count() <= maximum
        && !value.chars().any(|character| {
            character.is_control()
                || matches!(character, '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
        })
}

fn request_is_valid(request: &ToolActivationRequest) -> bool {
    request_id_is_valid(&request.request_id)
        && matches!(
            request.line_id.as_str(),
            "mainland_optimized" | "global_accelerated"
        )
        && matches!(
            request.tool_id.as_str(),
            "claude_code" | "claude_desktop" | "codex_desktop" | "pi" | "dsh_web" | "workbuddy"
        )
        && bounded_plain_text(&request.model_id, 200)
        && bounded_plain_text(&request.billing_group, 128)
        && request.billing_group != "auto"
        && valid_model_bindings(
            &request.bindings(),
            &request.model_id,
            &request.billing_group,
        )
        && request.installation_id.len() <= 128
        && request
            .installation_job_id
            .as_ref()
            .is_none_or(|id| request_id_is_valid(id))
        && request
            .installation_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
        && (!request.restart_running_app || desktop_lifecycle::requires_reload(&request.tool_id))
}

fn valid_model_bindings(models: &[ModelBinding], default: &str, group: &str) -> bool {
    let mut ids = std::collections::HashSet::new();
    !models.is_empty()
        && models.len() <= 200
        && models.iter().all(|m| {
            bounded_plain_text(&m.model_id, 200)
                && !m.model_id.contains(',')
                && bounded_plain_text(&m.billing_group, 128)
                && m.billing_group != "auto"
                && ids.insert(m.model_id.as_str())
        })
        && models
            .iter()
            .any(|m| m.model_id == default && m.billing_group == group)
}

fn data(value: &Value) -> Option<&Value> {
    let object = value.as_object()?;
    object
        .get("success")?
        .as_bool()?
        .then(|| object.get("data"))
        .flatten()
}

fn server_success(status: u16, value: &Value) -> bool {
    (200..300).contains(&status)
        && value
            .as_object()
            .and_then(|object| object.get("success"))
            .and_then(Value::as_bool)
            == Some(true)
}

async fn validate_models(
    origin: &str,
    access_token: &str,
    models: &[ModelBinding],
    tool_id: &str,
) -> Result<Vec<Option<ModelTransport>>, ActivationFailure> {
    let (status, value) = native_account_json(
        Method::GET,
        &format!("{origin}/api/user/models"),
        access_token,
        None,
    )
    .await
    .map_err(|_| ActivationFailure::ServerUnavailable)?;
    if matches!(status, 401 | 403) {
        return Err(ActivationFailure::SignedOut);
    }
    if !server_success(status, &value) {
        return Err(ActivationFailure::ServerUnavailable);
    }
    let available = data(&value)
        .and_then(Value::as_array)
        .is_some_and(|available| {
            models.iter().all(|binding| {
                available
                    .iter()
                    .any(|m| m.as_str() == Some(&binding.model_id))
            })
        });
    if !available {
        return Err(ActivationFailure::UnsupportedModel);
    }
    let (pricing_status, pricing) = native_account_json(
        Method::GET,
        &format!("{origin}/api/pricing"),
        access_token,
        None,
    )
    .await
    .map_err(|_| ActivationFailure::ServerUnavailable)?;
    if matches!(pricing_status, 401 | 403) {
        return Err(ActivationFailure::SignedOut);
    }
    if !server_success(pricing_status, &pricing) {
        return Err(ActivationFailure::ServerUnavailable);
    }
    models
        .iter()
        .map(|binding| {
            let model_id = binding.model_id.as_str();
            // The billing group is no longer checked against the catalogue.
            // `/api/pricing` only lists models it has priced, so a model the
            // account can use may have no groups here at all -- and refusing
            // then means refusing at apply what the picker already offered,
            // with nothing the user could have done differently. The relay
            // decides whether a group is valid, and says so plainly when it
            // is not. Same reasoning as the model gate; see
            // src/configuration/modelCompatibility.ts.
            if tool_id == "codex_desktop" {
                codex_transport(&pricing, model_id)
                    .map(ModelTransport::Codex)
                    .map(Some)
                    .ok_or(ActivationFailure::UnsupportedModel)
            } else if matches!(tool_id, "claude_code" | "claude_desktop") {
                claude_transport(&pricing, model_id)
                    .map(ModelTransport::Claude)
                    .map(Some)
                    .ok_or(ActivationFailure::UnsupportedModel)
            } else if model_supports_tool(&pricing, model_id, tool_id) {
                Ok(None)
            } else {
                Err(ActivationFailure::UnsupportedModel)
            }
        })
        .collect()
}

/// Every model the account lists can be activated for every target.
///
/// There used to be a protocol gate here, mirrored in
/// `src/configuration/modelCompatibility.ts`. It was withdrawn: it never got
/// a verdict right, and every error ran the same way -- treating what we did
/// not know as something that does not work. The relay does not gate on
/// `supported_endpoint_types` either (zero references under `relay/`; only
/// two display controllers read it), every channel adaptor converts between
/// the four entry formats, and the relay adds models faster than it describes
/// them, so any rule built on that field greys more working models over time.
///
/// A model that genuinely cannot serve a request now fails at the relay with
/// a message saying so, which is more honest than us pre-emptying it on a
/// field that was never meant to answer this question. See the TypeScript
/// file for the findings, in case this is ever reopened.
fn model_supports_tool(_pricing: &Value, _model_id: &str, tool_id: &str) -> bool {
    matches!(
        tool_id,
        "claude_code" | "claude_desktop" | "codex_desktop" | "pi" | "dsh_web" | "workbuddy"
    )
}

/// The transport is still chosen per model, because it decides what gets
/// written into the app's configuration. It is no longer a veto: both arms
/// are usable, and the choice only records which wire format the app will
/// speak to the relay.
fn claude_transport(pricing: &Value, model_id: &str) -> Option<ClaudeTransport> {
    let _ = (pricing, model_id);
    Some(ClaudeTransport::DirectAnthropic)
}

fn codex_transport(pricing: &Value, model_id: &str) -> Option<codex_desktop::CodexTransport> {
    // 接入时真探一次才知道（见 `probe_codex_transport`）。这里给的是保守
    // 起点：直连，也就是今天的行为。探测拿不到结论时就停在这个值上。
    let _ = (pricing, model_id);
    Some(codex_desktop::CodexTransport::DirectResponses)
}

/// 一次探测请求的结论。
///
/// 分三档而不是「成/败」两档，是因为**只有中间那档才该改变路由**：
/// 网络抖动、超时、5xx 说明的是「这次没问到」，不是「这个协议不行」。
/// 拿它们去翻转传输方式，用户会在网不好的时候被永久切到桥上。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ProbeOutcome {
    /// 端点受理了这个模型。
    Accepted,
    /// 端点明确拒绝（4xx）—— 路由不认、渠道没配、协议没实现。
    Rejected,
    /// 没问出结论：超时、网络错误、5xx。
    Inconclusive,
}

/// 由两次探测的结论决定这个模型怎么走。
///
/// 抽成纯函数是因为判定规则比网络那层重要得多，而且必须能不联网测。
///
/// 规则只有一条能翻转路由：**Responses 被明确拒绝、而 Chat 明确可用**。
/// 其余一律停在直连 —— 也就是今天的行为。方向是有意选的：
///
/// - 错判成桥，代价是把一个本来原生说 Responses 的模型转一圈，
///   它真的 `encrypted_content` 会被我们伪造的令牌换掉，**是主动降级**。
/// - 错判成直连，代价是这个模型维持现状（本来就不能用），**不造成回退**。
///
/// 两种错不对称，所以宁可漏判。
fn transport_from_probes(
    responses: ProbeOutcome,
    chat: Option<ProbeOutcome>,
) -> codex_desktop::CodexTransport {
    if responses == ProbeOutcome::Rejected && chat == Some(ProbeOutcome::Accepted) {
        codex_desktop::CodexTransport::ChatBridge
    } else {
        codex_desktop::CodexTransport::DirectResponses
    }
}

/// 探测用的最小请求体。
///
/// `max_output_tokens` 取 16：Responses API 的下限就是它（上游 cc-switch
/// 有一条 `clamp sub-floor max_tokens to the Responses API minimum` 的修复），
/// 再小会被当成参数错误而不是路由错误，把探测本身变成噪音。
///
/// **这次探测会真的产生一点点用量。** 实测那几种失败（`unknown provider
/// for model gi/…`、`分组 default 下无可用渠道`）都发生在路由阶段、还没到
/// 上游，不计费；只有成功的那次会生成十几个 token。每个模型至多一到两次，
/// 只在接入时发生。
fn probe_body(model: &str, responses: bool) -> Value {
    if responses {
        json!({
            "model": model,
            "input": "hi",
            "max_output_tokens": 16,
            "stream": false,
        })
    } else {
        json!({
            "model": model,
            "messages": [{"role": "user", "content": "hi"}],
            "max_tokens": 16,
            "stream": false,
        })
    }
}

/// 单次探测的时间预算。
///
/// 共用客户端自带 20 秒超时，对一个 16 token 的请求太宽裕了：真机上
/// `hy4-preview` 那次耗了约 18 秒才得出 `Inconclusive`，接入被它一个人拖住。
/// 收到 8 秒，一次问不出来就换下一次问 —— 总预算反而只比原来多几秒，
/// 却能问三遍。
const PROBE_ATTEMPT_BUDGET: Duration = Duration::from_secs(8);
/// 最多问几遍。只有 `Inconclusive` 才会重问。
const PROBE_ATTEMPTS: u8 = 3;
/// 两次之间歇一下，让瞬时抖动过去；短到不会让用户觉得卡住。
const PROBE_RETRY_DELAY: Duration = Duration::from_millis(300);

/// 问到有结论为止，最多 `PROBE_ATTEMPTS` 次。
///
/// **为什么要重问**：`Inconclusive` 说明的是「这次没问到」，而探测结果是
/// **落盘的、之后不再探**。也就是说接入那一瞬间网抖了一下，这个模型就永久
/// 停在直连 —— 而用户看到的只是「这个模型在 Codex 里用不了」，没有任何线索
/// 指向「当时没问出结论」。保守方向（不确定就不翻转）是对的，
/// 但「漏判之后没有第二次机会」是缺口，不是保守。
async fn probe_until_conclusive(
    client: &reqwest::Client,
    origin: &str,
    key: &str,
    model: &str,
    responses: bool,
) -> (ProbeOutcome, u8) {
    for attempt in 1..=PROBE_ATTEMPTS {
        let outcome = probe_once(client, origin, key, model, responses).await;
        if outcome != ProbeOutcome::Inconclusive {
            return (outcome, attempt);
        }
        if attempt < PROBE_ATTEMPTS {
            tokio::time::sleep(PROBE_RETRY_DELAY).await;
        }
    }
    (ProbeOutcome::Inconclusive, PROBE_ATTEMPTS)
}

async fn probe_once(
    client: &reqwest::Client,
    origin: &str,
    key: &str,
    model: &str,
    responses: bool,
) -> ProbeOutcome {
    let path = if responses {
        "/v1/responses"
    } else {
        "/v1/chat/completions"
    };
    let url = format!("{}{path}", origin.trim_end_matches('/'));
    let sent = tokio::time::timeout(
        PROBE_ATTEMPT_BUDGET,
        client
            .post(url)
            .bearer_auth(key)
            .json(&probe_body(model, responses))
            .send(),
    )
    .await;
    match sent {
        Ok(Ok(response)) if response.status().is_success() => ProbeOutcome::Accepted,
        Ok(Ok(response)) if response.status().is_client_error() => ProbeOutcome::Rejected,
        // 5xx、网络错误、超时都归到「没问出结论」，见 `ProbeOutcome`。
        _ => ProbeOutcome::Inconclusive,
    }
}

/// 把探测结果写回这一批模型的传输方式。
///
/// 只对 Codex 做：Claude 走的是中转原生支持的 Anthropic 协议，没有这个问题。
///
/// 探测失败不让接入失败 —— 探不出来就停在直连，也就是今天的行为。
/// 为了一次「问不出结论」而挡住用户接入，是拿一个可恢复的未知换一个确定的
/// 失败，不划算。
async fn apply_codex_probes(
    origin: &str,
    bindings: &[ModelBinding],
    leases: &[(String, TokenLease)],
    transports: &mut [Option<ModelTransport>],
) {
    let Ok(client) = crate::account_v2::shared_http_client() else {
        return;
    };
    // 带着下标探、按下标写回。
    //
    // 第一版是「筛出要探的 → join_all → 再遍历一遍按同样的条件写回」，
    // 那样两处条件必须永远一致：筛选里多一个「有 lease」的判断，
    // 回写里没有，某个 binding 没 lease 时结果就会整体错位，
    // **探测结论被安到别的模型头上**。今天每个 binding 都有 lease，
    // 但那是另一个函数的不变量。按下标配对，这类错误就不可能发生。
    let probes = bindings
        .iter()
        .enumerate()
        .filter(|(index, _)| {
            // 只探本来判成直连的。以后若有别的来源已经判成桥，不覆盖它。
            matches!(
                transports.get(*index),
                Some(Some(ModelTransport::Codex(
                    codex_desktop::CodexTransport::DirectResponses
                )))
            )
        })
        .filter_map(|(index, binding)| {
            let lease = leases
                .iter()
                .find(|(group, _)| group == &binding.billing_group)?;
            Some(async move {
                (
                    index,
                    probe_codex_transport(client, origin, &lease.1.key, &binding.model_id).await,
                )
            })
        });
    // 并发探：一次接入可能绑了十几个模型，串行会把接入时间拖成十几倍。
    for (index, probed) in futures::future::join_all(probes).await {
        if let Some(slot) = transports.get_mut(index) {
            *slot = Some(ModelTransport::Codex(probed));
        }
    }
}

/// 接入时对一个模型探一次：中转的 `/v1/responses` 到底能不能用。
///
/// **为什么必须探，不能查元数据**：`supported_endpoint_types` 在中转的
/// `relay/` 下零引用，是展示元数据；实测 `/api/pricing` 的 27 个模型里
/// 没有任何一个带 `openai-response` —— 拿它当信号会把 27 个全灰掉。
/// 这个字段已经判错过三次，每次都是同一个方向：把不知道的当成不行。
///
/// Chat 那一探**只在 Responses 被明确拒绝时才发**，所以能用的模型
/// 只花一次请求。
async fn probe_codex_transport(
    client: &reqwest::Client,
    origin: &str,
    key: &str,
    model: &str,
) -> codex_desktop::CodexTransport {
    let (responses, responses_tries) =
        probe_until_conclusive(client, origin, key, model, true).await;
    let chat = if responses == ProbeOutcome::Rejected {
        Some(probe_until_conclusive(client, origin, key, model, false).await)
    } else {
        None
    };
    let chat_outcome = chat.map(|(outcome, _)| outcome);
    let transport = transport_from_probes(responses, chat_outcome);
    // 只记常量名、结论与次数，不记模型之外的任何东西；
    // 报错正文一个字都不进日志。次数要记：看日志的人得能分辨
    // 「一问就知道」和「问了三遍还是不知道」。
    log::info!(
        "codex_probe model={model} responses={responses:?} tries={responses_tries} \
         chat={chat_outcome:?} chat_tries={} transport={}",
        chat.map(|(_, tries)| tries).unwrap_or(0),
        transport.credential_value()
    );
    // 问了三遍仍然没有结论 ⇒ 这个模型停在直连**不是因为它适合直连**，
    // 而是因为我们不知道。这两件事在凭据里长得一模一样，只有这行日志能分辨。
    // 用户回头说「这个模型用不了」时，看到这行就知道该让他重新接入一次。
    if responses == ProbeOutcome::Inconclusive {
        log::warn!(
            "codex_probe model={model} result=never_answered tries={responses_tries} \
             note=left_on_direct_because_unknown_not_because_suitable"
        );
    }
    // 两个端点都明确拒绝 = 这个模型对 Codex 根本不可用，桥也救不了
    // （实测有两个就是这样：定价或渠道没配，与协议无关）。
    // **不因此挡住接入** —— 我们这次只发了一个十六 token 的最小请求，
    // 拿它的失败去否决用户的选择，等于用一次猜测替用户做决定，而探测本身
    // 也可能因为请求太小被拒。但这行日志要能一眼看出来：用户回头说
    // 「这个模型在 Codex 里用不了」时，答案已经在日志里了。
    if responses == ProbeOutcome::Rejected && chat_outcome == Some(ProbeOutcome::Rejected) {
        log::warn!("codex_probe model={model} result=unusable_on_both_endpoints");
    }
    transport
}

#[cfg(test)]
fn token_name(tool_id: &str) -> &'static str {
    match tool_id {
        "claude_code" => "野菜API Claude Code",
        "claude_desktop" => "野菜API Claude Desktop",
        "codex_desktop" => "野菜API Codex Desktop",
        "pi" => "野菜API Pi",
        "dsh_web" => "野菜API DSH web",
        "workbuddy" => "野菜API WorkBuddy",
        _ => "野菜API Desktop",
    }
}

/// 服务端用量日志里的 token 名称反查工具。客户端为每个工具创建独立 token，
/// 名称形如 `野菜API cx-<分组哈希>-<随机后缀>`，因此按工具代码前缀归因。
pub(crate) fn tool_for_token_name(name: &str) -> Option<&'static str> {
    [
        "claude_code",
        "claude_desktop",
        "codex_desktop",
        "pi",
        "dsh_web",
        "workbuddy",
    ]
    .into_iter()
    // 新名字写应用名，旧名字写两字母代码。两种都要认：用量日志里它们会长期
    // 并存，认不出就归不了因。
    .find(|tool| {
        name.starts_with(&format!("野菜API {}-", token_label(tool)))
            || name.starts_with(&format!("野菜API {}-", token_code(tool)))
    })
}

fn token_list_url(origin: &str, page: usize) -> Result<String, ActivationFailure> {
    let mut url = Url::parse(&format!("{origin}/api/token/"))
        .map_err(|_| ActivationFailure::ServerUnavailable)?;
    url.query_pairs_mut()
        .append_pair("p", &page.to_string())
        .append_pair("size", &TOKEN_PAGE_SIZE.to_string());
    Ok(url.to_string())
}

/// 账户上的全部密钥，逐页拉完，本地再认。
///
/// 以前拿名字前缀去 `/api/token/search?keyword=…` 找自己的密钥。中转
/// （yeschoy/new-api `ccd535e`）把那个接口改成了两条规则：关键词不带 `%`
/// 就按**整个名字**精确匹配；每用户每分钟只准搜 10 次，超了回 429 空响应。
/// 于是前缀查找永远查不到 —— 每次接入都铸一把新的，旧的一把也退不掉；
/// 一次接四个分组，每组三次搜索，第 11 次撞上 429，第四个分组就被当成
/// `server_unavailable` 跳过。2026-09-20 22:43 真机日志里那行
/// `token_group_skipped group_hash=915eb5e2` 就是这么来的。
///
/// 列表接口没有这两条：只受全局限流（每 IP 每 30 秒 300 次），返回完整的
/// 名字、分组和模型范围。本地过滤既不依赖中转的匹配语义，也不消耗搜索配额。
async fn list_tokens(
    api: &impl TokenApi,
    origin: &str,
) -> Result<Vec<serde_json::Map<String, Value>>, ActivationFailure> {
    let mut tokens = Vec::new();
    for page in 1..=TOKEN_PAGE_LIMIT {
        let url = token_list_url(origin, page)?;
        let (status, value) = api.request(Method::GET, &url, None).await?;
        if matches!(status, 401 | 403) {
            return Err(ActivationFailure::SignedOut);
        }
        if !server_success(status, &value) {
            log_token_rejection("list", status, &value);
            return Err(ActivationFailure::ServerUnavailable);
        }
        let items = data(&value)
            .and_then(Value::as_object)
            .and_then(|page| page.get("items"))
            .and_then(Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .filter_map(Value::as_object)
                    .cloned()
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let last_page = items.len() < TOKEN_PAGE_SIZE;
        tokens.extend(items);
        if last_page {
            break;
        }
    }
    // 翻页期间别处插进一把新密钥，页就会错开一位，同一把出现两次。按 id 去重，
    // 免得退役队列里有重复的 DELETE。
    tokens.sort_by_key(|token| token.get("id").and_then(Value::as_u64));
    tokens.dedup_by_key(|token| token.get("id").and_then(Value::as_u64));
    Ok(tokens)
}

/// 在拉回来的列表里认自己的密钥。纯函数，好测。
///
/// `exact` 为 true 时按整个名字找刚铸好的那一把；为 false 时按前缀扫，并且
/// 只认本机的（见 `token_is_ours`）。返回选中复用的一把，以及其余该退役的。
fn select_token(
    tokens: &[serde_json::Map<String, Value>],
    name: &str,
    group: &str,
    model_ids: &[String],
    exact: bool,
) -> (Option<u64>, Vec<u64>) {
    let mut owned = tokens
        .iter()
        .filter(|token| owned_token(token, name, group, exact))
        // An exact lookup addresses one known name, so it is already scoped.
        // A prefix sweep is the dangerous one: it sees every computer's keys.
        .filter(|token| exact || token_is_ours(token, name))
        .filter_map(|token| {
            token
                .get("id")
                .and_then(Value::as_u64)
                .map(|id| (id, reusable_token(token, name, group, model_ids, exact)))
        })
        .collect::<Vec<_>>();
    owned.sort_unstable_by_key(|(id, _)| *id);
    let selected = owned
        .iter()
        .rev()
        .find_map(|(id, reusable)| reusable.then_some(*id));
    let retire = owned
        .into_iter()
        .filter_map(|(id, _)| (Some(id) != selected).then_some(id))
        .collect();
    (selected, retire)
}

/// 中转拒绝一次密钥操作时，把状态码和它的原话记进日志 —— **只进日志**。
/// 那是自由文本，会变，所以判断仍然只看 `success`；可能带账户信息，所以
/// 界面上不出现。没有这一行，铸不出密钥的十几种原因在日志里长得一模一样。
fn log_token_rejection(op: &str, status: u16, value: &Value) {
    let message = value
        .as_object()
        .and_then(|object| object.get("message"))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .chars()
        .filter(|character| !character.is_control())
        .take(200)
        .collect::<String>();
    log::warn!("activation stage=token_api_rejected op={op} status={status} message={message}");
}

/// 日志里给密钥操作起个名。只看方法和路径形状，不带 id，不带查询串。
fn token_operation(method: &Method, url: &str) -> &'static str {
    let path = Url::parse(url)
        .map(|url| url.path().to_owned())
        .unwrap_or_default();
    match (method.as_str(), path.as_str()) {
        ("GET", "/api/token/") => "list",
        ("POST", "/api/token/") => "create",
        ("POST", path) if path.ends_with("/key") => "key",
        ("DELETE", _) => "delete",
        _ => "other",
    }
}

fn token_code(tool_id: &str) -> &'static str {
    match tool_id {
        "claude_code" => "cc",
        "claude_desktop" => "cd",
        "codex_desktop" => "cx",
        "pi" => "pi",
        "dsh_web" => "ds",
        "workbuddy" => "wb",
        _ => "tool",
    }
}

/// 密钥名里看得见的那一截。
///
/// 中转对 token 名字有 50 **字节**的硬上限（`controller/token.go`，Go 的
/// `len()` 是字节数，一个汉字 3 字节），所以这里不带空格：最长的
/// `ClaudeDesktop` 配上其余部分正好 49 字节，一个字节的余量都别再占。
fn token_label(tool_id: &str) -> &'static str {
    match tool_id {
        "claude_code" => "ClaudeCode",
        "claude_desktop" => "ClaudeDesktop",
        "codex_desktop" => "Codex",
        "pi" => "Pi",
        "dsh_web" => "DSH",
        "workbuddy" => "WorkBuddy",
        _ => "Desktop",
    }
}

/// 分组哈希。只是个查找标签，真正比对的永远是中转返回的完整分组名。
///
/// 从 16 位十六进制缩到 8 位，腾出的 8 个字节给了前面的应用名。账户上的分组
/// 是个位数，32 位空间碰撞概率可以忽略；真撞上了也只是两个分组共用一个查找
/// 前缀，而 `owned_token` 随后还会逐个核对完整分组名，不会串。
fn group_hash(group: &str) -> u32 {
    let hash = group.bytes().fold(0xcbf29ce484222325u64, |h, b| {
        (h ^ u64::from(b)).wrapping_mul(0x100000001b3)
    });
    (hash >> 32) as u32 ^ hash as u32
}

/// 0.4.22 及更早铸的名字：`野菜API cx-<16位哈希>`。仍然要认得，否则升级一次
/// 就在账户上留一把没人认领的旧密钥，旁边再铸一把新的。
fn legacy_token_prefix(tool_id: &str, group: &str) -> String {
    let hash = group.bytes().fold(0xcbf29ce484222325u64, |h, b| {
        (h ^ u64::from(b)).wrapping_mul(0x100000001b3)
    });
    format!("野菜API {}-{hash:016x}", token_code(tool_id))
}

/// 密钥名的前半截。后半截恒定是 `-<设备8位><随机8位>`，那是身份，不能动。
///
/// 旧名字 `野菜API cx-e8039bc07a1b2c3d-…` 在后台里看得见的只有
/// `野菜API cx-…`，一列密钥长得一模一样，认不出哪把给哪个应用。现在写应用名：
///
///     野菜API Codex-e8039bc0-1a2b3c4d5e6f7a8b
fn token_prefix(tool_id: &str, group: &str) -> String {
    format!("野菜API {}-{:08x}", token_label(tool_id), group_hash(group))
}

const DEVICE_SCOPE_LEN: usize = 8;
const UNIDENTIFIED_DEVICE: &str = "00000000";
static DEVICE_SCOPE: OnceLock<String> = OnceLock::new();

/// Eight hex characters identifying this installation, stable across restarts.
///
/// Token names carry it so one computer can tell its own keys from another's.
/// Without it every machine claimed every key matching `野菜API <tool>-<group>-`
/// and deleted the ones it did not recognise — so signing in on a second
/// computer and choosing a slightly different model set silently revoked the
/// first computer's key, and that machine started failing with no message.
///
/// It lives in a plain file rather than the keyring on purpose: losing it would
/// orphan this machine's keys, and the keyring is the one store we already know
/// can disappear (see `connection_recovery`). Hostname is the fallback, and a
/// fixed placeholder the last resort — two unidentified machines then behave the
/// way every machine used to, which is the bug, but only for a machine with
/// neither a writable home directory nor a hostname.
fn device_scope() -> &'static str {
    DEVICE_SCOPE.get_or_init(|| {
        if let Some(home) = crate::tool_adapters::user_home() {
            let path = home.join(".yeschoy").join("device-id");
            if let Some(existing) = std::fs::read_to_string(&path)
                .ok()
                .map(|text| text.trim().to_owned())
                .filter(|text| {
                    text.len() == DEVICE_SCOPE_LEN
                        && text.bytes().all(|byte| byte.is_ascii_hexdigit())
                })
            {
                return existing;
            }
            let mut bytes = [0u8; 4];
            if getrandom::fill(&mut bytes).is_ok() {
                let minted = bytes
                    .iter()
                    .map(|byte| format!("{byte:02x}"))
                    .collect::<String>();
                if std::fs::create_dir_all(path.parent().unwrap_or(&home)).is_ok()
                    && crate::tool_adapters::common::atomic_write_bounded(
                        &path,
                        minted.as_bytes(),
                        64,
                    )
                    .is_ok()
                {
                    return minted;
                }
            }
        }
        hostname_scope().unwrap_or_else(|| UNIDENTIFIED_DEVICE.to_owned())
    })
}

fn hostname_scope() -> Option<String> {
    let raw = std::env::var("COMPUTERNAME")
        .ok()
        .or_else(|| std::env::var("HOSTNAME").ok())?;
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    let hash = trimmed.bytes().fold(0xcbf29ce484222325u64, |h, b| {
        (h ^ u64::from(b)).wrapping_mul(0x100000001b3)
    });
    Some(format!("{:08x}", hash as u32))
}

/// True when this token's name carries *this* machine's scope.
///
/// The suffix stays sixteen hex characters, so a client that predates this
/// change still parses the new names; the first eight are now the device and the
/// last eight the nonce. A name without our scope belongs to another computer —
/// or to this one before it had an identity — and is never reused and never
/// retired. That leaves one inert key per tool and group behind at upgrade,
/// which the user can delete in the web console; deleting it here would risk
/// deleting the key another computer is using right now, which is the whole
/// problem being fixed.
fn token_is_ours(token: &serde_json::Map<String, Value>, prefix: &str) -> bool {
    token
        .get("name")
        .and_then(Value::as_str)
        .and_then(|name| name.strip_prefix(&format!("{prefix}-")))
        .is_some_and(|suffix| suffix.starts_with(device_scope()))
}

fn owned_token(
    token: &serde_json::Map<String, Value>,
    name: &str,
    group: &str,
    exact: bool,
) -> bool {
    let actual = token.get("name").and_then(Value::as_str).unwrap_or("");
    let name_matches = if exact {
        actual == name
    } else {
        actual
            .strip_prefix(&format!("{name}-"))
            .is_some_and(|suffix| {
                suffix.len() == 16 && suffix.bytes().all(|c| c.is_ascii_hexdigit())
            })
    };
    name_matches && token.get("group").and_then(Value::as_str) == Some(group)
}

fn reusable_token(
    token: &serde_json::Map<String, Value>,
    name: &str,
    group: &str,
    model_ids: &[String],
    exact: bool,
) -> bool {
    let expiry = token
        .get("expired_time")
        .and_then(Value::as_i64)
        .unwrap_or(0);
    owned_token(token, name, group, exact)
        && token.get("status").and_then(Value::as_i64) == Some(1)
        && (expiry == -1 || expiry > (now_epoch_ms() / 1000) as i64)
        && token.get("model_limits_enabled").and_then(Value::as_bool) == Some(true)
        && token
            .get("model_limits")
            .and_then(Value::as_str)
            .is_some_and(|actual| {
                let mut actual = actual.split(',').collect::<Vec<_>>();
                let mut expected = model_ids.iter().map(String::as_str).collect::<Vec<_>>();
                actual.sort_unstable();
                expected.sort_unstable();
                !actual.iter().any(|id| id.is_empty()) && actual == expected
            })
}

fn token_request(name: &str, group: &str, model_ids: &[String]) -> Value {
    json!({
        "name": name,
        "remain_quota": 0,
        "expired_time": -1,
        "unlimited_quota": true,
        "model_limits_enabled": true,
        "model_limits": model_ids.join(","),
        "allow_ips": "",
        "group": group,
        "auto_groups": [],
        "cross_group_retry": false
    })
}

fn normalize_api_key(value: &str) -> Option<String> {
    let value = value.trim();
    if value.len() < 16
        || value.len() > 256
        || value
            .chars()
            .any(|character| character.is_control() || character.is_whitespace())
    {
        return None;
    }
    Some(if value.starts_with("sk-") {
        value.to_owned()
    } else {
        format!("sk-{value}")
    })
}

async fn fetch_token_key(
    api: &impl TokenApi,
    origin: &str,
    id: u64,
) -> Result<String, ActivationFailure> {
    let (status, value) = api
        .request(Method::POST, &format!("{origin}/api/token/{id}/key"), None)
        .await?;
    if matches!(status, 401 | 403) {
        return Err(ActivationFailure::SignedOut);
    }
    if !server_success(status, &value) {
        log_token_rejection("key", status, &value);
        return Err(ActivationFailure::ServerUnavailable);
    }
    data(&value)
        .and_then(Value::as_object)
        .and_then(|value| value.get("key"))
        .and_then(Value::as_str)
        .and_then(normalize_api_key)
        .ok_or(ActivationFailure::ServerUnavailable)
}

async fn acquire_token(
    origin: &str,
    access_token: &str,
    tool_id: &str,
    group: &str,
    model_ids: &[String],
) -> Result<TokenLease, ActivationFailure> {
    acquire_token_using(
        &NativeTokenApi { access_token },
        origin,
        tool_id,
        group,
        model_ids,
    )
    .await
}

trait TokenApi {
    fn request(
        &self,
        method: Method,
        url: &str,
        body: Option<Value>,
    ) -> impl std::future::Future<Output = Result<(u16, Value), ActivationFailure>> + Send;
}

struct NativeTokenApi<'a> {
    access_token: &'a str,
}
impl TokenApi for NativeTokenApi<'_> {
    async fn request(
        &self,
        method: Method,
        url: &str,
        body: Option<Value>,
    ) -> Result<(u16, Value), ActivationFailure> {
        let op = token_operation(&method, url);
        native_account_json(method, url, self.access_token, body)
            .await
            .map_err(|_| {
                // 传输层失败：网络、超时，或者中转回了个不是 JSON 的东西 ——
                // 限流的 429 就是空响应。状态码到不了这一层，至少记下是哪一步。
                log::warn!("activation stage=token_api_transport_failed op={op}");
                ActivationFailure::ServerUnavailable
            })
    }
}

async fn acquire_token_using(
    api: &impl TokenApi,
    origin: &str,
    tool_id: &str,
    group: &str,
    model_ids: &[String],
) -> Result<TokenLease, ActivationFailure> {
    let prefix = token_prefix(tool_id, group);
    let tokens = list_tokens(api, origin).await?;
    let (current, current_stale) = select_token(&tokens, &prefix, group, model_ids, false);
    // Also look under the name 0.4.22 and earlier used: the key is still good,
    // and the name is only a label. Adopting it costs nothing, while skipping
    // this would leave an orphaned key on the account at every upgrade and mint
    // a duplicate beside it. Keys keep their old name until something else
    // replaces them -- renaming is not worth a round trip. When both shapes
    // exist the current one wins and the legacy one is retired with the rest.
    let legacy = legacy_token_prefix(tool_id, group);
    let (adoptable, legacy_stale) = select_token(&tokens, &legacy, group, model_ids, false);
    let existing = current.or(adoptable);
    let retire_after_commit = current_stale
        .into_iter()
        .chain(legacy_stale)
        .chain(adoptable.filter(|id| Some(*id) != existing))
        .collect::<Vec<_>>();
    if let Some(id) = existing {
        // Reuse only an exact group and model scope; broad legacy keys are
        // retired after the new local transaction commits successfully.
        return Ok(TokenLease {
            id,
            key: fetch_token_key(api, origin, id).await?,
            created: false,
            retire_after_commit,
        });
    }

    // Sixteen hex characters as before, now split: this machine's scope, then a
    // nonce. Same shape, so older clients still recognise the name; new clients
    // can tell whose key it is.
    let mut nonce = [0u8; 4];
    getrandom::fill(&mut nonce).map_err(|_| ActivationFailure::ServerUnavailable)?;
    let name = format!(
        "{prefix}-{}{:08x}",
        device_scope(),
        u32::from_be_bytes(nonce)
    );

    if shutdown_coordinator::global().is_shutting_down() {
        return Err(ActivationFailure::ConfigurationFailed(
            "assistant_shutting_down",
        ));
    }
    let (status, value) = api
        .request(
            Method::POST,
            &format!("{origin}/api/token/"),
            Some(token_request(&name, group, model_ids)),
        )
        .await?;
    if matches!(status, 401 | 403) {
        return Err(ActivationFailure::SignedOut);
    }
    if !server_success(status, &value) {
        log_token_rejection("create", status, &value);
        return Err(ActivationFailure::ServerUnavailable);
    }
    let id = match created_token_id(&value, &name) {
        Some(id) => id,
        None => {
            // Standard NewAPI returns success with no data/key. Read the exact
            // newly created name back, including its group, then use the
            // dedicated key API.
            let tokens = list_tokens(api, origin).await?;
            select_token(&tokens, &name, group, model_ids, true)
                .0
                .ok_or(ActivationFailure::ServerUnavailable)?
        }
    };
    let key = match fetch_token_key(api, origin, id).await {
        Ok(key) => key,
        Err(error) => {
            let _ = api
                .request(Method::DELETE, &format!("{origin}/api/token/{id}"), None)
                .await;
            return Err(error);
        }
    };
    Ok(TokenLease {
        id,
        key,
        created: true,
        retire_after_commit,
    })
}

/// 野菜的中转（yeschoy/new-api）创建密钥时把新密钥整个回传在 `data` 里，
/// 标准 NewAPI 什么都不回。有就直接用，省一次列表；名字对不上就当没有。
fn created_token_id(value: &Value, name: &str) -> Option<u64> {
    let created = data(value)?.as_object()?;
    (created.get("name").and_then(Value::as_str) == Some(name))
        .then(|| created.get("id").and_then(Value::as_u64))
        .flatten()
}

async fn delete_created_token(origin: &str, access_token: &str, lease: &TokenLease) {
    if lease.created {
        let _ = native_account_json(
            Method::DELETE,
            &format!("{origin}/api/token/{}", lease.id),
            access_token,
            None,
        )
        .await;
    }
}

async fn delete_created_tokens(origin: &str, access_token: &str, leases: &[(String, TokenLease)]) {
    futures::future::join_all(
        leases
            .iter()
            .map(|(_, lease)| delete_created_token(origin, access_token, lease)),
    )
    .await;
}

fn retire_superseded_tokens(origin: &str, access_token: &str, leases: &[(String, TokenLease)]) {
    let ids = leases
        .iter()
        .flat_map(|(_, lease)| lease.retire_after_commit.iter().copied())
        .collect::<std::collections::HashSet<_>>();
    if ids.is_empty() {
        return;
    }
    let origin = origin.to_owned();
    let access_token = access_token.to_owned();
    // Old scoped keys are no longer on the critical path once the new local
    // transaction has committed. Retire them in the background so several
    // bounded DELETE calls cannot hold the application restart for minutes.
    //
    // 起止各记一行。以前只在失败时 warn，日志里零命中既可能是「从没尝试」也
    // 可能是「全成功了」—— 2026-09-20 排查旧密钥堆积时就分不开这两种。
    tokio::spawn(async move {
        let attempted = ids.len();
        let mut retired_count = 0usize;
        log::info!("tool_token_cleanup stage=retire_start count={attempted}");
        for id in ids {
            let retired = native_account_json(
                Method::DELETE,
                &format!("{origin}/api/token/{id}"),
                &access_token,
                None,
            )
            .await;
            if matches!(retired, Ok((status, ref value)) if server_success(status, value)) {
                retired_count += 1;
            } else {
                log::warn!("tool_token_cleanup stage=retire_failed");
            }
        }
        log::info!("tool_token_cleanup stage=retire_done retired={retired_count} of={attempted}");
    });
}

async fn revoke_owned_tool_tokens(
    origin: &str,
    access_token: &str,
    tool_id: &str,
    credential: &ToolCredential,
) -> bool {
    if credential.models.is_empty() {
        return true;
    }
    let api = NativeTokenApi { access_token };
    let mut grouped = std::collections::BTreeMap::<String, Vec<String>>::new();
    for model in &credential.models {
        grouped
            .entry(model.billing_group.clone())
            .or_default()
            .push(model.model_id.clone());
    }
    let Ok(tokens) = list_tokens(&api, origin).await else {
        return false;
    };
    let mut complete = true;
    for (group, mut model_ids) in grouped {
        model_ids.sort_unstable();
        model_ids.dedup();
        // Both name shapes are this machine's; a disconnect leaves neither behind.
        let ids = [
            token_prefix(tool_id, &group),
            legacy_token_prefix(tool_id, &group),
        ]
        .iter()
        .flat_map(|prefix| {
            let (selected, stale) = select_token(&tokens, prefix, &group, &model_ids, false);
            selected.into_iter().chain(stale)
        })
        .collect::<Vec<_>>();
        for id in ids {
            match api
                .request(Method::DELETE, &format!("{origin}/api/token/{id}"), None)
                .await
            {
                Ok((status, value)) if server_success(status, &value) => {}
                _ => complete = false,
            }
        }
    }
    complete
}

/// 跳过失败的分组之后，谁还留下 —— 以及谁把整单挡住。
///
/// 抽成纯函数，是因为外层那个 async fn 要 Tauri handle、钥匙串和真网络，测不了；
/// 而这里每一条分支都会让用户看到完全不同的结果。`resume_if_configured` 上一轮
/// 就是这么找出一个静默 bug 的。
///
/// 两种情况仍然整单失败，各有各的理由：
///
/// - **一个分组都没成**：没有任何东西可写，"接入完成" 会是假话。
/// - **默认模型所在的分组没成**：默认模型是用户不选时应用实际会用的那个，价格
///   也跟着它。悄悄换成另一个幸存模型，等于替他改了计费对象 —— 那比失败更糟。
///   所以这里失败，但**点名是谁**，界面据此给出「移除它」的出路。
fn surviving_bindings(
    bindings: Vec<ModelBinding>,
    transports: Vec<Option<ModelTransport>>,
    leased_groups: &[String],
    skipped: &[SkippedBinding],
    default_model_id: &str,
) -> Result<(Vec<ModelBinding>, Vec<Option<ModelTransport>>), SkippedBinding> {
    if let Some(blocker) = skipped
        .iter()
        .find(|s| s.model_id == default_model_id)
        .or_else(|| leased_groups.is_empty().then(|| skipped.first()).flatten())
    {
        return Err(blocker.clone());
    }
    // 绑定和 transport 一起过滤 —— 两个 Vec 按下标配对，分开过滤迟早错位，
    // 而错位的后果是某个模型带着别人的 transport 被写进应用。
    Ok(bindings
        .into_iter()
        .zip(transports)
        .filter(|(binding, _)| {
            leased_groups
                .iter()
                .any(|group| group == &binding.billing_group)
        })
        .unzip())
}

fn credential_for_models(
    request: &ToolActivationRequest,
    origin: &str,
    // 存活下来的绑定，不是 `request.bindings()`：铸不出密钥的分组已经在上面被
    // 跳过了，这里再取一遍请求里的全量，就会要求一个不存在的 lease。
    bindings: &[ModelBinding],
    transports: &[Option<ModelTransport>],
    leases: &[(String, TokenLease)],
    previous: Option<&ToolCredential>,
) -> Result<ToolCredential, ActivationFailure> {
    let models = bindings
        .iter()
        .zip(transports)
        .map(|(m, transport)| {
            let lease = leases
                .iter()
                .find(|(group, _)| group == &m.billing_group)
                .ok_or(ActivationFailure::ConfigurationFailed("invalid_request"))?;
            Ok(ToolModelRoute {
                model_id: m.model_id.clone(),
                billing_group: m.billing_group.clone(),
                origin: origin.into(),
                api_key: lease.1.key.clone(),
                claude_transport: match transport {
                    Some(ModelTransport::Claude(t)) => Some(t.credential_value().into()),
                    _ => None,
                },
                codex_transport: match transport {
                    Some(ModelTransport::Codex(t)) => Some(t.credential_value().into()),
                    _ => None,
                },
            })
        })
        .collect::<Result<Vec<_>, ActivationFailure>>()?;
    let default = models
        .iter()
        .find(|m| m.model_id == request.model_id)
        .ok_or(ActivationFailure::ConfigurationFailed("invalid_request"))?;
    let local_token = match previous
        .and_then(|p| p.local_gateway_token.as_ref())
        .filter(|v| v.starts_with("ycg-") && v.len() == 68)
    {
        Some(v) => v.clone(),
        None => {
            let mut bytes = [0u8; 32];
            getrandom::fill(&mut bytes).map_err(|_| {
                ActivationFailure::Adapter(AdapterFailure::SecureStorageUnavailable)
            })?;
            format!(
                "ycg-{}",
                bytes.iter().map(|b| format!("{b:02x}")).collect::<String>()
            )
        }
    };
    Ok(ToolCredential {
        model_id: default.model_id.clone(),
        api_key: default.api_key.clone(),
        origin: origin.into(),
        claude_transport: default.claude_transport.clone(),
        codex_transport: default.codex_transport.clone(),
        local_gateway_token: Some(local_token),
        models,
    })
}

enum PreparedAdapter {
    ClaudeCode(claude_code::Prepared),
    ClaudeDesktop(claude_desktop::Prepared),
    CodexDesktop(codex_desktop::Prepared),
    Pi(pi::Prepared),
    DshWeb(dsh_web::Prepared),
    WorkBuddy(workbuddy::Prepared),
}

impl PreparedAdapter {
    fn changes(&self) -> &[tool_adapters::common::FileChange] {
        match self {
            Self::ClaudeCode(v) => v.changes(),
            Self::ClaudeDesktop(v) => v.changes(),
            Self::CodexDesktop(v) => v.changes(),
            Self::Pi(v) => v.changes(),
            Self::DshWeb(v) => v.changes(),
            Self::WorkBuddy(v) => v.changes(),
        }
    }

    fn codex_provider_id(&self) -> Option<&str> {
        match self {
            Self::CodexDesktop(value) => Some(value.provider_id()),
            _ => None,
        }
    }

    fn commit(&mut self) -> Result<(), AdapterFailure> {
        match self {
            Self::ClaudeCode(value) => value.commit(),
            Self::ClaudeDesktop(value) => value.commit(),
            Self::CodexDesktop(value) => value.commit(),
            Self::Pi(value) => value.commit(),
            Self::DshWeb(value) => value.commit(),
            Self::WorkBuddy(value) => value.commit(),
        }
    }

    fn rollback(&mut self) -> Result<(), AdapterFailure> {
        match self {
            Self::ClaudeCode(value) => value.rollback(),
            Self::ClaudeDesktop(value) => value.rollback(),
            Self::CodexDesktop(value) => value.rollback(),
            Self::Pi(value) => value.rollback(),
            Self::DshWeb(value) => value.rollback(),
            Self::WorkBuddy(value) => value.rollback(),
        }
    }
}

fn prepare_adapter(
    request: &ToolActivationRequest,
    credential: &ToolCredential,
    model_transport: Option<ModelTransport>,
    codex_provider_hint: Option<&str>,
) -> Result<PreparedAdapter, AdapterFailure> {
    let home = tool_adapters::user_home()
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
        "dsh_web" => dsh_web::prepare_catalog(&home, &origin, &request.model_id, &models)
            .map(PreparedAdapter::DshWeb),
        "workbuddy" => {
            workbuddy::prepare_catalog(&home, credential).map(PreparedAdapter::WorkBuddy)
        }
        _ => Err(AdapterFailure::ConfigurationFailed("invalid_request")),
    }
}

async fn start_local_adapter(
    request: &ToolActivationRequest,
    credential: &ToolCredential,
    claude_code_runtime: &claude_code::ClaudeCodeRuntimeState,
    claude_runtime: &claude_desktop::ClaudeDesktopRuntimeState,
    codex_runtime: &codex_desktop::CodexRuntimeState,
) -> Result<(), AdapterFailure> {
    match request.tool_id.as_str() {
        "claude_code" => claude_code_runtime.start(credential.clone()).await,
        "claude_desktop" => claude_runtime.start(credential.clone()).await.map(|_| ()),
        "codex_desktop" => {
            // 桥要先在监听，Codex 才有地方可发 —— 它的 config.toml 现在指向
            // `127.0.0.1:15731/codex/v1`。
            codex_runtime.start(credential.clone()).await?;
            codex_desktop::ensure_credential_ready(credential).await
        }
        "pi" | "dsh_web" | "workbuddy" => Ok(()),
        _ => Err(AdapterFailure::ConfigurationFailed("invalid_request")),
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
fn needs_claude_login_consent(
    tool_id: &str,
    already_consented: bool,
    present: impl FnOnce() -> bool,
) -> bool {
    tool_id == "claude_code" && !already_consented && present()
}

async fn open_configured_adapter(
    request: &ToolActivationRequest,
    installation: &ResolvedInstallation,
    credential: &ToolCredential,
    dsh_runtime: &dsh_web::DshRuntimeState,
) -> Result<(), AdapterFailure> {
    match request.tool_id.as_str() {
        "claude_desktop" => claude_desktop::launch(&installation.path),
        "codex_desktop" => codex_desktop::launch(&installation.path),
        "dsh_web" => {
            dsh_web::open_existing(dsh_runtime, installation, credential.upstream_key()).await
        }
        "workbuddy" => {
            desktop_lifecycle::open_unless_running("workbuddy", &installation.path).await
        }
        "claude_code" | "pi" => {
            let home = tool_adapters::user_home()
                .ok_or(AdapterFailure::ConfigurationFailed("home_unavailable"))?;
            tool_adapters::terminal_launch::launch_async(installation, &request.tool_id, &home)
                .await
        }
        _ => Err(AdapterFailure::ConfigurationFailed("invalid_request")),
    }
}

struct DesktopReloadGuard {
    tool_id: String,
    path: PathBuf,
    closed: bool,
}

impl DesktopReloadGuard {
    fn new(request: &ToolActivationRequest, installation: &ResolvedInstallation) -> Self {
        Self {
            tool_id: request.tool_id.clone(),
            path: installation.path.clone(),
            closed: false,
        }
    }

    fn record_normal_quit(&mut self, closed: bool) {
        self.closed |= closed;
    }

    fn disarm(&mut self) {
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

async fn stop_helper_runtime(
    tool: &str,
    claude_code_runtime: &claude_code::ClaudeCodeRuntimeState,
    claude_runtime: &claude_desktop::ClaudeDesktopRuntimeState,
    codex_runtime: &codex_desktop::CodexRuntimeState,
    dsh_runtime: &dsh_web::DshRuntimeState,
) {
    match tool {
        "claude_code" => claude_code_runtime.stop().await,
        "claude_desktop" => claude_runtime.stop().await,
        "dsh_web" => dsh_runtime.stop().await,
        "codex_desktop" => codex_runtime.stop().await,
        // Pi connects straight to the relay origin.
        _ => {}
    }
}

async fn codex_history_on_worker<T: Send + 'static>(
    operation: impl FnOnce() -> Result<T, AdapterFailure> + Send + 'static,
) -> Result<T, AdapterFailure> {
    tokio::task::spawn_blocking(operation)
        .await
        .map_err(|_| AdapterFailure::ConfigurationFailed("codex_history_takeover_failed"))?
}

#[allow(clippy::too_many_arguments)] // Explicit transaction/runtime inputs; no hidden mutable global rollback context.
async fn restore_after_failure(
    request: &ToolActivationRequest,
    prepared: &mut PreparedAdapter,
    recovery_record: &connection_recovery::Record,
    credential_before: Option<&str>,
    previous_record: Option<ToolCredential>,
    origin: &str,
    access_token: &str,
    leases: &[(String, TokenLease)],
    claude_code_runtime: &claude_code::ClaudeCodeRuntimeState,
    claude_runtime: &claude_desktop::ClaudeDesktopRuntimeState,
    codex_runtime: &codex_desktop::CodexRuntimeState,
    dsh_runtime: &dsh_web::DshRuntimeState,
    permit: &shutdown_coordinator::OperationPermit,
    codex_history: &mut Option<crate::codex_history_takeover::Takeover>,
) -> Option<AdapterFailure> {
    let rollback_failed = if desktop_lifecycle::requires_reload(&request.tool_id) {
        connection_recovery::restore_attempt(recovery_record).is_err()
    } else {
        prepared.rollback().is_err()
    };
    if rollback_failed {
        // The live files may still reference the new key. Keep both keys and
        // the encrypted pending checkpoint until recovery really succeeds.
        return restoration_failure(true, false);
    }
    if let Some(history) = codex_history.as_mut() {
        if history.rollback().is_err() {
            return restoration_failure(true, false);
        }
    }
    let credential_failure = tool_credentials::restore(&request.tool_id, credential_before).err();
    if credential_failure.is_some() {
        return restoration_failure(false, true);
    }
    let runtime_result = permit
        .cancel_safe(async {
            if request.tool_id == "dsh_web" {
                dsh_runtime.stop().await;
            }
            if request.tool_id == "claude_code" {
                claude_code_runtime.stop().await;
                if let Some(previous) = previous_record.clone().filter(|credential| {
                    credential.has_model_set() && !shutdown_coordinator::global().is_shutting_down()
                }) {
                    claude_code_runtime.start(previous).await?;
                }
            }
            if request.tool_id == "codex_desktop" {
                codex_runtime.stop().await;
                if let Some(previous) = previous_record
                    .clone()
                    .filter(|_| !shutdown_coordinator::global().is_shutting_down())
                {
                    codex_runtime.start(previous).await?;
                }
            }
            if request.tool_id == "claude_desktop" {
                claude_runtime.stop().await;
                if let Some(previous) = previous_record
                    .clone()
                    .filter(|_| !shutdown_coordinator::global().is_shutting_down())
                {
                    claude_runtime.start(previous).await?;
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
    delete_created_tokens(origin, access_token, leases).await;
    None
}

// Codex history must be committed after its provider config but before Codex
// starts and reads the old provider bucket. The durable receipt still follows
// startup observation. Kept separate so fixtures exercise the exact ordering.
async fn finalize_after_start<BeforeStart, Start>(
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

async fn commit_codex_history_on_worker(
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
async fn recover_after_stop<Stop, Restore, RestoreFuture>(
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
fn restoration_failure(files_failed: bool, credential_failed: bool) -> Option<AdapterFailure> {
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

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ActivationTargetScanRequest {
    request_id: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActivationTargetScanResponse {
    request_id: String,
    schema_version: u8,
    platform: &'static str,
    targets: Vec<tool_adapters::TargetProjection>,
}

#[tauri::command]
pub async fn scan_activation_targets_v1(
    request: ActivationTargetScanRequest,
) -> Result<ActivationTargetScanResponse, String> {
    if !request_id_is_valid(&request.request_id) {
        return Err("invalid_activation_target_scan".into());
    }
    Ok(ActivationTargetScanResponse {
        request_id: request.request_id,
        schema_version: 1,
        platform: crate::tool_discovery::platform_name(),
        targets: tool_adapters::scan_targets().await,
    })
}

#[tauri::command]
#[allow(clippy::too_many_arguments)] // Tauri injects the independently owned runtime states.
pub async fn configure_desktop_tool_v2(
    app: tauri::AppHandle,
    activation_state: tauri::State<'_, ActivationOperationState>,
    account_state: tauri::State<'_, AccountV2State>,
    installation_state: tauri::State<'_, crate::app_installation::AppInstallationState>,
    claude_code_runtime: tauri::State<'_, claude_code::ClaudeCodeRuntimeState>,
    claude_runtime: tauri::State<'_, claude_desktop::ClaudeDesktopRuntimeState>,
    codex_runtime: tauri::State<'_, codex_desktop::CodexRuntimeState>,
    dsh_runtime: tauri::State<'_, dsh_web::DshRuntimeState>,
    request: ToolActivationRequest,
) -> Result<ToolActivationProjection, String> {
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
    emit_activation_progress(&app, &request, "queued", 0);
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
            &account_state,
        ) {
            Ok(epoch) => epoch,
            Err(reason) => {
                return Ok(ActivationFailure::ConfigurationFailed(reason).projection(&request))
            }
        }
    } else {
        match native_session_epoch(&account_state) {
            Ok(epoch) => epoch,
            Err(_) => return Ok(ActivationFailure::ServerUnavailable.projection(&request)),
        }
    };
    let _activation_guard = match permit.cancel_safe(ACTIVATION_LOCK.lock()).await {
        Ok(g) => g,
        Err(_) => return Ok(cancelled()),
    };
    stage!("lock");
    if is_cancelled() {
        return Ok(cancelled());
    }
    emit_activation_progress(&app, &request, "checking_application", 1);
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
                    &claude_code_runtime,
                    &claude_runtime,
                    &codex_runtime,
                    &dsh_runtime,
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
    emit_activation_progress(&app, &request, "authenticating", 2);
    let (origin, access_token) =
        match native_session_access(&account_state, &request.line_id, session_epoch).await {
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
    let bindings = request.bindings();
    if is_cancelled() {
        return Ok(cancelled());
    }
    emit_activation_progress(&app, &request, "checking_models", 3);
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
    emit_activation_progress(&app, &request, "securing_access", 4);
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
        if is_cancelled() || ensure_session_epoch(&account_state, session_epoch).is_err() {
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
    if is_cancelled() || ensure_session_epoch(&account_state, session_epoch).is_err() {
        delete_created_tokens(&origin, &access_token, &leases).await;
        return Ok(if is_cancelled() {
            cancelled()
        } else {
            ActivationFailure::ConfigurationFailed("account_changed").projection(&request)
        });
    }
    if request.tool_id == "codex_desktop" {
        // 接入时探一次真实请求，把结论落进凭据 —— 桥之后按模型读它。
        // 放在这里是因为要用 lease 铸出来的受限 key：`/v1/*` 不收账户令牌，
        // 而 lease 到上一步才存在。
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
    emit_activation_progress(&app, &request, "preparing_settings", 5);
    // A pre-0.4.19 receipt may currently show `yeschoy`, while its encrypted
    // original snapshot still records CC Switch's provider identifier. Reuse
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
    // 分支会把它搬回去。
    if request.tool_id == "claude_code" && request.displace_claude_login {
        match tokio::task::spawn_blocking(crate::claude_login_takeover::take_over).await {
            Ok(Ok(_)) => {}
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
        delete_created_tokens(&origin, &access_token, &leases).await;
        return Ok(cancelled());
    }
    // No cancellation point between credential publication and local file commit.
    let reopen_previous = reload_guard.closed;
    // From here failures require an explicit successful rollback, not a Drop
    // that could reopen a partially-written or unrecoverable configuration.
    reload_guard.disarm();
    emit_activation_progress(&app, &request, "applying_settings", 6);
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
                &claude_code_runtime,
                &claude_runtime,
                &codex_runtime,
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
            if desktop_lifecycle::requires_reload(&request.tool_id) {
                if is_cancelled() {
                    return Err(AdapterFailure::ConfigurationFailed("activation_cancelled"));
                }
                desktop_open_attempted = true;
                emit_activation_progress(&app, &request, "opening_application", 7);
                emit_activation_progress(&app, &request, "checking_application_started", 7);
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
            if ensure_session_epoch(&account_state, session_epoch).is_err() {
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
        emit_activation_progress(&app, &request, "restoring_settings", 6);
        let cleanup = recover_after_stop(
            async {
                if desktop_open_attempted {
                    desktop_lifecycle::quit_for_reconfigure(&request.tool_id, &installation.path)
                        .await?;
                }
                Ok(())
            },
            || {
                restore_after_failure(
                    &request,
                    &mut prepared,
                    &recovery_record,
                    credential_before.as_deref(),
                    previous_record,
                    &origin,
                    &access_token,
                    &leases,
                    &claude_code_runtime,
                    &claude_runtime,
                    &codex_runtime,
                    &dsh_runtime,
                    &permit,
                    &mut codex_history,
                )
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
    let open_result = if desktop_lifecycle::requires_reload(&request.tool_id) {
        Ok(()) // Already opened and observed before the transaction finished.
    } else {
        emit_activation_progress(&app, &request, "opening_application", 7);
        open_configured_adapter(&request, &installation, &credential, &dsh_runtime).await
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
    emit_activation_progress(&app, &request, "complete", 7);
    // 这两条是「设置已经写进去了」的出口，所以必须带上 `skipped`：其余返回点
    // 都已经回滚，跳过了谁不再有意义。
    Ok(ToolActivationProjection::new(
        &request,
        "ready",
        if desktop_lifecycle::requires_reload(&request.tool_id) {
            "desktop_start_observed"
        } else {
            "configuration_ready"
        },
    )
    .with_skipped(skipped))
}

const CONNECTION_TOOLS: [&str; 6] = [
    "claude_code",
    "claude_desktop",
    "codex_desktop",
    "pi",
    "dsh_web",
    "workbuddy",
];

/// Called only by the exit owner after admission closes and active operations
/// drain. Blocking storage work stays off the UI/async executor. No remote keys
/// are revoked, and missing originals fail visibly instead of inventing them.
pub(crate) async fn restore_connection_for_exit(tool: &'static str) -> Result<(), ()> {
    let _guard = tokio::time::timeout(CONNECTION_LOCK_WAIT_TIMEOUT, ACTIVATION_LOCK.lock())
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
        // Same blocking-worker boundary as CC Switch's provider commands. Keep
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

pub(crate) fn needs_background(tool: &str, credential: &ToolCredential) -> bool {
    if tool == "workbuddy" {
        return false;
    }
    credential.has_model_set()
        || matches!(tool, "claude_desktop" | "dsh_web")
        || credential.claude_transport.as_deref() == Some("chat_bridge")
        || credential.codex_transport.as_deref() == Some("chat_bridge")
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
        "dsh_web" => {
            vec![dsh_web::dsh_home(&home, std::env::var_os("DSH_HOME"))?.join("settings.yaml")]
        }
        "workbuddy" => vec![home.join(".workbuddy/models.json")],
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

#[tauri::command]
pub async fn manage_tool_connections_v1(
    account_state: tauri::State<'_, AccountV2State>,
    claude_code_runtime: tauri::State<'_, claude_code::ClaudeCodeRuntimeState>,
    claude_runtime: tauri::State<'_, claude_desktop::ClaudeDesktopRuntimeState>,
    codex_runtime: tauri::State<'_, codex_desktop::CodexRuntimeState>,
    dsh_runtime: tauri::State<'_, dsh_web::DshRuntimeState>,
    request: ConnectionRequest,
) -> Result<ConnectionResponse, String> {
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
                &ACTIVATION_LOCK,
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
        permit.cancel_safe(ACTIVATION_LOCK.lock()),
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
        let session_epoch = native_session_epoch(&account_state).ok();
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
                        &claude_code_runtime,
                        &claude_runtime,
                        &codex_runtime,
                        &dsh_runtime,
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
                            &account_state,
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
mod tests {
    use super::*;

    /// 只有 `Inconclusive` 才重问，而且问到有结论就停。
    ///
    /// 这条守的是这次改动的全部意义：探测结果是**落盘、之后不再探**的，
    /// 所以接入那一瞬间的一次网抖会让模型永久停在直连。不重问 = 没有第二次机会。
    ///
    /// 同时守「问到就停」—— 每一次探测都是一个真实请求，成功那次还会产生用量。
    /// 已经有结论还接着问，是白花用户的钱。
    #[tokio::test]
    async fn an_inconclusive_probe_is_asked_again_but_a_clear_one_is_not() {
        use std::sync::atomic::{AtomicU8, Ordering};

        // `before` 次 503（归入 Inconclusive），之后 200。
        async fn stub(before: u8) -> (String, Arc<AtomicU8>) {
            let seen = Arc::new(AtomicU8::new(0));
            let counter = seen.clone();
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let origin = format!("http://{}", listener.local_addr().unwrap());
            let router = axum::Router::new().route(
                "/v1/responses",
                axum::routing::post(move || {
                    let counter = counter.clone();
                    async move {
                        let n = counter.fetch_add(1, Ordering::SeqCst);
                        if n < before {
                            axum::http::StatusCode::SERVICE_UNAVAILABLE
                        } else {
                            axum::http::StatusCode::OK
                        }
                    }
                }),
            );
            tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
            (origin, seen)
        }

        let client = reqwest::Client::builder()
            .no_proxy()
            .timeout(Duration::from_secs(5))
            .build()
            .unwrap();

        // 一问就有结论：只发一次，不多花一个请求。
        let (origin, seen) = stub(0).await;
        let (outcome, tries) = probe_until_conclusive(&client, &origin, "sk-x", "m", true).await;
        assert_eq!(outcome, ProbeOutcome::Accepted);
        assert_eq!(tries, 1);
        assert_eq!(seen.load(Ordering::SeqCst), 1, "有结论了还接着问");

        // 前两次抖动，第三次答上来 —— 正是要救的那种情况。
        let (origin, seen) = stub(2).await;
        let (outcome, tries) = probe_until_conclusive(&client, &origin, "sk-x", "m", true).await;
        assert_eq!(outcome, ProbeOutcome::Accepted, "重问之后应当拿到结论");
        assert_eq!(tries, 3);
        assert_eq!(seen.load(Ordering::SeqCst), 3);

        // 一直抖：问满上限就放弃，落回 Inconclusive（而不是当成「被拒绝」）。
        let (origin, seen) = stub(u8::MAX).await;
        let (outcome, tries) = probe_until_conclusive(&client, &origin, "sk-x", "m", true).await;
        assert_eq!(outcome, ProbeOutcome::Inconclusive);
        assert_eq!(tries, PROBE_ATTEMPTS);
        assert_eq!(seen.load(Ordering::SeqCst), PROBE_ATTEMPTS);
    }

    /// 探测结论 → 传输方式，**穷举**。
    ///
    /// 穷举而不是挑几个例子，是因为这张表的重点在于「只有一格会翻转」。
    /// 挑例子写，以后有人放宽某一格（比如让 `Inconclusive` 也走桥），
    /// 测试不会红。穷举会。
    ///
    /// 为什么必须偏向直连：错判成桥，是把一个原生说 Responses 的模型转一圈，
    /// 它真的 `encrypted_content` 被我们伪造的令牌换掉 —— **主动降级**；
    /// 错判成直连，是这个模型维持现状（本来就不能用）—— **不造成回退**。
    /// 两种错不对称。
    #[test]
    fn only_a_refused_responses_with_a_working_chat_moves_a_model_onto_the_bridge() {
        use codex_desktop::CodexTransport::{ChatBridge, DirectResponses};
        use ProbeOutcome::{Accepted, Inconclusive, Rejected};
        for responses in [Accepted, Rejected, Inconclusive] {
            for chat in [None, Some(Accepted), Some(Rejected), Some(Inconclusive)] {
                let expected = if responses == Rejected && chat == Some(Accepted) {
                    ChatBridge
                } else {
                    DirectResponses
                };
                assert_eq!(
                    transport_from_probes(responses, chat),
                    expected,
                    "responses={responses:?} chat={chat:?}"
                );
            }
        }
    }

    /// 网络层的三档归类。5xx 和网络错误必须落到 `Inconclusive` ——
    /// 归成 `Rejected` 的话，中转抖一下就会把模型永久切到桥上。
    #[test]
    fn a_server_side_failure_is_never_read_as_a_protocol_refusal() {
        // 这里钉的是 `probe_once` 里那三条 match 臂的意图。状态码的分类交给
        // reqwest 的 `is_success` / `is_client_error`，我们只钉边界。
        for status in [200u16, 201, 204] {
            assert!(reqwest::StatusCode::from_u16(status).unwrap().is_success());
        }
        for status in [400u16, 401, 404, 422, 429] {
            let code = reqwest::StatusCode::from_u16(status).unwrap();
            assert!(!code.is_success() && code.is_client_error(), "{status}");
        }
        for status in [500u16, 502, 503, 504] {
            let code = reqwest::StatusCode::from_u16(status).unwrap();
            assert!(!code.is_success() && !code.is_client_error(), "{status}");
        }
    }

    /// 两个端点的请求体形状不一样，发错一个就等于探了个寂寞：
    /// 拿 Chat 的形状去问 Responses，中转会回参数错误（4xx），
    /// 于是每个模型都会被判成「Responses 不行」。
    #[test]
    fn each_endpoint_gets_the_body_shape_it_expects() {
        let responses = probe_body("kimi-k3", true);
        assert_eq!(responses["model"], "kimi-k3");
        assert!(responses.get("input").is_some());
        assert!(responses.get("messages").is_none());
        assert_eq!(responses["max_output_tokens"], 16);
        assert_eq!(responses["stream"], false);

        let chat = probe_body("kimi-k3", false);
        assert_eq!(chat["model"], "kimi-k3");
        assert!(chat.get("messages").is_some());
        assert!(chat.get("input").is_none());
        assert_eq!(chat["max_tokens"], 16);
        assert_eq!(chat["stream"], false);
    }

    #[tokio::test]
    async fn refused_stop_does_not_restore_files_or_keys_or_delete_live_tokens() {
        assert!(matches!(
            recover_after_stop(
                async { Err(AdapterFailure::LaunchError("graceful_restart_required")) },
                || async { panic!("must not change live files or revoke live keys") }
            )
            .await,
            Some(AdapterFailure::ConfigurationFailed(
                "desktop_recovery_waiting_for_exit"
            ))
        ));
        assert!(matches!(
            recover_after_stop(async { Ok(()) }, || async {
                Some(AdapterFailure::ConfigurationFailed(
                    "credential_restore_failed",
                ))
            })
            .await,
            Some(AdapterFailure::ConfigurationFailed(
                "credential_restore_failed"
            ))
        ));
        assert!(recover_after_stop(async { Ok(()) }, || async { None })
            .await
            .is_none());
    }

    #[tokio::test]
    async fn desktop_receipt_cannot_complete_before_startup_observation() {
        use std::cell::RefCell;
        for startup_ok in [false, true] {
            let events = RefCell::new(Vec::new());
            let result = finalize_after_start(
                Ok(()),
                async {
                    events.borrow_mut().push("history-commit");
                    Ok(())
                },
                async {
                    events.borrow_mut().push("start-observed");
                    if startup_ok {
                        Ok(())
                    } else {
                        Err(AdapterFailure::LaunchError("desktop_start_unconfirmed"))
                    }
                },
                || {
                    events.borrow_mut().push("finish-receipt");
                    Ok(())
                },
            )
            .await;
            assert_eq!(result.is_ok(), startup_ok);
            assert_eq!(
                *events.borrow(),
                if startup_ok {
                    vec!["history-commit", "start-observed", "finish-receipt"]
                } else {
                    vec!["history-commit", "start-observed"]
                }
            );
        }
        assert!(finalize_after_start(
            Err(AdapterFailure::ConfigurationFailed(
                "synthetic-write-failed"
            )),
            async { panic!("must not commit history after a failed configuration") },
            async { panic!("must not launch after a failed configuration") },
            || panic!("must not finish after a failed configuration")
        )
        .await
        .is_err());
        assert!(
            finalize_after_start(Ok(()), async { Ok(()) }, async { Ok(()) }, || Err(
                AdapterFailure::ConfigurationFailed("activation_cancelled")
            ))
            .await
            .is_err()
        );
    }

    #[tokio::test]
    async fn inspection_regression_one_adapter_panic_keeps_five_other_results() {
        static LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
        let result = inspect_on_worker(&LOCK, Duration::from_secs(2), || {
            inspect_connections_with(|tool| {
                if tool == "codex_desktop" {
                    panic!("synthetic-private-value-must-not-be-in-response");
                }
                let mut projection = ConnectionProjection::empty(tool);
                if tool == "claude_desktop" {
                    projection.state = "connected";
                    projection.reason_code = "connected";
                }
                projection
            })
        })
        .await
        .unwrap();
        assert_eq!(result.len(), 6);
        assert_eq!(result[1].state, "connected");
        assert_eq!(result[2].state, "unavailable");
        assert_eq!(result[2].reason_code, "connection_inspection_failed");
        assert_eq!(
            result.iter().filter(|c| c.state == "not_connected").count(),
            4
        );
        assert!(!serde_json::to_string(&result)
            .unwrap()
            .contains("synthetic-private"));
        assert!(LOCK.try_lock().is_ok());
        let next = inspect_on_worker(&LOCK, Duration::from_secs(2), || {
            inspect_connections_with(ConnectionProjection::empty)
        })
        .await
        .unwrap();
        assert!(next.iter().all(|c| c.state == "not_connected"));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn ru076_inspection_worker_keeps_runtime_responsive_and_write_lock_until_read_finishes() {
        static LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        let (finish_tx, finish_rx) = std::sync::mpsc::channel();
        let task = tokio::spawn(inspect_on_worker(
            &LOCK,
            Duration::from_secs(2),
            move || {
                started_tx.send(()).unwrap();
                finish_rx.recv_timeout(Duration::from_secs(2)).unwrap();
                7
            },
        ));
        started_rx.await.unwrap();
        // If the synchronous reader ran on this current-thread executor, we
        // could not reach this assertion or send its completion signal.
        assert!(LOCK.try_lock().is_err());
        tokio::task::yield_now().await;
        finish_tx.send(()).unwrap();
        assert_eq!(task.await.unwrap(), Ok(7));
        assert!(LOCK.try_lock().is_ok());
    }

    #[tokio::test]
    async fn codex_history_worker_does_not_block_async_runtime() {
        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        let (finish_tx, finish_rx) = std::sync::mpsc::channel();
        let task = tokio::spawn(codex_history_on_worker(move || {
            started_tx.send(()).unwrap();
            finish_rx.recv_timeout(Duration::from_secs(2)).unwrap();
            Ok::<_, AdapterFailure>(7)
        }));
        started_rx.await.unwrap();
        tokio::time::timeout(Duration::from_millis(100), tokio::task::yield_now())
            .await
            .unwrap();
        finish_tx.send(()).unwrap();
        assert_eq!(task.await.unwrap().unwrap(), 7);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn ru076_timed_out_reader_does_not_release_write_lock_or_queue_another_os_read() {
        static LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        let (finish_tx, finish_rx) = std::sync::mpsc::channel();
        let task = tokio::spawn(inspect_on_worker(
            &LOCK,
            Duration::from_millis(80),
            move || {
                started_tx.send(()).unwrap();
                finish_rx.recv_timeout(Duration::from_secs(2)).unwrap();
            },
        ));
        started_rx.await.unwrap();
        assert_eq!(task.await.unwrap(), Err("connection_inspect_timed_out"));
        assert!(LOCK.try_lock().is_err());
        let invoked = Arc::new(AtomicBool::new(false));
        let worker_invoked = invoked.clone();
        assert_eq!(
            inspect_on_worker(&LOCK, Duration::from_millis(20), move || {
                worker_invoked.store(true, Ordering::SeqCst);
            })
            .await,
            Err("connection_inspect_timed_out")
        );
        assert!(!invoked.load(Ordering::SeqCst));
        finish_tx.send(()).unwrap();
        let _guard = tokio::time::timeout(Duration::from_secs(2), LOCK.lock())
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn ru076_panicking_reader_has_a_safe_failure_and_releases_the_lock() {
        static LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
        let result = inspect_on_worker(&LOCK, Duration::from_secs(2), || {
            panic!("synthetic private storage detail")
        })
        .await;
        assert_eq!(result, Err("connection_inspection_failed"));
        assert!(LOCK.try_lock().is_ok());
    }

    /// 探测结论要一路走到桥：`transports` → 凭据的逐模型 `codex_transport`
    /// → `needs_chat_conversion`。
    ///
    /// 这条缝没人钉过。中间任何一环改了字符串或字段，症状都是
    /// **「探测明明判了走桥，桥却一直直连」** —— 没有报错、没有日志异常，
    /// 只是那几个模型继续用不了，和没做探测时一模一样。
    #[test]
    fn a_probe_verdict_reaches_the_bridge() {
        use crate::codex_responses_bridge::needs_chat_conversion;
        use codex_desktop::CodexTransport::{ChatBridge, DirectResponses};

        let request: ToolActivationRequest = serde_json::from_value(json!({
            "requestId": "fixture",
            "lineId": "mainland_optimized",
            "toolId": "codex_desktop",
            "modelId": "gemini-3.8-flash",
            "billingGroup": "cheap",
            "installationId": "i0123456789abcdef",
            "models": [
                {"modelId": "gemini-3.8-flash", "billingGroup": "cheap"},
                {"modelId": "gpt-5.6-sol", "billingGroup": "cheap"},
            ],
        }))
        .unwrap();
        let leases = vec![(
            "cheap".to_owned(),
            TokenLease {
                id: 1,
                key: "synthetic-key".into(),
                created: false,
                retire_after_commit: vec![],
            },
        )];
        let credential = credential_for_models(
            &request,
            "https://yeschoy.com",
            &request.bindings(),
            &[
                Some(ModelTransport::Codex(ChatBridge)),
                Some(ModelTransport::Codex(DirectResponses)),
            ],
            &leases,
            None,
        )
        .unwrap();

        // 被判成桥的那个，桥要认得。
        assert!(needs_chat_conversion(&credential, "gemini-3.8-flash"));
        // 原生说 Responses 的那个，必须原样直连 —— 它带的是真的
        // `encrypted_content`，转一圈就是主动降级。
        assert!(!needs_chat_conversion(&credential, "gpt-5.6-sol"));
    }

    use crate::tool_adapters::codex_desktop::CodexTransport::{ChatBridge, DirectResponses};

    fn binding(model: &str, group: &str) -> ModelBinding {
        ModelBinding {
            model_id: model.into(),
            billing_group: group.into(),
        }
    }

    fn skip(model: &str, group: &str) -> SkippedBinding {
        SkippedBinding {
            model_id: model.into(),
            billing_group: group.into(),
            reason_code: "server_unavailable",
        }
    }

    #[test]
    fn a_failed_group_is_skipped_without_taking_the_rest_of_the_activation_with_it() {
        // 以前第一个铸不出密钥的分组就让整次接入回滚，用户只看到一句
        // 「暂时无法从野菜API获取接入信息」—— 而那句话背后有十七个原因，
        // 日志里一行都没有。重试多少次都是同一个分组失败。
        let bindings = vec![
            binding("kimi-k3", "default"),
            binding("glm-5.3-flash", "promo"),
            binding("gemini-3.8-flash", "default"),
        ];
        let transports = vec![
            None,
            Some(ModelTransport::Codex(ChatBridge)),
            Some(ModelTransport::Codex(DirectResponses)),
        ];
        let (kept, kept_transports) = surviving_bindings(
            bindings,
            transports,
            &["default".to_owned()],
            &[skip("glm-5.3-flash", "promo")],
            "kimi-k3",
        )
        .expect("默认模型还在，应当继续");

        assert_eq!(
            kept.iter().map(|b| b.model_id.as_str()).collect::<Vec<_>>(),
            ["kimi-k3", "gemini-3.8-flash"],
        );
        // transport 必须跟着各自的绑定走。分开过滤的话这里会拿到 ChatBridge ——
        // 那意味着某个模型带着别人的 transport 被写进应用。
        assert_eq!(kept_transports.len(), kept.len());
        assert!(kept_transports[0].is_none());
        assert!(matches!(
            kept_transports[1],
            Some(ModelTransport::Codex(DirectResponses))
        ));
    }

    #[test]
    fn the_default_models_group_failing_stops_the_activation_and_names_it() {
        // 默认模型是用户不选时应用实际会用的那个，价格也跟着它。悄悄换成另一个
        // 幸存模型，等于替他改了计费对象。所以这里失败，但必须点名是谁 ——
        // 界面据此给出「移除它」的出路，用户一次点击就能继续。
        let blocker = surviving_bindings(
            vec![
                binding("glm-5.3-flash", "promo"),
                binding("kimi-k3", "default"),
            ],
            vec![None, None],
            &["default".to_owned()],
            &[skip("glm-5.3-flash", "promo")],
            "glm-5.3-flash",
        )
        .expect_err("默认模型所在的分组失败了，不该继续");
        assert_eq!(blocker.model_id, "glm-5.3-flash");
        assert_eq!(blocker.billing_group, "promo");
    }

    #[test]
    fn nothing_leased_means_nothing_to_write_so_it_still_fails() {
        let blocker = surviving_bindings(
            vec![binding("glm-5.3-flash", "promo")],
            vec![None],
            &[],
            &[skip("glm-5.3-flash", "promo")],
            // 默认模型不在跳过名单里也一样：一个分组都没成，没有东西可写，
            // 报「接入完成」就是假话。
            "someone-else",
        )
        .expect_err("一个 lease 都没有，不该继续");
        assert_eq!(blocker.model_id, "glm-5.3-flash");
    }

    #[test]
    fn ru042_activation_models_require_explicit_unique_binding_and_default_member() {
        let json = json!({"requestId":"fixture", "lineId":"mainland_optimized", "toolId":"pi", "modelId":"a", "billingGroup":"cheap", "installationId":"i0123456789abcdef"});
        let legacy: ToolActivationRequest = serde_json::from_value(json.clone()).unwrap();
        assert!(request_is_valid(&legacy));
        let mut modern = json;
        modern["models"] = json!([{"modelId":"a","billingGroup":"cheap"},{"modelId":"b","billingGroup":"standard"}]);
        let r: ToolActivationRequest = serde_json::from_value(modern.clone()).unwrap();
        assert!(request_is_valid(&r));
        let leases = vec![
            (
                "cheap".into(),
                TokenLease {
                    id: 1,
                    key: "synthetic-key-cheap".into(),
                    created: false,
                    retire_after_commit: vec![],
                },
            ),
            (
                "standard".into(),
                TokenLease {
                    id: 2,
                    key: "synthetic-key-standard".into(),
                    created: false,
                    retire_after_commit: vec![],
                },
            ),
        ];
        let c = credential_for_models(
            &r,
            "https://yeschoy.com",
            &r.bindings(),
            &[None, None],
            &leases,
            None,
        )
        .unwrap();
        assert_eq!(
            c.resolve_model("b").unwrap().api_key,
            "synthetic-key-standard"
        );
        let c2 = credential_for_models(
            &r,
            "https://yeschoy.com",
            &r.bindings(),
            &[None, None],
            &leases,
            Some(&c),
        )
        .unwrap();
        assert_eq!(c.local_gateway_token, c2.local_gateway_token);
        let out = serde_json::to_value(ToolActivationProjection::new(
            &r,
            "ready",
            "tool_request_verified",
        ))
        .unwrap();
        // 5：新增 `skipped`（跳过的绑定）。渲染层按键集精确校验，加字段必须升版本。
        assert_eq!(out["schemaVersion"], 5);
        assert_eq!(out["models"].as_array().unwrap().len(), 2);
        assert!(out["skipped"].as_array().unwrap().is_empty());
        assert!(!out.to_string().contains("synthetic-key"));
        for bad in [
            json!([]),
            json!([{"modelId":"b","billingGroup":"standard"}]),
            json!([{"modelId":"a","billingGroup":"cheap"},{"modelId":"a","billingGroup":"standard"}]),
        ] {
            modern["models"] = bad;
            assert!(!request_is_valid(
                &serde_json::from_value(modern.clone()).unwrap()
            ));
        }
    }

    #[test]
    fn restoration_errors_never_masquerade_as_initial_storage_failure() {
        assert_eq!(restoration_failure(false, false), None);
        assert_eq!(
            restoration_failure(true, false),
            Some(AdapterFailure::ConfigurationFailed(
                "configuration_rollback_failed"
            ))
        );
        assert_eq!(
            restoration_failure(false, true),
            Some(AdapterFailure::ConfigurationFailed(
                "credential_restore_failed"
            ))
        );
        assert_eq!(
            restoration_failure(true, true),
            Some(AdapterFailure::ConfigurationFailed(
                "configuration_rollback_failed"
            ))
        );
    }

    #[derive(Default)]
    struct FakeTokens {
        tokens: std::sync::Mutex<Vec<Value>>,
        requests: std::sync::Mutex<Vec<(Method, String)>>,
        fail_keys: std::sync::atomic::AtomicBool,
        /// 列表接口挂掉（网络、限流的 429 空响应），传输层直接报错。
        fail_list: std::sync::atomic::AtomicBool,
        /// 野菜的中转创建密钥时把新密钥回传在 `data` 里；标准 NewAPI 不回。
        echo_created: std::sync::atomic::AtomicBool,
    }

    impl FakeTokens {
        fn seed(&self, token: Value) {
            self.tokens.lock().unwrap().push(token);
        }

        fn list_calls(&self) -> usize {
            self.requests
                .lock()
                .unwrap()
                .iter()
                .filter(|(method, path)| method == Method::GET && path == "/api/token/")
                .count()
        }
    }

    impl TokenApi for FakeTokens {
        async fn request(
            &self,
            method: Method,
            url: &str,
            body: Option<Value>,
        ) -> Result<(u16, Value), ActivationFailure> {
            let parsed = Url::parse(url).unwrap();
            let path = parsed.path().to_string();
            self.requests
                .lock()
                .unwrap()
                .push((method.clone(), path.clone()));
            let mut tokens = self.tokens.lock().unwrap();
            if method == Method::GET && path == "/api/token/" {
                if self.fail_list.load(std::sync::atomic::Ordering::Relaxed) {
                    return Err(ActivationFailure::ServerUnavailable);
                }
                // Paginated like the relay: newest first, at most 100 per page.
                let query = parsed
                    .query_pairs()
                    .map(|(key, value)| (key.into_owned(), value.into_owned()))
                    .collect::<std::collections::HashMap<_, _>>();
                let page = query
                    .get("p")
                    .and_then(|p| p.parse::<usize>().ok())
                    .unwrap_or(1)
                    .max(1);
                let size = query
                    .get("size")
                    .and_then(|s| s.parse::<usize>().ok())
                    .unwrap_or(10)
                    .min(100);
                let mut newest_first = tokens.clone();
                newest_first.sort_by_key(|t| std::cmp::Reverse(t["id"].as_u64().unwrap_or(0)));
                let items = newest_first
                    .into_iter()
                    .skip((page - 1) * size)
                    .take(size)
                    .collect::<Vec<_>>();
                return Ok((
                    200,
                    json!({"success":true,"data":{"items":items,"total":tokens.len(),"page":page,"page_size":size}}),
                ));
            }
            if method == Method::POST && path == "/api/token/" {
                let mut token = body.unwrap();
                assert!(token["name"].as_str().unwrap().len() <= 50);
                assert_ne!(token["group"], "");
                let next_id = tokens
                    .iter()
                    .filter_map(|t| t["id"].as_u64())
                    .max()
                    .unwrap_or(0)
                    + 1;
                token["id"] = json!(next_id);
                token["status"] = json!(1);
                tokens.push(token.clone());
                if self.echo_created.load(std::sync::atomic::Ordering::Relaxed) {
                    token["key"] = json!("sk-****masked");
                    return Ok((200, json!({"success":true,"message":"","data":token})));
                }
                // The standard server does not return a key or data here.
                return Ok((200, json!({"success":true,"message":""})));
            }
            let id: u64 = path.split('/').nth(3).unwrap_or("").parse().unwrap_or(0);
            if method == Method::POST && path.ends_with("/key") {
                if self.fail_keys.load(std::sync::atomic::Ordering::Relaxed) {
                    return Err(ActivationFailure::ServerUnavailable);
                }
                assert!(tokens.iter().any(|t| t["id"] == id));
                return Ok((
                    200,
                    json!({"success":true,"data":{"key":format!("synthetic-only-test-token-{id:04}")}}),
                ));
            }
            if method == Method::DELETE {
                tokens.retain(|t| t["id"] != id);
                return Ok((200, json!({"success":true})));
            }
            // In particular `/api/token/search` lands here: the relay matches a
            // keyword without `%` against the whole name and rate-limits the
            // endpoint to ten calls a minute, so the client must never use it.
            panic!("unexpected token operation: {method} {path}");
        }
    }

    /// 一把本机铸的、范围完全一致的密钥。
    fn our_key(id: u64, prefix: &str, group: &str, models: &str) -> Value {
        json!({
            "id": id,
            "name": format!("{prefix}-{}{:08x}", device_scope(), id as u32),
            "group": group, "status": 1, "expired_time": -1,
            "model_limits_enabled": true, "model_limits": models,
        })
    }

    #[tokio::test]
    async fn an_existing_key_is_found_by_listing_not_by_searching() {
        // 2026-09-20: the relay's search endpoint stopped matching prefixes
        // (exact unless the keyword carries `%`) and gained a ten-per-minute
        // budget. Every activation then minted a fresh key, retired nothing,
        // and the fourth billing group of one activation tripped the budget.
        let api = FakeTokens::default();
        let prefix = token_prefix("codex_desktop", "gpt pool");
        api.seed(our_key(710, &prefix, "gpt pool", "gpt-5.6-sol"));
        let lease = acquire_token_using(
            &api,
            "https://mock.invalid",
            "codex_desktop",
            "gpt pool",
            &["gpt-5.6-sol".to_string()],
        )
        .await
        .unwrap();
        assert!(!lease.created);
        assert_eq!(lease.id, 710);
        assert!(lease.retire_after_commit.is_empty());
        let requests = api.requests.lock().unwrap();
        assert!(!requests.iter().any(|(_, path)| path.contains("search")));
        assert_eq!(
            requests
                .iter()
                .filter(|(method, path)| method == Method::POST && path == "/api/token/")
                .count(),
            0,
            "a key that exists must not be minted again"
        );
    }

    #[tokio::test]
    async fn four_groups_cost_one_list_each_and_no_search_budget() {
        let api = FakeTokens::default();
        for group in [
            "DeepSeek Flash",
            "限时国模特价渠道",
            "【特价】glm5.3 flash",
            "gpt pool",
        ] {
            acquire_token_using(
                &api,
                "https://mock.invalid",
                "codex_desktop",
                group,
                &["model".to_string()],
            )
            .await
            .unwrap_or_else(|_| panic!("group {group} must not be skipped"));
        }
        // One sweep before minting, one read-back after: the standard relay
        // returns nothing on create. Still nowhere near any per-minute budget,
        // and none of it touches the search endpoint (the fake would panic).
        assert_eq!(api.list_calls(), 8);
        assert_eq!(api.tokens.lock().unwrap().len(), 4);
    }

    #[tokio::test]
    async fn a_key_on_the_second_page_is_still_recognised() {
        let api = FakeTokens::default();
        let prefix = token_prefix("claude_code", "default");
        // Oldest key is ours; a hundred newer ones (other groups) push it to
        // page two of a newest-first listing.
        api.seed(our_key(1, &prefix, "default", "model-a"));
        for id in 1000..1100u64 {
            api.seed(our_key(
                id,
                &token_prefix("pi", "other"),
                "other",
                "model-b",
            ));
        }
        let lease = acquire_token_using(
            &api,
            "https://mock.invalid",
            "claude_code",
            "default",
            &["model-a".to_string()],
        )
        .await
        .unwrap();
        assert!(!lease.created);
        assert_eq!(lease.id, 1);
        assert_eq!(
            api.list_calls(),
            2,
            "page one was full, so page two was read"
        );
    }

    #[tokio::test]
    async fn a_relay_that_echoes_the_new_key_saves_the_read_back() {
        let api = FakeTokens::default();
        api.echo_created
            .store(true, std::sync::atomic::Ordering::Relaxed);
        let lease = acquire_token_using(
            &api,
            "https://mock.invalid",
            "pi",
            "default",
            &["model-a".to_string()],
        )
        .await
        .unwrap();
        assert!(lease.created);
        assert_eq!(api.list_calls(), 1, "only the sweep before minting");
        assert_eq!(api.tokens.lock().unwrap()[0]["id"], lease.id);
    }

    #[test]
    fn a_created_echo_is_trusted_only_when_the_name_matches() {
        let echo =
            json!({"success":true,"data":{"id":42,"name":"野菜API Pi-00000000-0000000000000000"}});
        assert_eq!(
            created_token_id(&echo, "野菜API Pi-00000000-0000000000000000"),
            Some(42)
        );
        assert_eq!(
            created_token_id(&echo, "野菜API Pi-00000000-ffffffffffffffff"),
            None
        );
        assert_eq!(
            created_token_id(&json!({"success":true,"message":""}), "x"),
            None
        );
        assert_eq!(
            created_token_id(&json!({"success":false,"data":{"id":42,"name":"x"}}), "x"),
            None
        );
    }

    #[tokio::test]
    async fn a_legacy_named_key_is_adopted_and_a_stale_current_one_retired() {
        let api = FakeTokens::default();
        let current = token_prefix("codex_desktop", "default");
        let legacy = legacy_token_prefix("codex_desktop", "default");
        // The current-name key has the wrong model scope; the 0.4.22-era key
        // is exactly right. Adopt the good one, retire the stale one.
        api.seed(our_key(5, &current, "default", "model-old"));
        api.seed(our_key(3, &legacy, "default", "model-a"));
        let lease = acquire_token_using(
            &api,
            "https://mock.invalid",
            "codex_desktop",
            "default",
            &["model-a".to_string()],
        )
        .await
        .unwrap();
        assert!(!lease.created);
        assert_eq!(lease.id, 3);
        assert_eq!(lease.retire_after_commit, vec![5]);

        // When both shapes fit, the current name wins and the legacy one goes.
        let api = FakeTokens::default();
        api.seed(our_key(5, &current, "default", "model-a"));
        api.seed(our_key(3, &legacy, "default", "model-a"));
        let lease = acquire_token_using(
            &api,
            "https://mock.invalid",
            "codex_desktop",
            "default",
            &["model-a".to_string()],
        )
        .await
        .unwrap();
        assert!(!lease.created);
        assert_eq!(lease.id, 5);
        assert_eq!(lease.retire_after_commit, vec![3]);
    }

    #[tokio::test]
    async fn an_unreadable_list_fails_the_group_instead_of_minting_blind() {
        // Minting without looking is exactly how keys piled up. When the list
        // cannot be read the group is skipped (and named in the log), which the
        // user can retry; a blind mint could not be undone.
        let api = FakeTokens::default();
        api.fail_list
            .store(true, std::sync::atomic::Ordering::Relaxed);
        assert!(acquire_token_using(
            &api,
            "https://mock.invalid",
            "pi",
            "default",
            &["model-a".to_string()],
        )
        .await
        .is_err());
        assert!(api.tokens.lock().unwrap().is_empty());
    }

    #[test]
    fn token_operations_are_named_without_ids_or_queries() {
        let o = "https://mock.invalid";
        assert_eq!(
            token_operation(&Method::GET, &format!("{o}/api/token/?p=1&size=100")),
            "list"
        );
        assert_eq!(
            token_operation(&Method::POST, &format!("{o}/api/token/")),
            "create"
        );
        assert_eq!(
            token_operation(&Method::POST, &format!("{o}/api/token/764/key")),
            "key"
        );
        assert_eq!(
            token_operation(&Method::DELETE, &format!("{o}/api/token/764")),
            "delete"
        );
        assert_eq!(
            token_operation(&Method::GET, &format!("{o}/api/token/search?keyword=x")),
            "other"
        );
    }

    #[tokio::test]
    async fn token_creation_without_key_reuses_only_the_selected_group_on_replay() {
        let api = FakeTokens::default();
        let default_models = vec!["model-a".to_string()];
        let first = acquire_token_using(
            &api,
            "https://mock.invalid",
            "pi",
            "default",
            &default_models,
        )
        .await
        .unwrap();
        assert!(first.created);
        let replay = acquire_token_using(
            &api,
            "https://mock.invalid",
            "pi",
            "default",
            &default_models,
        )
        .await
        .unwrap();
        assert!(!replay.created);
        assert_eq!(first.id, replay.id);
        assert_eq!(first.key, replay.key);
        let discounted_models = vec!["deepseek-v4-flash".to_string()];
        let discounted = acquire_token_using(
            &api,
            "https://mock.invalid",
            "pi",
            "国模特价分组",
            &discounted_models,
        )
        .await
        .unwrap();
        assert!(discounted.created);
        assert_ne!(first.id, discounted.id);
        let tokens = api.tokens.lock().unwrap();
        assert_eq!(tokens[0]["group"], "default");
        assert_eq!(tokens[1]["group"], "国模特价分组");
        assert_eq!(tokens[0]["model_limits_enabled"], true);
        assert_eq!(tokens[0]["model_limits"], "model-a");
        assert!(!api
            .requests
            .lock()
            .unwrap()
            .iter()
            .any(|(method, _)| method == Method::PUT));
    }

    #[tokio::test]
    async fn failed_key_read_removes_only_the_new_token_and_leaves_original_group_unchanged() {
        let api = FakeTokens::default();
        let default_models = vec!["model-a".to_string()];
        acquire_token_using(
            &api,
            "https://mock.invalid",
            "pi",
            "default",
            &default_models,
        )
        .await
        .unwrap();
        let before = api.tokens.lock().unwrap().clone();
        api.fail_keys
            .store(true, std::sync::atomic::Ordering::Relaxed);
        assert!(acquire_token_using(
            &api,
            "https://mock.invalid",
            "pi",
            "discount",
            &["model-b".to_string()],
        )
        .await
        .is_err());
        assert_eq!(*api.tokens.lock().unwrap(), before);
        // Failure while reading an existing key must not delete it.
        assert!(acquire_token_using(
            &api,
            "https://mock.invalid",
            "pi",
            "default",
            &default_models,
        )
        .await
        .is_err());
        assert_eq!(*api.tokens.lock().unwrap(), before);
    }

    #[tokio::test]
    async fn changed_model_scope_rotates_without_deleting_the_old_key_before_commit() {
        let api = FakeTokens::default();
        let first = acquire_token_using(
            &api,
            "https://mock.invalid",
            "codex_desktop",
            "default",
            &["model-b".to_string(), "model-a".to_string()],
        )
        .await
        .unwrap();
        assert!(first.created);

        let replay = acquire_token_using(
            &api,
            "https://mock.invalid",
            "codex_desktop",
            "default",
            &["model-a".to_string(), "model-b".to_string()],
        )
        .await
        .unwrap();
        assert!(!replay.created);
        assert_eq!(replay.id, first.id);

        let replacement = acquire_token_using(
            &api,
            "https://mock.invalid",
            "codex_desktop",
            "default",
            &["model-c".to_string()],
        )
        .await
        .unwrap();
        assert!(replacement.created);
        assert_ne!(replacement.id, first.id);
        assert_eq!(replacement.retire_after_commit, vec![first.id]);
        let tokens = api.tokens.lock().unwrap();
        assert_eq!(tokens.len(), 2, "old key stays valid until local commit");
        assert_eq!(tokens[1]["model_limits"], "model-c");
    }

    #[test]
    fn token_identity_checks_real_group_status_and_expiry_not_just_the_label() {
        let prefix = token_prefix("pi", "special");
        let models = vec!["model-a".to_string()];
        let mut token = json!({"name":format!("{prefix}-0123456789abcdef"),"group":"special","status":1,"expired_time":-1,"model_limits_enabled":true,"model_limits":"model-a"});
        assert!(reusable_token(
            token.as_object().unwrap(),
            &prefix,
            "special",
            &models,
            false
        ));
        token["group"] = json!("default");
        assert!(!reusable_token(
            token.as_object().unwrap(),
            &prefix,
            "special",
            &models,
            false
        ));
        token["group"] = json!("special");
        token["status"] = json!(2);
        assert!(!reusable_token(
            token.as_object().unwrap(),
            &prefix,
            "special",
            &models,
            false
        ));
        token["status"] = json!(1);
        token["expired_time"] = json!(1);
        assert!(!reusable_token(
            token.as_object().unwrap(),
            &prefix,
            "special",
            &models,
            false
        ));
        token["expired_time"] = json!(-1);
        token["model_limits_enabled"] = json!(false);
        assert!(!reusable_token(
            token.as_object().unwrap(),
            &prefix,
            "special",
            &models,
            false
        ));
        assert_ne!(prefix, token_prefix("pi", "default"));
    }

    #[test]
    fn a_second_computer_never_claims_the_first_computers_key() {
        // Signing in on a second machine and choosing any different model set
        // used to delete the first machine's key: the prefix sweep saw every
        // computer's tokens as its own, and everything it did not reuse went
        // into `retire_after_commit` and then into DELETE /api/token/{id}.
        // The first machine then failed on its next request with no message.
        let prefix = token_prefix("claude_code", "default");
        let ours = json!({
            "id": 1,
            "name": format!("{prefix}-{}{:08x}", device_scope(), 0xabcd1234u32),
            "group": "default", "status": 1, "expired_time": -1,
            "model_limits_enabled": true, "model_limits": "model-a",
        });
        // Same shape, different machine. The scope is derived from ours by
        // flipping every digit, so the fixture cannot collide with whatever
        // identity this machine happens to have minted.
        let other_scope = device_scope()
            .chars()
            .map(|digit| char::from_digit(15 - digit.to_digit(16).unwrap_or(0), 16).unwrap_or('f'))
            .collect::<String>();
        assert_ne!(other_scope, device_scope());
        let theirs = json!({
            "id": 2,
            "name": format!("{prefix}-{other_scope}{:08x}", 0x99887766u32),
            "group": "default", "status": 1, "expired_time": -1,
            "model_limits_enabled": true, "model_limits": "model-a",
        });
        assert!(token_is_ours(ours.as_object().unwrap(), &prefix));
        assert!(!token_is_ours(theirs.as_object().unwrap(), &prefix));

        // A key minted before this change has a random sixteen-hex suffix and is
        // therefore indistinguishable from another computer's — deliberately so.
        // Both are left alone, because guessing wrong in the other direction is
        // the failure this is fixing.
        let legacy = json!({
            "id": 3,
            "name": format!("{prefix}-{other_scope}0123abcd"),
            "group": "default", "status": 1, "expired_time": -1,
            "model_limits_enabled": true, "model_limits": "model-a",
        });
        assert!(!token_is_ours(legacy.as_object().unwrap(), &prefix));

        // Both still satisfy the old ownership and reuse predicates, which is
        // what keeps already-installed clients able to read the new names.
        for token in [&ours, &theirs, &legacy] {
            assert!(owned_token(
                token.as_object().unwrap(),
                &prefix,
                "default",
                false
            ));
        }
    }

    #[test]
    fn every_minted_name_fits_the_relay_fifty_byte_limit() {
        // 中转 `controller/token.go` 硬限制 50 字节，超了直接拒绝创建密钥。
        // Go 的 len() 数的是字节，`野菜API` 就占 9 个 —— 这一条是用来在加任何
        // 东西进名字之前先撞墙的，比等中转报错便宜得多。
        for tool in [
            "claude_code",
            "claude_desktop",
            "codex_desktop",
            "pi",
            "dsh_web",
            "workbuddy",
            "unknown-tool",
        ] {
            for group in ["default", "限时国模特价渠道", "DeepSeek / Kimi / MiMo"] {
                let name = format!(
                    "{}-{}{:08x}",
                    token_prefix(tool, group),
                    device_scope(),
                    u32::MAX
                );
                assert!(
                    name.len() <= 50,
                    "{name} is {} bytes, relay rejects over 50",
                    name.len()
                );
            }
        }
    }

    #[test]
    fn a_name_says_which_application_it_belongs_to() {
        let name = token_prefix("codex_desktop", "default");
        // 后台列表里截断后看得见的就是这一截，它得说明白是哪个应用。
        assert!(name.starts_with("野菜API Codex-"), "{name}");
        // 新旧两种形状都要能归因，用量日志里会长期并存。
        let minted = format!("{name}-{}{:08x}", device_scope(), 0u32);
        assert_eq!(tool_for_token_name(&minted), Some("codex_desktop"));
        let legacy = format!(
            "{}-{}{:08x}",
            legacy_token_prefix("codex_desktop", "default"),
            device_scope(),
            0u32
        );
        assert!(legacy.starts_with("野菜API cx-"), "{legacy}");
        assert_eq!(tool_for_token_name(&legacy), Some("codex_desktop"));
        // ClaudeCode 和 ClaudeDesktop 不能互相认错。
        for (tool, label) in [
            ("claude_code", "ClaudeCode"),
            ("claude_desktop", "ClaudeDesktop"),
        ] {
            let n = format!(
                "{}-{}{:08x}",
                token_prefix(tool, "default"),
                device_scope(),
                1u32
            );
            assert!(n.starts_with(&format!("野菜API {label}-")), "{n}");
            assert_eq!(tool_for_token_name(&n), Some(tool));
        }
        // 不同分组仍然是不同的前缀 —— 缩短哈希不能把它们并成一个。
        assert_ne!(
            token_prefix("codex_desktop", "default"),
            token_prefix("codex_desktop", "限时国模特价渠道")
        );
    }

    #[test]
    fn a_minted_name_keeps_the_sixteen_hex_suffix_older_clients_expect() {
        let prefix = token_prefix("codex_desktop", "default");
        let name = format!("{prefix}-{}{:08x}", device_scope(), 0u32);
        let suffix = name.strip_prefix(&format!("{prefix}-")).unwrap();
        assert_eq!(suffix.len(), 16);
        assert!(suffix.bytes().all(|byte| byte.is_ascii_hexdigit()));
        assert_eq!(device_scope().len(), DEVICE_SCOPE_LEN);
        assert!(device_scope().bytes().all(|byte| byte.is_ascii_hexdigit()));
        // Stable within a run, which is what makes the scope usable as identity.
        assert_eq!(device_scope(), device_scope());
        // Usage attribution reads the tool out of the name and must not care.
        assert_eq!(tool_for_token_name(&name), Some("codex_desktop"));
    }

    /// 这条测试的存在本身就是那个 bug 的纪念碑。
    ///
    /// 判定原先写在 `if desktop_lifecycle::requires_reload(..)` 里面，而
    /// `requires_reload` 只对 `claude_desktop` / `codex_desktop` 为真 ——
    /// 里面再判 `claude_code`，两个互斥条件相与，永远为假。那段代码一次都没
    /// 跑过，用户接入 Claude Code 后照旧看到
    /// `Both claude.ai and apiKeyHelper set`，中转从头到尾没被用上。
    ///
    /// 当时的测试直接 mock 了 `configure_desktop_tool_v2` 的返回，测的是界面
    /// 契约，完全没经过这里 —— 绿着，而功能是死的。
    #[test]
    fn only_claude_code_without_consent_and_with_a_login_asks() {
        let present = || true;
        let absent = || false;

        assert!(needs_claude_login_consent("claude_code", false, present));

        // 已经同意过就不再问，否则每次接入都弹一次系统授权框。
        assert!(!needs_claude_login_consent("claude_code", true, present));
        // 没有 claude.ai 登录态的人不该平白收到任何东西。
        assert!(!needs_claude_login_consent("claude_code", false, absent));
        // 只有 Claude Code 会读 `~/.claude` 的凭据；别的工具不受影响。
        for tool in [
            "claude_desktop",
            "codex_desktop",
            "pi",
            "dsh_web",
            "workbuddy",
        ] {
            assert!(
                !needs_claude_login_consent(tool, false, present),
                "{tool} 不该被问"
            );
        }
    }

    /// 探测要 shell 出去查钥匙串。除了 claude_code 且尚未同意，谁都不该付这个
    /// 代价 —— 每次接入都多跑一个进程是白花的。
    #[test]
    fn the_keychain_is_not_probed_when_the_answer_is_already_no() {
        let mut probed = false;
        let mut probe = || {
            probed = true;
            true
        };
        assert!(!needs_claude_login_consent("workbuddy", false, &mut probe));
        assert!(!probed, "别的工具不该触发探测");

        let mut probed = false;
        let mut probe = || {
            probed = true;
            true
        };
        assert!(!needs_claude_login_consent("claude_code", true, &mut probe));
        assert!(!probed, "已经同意过就不必再探测");
    }

    #[test]
    fn request_requires_one_of_six_exact_targets_and_selection_shape() {
        let valid = ToolActivationRequest {
            request_id: "activation-1".into(),
            line_id: "mainland_optimized".into(),
            tool_id: "pi".into(),
            model_id: "glm-5.3".into(),
            installation_id: "i0123456789abcdef".into(),
            billing_group: "default".into(),
            models: None,
            installation_job_id: None,
            restart_running_app: false,
            displace_claude_login: false,
        };
        assert!(request_is_valid(&valid));
        let mut invalid = valid.clone();
        invalid.restart_running_app = true;
        assert!(!request_is_valid(&invalid));
        let mut invalid = valid;
        invalid.tool_id = "opencode".into();
        assert!(!request_is_valid(&invalid));

        let comma = json!({"requestId":"fixture", "lineId":"mainland_optimized", "toolId":"pi", "modelId":"model,a", "billingGroup":"default", "installationId":"i0123456789abcdef"});
        let comma: ToolActivationRequest = serde_json::from_value(comma).unwrap();
        assert!(!request_is_valid(&comma));
    }

    #[test]
    fn activation_cancellation_allows_only_one_live_transaction() {
        let state = ActivationOperationState::default();
        let registration = state.begin("activation-one").unwrap();
        assert!(state.begin("activation-one").is_err());
        assert!(state.begin("activation-two").is_err());
        assert!(!registration.cancellation.is_requested());
        assert_eq!(state.cancel("activation-one"), Ok(true));
        assert!(registration.cancellation.is_requested());
        drop(registration);
        assert_eq!(state.cancel("activation-one"), Ok(false));
    }

    #[test]
    fn every_listed_model_can_be_activated_for_every_target() {
        // 闸门撤掉了。这里钉住的是「撤掉」这个决定本身：无论模型声明了什么、
        // 声明了空的、还是压根不在价目表里，六个目标都不拒绝。理由和查证过的
        // 中转事实写在 src/configuration/modelCompatibility.ts 顶部。
        let pricing = json!({
            "success": true,
            "data": [
                {"model_name": "chat", "supported_endpoint_types": ["openai"]},
                {"model_name": "responses", "supported_endpoint_types": ["openai-response"]},
                {"model_name": "messages", "supported_endpoint_types": ["anthropic"]},
                {"model_name": "image", "supported_endpoint_types": ["image-generation"]},
                {"model_name": "silent", "supported_endpoint_types": []}
            ]
        });
        for tool in [
            "claude_code",
            "claude_desktop",
            "codex_desktop",
            "pi",
            "dsh_web",
            "workbuddy",
        ] {
            for model in ["chat", "responses", "messages", "image", "silent", "absent"] {
                assert!(
                    model_supports_tool(&pricing, model, tool),
                    "{tool} refused {model}"
                );
            }
        }
        assert!(!model_supports_tool(&pricing, "chat", "opencode"));
        // 传输方式仍然按目标定，因为它决定往应用配置里写什么，不再是否决权。
        assert_eq!(
            claude_transport(&pricing, "image"),
            Some(ClaudeTransport::DirectAnthropic)
        );
        assert_eq!(
            codex_transport(&pricing, "messages"),
            Some(codex_desktop::CodexTransport::DirectResponses)
        );
    }

    #[test]
    fn token_names_are_distinct_per_target() {
        let names = [
            token_name("claude_code"),
            token_name("claude_desktop"),
            token_name("codex_desktop"),
            token_name("pi"),
            token_name("dsh_web"),
            token_name("workbuddy"),
        ];
        for (index, left) in names.iter().enumerate() {
            assert!(!names[index + 1..].contains(left));
        }
    }
}

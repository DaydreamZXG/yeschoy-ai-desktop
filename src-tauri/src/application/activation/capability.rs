//! Model/protocol evidence used by activation: whether the relay model list
//! accepts the selection and which wire format is written.
//!
//! The Codex probe spends a few generated tokens on the models it accepts, so
//! it is **opt-in**: it runs only when the renderer says the user turned it on
//! (see `probe_requested`). Off means every model keeps the direct Responses
//! route — the same endpoint an inconclusive probe lands on — and activation
//! sends no inference request at all.
use std::time::Duration;

use futures::StreamExt;

use serde_json::{json, Value};

use super::error::ActivationFailure;
use super::tokens::TokenLease;
use crate::application::ports::station::{StationMethod, StationProvider};
use crate::claude_bridge::ClaudeTransport;
use crate::domain::model::ModelBinding;
use crate::tool_adapters::codex_desktop;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ModelTransport {
    Claude(ClaudeTransport),
    Codex(codex_desktop::CodexTransport),
}

pub(crate) use super::envelope::{data, server_success};

pub(crate) async fn validate_models(
    origin: &str,
    access_token: &str,
    models: &[ModelBinding],
    tool_id: &str,
) -> Result<Vec<Option<ModelTransport>>, ActivationFailure> {
    let provider = crate::adapters::stations::NewApiStationProvider::new(origin, access_token)
        .map_err(|_| ActivationFailure::ServerUnavailable)?;
    let (status, value) =
        StationProvider::account_json(&provider, StationMethod::Get, "/api/user/models", None)
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
    let (pricing_status, pricing) =
        StationProvider::account_json(&provider, StationMethod::Get, "/api/pricing", None)
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
pub(crate) fn model_supports_tool(_pricing: &Value, _model_id: &str, tool_id: &str) -> bool {
    matches!(
        tool_id,
        "claude_code"
            | "claude_desktop"
            | "codex_desktop"
            | "pi"
            | "opencode"
            | "dsh_web"
            | "workbuddy"
            | "dsh_desktop"
    )
}

/// The transport is still chosen per model, because it decides what gets
/// written into the app's configuration. It is no longer a veto: both arms
/// are usable, and the choice only records which wire format the app will
/// speak to the relay.
pub(crate) fn claude_transport(pricing: &Value, model_id: &str) -> Option<ClaudeTransport> {
    let _ = (pricing, model_id);
    Some(ClaudeTransport::DirectAnthropic)
}

pub(crate) fn codex_transport(
    pricing: &Value,
    model_id: &str,
) -> Option<codex_desktop::CodexTransport> {
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
pub(crate) enum ProbeOutcome {
    /// 端点受理了这个模型。
    Accepted,
    /// 端点明确拒绝（4xx，429/408 除外）—— 路由不认、渠道没配、协议没实现。
    Rejected,
    /// 没问出结论：超时、网络错误、5xx、429、408。
    Inconclusive,
}

/// 一个状态码归到哪一档。
///
/// 抽成纯函数是为了把 429 和 408 这两个例外钉住：它们是 4xx，说的却不是
/// 「这个协议不行」—— 429 是「问得太急」，408 是「这次没等到」。归成 `Rejected`
/// 的后果真发生过：十几个模型一起探，中转按用户限流回 429，Responses 被判成
/// 拒绝，稍后发的 Chat 那一探刚好没被限、通过了，于是模型被**永久**钉到桥上
/// （探测结果落盘、之后不再探），而用户什么都没做错。
pub(crate) fn probe_outcome_for(status: reqwest::StatusCode) -> ProbeOutcome {
    if status.is_success() {
        ProbeOutcome::Accepted
    } else if status == reqwest::StatusCode::TOO_MANY_REQUESTS
        || status == reqwest::StatusCode::REQUEST_TIMEOUT
    {
        ProbeOutcome::Inconclusive
    } else if status.is_client_error() {
        ProbeOutcome::Rejected
    } else {
        ProbeOutcome::Inconclusive
    }
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
pub(crate) fn transport_from_probes(
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
/// `max_output_tokens` 取 16：Responses API 的下限就是它（上游
/// 有一条 `clamp sub-floor max_tokens to the Responses API minimum` 的修复），
/// 再小会被当成参数错误而不是路由错误，把探测本身变成噪音。
///
/// **这次探测会真的产生一点点用量。** 实测那几种失败（`unknown provider
/// for model gi/…`、`分组 default 下无可用渠道`）都发生在路由阶段、还没到
/// 上游，不计费；只有成功的那次会生成十几个 token。每个模型至多一到两次，
/// 只在接入时发生 —— 而且只在用户打开开关时发生（`probe_requested`）。
pub(crate) fn probe_body(model: &str, responses: bool) -> Value {
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
pub(crate) async fn probe_until_conclusive(
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
        Ok(Ok(response)) => probe_outcome_for(response.status()),
        // 网络错误、超时都归到「没问出结论」，见 `ProbeOutcome`。
        _ => ProbeOutcome::Inconclusive,
    }
}

/// 同时在路上的探测数。
///
/// 第一版是全部 `join_all`：一次接入绑十几个模型就同时发十几个请求，中转按
/// 用户限流，探测于是**自己制造**了 429 —— 而 429 当时又被读成「拒绝」。
/// 取 2：比串行快一倍，又远在限流阈值之下；每个探测至多问三遍，总时长仍可接受。
pub(crate) const PROBE_CONCURRENCY: usize = 2;

/// 这次接入要不要发探测请求。
///
/// 抽成函数，是因为「不发收费的测试请求」是产品对用户的承诺，它不能只活在
/// `configure_desktop_tool` 的一行 `if` 里 —— 那样改别的条件时很容易顺带
/// 把探测放出来，而且没有任何测试能直接盯住它。两个条件缺一不可：
/// 工具是 Codex（只有它需要判协议），且用户在设置里显式打开了开关。
pub(crate) fn probe_requested(tool_id: &str, opted_in: bool) -> bool {
    tool_id == "codex_desktop" && opted_in
}

/// 把探测结果写回这一批模型的传输方式。
///
/// 只对 Codex 做：Claude 走的是中转原生支持的 Anthropic 协议，没有这个问题。
///
/// 探测失败不让接入失败 —— 探不出来就停在直连，也就是今天的行为。
/// 为了一次「问不出结论」而挡住用户接入，是拿一个可恢复的未知换一个确定的
/// 失败，不划算。
pub(crate) async fn apply_codex_probes(
    origin: &str,
    bindings: &[ModelBinding],
    leases: &[(String, TokenLease)],
    transports: &mut [Option<ModelTransport>],
) {
    let Ok(client) = crate::account_v2::shared_http_client() else {
        return;
    };
    apply_codex_probes_with(client, origin, bindings, leases, transports).await
}

/// `apply_codex_probes` 去掉取共享客户端那一步：测试要把它指向本地桩。
pub(crate) async fn apply_codex_probes_with(
    client: &reqwest::Client,
    origin: &str,
    bindings: &[ModelBinding],
    leases: &[(String, TokenLease)],
    transports: &mut [Option<ModelTransport>],
) {
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
        })
        // 先收成 Vec：让 `filter` 里对 `transports` 的借用在 await 之前结束，
        // 而不是被流一路带过去。
        .collect::<Vec<_>>();
    // 并发探，但有上限（`PROBE_CONCURRENCY`）：一次接入可能绑了十几个模型，
    // 串行会把接入时间拖成十几倍；全放开又会撞上中转的限流，让探测自己
    // 制造出它要判读的错误。
    let probed = futures::stream::iter(probes)
        .buffer_unordered(PROBE_CONCURRENCY)
        .collect::<Vec<_>>()
        .await;
    for (index, probed) in probed {
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
pub(crate) async fn probe_codex_transport(
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
#[path = "capability_tests.rs"]
mod tests;

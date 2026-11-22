//! 粘贴密钥接入：用户自己的那把密钥直接写进应用，不向站点另铸密钥。
//!
//! 授权登录的站点能替每个应用、每个分组铸一把受限密钥；只给了一把密钥的站点
//! 做不到，也没有账户接口可问。所以这里只做 OpenAI 兼容接口能回答的两件事：
//! 这把密钥看不看得到这些模型，以及 Claude 该直连还是过桥。
use super::*;

/// 没有账户，也就没有分组：所有模型共用这一把密钥。
pub(super) fn lease(key: &str) -> TokenLease {
    TokenLease {
        id: 0,
        key: key.to_owned(),
        // 不是我们铸的：失败回滚时不删，换新时也没有旧的要退。
        created: false,
        retire_after_commit: Vec::new(),
    }
}

/// 每个绑定的分组各配一份同样的密钥，后面的流程照旧按分组找。
pub(super) fn leases(bindings: &[ModelBinding], key: &str) -> Vec<(String, TokenLease)> {
    let mut leases: Vec<(String, TokenLease)> = Vec::new();
    for binding in bindings {
        if !leases
            .iter()
            .any(|(group, _)| group == &binding.billing_group)
        {
            leases.push((binding.billing_group.clone(), lease(key)));
        }
    }
    leases
}

/// 起点和账户路径一样保守：Codex、Claude 都先按直连，探测之后才可能改。
fn starting_transport(tool_id: &str) -> Option<ModelTransport> {
    match tool_id {
        "codex_desktop" => Some(ModelTransport::Codex(
            codex_desktop::CodexTransport::DirectResponses,
        )),
        "claude_code" | "claude_desktop" => {
            Some(ModelTransport::Claude(ClaudeTransport::DirectAnthropic))
        }
        _ => None,
    }
}

/// 一次 `/v1/models` 的结果怎么判。抽成纯函数，不联网也能测。
fn transports_from_catalogue(
    status: u16,
    catalogue: &Value,
    bindings: &[ModelBinding],
    tool_id: &str,
) -> Result<Vec<Option<ModelTransport>>, ActivationFailure> {
    if matches!(status, 401 | 403) {
        return Err(ActivationFailure::SignedOut);
    }
    if !(200..300).contains(&status) {
        return Err(ActivationFailure::ServerUnavailable);
    }
    let listed = crate::station_probe::model_ids(catalogue);
    if !bindings
        .iter()
        .all(|binding| listed.contains(&binding.model_id))
        || !model_supports_tool(catalogue, "", tool_id)
    {
        return Err(ActivationFailure::UnsupportedModel);
    }
    Ok(bindings
        .iter()
        .map(|_| starting_transport(tool_id))
        .collect())
}

pub(super) async fn validate_models(
    origin: &str,
    key: &str,
    bindings: &[ModelBinding],
    tool_id: &str,
) -> Result<Vec<Option<ModelTransport>>, ActivationFailure> {
    let (status, catalogue) = native_account_json(
        Method::GET,
        &format!("{}/models", crate::provider_presets::openai_base(origin)),
        key,
        None,
    )
    .await
    .map_err(|_| ActivationFailure::ServerUnavailable)?;
    let mut transports = transports_from_catalogue(status, &catalogue, bindings, tool_id)?;
    if let Some(known) = preset_transport(origin, tool_id) {
        transports.fill(Some(known));
    }
    Ok(transports)
}

/// 云厂商说哪种格式是表里写好的，不用发一段对话去问。套餐的额度是按请求算的，
/// 条款也只许编程工具用它：客户端自己不该拿这把密钥发任何对话。
pub(super) fn preset_transport(origin: &str, tool_id: &str) -> Option<ModelTransport> {
    let preset = crate::provider_presets::for_origin(origin)?;
    match tool_id {
        "codex_desktop" => Some(ModelTransport::Codex(if preset.responses {
            codex_desktop::CodexTransport::DirectResponses
        } else {
            codex_desktop::CodexTransport::ChatBridge
        })),
        "claude_code" | "claude_desktop" => Some(ModelTransport::Claude(
            if crate::provider_presets::anthropic_base(origin).is_some() {
                ClaudeTransport::DirectAnthropic
            } else {
                ClaudeTransport::ChatBridge
            },
        )),
        _ => None,
    }
}

/// Claude 该直连还是过桥。
///
/// 只有一种情况改走桥：站点**明确拒绝** Anthropic 格式、而 Chat 格式**明确可用**
/// —— 也就是只会说 OpenAI 格式的站。问不出结论一律停在直连，理由和 Codex
/// 那边一样：把能直连的错判成过桥是主动降级，反过来只是维持现状。
fn claude_transport_from_probes(
    messages: ProbeOutcome,
    chat: Option<ProbeOutcome>,
) -> ClaudeTransport {
    if messages == ProbeOutcome::Rejected && chat == Some(ProbeOutcome::Accepted) {
        ClaudeTransport::ChatBridge
    } else {
        ClaudeTransport::DirectAnthropic
    }
}

fn messages_probe_body(model: &str) -> Value {
    json!({
        "model": model,
        "messages": [{"role": "user", "content": "hi"}],
        "max_tokens": 16,
        "stream": false,
    })
}

/// 和真实请求用同一种认证方式探：Claude Desktop 发 Bearer，Claude Code 发
/// `x-api-key`，只认 Bearer 的厂商两个都发 Bearer（见
/// `claude_bridge::send_upstream`）。换一种方式探出来的结论
/// 不能代表应用真正发出去的那一种。
async fn probe_messages_once(
    client: &reqwest::Client,
    origin: &str,
    key: &str,
    model: &str,
    tool_id: &str,
) -> ProbeOutcome {
    let url = format!(
        "{}/v1/messages",
        crate::provider_presets::anthropic_base(origin).unwrap_or_else(|| origin.to_owned())
    );
    let request = client
        .post(url)
        .header("anthropic-version", "2023-06-01")
        .json(&messages_probe_body(model));
    let request =
        if tool_id == "claude_desktop" || crate::provider_presets::anthropic_bearer(origin) {
            request.bearer_auth(key)
        } else {
            request.header("x-api-key", key)
        };
    match tokio::time::timeout(PROBE_ATTEMPT_BUDGET, request.send()).await {
        Ok(Ok(response)) => probe_outcome_for(response.status()),
        _ => ProbeOutcome::Inconclusive,
    }
}

async fn probe_claude_transport(
    client: &reqwest::Client,
    origin: &str,
    key: &str,
    model: &str,
    tool_id: &str,
) -> ClaudeTransport {
    let mut messages = ProbeOutcome::Inconclusive;
    for attempt in 1..=PROBE_ATTEMPTS {
        messages = probe_messages_once(client, origin, key, model, tool_id).await;
        if messages != ProbeOutcome::Inconclusive {
            break;
        }
        if attempt < PROBE_ATTEMPTS {
            tokio::time::sleep(PROBE_RETRY_DELAY).await;
        }
    }
    let chat = if messages == ProbeOutcome::Rejected {
        Some(
            probe_until_conclusive(client, origin, key, model, false)
                .await
                .0,
        )
    } else {
        None
    };
    let transport = claude_transport_from_probes(messages, chat);
    log::info!(
        "claude_probe model={model} messages={messages:?} chat={chat:?} transport={}",
        transport.credential_value()
    );
    transport
}

pub(super) async fn apply_claude_probes(
    origin: &str,
    key: &str,
    tool_id: &str,
    bindings: &[ModelBinding],
    transports: &mut [Option<ModelTransport>],
) {
    let Ok(client) = crate::account_v2::shared_http_client() else {
        return;
    };
    apply_claude_probes_with(client, origin, key, tool_id, bindings, transports).await
}

async fn apply_claude_probes_with(
    client: &reqwest::Client,
    origin: &str,
    key: &str,
    tool_id: &str,
    bindings: &[ModelBinding],
    transports: &mut [Option<ModelTransport>],
) {
    // 带着下标探、按下标写回，和 `apply_codex_probes_with` 同一个理由。
    let probes = bindings
        .iter()
        .enumerate()
        .filter(|(index, _)| {
            matches!(
                transports.get(*index),
                Some(Some(ModelTransport::Claude(
                    ClaudeTransport::DirectAnthropic
                )))
            )
        })
        .map(|(index, binding)| async move {
            (
                index,
                probe_claude_transport(client, origin, key, &binding.model_id, tool_id).await,
            )
        })
        .collect::<Vec<_>>();
    let probed = probed_within(probes, PROBE_BATCH_BUDGET).await;
    for (index, probed) in probed {
        if let Some(slot) = transports.get_mut(index) {
            *slot = Some(ModelTransport::Claude(probed));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_vendor_is_wired_from_the_table_and_never_probed_with_a_conversation() {
        use codex_desktop::CodexTransport;
        for (origin, tool, expected) in [
            (
                "https://open.bigmodel.cn",
                "codex_desktop",
                Some(ModelTransport::Codex(CodexTransport::ChatBridge)),
            ),
            (
                "https://openrouter.ai",
                "codex_desktop",
                Some(ModelTransport::Codex(CodexTransport::DirectResponses)),
            ),
            (
                "https://open.bigmodel.cn",
                "claude_code",
                Some(ModelTransport::Claude(ClaudeTransport::DirectAnthropic)),
            ),
            (
                "https://api.openai.com",
                "claude_desktop",
                Some(ModelTransport::Claude(ClaudeTransport::ChatBridge)),
            ),
            ("https://open.bigmodel.cn", "pi", None),
            ("https://relay.example.com", "codex_desktop", None),
            ("https://relay.example.com", "claude_code", None),
        ] {
            assert_eq!(preset_transport(origin, tool), expected, "{origin} {tool}");
        }
    }

    fn binding(model: &str, group: &str) -> ModelBinding {
        ModelBinding {
            model_id: model.into(),
            billing_group: group.into(),
        }
    }

    #[test]
    fn a_pasted_key_is_never_deleted_or_retired() {
        let leases = leases(
            &[
                binding("a", "default"),
                binding("b", "default"),
                binding("c", "vip"),
            ],
            "sk-user-key",
        );
        assert_eq!(leases.len(), 2);
        for (_, lease) in &leases {
            assert_eq!(lease.key, "sk-user-key");
            assert!(!lease.created);
            assert!(lease.retire_after_commit.is_empty());
        }
    }

    #[test]
    fn the_catalogue_decides_what_the_key_can_use() {
        let catalogue = json!({"data": [{"id": "gpt-5"}, {"id": "claude-x"}]});
        let wanted = [binding("gpt-5", "default")];
        assert_eq!(
            transports_from_catalogue(200, &catalogue, &wanted, "codex_desktop").unwrap(),
            vec![Some(ModelTransport::Codex(
                codex_desktop::CodexTransport::DirectResponses
            ))]
        );
        assert_eq!(
            transports_from_catalogue(200, &catalogue, &wanted, "pi").unwrap(),
            vec![None]
        );
        let missing = [binding("not-here", "default")];
        assert!(matches!(
            transports_from_catalogue(200, &catalogue, &missing, "pi"),
            Err(ActivationFailure::UnsupportedModel)
        ));
        // 密钥被站点收回，和「站点暂时没答上来」是两件事。
        assert!(matches!(
            transports_from_catalogue(401, &Value::Null, &wanted, "pi"),
            Err(ActivationFailure::SignedOut)
        ));
        assert!(matches!(
            transports_from_catalogue(502, &Value::Null, &wanted, "pi"),
            Err(ActivationFailure::ServerUnavailable)
        ));
    }

    #[test]
    fn only_a_clear_no_plus_a_clear_yes_moves_claude_to_the_bridge() {
        use ProbeOutcome::{Accepted, Inconclusive, Rejected};
        assert_eq!(
            claude_transport_from_probes(Rejected, Some(Accepted)),
            ClaudeTransport::ChatBridge
        );
        for (messages, chat) in [
            (Accepted, None),
            (Inconclusive, None),
            (Rejected, Some(Rejected)),
            (Rejected, Some(Inconclusive)),
        ] {
            assert_eq!(
                claude_transport_from_probes(messages, chat),
                ClaudeTransport::DirectAnthropic,
                "{messages:?} {chat:?}"
            );
        }
    }

    /// 只会说 OpenAI 格式的站：`/v1/messages` 404，Chat 可用 → 过桥；
    /// 并且 Claude Code 探测时带的是 `x-api-key`，不是 Bearer。
    #[tokio::test]
    async fn an_openai_only_station_puts_claude_on_the_bridge() {
        use std::sync::{Arc, Mutex};

        let seen = Arc::new(Mutex::new(Vec::<String>::new()));
        let record = seen.clone();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let router = axum::Router::new()
            .route(
                "/v1/messages",
                axum::routing::post(move |headers: axum::http::HeaderMap, body: String| {
                    let record = record.clone();
                    async move {
                        record.lock().unwrap().push(format!(
                            "x-api-key={} bearer={}",
                            headers.contains_key("x-api-key"),
                            headers.contains_key("authorization")
                        ));
                        if body.contains("speaks-anthropic") {
                            axum::http::StatusCode::OK
                        } else {
                            axum::http::StatusCode::NOT_FOUND
                        }
                    }
                }),
            )
            .route(
                "/v1/chat/completions",
                axum::routing::post(|| async { axum::http::StatusCode::OK }),
            );
        tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
        let client = reqwest::Client::builder()
            .no_proxy()
            .timeout(Duration::from_secs(5))
            .build()
            .unwrap();

        let bindings = [
            binding("chat-only", "default"),
            binding("speaks-anthropic", "default"),
        ];
        let mut transports =
            vec![Some(ModelTransport::Claude(ClaudeTransport::DirectAnthropic)); bindings.len()];
        apply_claude_probes_with(
            &client,
            &origin,
            "sk-user-key",
            "claude_code",
            &bindings,
            &mut transports,
        )
        .await;

        assert_eq!(
            transports,
            vec![
                Some(ModelTransport::Claude(ClaudeTransport::ChatBridge)),
                Some(ModelTransport::Claude(ClaudeTransport::DirectAnthropic)),
            ]
        );
        assert!(seen
            .lock()
            .unwrap()
            .iter()
            .all(|line| line == "x-api-key=true bearer=false"));
    }
}

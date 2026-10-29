//! Capability/probe policy tests moved with the code they protect. These use a
//! loopback stub only; no real station or paid endpoint is contacted.
#![cfg(test)]

use super::*;
use crate::domain::model::ModelBinding;
use crate::tool_adapters::codex_desktop::CodexTransport::{ChatBridge, DirectResponses};
use std::sync::Arc;
use std::time::Duration;

fn binding(model: &str, group: &str) -> ModelBinding {
    ModelBinding {
        model_id: model.into(),
        billing_group: group.into(),
    }
}
/// 网络层的三档归类。5xx 和网络错误必须落到 `Inconclusive` ——
/// 归成 `Rejected` 的话，中转抖一下就会把模型永久切到桥上。
///
/// 429 和 408 同理：它们是 4xx，却不是「这个协议不行」。十几个模型一起探
/// 就会撞上中转的用户级限流，回的正是 429；把它读成拒绝，再碰上一次没被
/// 限的 Chat 探测，模型就被**永久**钉到桥上，而用户什么都没做错。
#[test]
fn a_server_side_failure_is_never_read_as_a_protocol_refusal() {
    use ProbeOutcome::{Accepted, Inconclusive, Rejected};
    let outcome = |status: u16| probe_outcome_for(reqwest::StatusCode::from_u16(status).unwrap());
    for status in [200u16, 201, 204] {
        assert_eq!(outcome(status), Accepted, "{status}");
    }
    // 真正的拒绝：参数错、没授权、路由不认、渠道没配。
    for status in [400u16, 401, 403, 404, 422] {
        assert_eq!(outcome(status), Rejected, "{status}");
    }
    // 「问得太急」和「这次没等到」不是拒绝，得重问，且绝不能翻转路由。
    for status in [408u16, 429] {
        assert_eq!(outcome(status), Inconclusive, "{status}");
    }
    for status in [500u16, 502, 503, 504] {
        assert_eq!(outcome(status), Inconclusive, "{status}");
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

/// Responses 被限流（429），Chat 通过 —— 结论必须是**留在直连**，不上桥。
///
/// 这是 2026-09-21 复查出的那条：以前 429 → `Rejected`，于是发出 Chat 探测，
/// Chat 恰好没被限、通过，`transport_from_probes(Rejected, Some(Accepted))`
/// 就把模型钉到了桥上。全程走真实的 `probe_codex_transport`，用本地桩
/// 回 429，钉住的是端到端的判定，不只是分类函数。
#[tokio::test]
async fn a_rate_limited_responses_probe_never_moves_a_model_onto_the_bridge() {
    use std::sync::atomic::{AtomicU8, Ordering};

    let chat_hits = Arc::new(AtomicU8::new(0));
    let chat_counter = chat_hits.clone();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let router = axum::Router::new()
        .route(
            "/v1/responses",
            axum::routing::post(|| async { axum::http::StatusCode::TOO_MANY_REQUESTS }),
        )
        .route(
            "/v1/chat/completions",
            axum::routing::post(move || {
                chat_counter.fetch_add(1, Ordering::SeqCst);
                async { axum::http::StatusCode::OK }
            }),
        );
    tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    let client = reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(5))
        .build()
        .unwrap();

    let transport = probe_codex_transport(&client, &origin, "sk-x", "m").await;
    assert_eq!(
        transport,
        codex_desktop::CodexTransport::DirectResponses,
        "429 不是拒绝，不能因此上桥"
    );
    // 没有明确拒绝，就不该去问 Chat：那一探只在 Responses 被拒时才发。
    assert_eq!(chat_hits.load(Ordering::SeqCst), 0);
}

/// 探测有并发上限，且结论仍按下标写回各自的模型。
///
/// 上限本身就是修复的一部分：全部 `join_all` 时十几个请求同时到达，中转
/// 按用户限流，探测于是自己制造了它要判读的 429。这里用一个会数「同时
/// 在路上」的桩，断言峰值不超过 `PROBE_CONCURRENCY`。
#[tokio::test]
async fn probes_are_bounded_in_flight_and_still_land_on_their_own_model() {
    use std::sync::atomic::{AtomicUsize, Ordering};

    let in_flight = Arc::new(AtomicUsize::new(0));
    let peak = Arc::new(AtomicUsize::new(0));
    let (flight, high) = (in_flight.clone(), peak.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let router = axum::Router::new()
        .route(
            "/v1/responses",
            axum::routing::post(move |body: String| {
                let (flight, high) = (flight.clone(), high.clone());
                async move {
                    let now = flight.fetch_add(1, Ordering::SeqCst) + 1;
                    high.fetch_max(now, Ordering::SeqCst);
                    tokio::time::sleep(Duration::from_millis(60)).await;
                    flight.fetch_sub(1, Ordering::SeqCst);
                    // 只有 `chat-only` 这个模型被 Responses 拒绝。
                    if body.contains("chat-only") {
                        axum::http::StatusCode::NOT_FOUND
                    } else {
                        axum::http::StatusCode::OK
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

    let bindings = ["a", "b", "chat-only", "d", "e", "f"]
        .into_iter()
        .map(|model| binding(model, "default"))
        .collect::<Vec<_>>();
    let leases = vec![(
        "default".to_string(),
        TokenLease {
            id: 1,
            key: "sk-x".into(),
            created: true,
            retire_after_commit: vec![],
        },
    )];
    let mut transports = vec![Some(ModelTransport::Codex(DirectResponses)); bindings.len()];
    apply_codex_probes_with(&client, &origin, &bindings, &leases, &mut transports).await;

    assert!(
        peak.load(Ordering::SeqCst) <= PROBE_CONCURRENCY,
        "同时在路上的探测超过了上限：{}",
        peak.load(Ordering::SeqCst)
    );
    for (index, binding) in bindings.iter().enumerate() {
        let expected = if binding.model_id == "chat-only" {
            ChatBridge
        } else {
            DirectResponses
        };
        assert_eq!(
            transports[index],
            Some(ModelTransport::Codex(expected)),
            "{}",
            binding.model_id
        );
    }
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
    assert!(!model_supports_tool(&pricing, "chat", "aider"));
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

/// 产品承诺「不发收费的测试请求」，而探测成功的那次会真的生成 token。
/// 所以判断「这次要不要探」必须是一个能单独钉住的函数：寄在
/// `configure_desktop_tool` 的某个分支条件里，这条承诺就只剩代码读起来像。
#[test]
fn the_probe_only_runs_when_the_user_turned_it_on_for_codex() {
    assert!(probe_requested("codex_desktop", true));
    // 缺省就是关：不带这个字段的旧渲染层、或者用户没点开关，都不许探。
    assert!(!probe_requested("codex_desktop", false));
    // 其他工具根本不探测，开关开着也不能把请求发到别处去。
    for tool in [
        "claude_code",
        "claude_desktop",
        "opencode",
        "pi",
        "dsh_web",
        "workbuddy",
        "dsh_desktop",
    ] {
        assert!(!probe_requested(tool, true), "{tool}");
    }
}

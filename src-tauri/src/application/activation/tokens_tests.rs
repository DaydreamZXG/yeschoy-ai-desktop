//! Token enrollment policy tests, moved with the code they protect. The fake
//! relay lives in memory only: no network, keychain or real home directory.
#![cfg(test)]

use serde_json::{json, Value};

use super::super::tokens_tests_support::{fake_key, minted_name, FakeTokens};

fn our_key(id: u64, prefix: &str, group: &str, models: &str) -> Value {
    let mut key = fake_key(id, &minted_name(prefix, id as u32), group);
    key["model_limits"] = Value::String(models.to_owned());
    key
}
use super::*;
use crate::application::activation::capability::ModelTransport;
use crate::application::activation::error::ActivationFailure;
use crate::domain::model::ModelBinding;
use crate::tool_adapters::codex_desktop::CodexTransport::{ChatBridge, DirectResponses};

fn binding(model: &str, group: &str) -> ModelBinding {
    ModelBinding {
        model_id: model.into(),
        billing_group: group.into(),
    }
}

fn skip(model: &str, group: &str) -> crate::tool_activation::SkippedBinding {
    crate::tool_activation::SkippedBinding {
        model_id: model.into(),
        billing_group: group.into(),
        reason_code: "server_unavailable",
    }
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

/// 会话失效和「助手正在退出」是整单的事，不是某个分组的。
///
/// 分组循环以前把每个 `Err` 都记成「这个分组跳过」：401 之后接着对下一个
/// 分组发请求（还是 401），最后把「请重新登录」藏进 `skipped`，界面照常
/// 宣布「接入完成」。这里钉住分类，循环按它决定是停下还是跳过。
#[tokio::test]
async fn a_signed_out_session_stops_the_group_loop_instead_of_being_skipped() {
    // 真正从传输层出来的 401，而不是手写的枚举：`list_tokens` 把它译成
    // `SignedOut`，循环必须认出来。
    let api = FakeTokens::default();
    api.signed_out
        .store(true, std::sync::atomic::Ordering::Relaxed);
    let Err(error) = acquire_token_using(
        &api,
        "https://mock.invalid",
        "codex_desktop",
        "default",
        &["kimi-k3".to_string()],
    )
    .await
    else {
        panic!("401 不该铸出密钥");
    };
    assert!(matches!(error, ActivationFailure::SignedOut));
    assert!(token_failure_is_global(&error));

    // 中转不可用是这个分组的事：其余分组照常，这条不变。
    let api = FakeTokens::default();
    api.fail_list
        .store(true, std::sync::atomic::Ordering::Relaxed);
    let Err(error) = acquire_token_using(
        &api,
        "https://mock.invalid",
        "codex_desktop",
        "default",
        &["kimi-k3".to_string()],
    )
    .await
    else {
        panic!("列表挂了不该铸出密钥");
    };
    assert!(matches!(error, ActivationFailure::ServerUnavailable));
    assert!(!token_failure_is_global(&error));

    // 整张表：只有这两个是整单的。
    assert!(token_failure_is_global(
        &ActivationFailure::ConfigurationFailed("assistant_shutting_down")
    ));
    for local in [
        ActivationFailure::ServerUnavailable,
        ActivationFailure::UnsupportedModel,
        ActivationFailure::ConfigurationFailed("invalid_request"),
        ActivationFailure::Adapter(AdapterFailure::SecureStorageUnavailable),
    ] {
        assert!(!token_failure_is_global(&local), "{local:?}");
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
fn token_names_are_distinct_per_target() {
    let prefixes: Vec<String> = [
        "claude_code",
        "claude_desktop",
        "codex_desktop",
        "pi",
        "dsh_web",
        "workbuddy",
        "dsh_desktop",
    ]
    .iter()
    .map(|tool| token_prefix(tool, "default"))
    .collect();
    for (index, left) in prefixes.iter().enumerate() {
        assert!(!prefixes[index + 1..].contains(left), "{left}");
    }
}

#[test]
fn a_business_rejection_is_recognised_without_logging_server_text() {
    // The relay can answer 200 with `success: false`; that is a rejection,
    // and its `message` is untrusted free text that must never reach logs.
    assert!(super::super::envelope::body_reports_failure(
        &json!({"success": false, "message": "sk-leaked-key"})
    ));
    assert!(!super::super::envelope::body_reports_failure(
        &json!({"success": true, "data": []})
    ));
    assert!(!super::super::envelope::body_reports_failure(
        &json!({"message": "no success flag"})
    ));
    assert!(!super::super::envelope::body_reports_failure(&json!(
        "plain text"
    )));
}

#[tokio::test]
async fn minting_persists_its_intent_before_the_create_and_clears_it_after_ownership() {
    // Only names this run could have written are considered: the ledger is
    // shared within the test process, so earlier tests may have left records.
    let before: std::collections::BTreeSet<String> = super::issuance_records()
        .into_iter()
        .map(|record| record.name)
        .collect();
    let api = FakeTokens::default();
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
    // The create request saw a persisted record: intent was written first.
    assert!(
        api.intent_at_create
            .lock()
            .unwrap()
            .first()
            .is_some_and(|n| *n >= 1),
        "create was sent without a persisted intent"
    );
    // Once this run owns the key, a crash can no longer orphan it: the exact
    // name this run minted has left the ledger (other tests' unrelated records
    // inside the shared test ledger are not this run's concern).
    let after: Vec<String> = super::issuance_records()
        .into_iter()
        .map(|record| record.name)
        .filter(|name| !before.contains(name))
        .filter(|name| name.starts_with(&token_prefix("pi", "default")))
        .collect();
    assert!(after.is_empty(), "owned intent must be cleared: {after:?}");
}

#[tokio::test]
// The recovery path reuses the live create request; prove that seam directly.
async fn recovery_create_mints_the_exact_recorded_name_through_the_shared_path() {
    use crate::application::activation::tokens::create_recorded_token;
    let api = FakeTokens::default();
    let name = format!("{}-0123456789abcdef", token_prefix("pi", "default"));
    let id = create_recorded_token(
        &api,
        "https://mock.invalid",
        &name,
        "default",
        &["model-a".to_string()],
    )
    .await
    .unwrap();
    let tokens = api.tokens.lock().unwrap();
    assert_eq!(tokens.len(), 1);
    assert_eq!(tokens[0]["id"], id);
    assert_eq!(tokens[0]["name"], name);
    assert_eq!(tokens[0]["group"], "default");
}

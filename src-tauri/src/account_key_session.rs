//! 粘贴密钥的会话：站点没有可登录的账户接口时，用户贴进来的那把密钥就是会话。
//!
//! 和授权登录的区别只有一条：我们手里没有账户令牌，所以凡是要账户令牌的事
//! （按分组铸密钥、读价格、读用量日志、读公告）都做不了，也不假装能做。
//! 能做的是 OpenAI 兼容接口本来就给的：模型列表，以及这把密钥的额度。
//!
//! 密钥只进系统钥匙串，不写日志、不回传给界面、不进任何链接。
use serde::Deserialize;

use super::*;
use crate::account_finance::key_money;
use crate::station_connect::connect_origin_is_acceptable;
use crate::station_probe::{classify_status, model_ids, token_is_plausible};

const LINE_PREFIX: &str = "station_key:";
const KEY_MODELS_PATH: &str = "/v1/models";
const KEY_SUBSCRIPTION_PATH: &str = "/v1/dashboard/billing/subscription";
const KEY_USAGE_PATH: &str = "/v1/dashboard/billing/usage";
/// 连接是用户盯着等的一步，比开机检查宽一点，但仍然必须有头。
const CONNECT_DEADLINE: Duration = Duration::from_secs(12);

pub(crate) fn origin_from_line_id(line_id: &str) -> Option<&str> {
    let origin = line_id.strip_prefix(LINE_PREFIX)?;
    (connect_origin_is_acceptable(origin).as_deref() == Some(origin)).then_some(origin)
}

fn line_id_for_origin(origin: &str) -> String {
    format!("{LINE_PREFIX}{origin}")
}

/// 接入流程问：这条线路该用哪种凭据。
pub(crate) enum KeyAccess {
    /// 不是粘贴密钥的线路，走原来的账户路径。
    NotKeyLine,
    /// 是密钥线路，但钥匙串里没有它的密钥（没连过，或已经换了站点）。
    SignedOut,
    Ready {
        origin: String,
        key: String,
    },
}

pub(crate) fn access(line_id: &str) -> Result<KeyAccess, ()> {
    let Some(origin) = origin_from_line_id(line_id) else {
        return Ok(KeyAccess::NotKeyLine);
    };
    Ok(load_stored_session()?
        .filter(|stored| stored.line_id == line_id)
        .and_then(|stored| stored.api_key)
        .map_or(KeyAccess::SignedOut, |key| KeyAccess::Ready {
            origin: origin.to_owned(),
            key,
        }))
}

/// 站点返回的文字原样进界面之前过一道：超长或带控制字符的一律不要。
fn plain(value: &str, maximum: usize) -> bool {
    !value.is_empty()
        && value.chars().count() <= maximum
        && !value.chars().any(|character| {
            character.is_control()
                || matches!(character, '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
        })
}

fn number_field(value: &Value, field: &str) -> Option<f64> {
    value.get(field).and_then(Value::as_f64)
}

async fn read(
    request_id: String,
    origin: &str,
    api_key: &str,
) -> Result<AccountProjection, AccountProjectionFailure> {
    let models_url = format!("{origin}{KEY_MODELS_PATH}");
    let status_url = format!("{origin}/api/status");
    let subscription_url = format!("{origin}{KEY_SUBSCRIPTION_PATH}");
    let usage_url = format!("{origin}{KEY_USAGE_PATH}");
    let (models, status, subscription, usage) = tokio::join!(
        send_json(Method::GET, &models_url, Some(api_key), None),
        send_json(Method::GET, &status_url, None, None),
        send_json(Method::GET, &subscription_url, Some(api_key), None),
        send_json(Method::GET, &usage_url, Some(api_key), None),
    );
    let models = models.map_err(AccountProjectionFailure::Transport)?;
    if models.status == StatusCode::UNAUTHORIZED || models.status == StatusCode::FORBIDDEN {
        return Err(AccountProjectionFailure::SessionExpired);
    }
    if !models.status.is_success() {
        return Err(AccountProjectionFailure::BackendUnavailable);
    }
    let success = |response: Result<JsonResponse, TransportFailure>| {
        response
            .ok()
            .filter(|response| response.status.is_success())
            .map(|response| response.value)
    };
    Ok(project(
        request_id,
        &models.value,
        success(status).as_ref(),
        success(subscription).as_ref(),
        success(usage).as_ref(),
    ))
}

/// 把站点的回答变成界面看的账户投影。只有 `models` 是必需的，其余三样
/// 站点给不出来就留空：界面会照实说「看不到」。
fn project(
    request_id: String,
    models: &Value,
    status: Option<&Value>,
    subscription: Option<&Value>,
    usage: Option<&Value>,
) -> AccountProjection {
    let mut projection = AccountProjection::empty(request_id, "signed_in", "key_session");
    projection.observed_at_epoch_ms = now_epoch_ms();
    projection.models = model_ids(models)
        .into_iter()
        .filter(|id| plain(id, 200))
        .map(unpriced_model)
        .collect();
    // 没有账户可言，但界面把「已连接」和「有账户」当成一回事；这里只说
    // 「连上了」，名字用站点自己报的名字，其余账户数字全部留空。
    projection.account.available = true;
    if let Some((_, name, _)) = status.and_then(classify_status) {
        if plain(&name, 160) {
            projection.account.display_name = name;
        }
    }
    // 两个数都读到才算；只读到一个就算不出余额，宁可说看不到。
    if let (Some(limit), Some(used)) = (
        subscription.and_then(|value| number_field(value, "hard_limit_usd")),
        usage.and_then(|value| number_field(value, "total_usage")),
    ) {
        projection.money = key_money(status, limit, used);
    }
    projection
}

pub(super) async fn inspect(
    request: &AccountRequest,
    origin: &str,
) -> Result<AccountProjection, AccountProjectionFailure> {
    let api_key = load_stored_session()
        .map_err(|_| AccountProjectionFailure::SecureStorage)?
        .filter(|stored| stored.line_id == request.line_id)
        .and_then(|stored| stored.api_key);
    let Some(api_key) = api_key else {
        return Ok(AccountProjection::empty(
            request.request_id.clone(),
            "signed_out",
            "signed_out",
        ));
    };
    let permit = crate::shutdown_coordinator::global()
        .admit_operation()
        .map_err(|_| AccountProjectionFailure::BackendUnavailable)?;
    permit
        .cancel_safe(read(request.request_id.clone(), origin, &api_key))
        .await
        .map_err(|_| AccountProjectionFailure::BackendUnavailable)?
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct KeyConnectRequest {
    request_id: String,
    origin: String,
    token: String,
}

impl std::fmt::Debug for KeyConnectRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("KeyConnectRequest")
            .field("request_id", &self.request_id)
            .field("token", &"[redacted]")
            .finish()
    }
}

/// 用粘贴的站点网址和密钥连接。先真的问站点一次，站点认这把密钥才保存。
#[tauri::command]
pub async fn account_connect_with_key_v1(
    state: tauri::State<'_, AccountV2State>,
    request: KeyConnectRequest,
) -> Result<AccountProjection, String> {
    let _permit = crate::shutdown_coordinator::global()
        .admit_operation()
        .map_err(|_| "assistant_shutting_down")?;
    if !request_id_is_valid(&request.request_id) {
        return Err("invalid_account_request".into());
    }
    let Some(origin) = connect_origin_is_acceptable(request.origin.trim()) else {
        return Err("invalid_account_request".into());
    };
    let token = request.token.trim();
    // 16..=256 是应用凭据库的上下限（`tool_credentials::record_is_valid`）：
    // 现在不拦，就会连接成功、到接入那一步才失败。
    if !token_is_plausible(token) || !(16..=256).contains(&token.len()) {
        return Ok(AccountProjection::empty(
            request.request_id,
            "signed_out",
            "key_rejected",
        ));
    }
    let read = tokio::time::timeout(
        CONNECT_DEADLINE,
        read(request.request_id.clone(), &origin, token),
    )
    .await;
    let projection = match read {
        Err(_) => {
            return Ok(failure_projection(
                request.request_id,
                AccountProjectionFailure::Transport(TransportFailure::TimedOut),
            ))
        }
        Ok(Err(AccountProjectionFailure::SessionExpired)) => {
            return Ok(AccountProjection::empty(
                request.request_id,
                "signed_out",
                "key_rejected",
            ))
        }
        Ok(Err(failure)) => return Ok(failure_projection(request.request_id, failure)),
        Ok(Ok(projection)) => projection,
    };
    let stored = StoredSession {
        line_id: line_id_for_origin(&origin),
        refresh_token: String::new(),
        session_id: String::new(),
        refresh_expires_at: 0,
        api_key: Some(token.to_owned()),
    };
    let mut runtime = state
        .runtime
        .lock()
        .map_err(|_| "account_state_unavailable")?;
    if save_stored_session(&stored).is_err() {
        log::warn!("account stage=key_session_store_failed");
        return Ok(failure_projection(
            request.request_id,
            AccountProjectionFailure::SecureStorage,
        ));
    }
    runtime.authorization_epoch += 1;
    runtime.access = None;
    runtime.pending = None;
    runtime.wallet_url = None;
    log::info!(
        "account stage=key_session_connected models={}",
        projection.models.len()
    );
    Ok(projection)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn fixture(name: &str) -> Value {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../src/account/fixtures")
            .join(name);
        serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap()
    }

    fn projected(
        status: Option<Value>,
        subscription: Option<Value>,
        usage: Option<Value>,
    ) -> Value {
        let models = json!({"object":"list","data":[
            {"id":"gpt-5.5","object":"model"},
            {"id":"claude-sonnet-4-5","object":"model"},
            {"id":"bad\u{202e}name","object":"model"}
        ]});
        let mut projection = project(
            "account-test-1".into(),
            &models,
            status.as_ref(),
            subscription.as_ref(),
            usage.as_ref(),
        );
        projection.observed_at_epoch_ms = 1;
        serde_json::to_value(projection).unwrap()
    }

    /// 界面那边用同一份文件做解码测试（`src/account/session.test.ts`）：
    /// 这里的输出一变，两边的测试一起失败，而不是界面悄悄显示「未连接」。
    #[test]
    fn a_station_that_only_lists_models_projects_exactly_what_the_renderer_decodes() {
        let bare = projected(None, None, None);
        assert_eq!(bare, fixture("keySessionProjection.json"));
        assert_eq!(bare["status"], "signed_in");
        assert_eq!(bare["money"]["currency"], "");
        assert_eq!(bare["account"]["balanceQuota"], "");
        assert_eq!(bare["models"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn a_new_api_station_adds_its_name_and_the_keys_own_balance() {
        let status = json!({"success":true,"data":{
            "system_name":"小明的中转","quota_display_type":"CNY",
            "quota_per_unit":500000,"usd_exchange_rate":7,"price":2
        }});
        let rich = projected(
            Some(status.clone()),
            Some(json!({"hard_limit_usd":20.0})),
            Some(json!({"total_usage":350.0})),
        );
        assert_eq!(rich, fixture("keySessionProjectionWithBalance.json"));
        assert_eq!(rich["account"]["displayName"], "小明的中转");
        assert_eq!(rich["money"]["balanceAmount"], "16.5");
        // 只读到额度、没读到用量：算不出余额，就不显示。
        let half = projected(Some(status), Some(json!({"hard_limit_usd":20.0})), None);
        assert_eq!(half["money"]["currency"], "");
    }

    #[test]
    fn key_lines_carry_only_a_canonical_https_origin() {
        assert_eq!(
            origin_from_line_id("station_key:https://relay.example.com"),
            Some("https://relay.example.com")
        );
        for bad in [
            "station_key:http://relay.example.com",
            "station_key:https://relay.example.com/",
            "station_key:https://relay.example.com/v1",
            "station_key:https://127.0.0.1",
            "station_key:",
            "station_connect:https://relay.example.com",
        ] {
            assert_eq!(origin_from_line_id(bad), None, "{bad}");
        }
        assert!(account_request_line_id_is_valid(
            "station_key:https://relay.example.com"
        ));
    }

    #[test]
    fn a_stored_key_session_round_trips_without_account_fields() {
        let stored = StoredSession {
            line_id: line_id_for_origin("https://relay.example.com"),
            refresh_token: String::new(),
            session_id: String::new(),
            refresh_expires_at: 0,
            api_key: Some("sk-test-key-0001".into()),
        };
        let payload = serde_json::to_string(&stored).unwrap();
        let back: StoredSession = serde_json::from_str(&payload).unwrap();
        assert_eq!(back.api_key.as_deref(), Some("sk-test-key-0001"));
        // 旧版本写下的账户会话没有这个字段，仍然要能读。
        let legacy: StoredSession = serde_json::from_str(
            r#"{"lineId":"mainland_optimized","refreshToken":"r","sessionId":"s","refreshExpiresAt":1}"#,
        )
        .unwrap();
        assert_eq!(legacy.api_key, None);
    }

    #[test]
    fn the_request_never_prints_the_key() {
        let request = KeyConnectRequest {
            request_id: "r".into(),
            origin: "https://relay.example.com".into(),
            token: "sk-secret-value".into(),
        };
        assert!(!format!("{request:?}").contains("sk-secret-value"));
    }
}

//! 通用站点检测：小白只粘贴站点网址（可选再贴一个 sk- 令牌），
//! 助手自己判断它是不是 New API / One API 系的中转、支不支持一键连接、
//! 令牌能不能用、能用哪些模型。站点不需要做任何改动。
//!
//! 安全约束：只走 HTTPS、不跟随跳转、限制响应大小；令牌只用于这一次请求，
//! 不落盘、不写日志、不回传给前端。
use crate::net_route::{SendWithFallback, StationRoute};
use std::time::Duration;

use reqwest::{header, redirect::Policy, Client, StatusCode};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::station_connect::{connect_origin_is_acceptable, station_host_is_refused};

const TIMEOUT: Duration = Duration::from_secs(4);
const MAX_BYTES: usize = 2 * 1024 * 1024;
const MAX_MODELS: usize = 500;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StationProbeRequest {
    origin: String,
    #[serde(default)]
    token: Option<String>,
}

#[derive(Serialize, Default, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StationProbeResult {
    /// 规范化后的站点来源；无效时为空。
    origin: String,
    /// `ok` | `invalid_origin` | `refused_station` | `unreachable` | `timeout` | `not_relay`
    status: &'static str,
    /// `new_api` | `one_api` | `openai_compatible` | `unknown`
    kind: &'static str,
    system_name: String,
    version: String,
    /// 站点已经部署了桌面一键连接接口（/api/desktop/v2/bootstrap）。
    desktop_connect: bool,
    /// `not_provided` | `valid` | `invalid` | `no_quota` | `error`
    token_status: &'static str,
    models: Vec<String>,
}

fn client() -> Result<Client, ()> {
    Client::builder()
        .https_only(true)
        .redirect(Policy::none())
        .timeout(TIMEOUT)
        .station_route(Duration::from_secs(4))
        .user_agent(concat!("YesChoy-Desktop/", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|_| ())
}

enum Fetch {
    Json(StatusCode, Value),
    Status(StatusCode),
    Timeout,
    Unreachable,
}

async fn get(client: &Client, url: &str, bearer: Option<&str>) -> Fetch {
    let mut request = client.get(url).header(header::ACCEPT, "application/json");
    if let Some(token) = bearer {
        request = request.bearer_auth(token);
    }
    let response = match request.send_with_fallback().await {
        Ok(response) => response,
        Err(error) if error.is_timeout() => return Fetch::Timeout,
        Err(_) => return Fetch::Unreachable,
    };
    let status = response.status();
    if response
        .content_length()
        .is_some_and(|len| len as usize > MAX_BYTES)
    {
        return Fetch::Status(status);
    }
    match response.bytes().await {
        Ok(bytes) if bytes.len() <= MAX_BYTES => match serde_json::from_slice(&bytes) {
            Ok(value) => Fetch::Json(status, value),
            Err(_) => Fetch::Status(status),
        },
        _ => Fetch::Status(status),
    }
}

fn short(value: Option<&Value>) -> String {
    value
        .and_then(Value::as_str)
        .map(|text| text.chars().take(80).collect())
        .unwrap_or_default()
}

/// 从 /api/status 判断面板类型。New API 在 One API 的基础上多了一批字段。
pub(crate) fn classify_status(value: &Value) -> Option<(&'static str, String, String)> {
    if value.get("success").and_then(Value::as_bool) != Some(true) {
        return None;
    }
    let data = value.get("data")?.as_object()?;
    let new_api_markers = [
        "quota_display_type",
        "default_use_auto_group",
        "self_use_mode_enabled",
        "setup",
        "price",
        "docs_link",
        "demo_site_enabled",
    ];
    let kind = if new_api_markers.iter().any(|key| data.contains_key(*key)) {
        "new_api"
    } else if data.contains_key("system_name") || data.contains_key("quota_per_unit") {
        "one_api"
    } else {
        return None;
    };
    Some((
        kind,
        short(data.get("system_name")),
        short(data.get("version")),
    ))
}

pub(crate) fn model_ids(value: &Value) -> Vec<String> {
    let mut ids: Vec<String> = value
        .get("data")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.get("id").and_then(Value::as_str))
                .filter(|id| !id.is_empty() && id.len() <= 200)
                .map(str::to_owned)
                .take(MAX_MODELS)
                .collect()
        })
        .unwrap_or_default();
    ids.sort();
    ids.dedup();
    ids
}

pub(crate) fn token_is_plausible(token: &str) -> bool {
    (8..=512).contains(&token.len())
        && token
            .bytes()
            .all(|byte| byte.is_ascii_graphic() && byte != b'"' && byte != b'\\')
}

/// 先敲哪扇门，以及这是不是云厂商。云厂商不是中转站，没有 `/api/status`：
/// 只看它的接口地址通不通，不去认是哪种中转站。不然不带密钥来体检时，
/// 好好的厂商会被说成“不是中转站”。
fn first_look(origin: &str) -> (String, bool) {
    if crate::provider_presets::for_origin(origin).is_some() {
        (
            format!("{}/models", crate::provider_presets::openai_base(origin)),
            true,
        )
    } else {
        (format!("{origin}/api/status"), false)
    }
}

#[tauri::command]
pub async fn station_probe_v1(request: StationProbeRequest) -> StationProbeResult {
    let raw = request.origin.trim();
    // 小白常常直接粘贴带路径的网址（…/console/token）或不带协议，替他收拾好。
    let with_scheme = if raw.contains("://") {
        raw.to_owned()
    } else {
        format!("https://{raw}")
    };
    let origin = reqwest::Url::parse(&with_scheme)
        .ok()
        .map(|url| url.origin().ascii_serialization())
        .and_then(|origin| connect_origin_is_acceptable(&origin));
    let Some(origin) = origin else {
        // 名单上不给连的站点单独说，不混在「网址看不懂」里。
        let refused = reqwest::Url::parse(&with_scheme)
            .ok()
            .and_then(|url| url.host_str().map(station_host_is_refused))
            .unwrap_or(false);
        return StationProbeResult {
            status: if refused {
                "refused_station"
            } else {
                "invalid_origin"
            },
            kind: "unknown",
            token_status: "not_provided",
            ..Default::default()
        };
    };
    let mut result = StationProbeResult {
        origin: origin.clone(),
        status: "ok",
        kind: "unknown",
        token_status: "not_provided",
        ..Default::default()
    };
    let Ok(client) = client() else {
        result.status = "unreachable";
        return result;
    };

    let (first_look, cloud) = first_look(&origin);
    if cloud {
        result.kind = "openai_compatible";
    }
    let relay_probe = get(&client, &first_look, None).await;
    match relay_probe {
        Fetch::Json(_, value) if !cloud => {
            if let Some((kind, name, version)) = classify_status(&value) {
                result.kind = kind;
                result.system_name = name;
                result.version = version;
            }
        }
        Fetch::Json(..) => {}
        Fetch::Timeout => {
            result.status = "timeout";
            return result;
        }
        Fetch::Unreachable => {
            result.status = "unreachable";
            return result;
        }
        Fetch::Status(_) => {}
    }

    if !cloud {
        if let Fetch::Json(status, value) =
            get(&client, &format!("{origin}/api/desktop/v2/bootstrap"), None).await
        {
            result.desktop_connect =
                status.is_success() && value.get("success").and_then(Value::as_bool) == Some(true);
        }
    }

    if let Some(token) = request
        .token
        .as_deref()
        .map(str::trim)
        .filter(|token| !token.is_empty())
    {
        if !token_is_plausible(token) {
            result.token_status = "invalid";
        } else {
            match get(
                &client,
                &format!("{}/models", crate::provider_presets::openai_base(&origin)),
                Some(token),
            )
            .await
            {
                Fetch::Json(status, value) if status.is_success() => {
                    result.models = model_ids(&value);
                    result.token_status = "valid";
                    if result.kind == "unknown" {
                        result.kind = "openai_compatible";
                    }
                }
                Fetch::Json(StatusCode::UNAUTHORIZED, _)
                | Fetch::Status(StatusCode::UNAUTHORIZED)
                | Fetch::Json(StatusCode::FORBIDDEN, _)
                | Fetch::Status(StatusCode::FORBIDDEN) => result.token_status = "invalid",
                Fetch::Json(StatusCode::PAYMENT_REQUIRED, _)
                | Fetch::Status(StatusCode::PAYMENT_REQUIRED) => result.token_status = "no_quota",
                _ => result.token_status = "error",
            }
        }
    }

    if result.kind == "unknown" && !result.desktop_connect {
        result.status = "not_relay";
    }
    log::info!(
        "station_probe origin={} status={} kind={} desktop_connect={} token_status={} models={}",
        result.origin,
        result.status,
        result.kind,
        result.desktop_connect,
        result.token_status,
        result.models.len()
    );
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn classifies_new_api_and_one_api() {
        let new_api = json!({"success": true, "data": {"system_name": "某某中转", "version": "v0.9", "quota_display_type": "USD"}});
        assert_eq!(classify_status(&new_api).unwrap().0, "new_api");
        let one_api =
            json!({"success": true, "data": {"system_name": "One API", "version": "v0.6"}});
        assert_eq!(classify_status(&one_api).unwrap().0, "one_api");
        assert!(classify_status(&json!({"success": false})).is_none());
        assert!(classify_status(&json!({"hello": "world"})).is_none());
    }

    #[test]
    fn model_ids_are_sorted_unique_and_bounded() {
        let value = json!({"data": [{"id": "b"}, {"id": "a"}, {"id": "b"}, {"id": ""}, {"x": 1}]});
        assert_eq!(model_ids(&value), vec!["a", "b"]);
    }

    #[test]
    fn cloud_providers_are_not_asked_whether_they_are_a_relay() {
        assert_eq!(
            first_look("https://openrouter.ai"),
            ("https://openrouter.ai/api/v1/models".to_owned(), true)
        );
        assert_eq!(
            first_look("https://relay.example.com"),
            ("https://relay.example.com/api/status".to_owned(), false)
        );
    }

    #[tokio::test]
    async fn a_refused_station_is_named_as_such_without_being_contacted() {
        for origin in ["open-design.ai", "https://api.open-design.ai/v1"] {
            let result = station_probe_v1(StationProbeRequest {
                origin: origin.into(),
                token: Some("sk-not-sent".into()),
            })
            .await;
            assert_eq!(result.status, "refused_station", "{origin}");
            assert_eq!(result.origin, "", "{origin}");
        }
        let unreadable = station_probe_v1(StationProbeRequest {
            origin: "http://[".into(),
            token: None,
        })
        .await;
        assert_eq!(unreadable.status, "invalid_origin");
    }

    #[test]
    fn token_shape_is_checked_before_sending() {
        assert!(token_is_plausible("sk-abcdefghijklmnop"));
        assert!(!token_is_plausible("short"));
        assert!(!token_is_plausible("sk-has space inside"));
    }
}

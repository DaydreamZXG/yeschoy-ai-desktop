//! Reading a station that speaks the neutral `desktop-connect` contract.
//!
//! Such a station answers in the currency its users pay in and prices every
//! plan per million tokens, so nothing here knows any relay's internal quota
//! unit. Account, usage and records are re-expressed in the shape the original
//! readers take, with one "quota" worth a millionth of the station's currency;
//! plans map straight onto the model list.

use std::collections::BTreeMap;

use serde::Serialize;
use serde_json::{json, Value};

use reqwest::Method;

use super::{decimal, send_json, AccountModel, BillingGroup, ModelBilling, NeutralPaths};

/// One "quota" in the synthesized legacy shapes is a millionth of a currency unit.
pub(super) const UNITS: f64 = 1_000_000.0;
const MAX_PLANS: usize = 64;
const MAX_MODELS_PER_PLAN: usize = 2_000;
const MAX_TIERS: usize = 8;

#[derive(Debug, Serialize, Clone, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PlanPricing {
    label: String,
    /// `CNY` or `USD`: what every price on this plan is in.
    currency: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    input_per_million: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    output_per_million: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    cache_read_per_million: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    cache_write_per_million: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    per_request: Option<f64>,
    /// Priced only for requests that come through this client.
    app_only: bool,
    subscription: bool,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    peak: Vec<PeakPeriod>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    long_context: Vec<LongContextTier>,
}

#[derive(Debug, Serialize, Clone, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PeakPeriod {
    start: String,
    end: String,
    multiplier: f64,
    #[serde(skip_serializing_if = "String::is_empty")]
    timezone: String,
    weekdays_only: bool,
}

#[derive(Debug, Serialize, Clone, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct LongContextTier {
    from_tokens: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    to_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    input_per_million: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    output_per_million: Option<f64>,
}

fn data(value: Option<&Value>) -> Option<&Value> {
    let value = value?;
    (value.get("success")?.as_bool()?)
        .then(|| value.get("data"))
        .flatten()
}

fn amount(value: Option<&Value>) -> Option<f64> {
    value?.as_f64().filter(|number| number.is_finite())
}

fn price(value: Option<&Value>) -> Option<f64> {
    amount(value).filter(|number| *number >= 0.0)
}

fn text(value: Option<&Value>, maximum: usize) -> String {
    value
        .and_then(Value::as_str)
        .map(|raw| {
            raw.chars()
                .filter(|c| !c.is_control())
                .take(maximum)
                .collect::<String>()
                .trim()
                .to_owned()
        })
        .unwrap_or_default()
}

/// `CNY` or `USD`; anything else is a currency this client cannot label.
pub(super) fn currency(payload: Option<&Value>) -> Option<&'static str> {
    match data(payload)?.get("currency")?.as_str()? {
        "CNY" => Some("CNY"),
        "USD" => Some("USD"),
        _ => None,
    }
}

/// The `/api/status` the original readers take their unit and currency from.
pub(super) fn status_shape(currency: &str) -> Value {
    json!({"success": true, "data": {
        "quota_per_unit": UNITS,
        "quota_display_type": currency,
        "usd_exchange_rate": 1,
        "price": 1,
    }})
}

pub(super) fn account_shape(payload: Option<&Value>) -> Option<Value> {
    let data = data(payload)?;
    let balance = amount(data.get("balance"))?;
    Some(json!({"success": true, "data": {
        "quota": (balance * UNITS).round() as i64,
        "display_name": text(data.get("displayName"), 160),
        "username": text(data.get("username"), 160),
    }}))
}

pub(super) fn usage_shape(payload: Option<&Value>) -> Option<Value> {
    let data = data(payload)?;
    Some(json!({"success": true, "data": {
        "quota": (price(data.get("consumed"))? * UNITS).round() as u64,
        "rpm": data.get("requests")?.as_u64()?,
        "tpm": data.get("tokens")?.as_u64()?,
    }}))
}

/// One page of records as a `/api/log/self` page. `None` when the page is not
/// a records page at all; rows that cannot be read are left out.
pub(super) fn records_shape(payload: Option<&Value>) -> Option<Value> {
    let data = data(payload)?;
    let items: Vec<Value> = data
        .get("records")?
        .as_array()?
        .iter()
        .filter_map(|row| {
            let cache = row.get("cacheTokens").and_then(Value::as_u64).unwrap_or(0);
            Some(json!({
                "type": 2,
                "created_at": row.get("atEpochMs")?.as_u64()? / 1000,
                "model_name": text(row.get("model"), 160),
                "token_name": text(row.get("keyName"), 160),
                "prompt_tokens": row.get("inputTokens").and_then(Value::as_u64).unwrap_or(0),
                "completion_tokens": row.get("outputTokens").and_then(Value::as_u64).unwrap_or(0),
                "quota": (price(row.get("amount"))? * UNITS).round() as i64,
                "other": json!({"cache_tokens": cache}).to_string(),
            }))
        })
        .collect();
    Some(json!({"success": true, "data": {
        "page": data.get("page").and_then(Value::as_u64).unwrap_or(1),
        "items": items,
    }}))
}

/// Where the original contract keeps keys; the rest of the client addresses
/// key operations here.
pub(super) const LEGACY_KEYS_PATH: &str = "/api/token/";

/// The original contract's model list and price list. A station on the
/// neutral contract answers both from its plans.
pub(super) const LEGACY_CATALOGUE_PATHS: [&str; 2] = ["/api/user/models", "/api/pricing"];

/// The station's plans, read as the list the original contract's caller asked for.
pub(super) fn catalogue_reply(path: &str, status: u16, value: &Value) -> (u16, Value) {
    let Some(plans) = data(Some(value))
        .filter(|_| (200..300).contains(&status))
        .and_then(|data| data.get("plans"))
        .and_then(Value::as_array)
    else {
        let status = if (200..300).contains(&status) {
            502
        } else {
            status
        };
        return (
            status,
            json!({"success": false, "message": text(value.get("code"), 64)}),
        );
    };
    if path != "/api/user/models" {
        // Callers only need to know the price list was readable; the prices
        // themselves are read through `models`.
        return (status, json!({"success": true, "data": []}));
    }
    let mut ids: Vec<String> = plans
        .iter()
        .filter_map(|plan| plan.get("models")?.as_array())
        .flatten()
        .filter_map(|model| Some(model.get("id")?.as_str()?.to_owned()))
        .collect();
    ids.sort();
    ids.dedup();
    (status, json!({"success": true, "data": ids}))
}

#[derive(Debug, PartialEq)]
enum KeyCall {
    List { page: u64 },
    Create,
    Reveal,
    Delete,
}

/// A key operation written for the original contract, as this station takes it.
fn key_call(
    method: &Method,
    url: &str,
    body: Option<&Value>,
    origin: &str,
    keys_path: &str,
) -> Option<(KeyCall, Method, String, Option<Value>)> {
    let parsed = reqwest::Url::parse(url).ok()?;
    let rest = parsed.path().strip_prefix(LEGACY_KEYS_PATH)?;
    let keys = format!("{origin}{keys_path}");
    let id = |raw: &str| {
        (!raw.is_empty() && raw.bytes().all(|b| b.is_ascii_digit())).then(|| raw.to_owned())
    };
    if rest.is_empty() && *method == Method::GET {
        let page = parsed
            .query_pairs()
            .find(|(key, _)| key == "p")
            .and_then(|(_, value)| value.parse().ok())
            .unwrap_or(1);
        return Some((KeyCall::List { page }, Method::GET, keys, None));
    }
    if rest.is_empty() && *method == Method::POST {
        let body = body?;
        let request = json!({
            "name": body.get("name")?.as_str()?,
            "planId": body.get("group")?.as_str()?,
        });
        return Some((KeyCall::Create, Method::POST, keys, Some(request)));
    }
    if *method == Method::POST {
        let id = id(rest.strip_suffix("/key")?)?;
        return Some((
            KeyCall::Reveal,
            Method::POST,
            format!("{keys}/{id}/reveal"),
            None,
        ));
    }
    if *method == Method::DELETE {
        return Some((
            KeyCall::Delete,
            Method::DELETE,
            format!("{keys}/{}", id(rest)?),
            None,
        ));
    }
    None
}

/// The station's answer, in the shape the original contract's callers read.
fn key_reply(call: &KeyCall, status: u16, value: &Value) -> (u16, Value) {
    let Some(data) = data(Some(value)).filter(|_| (200..300).contains(&status)) else {
        let code = text(value.get("code"), 64);
        // Callers read 401/403 as "signed out". A refusal that is about the
        // plan or the station's mode is not that: the session is fine.
        let status = match (status, code.as_str()) {
            (403, "plan_not_allowed" | "backend_mode") => 400,
            (200..=299, _) => 502,
            _ => status,
        };
        return (status, json!({"success": false, "message": code}));
    };
    let key_id = |row: &Value| row.get("id")?.as_str()?.parse::<u64>().ok();
    let reply = match call {
        KeyCall::List { page } => {
            let items: Vec<Value> = data
                .get("keys")
                .and_then(Value::as_array)
                .filter(|_| *page == 1)
                .into_iter()
                .flatten()
                .filter_map(|row| {
                    Some(json!({
                        "id": key_id(row)?,
                        "name": text(row.get("name"), 200),
                        "group": text(row.get("planId"), 64),
                        "status": if row.get("status").and_then(Value::as_str) == Some("active") { 1 } else { 2 },
                        "expired_time": -1,
                        "plan_scoped": true,
                    }))
                })
                .collect();
            json!({"total": items.len(), "items": items})
        }
        KeyCall::Create => match (key_id(data), data.get("key").and_then(Value::as_str)) {
            (Some(id), Some(key)) => {
                json!({"id": id, "name": text(data.get("name"), 200), "key": key})
            }
            _ => {
                return (
                    502,
                    json!({"success": false, "message": "invalid_response"}),
                )
            }
        },
        KeyCall::Reveal => match data.get("key").and_then(Value::as_str) {
            Some(key) => json!({"key": key}),
            None => {
                return (
                    502,
                    json!({"success": false, "message": "invalid_response"}),
                )
            }
        },
        KeyCall::Delete => Value::Null,
    };
    (status, json!({"success": true, "data": reply}))
}

pub(super) async fn key_exchange(
    method: Method,
    url: &str,
    body: Option<&Value>,
    origin: &str,
    paths: &NeutralPaths,
    access_token: &str,
) -> Result<(u16, Value), ()> {
    let Some((call, method, url, body)) = key_call(&method, url, body, origin, &paths.keys) else {
        // Nothing on this station answers that operation.
        return Ok((404, json!({"success": false, "message": "unsupported"})));
    };
    let response = send_json(method, &url, Some(access_token), body)
        .await
        .map_err(|_| ())?;
    Ok(key_reply(&call, response.status.as_u16(), &response.value))
}

fn endpoint_type(format: &str) -> Option<&'static str> {
    match format {
        "anthropic" => Some("anthropic"),
        "openai-chat" => Some("openai"),
        "openai-responses" => Some("openai-response"),
        "gemini" => Some("gemini"),
        _ => None,
    }
}

fn peak_periods(value: Option<&Value>) -> Vec<PeakPeriod> {
    value
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .take(MAX_TIERS)
        .filter_map(|row| {
            let start = text(row.get("start"), 5);
            let end = text(row.get("end"), 5);
            (!start.is_empty() && !end.is_empty()).then_some(PeakPeriod {
                start,
                end,
                multiplier: price(row.get("multiplier"))?,
                timezone: text(row.get("timezone"), 64),
                weekdays_only: row.get("weekdaysOnly").and_then(Value::as_bool) == Some(true),
            })
        })
        .collect()
}

fn long_context(value: Option<&Value>) -> Vec<LongContextTier> {
    value
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .take(MAX_TIERS)
        .filter_map(|row| {
            Some(LongContextTier {
                from_tokens: row.get("fromTokens")?.as_u64()?,
                to_tokens: row.get("toTokens").and_then(Value::as_u64),
                input_per_million: price(row.get("price").and_then(|p| p.get("input"))),
                output_per_million: price(row.get("price").and_then(|p| p.get("output"))),
            })
        })
        .collect()
}

struct Listed {
    official: Option<Value>,
    endpoints: Vec<String>,
    endpoints_known: bool,
    groups: Vec<BillingGroup>,
}

/// Every model the account can use, with one billing group per plan that
/// carries it. A plan's `ratio` is its price against the official price, which
/// is what the card shows as a discount; without an official price it is
/// unknown, and the plan's own prices still stand.
pub(super) fn models(payload: Option<&Value>) -> Option<Vec<AccountModel>> {
    let currency = currency(payload)?;
    let plans = data(payload)?.get("plans")?.as_array()?;
    let mut listed: BTreeMap<String, Listed> = BTreeMap::new();
    for plan in plans.iter().take(MAX_PLANS) {
        let id = text(plan.get("id"), 64);
        if id.is_empty() {
            continue;
        }
        let name = text(plan.get("name"), 80);
        let label = if name.is_empty() { id.clone() } else { name };
        let formats = plan.get("formats").and_then(Value::as_array);
        let endpoints: Vec<String> = formats
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .filter_map(endpoint_type)
            .map(str::to_owned)
            .collect();
        let plan_peak = peak_periods(plan.get("peak"));
        let rows = plan.get("models").and_then(Value::as_array);
        for row in rows.into_iter().flatten().take(MAX_MODELS_PER_PLAN) {
            let model_id = text(row.get("id"), 200);
            if model_id.is_empty() {
                continue;
            }
            let own = row.get("price");
            let official = row.get("official");
            let field = |from: Option<&Value>, key: &str| price(from.and_then(|p| p.get(key)));
            let input = field(own, "input");
            let ratio = input
                .zip(field(official, "input"))
                .or_else(|| field(own, "perRequest").zip(field(official, "perRequest")))
                .filter(|(_, reference)| *reference > 0.0)
                .map(|(charged, reference)| charged / reference);
            let model_peak = peak_periods(row.get("peak"));
            let entry = listed.entry(model_id).or_insert_with(|| Listed {
                official: None,
                endpoints: Vec::new(),
                endpoints_known: false,
                groups: Vec::new(),
            });
            if entry.official.is_none() {
                entry.official = official.cloned();
            }
            entry.endpoints_known |= formats.is_some_and(|list| !list.is_empty());
            for endpoint in &endpoints {
                if !entry.endpoints.contains(endpoint) {
                    entry.endpoints.push(endpoint.clone());
                }
            }
            entry.groups.push(BillingGroup {
                id: id.clone(),
                description: text(plan.get("description"), 240),
                ratio,
                supported_endpoint_types: endpoints.clone(),
                plan: Some(PlanPricing {
                    label: label.clone(),
                    currency,
                    input_per_million: input,
                    output_per_million: field(own, "output"),
                    cache_read_per_million: field(own, "cacheRead"),
                    cache_write_per_million: field(own, "cacheWrite"),
                    per_request: field(own, "perRequest"),
                    app_only: plan.get("appOnly").and_then(Value::as_bool) == Some(true),
                    subscription: plan.get("subscription").and_then(Value::as_bool) == Some(true),
                    peak: if model_peak.is_empty() {
                        plan_peak.clone()
                    } else {
                        model_peak
                    },
                    long_context: long_context(row.get("longContext")),
                }),
            });
        }
    }
    Some(
        listed
            .into_iter()
            .map(|(id, entry)| {
                let official = entry.official.as_ref();
                let field = |key: &str| price(official.and_then(|p| p.get(key)));
                let (official_input, official_output) = (field("input"), field("output"));
                let request = field("perRequest");
                let first = entry.groups.first().and_then(|group| group.plan.as_ref());
                let per_request = official_input.is_none()
                    && (request.is_some() || first.is_some_and(|plan| plan.per_request.is_some()));
                let shown = |value: Option<f64>| value.map(decimal).unwrap_or_default();
                AccountModel {
                    id,
                    description: String::new(),
                    billing_mode: if per_request { "per_request" } else { "ratio" },
                    supported_endpoint_types: entry.endpoints_known.then_some(entry.endpoints),
                    pricing_available: official_input.is_some() && official_output.is_some(),
                    official_input_cny_per_million: shown(official_input),
                    official_output_cny_per_million: shown(official_output),
                    actual_input_cny_per_million: shown(first.and_then(|p| p.input_per_million)),
                    actual_output_cny_per_million: shown(first.and_then(|p| p.output_per_million)),
                    billing: Some(ModelBilling {
                        groups: entry.groups,
                        base_input_usd: official_input,
                        base_output_usd: official_output,
                        cache_read_usd: field("cacheRead"),
                        cache_write_usd: field("cacheWrite"),
                        request_usd: request,
                        expression: String::new(),
                    }),
                }
            })
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::super::{
        account_money, neutral_bootstrap, parse_account, parse_usage, usage_log_report,
        wallet_page_is_allowed, StationDialect,
    };
    use super::*;

    fn bootstrap() -> Value {
        json!({
            "schema_version": 2,
            "service": "desktop-connect",
            "contract_id": "desktop-connect-v1",
            "minimum_client_version": "0.0.0",
            "device_authorization_available": false,
            "oauth_loopback_pkce_available": false,
            "account_read": true,
            "usage_read": true,
            "models_read": true,
            "pricing_read": true,
            "tool_keys_manage": true,
            "session_refresh_path": "/api/desktop/v2/sessions/refresh",
            "session_logout_path": "/api/desktop/v2/sessions/current",
            "account_path": "/api/desktop/v2/account",
            "plans_path": "/api/desktop/v2/plans",
            "keys_path": "/api/desktop/v2/keys",
            "usage_summary_path": "/api/desktop/v2/usage/summary",
            "usage_records_path": "/api/desktop/v2/usage/records",
            "announcements_path": "/api/desktop/v2/notices",
            "app_tokens_path": "/api/desktop/v2/app-tokens",
            "app_token_header": "X-Desktop-App-Token",
            "wallet_url": "https://relay.example.com/topup"
        })
    }

    #[test]
    fn a_station_on_the_neutral_contract_names_its_own_paths() {
        let read = neutral_bootstrap(bootstrap(), "https://relay.example.com").unwrap();
        let StationDialect::Neutral(paths) = read.dialect else {
            panic!("expected the neutral dialect");
        };
        assert_eq!(paths.plans, "/api/desktop/v2/plans");
        assert_eq!(paths.app_tokens, "/api/desktop/v2/app-tokens");
        assert_eq!(paths.app_token_header, "X-Desktop-App-Token");
        assert_eq!(read.announcements_path, "/api/desktop/v2/notices");
    }

    #[test]
    fn the_top_up_page_a_neutral_station_names_is_the_one_that_opens() {
        let origin = "https://relay.example.com";
        let read = neutral_bootstrap(bootstrap(), origin).unwrap();
        assert!(wallet_page_is_allowed(
            &read.dialect,
            &read.wallet_url,
            origin
        ));
        assert!(!wallet_page_is_allowed(
            &read.dialect,
            "https://evil.example/topup",
            origin
        ));
        // The original contract still fixes the path.
        assert!(!wallet_page_is_allowed(
            &StationDialect::Legacy,
            &read.wallet_url,
            origin
        ));
    }

    #[test]
    fn a_neutral_bootstrap_cannot_point_the_client_off_the_station() {
        let rejected = |key: &str, value: Value| {
            let mut data = bootstrap();
            data[key] = value;
            neutral_bootstrap(data, "https://relay.example.com").err()
        };
        assert_eq!(
            rejected("account_path", json!("https://evil.example/a")),
            Some("paths")
        );
        assert_eq!(
            rejected("keys_path", json!("//evil.example/k")),
            Some("paths")
        );
        assert_eq!(
            rejected("wallet_url", json!("https://evil.example/topup")),
            Some("wallet_url")
        );
        assert_eq!(
            rejected("contract_id", json!("desktop-connect-v9")),
            Some("contract_id")
        );
        assert_eq!(
            rejected("minimum_client_version", json!("999.0.0")),
            Some("client_too_old")
        );
        assert_eq!(
            rejected("tool_keys_manage", json!(false)),
            Some("capabilities")
        );
    }

    #[test]
    fn a_header_the_station_may_not_claim_turns_client_only_prices_off() {
        for header in ["Authorization", "x-api-key", "X-Bad Header", ""] {
            let mut data = bootstrap();
            data["app_token_header"] = json!(header);
            let read = neutral_bootstrap(data, "https://relay.example.com").unwrap();
            let StationDialect::Neutral(paths) = read.dialect else {
                panic!("expected the neutral dialect");
            };
            assert_eq!(paths.app_token_header, "", "{header}");
            assert_eq!(paths.app_tokens, "", "{header}");
        }
    }

    #[test]
    fn the_balance_and_the_ledger_come_out_in_the_station_currency() {
        let account = json!({"success": true, "data": {
            "username": "ada", "displayName": "Ada", "currency": "CNY", "balance": 12.5
        }});
        assert_eq!(currency(Some(&account)), Some("CNY"));
        let status = status_shape("CNY");
        let summary = parse_account(account_shape(Some(&account)).as_ref(), Some(&status));
        assert!(summary.available);
        assert_eq!(summary.display_name, "Ada");
        let money = account_money(Some(&status), &summary.balance_quota, &summary.used_quota);
        assert_eq!(money.currency, "CNY");
        assert_eq!(money.balance_amount.parse::<f64>().unwrap(), 12.5);

        let usage = json!({"success": true, "data": {
            "currency": "CNY", "windowDays": 30, "consumed": 1.25, "requests": 8, "tokens": 900
        }});
        assert!(parse_usage(usage_shape(Some(&usage)).as_ref()).available);

        let now = 1_800_000_000_000_u64;
        let records = json!({"success": true, "data": {"page": 1, "records": [
            {"id": "1", "atEpochMs": now - 60_000, "model": "m", "keyName": "k",
             "inputTokens": 100, "outputTokens": 20, "cacheTokens": 5, "amount": 0.75},
            {"id": "2", "model": "no-time", "amount": 1}
        ]}});
        let page = records_shape(Some(&records)).unwrap();
        assert_eq!(page["data"]["items"].as_array().unwrap().len(), 1);
        let report = usage_log_report(Some(&status), &[page], now);
        assert_ne!(report.status, "unavailable");
    }

    #[test]
    fn an_unknown_currency_or_a_failed_envelope_is_not_an_account() {
        assert_eq!(
            currency(Some(&json!({"success": true, "data": {"currency": "EUR"}}))),
            None
        );
        assert_eq!(
            currency(Some(&json!({"success": false, "code": "x"}))),
            None
        );
        assert!(
            account_shape(Some(&json!({"success": true, "data": {"currency": "USD"}}))).is_none()
        );
    }

    #[test]
    fn each_plan_becomes_a_billing_group_with_its_own_prices() {
        let plans = json!({"success": true, "data": {"currency": "CNY", "plans": [
            {"id": "7", "name": "Standard", "formats": ["anthropic"], "subscription": false,
             "appOnly": false, "models": [
                {"id": "claude", "price": {"input": 10.0, "output": 50.0, "cacheRead": 1.0},
                 "official": {"input": 20.0, "output": 100.0},
                 "longContext": [{"fromTokens": 200000, "price": {"input": 20.0, "output": 75.0}}]}
             ]},
            {"id": "9", "name": "Garden", "formats": ["anthropic", "openai-chat"], "subscription": true,
             "appOnly": true, "peak": [{"start": "09:00", "end": "18:00", "multiplier": 1.5}],
             "models": [
                {"id": "claude", "price": {"input": 5.0, "output": 25.0},
                 "official": {"input": 20.0, "output": 100.0}},
                {"id": "image", "price": {"perRequest": 0.2}}
             ]}
        ]}});
        let models = models(Some(&plans)).unwrap();
        assert_eq!(models.len(), 2);
        let claude = models.iter().find(|m| m.id == "claude").unwrap();
        assert_eq!(claude.billing_mode, "ratio");
        assert!(claude.pricing_available);
        assert_eq!(claude.official_input_cny_per_million, "20");
        assert_eq!(
            claude.supported_endpoint_types,
            Some(vec!["anthropic".to_owned(), "openai".to_owned()])
        );
        let groups = &claude.billing.as_ref().unwrap().groups;
        assert_eq!(groups.len(), 2);
        assert_eq!(groups[0].ratio, Some(0.5));
        assert_eq!(groups[1].ratio, Some(0.25));
        let standard = groups[0].plan.as_ref().unwrap();
        assert_eq!(standard.label, "Standard");
        assert_eq!(standard.currency, "CNY");
        assert_eq!(standard.cache_read_per_million, Some(1.0));
        assert_eq!(standard.long_context[0].output_per_million, Some(75.0));
        let garden = groups[1].plan.as_ref().unwrap();
        assert!(garden.app_only && garden.subscription);
        assert_eq!(garden.peak[0].multiplier, 1.5);

        let image = models.iter().find(|m| m.id == "image").unwrap();
        assert_eq!(image.billing_mode, "per_request");
        assert!(!image.pricing_available);
        let only = &image.billing.as_ref().unwrap().groups[0];
        assert_eq!(only.ratio, None);
        assert_eq!(only.plan.as_ref().unwrap().per_request, Some(0.2));
    }

    const STATION: &str = "https://station.example";
    const KEYS: &str = "/api/desktop/v2/keys";

    fn call(
        method: Method,
        path: &str,
        body: Option<Value>,
    ) -> Option<(KeyCall, Method, String, Option<Value>)> {
        key_call(
            &method,
            &format!("{STATION}{path}"),
            body.as_ref(),
            STATION,
            KEYS,
        )
    }

    #[test]
    fn key_operations_are_re_addressed_to_the_station_paths() {
        let (kind, method, url, body) =
            call(Method::GET, "/api/token/?p=2&size=100", None).unwrap();
        assert_eq!(kind, KeyCall::List { page: 2 });
        assert_eq!(
            (method, url.as_str(), body),
            (
                Method::GET,
                "https://station.example/api/desktop/v2/keys",
                None
            )
        );

        let created = call(
            Method::POST,
            "/api/token/",
            Some(json!({"name": "App-1", "group": "7", "model_limits": "a,b", "unlimited_quota": true})),
        )
        .unwrap();
        assert_eq!(created.0, KeyCall::Create);
        // Only what the neutral contract defines leaves the machine.
        assert_eq!(created.3, Some(json!({"name": "App-1", "planId": "7"})));

        let revealed = call(Method::POST, "/api/token/42/key", None).unwrap();
        assert_eq!(
            (revealed.0, revealed.2.as_str()),
            (
                KeyCall::Reveal,
                "https://station.example/api/desktop/v2/keys/42/reveal"
            )
        );

        let deleted = call(Method::DELETE, "/api/token/42", None).unwrap();
        assert_eq!(
            (deleted.0, deleted.1, deleted.2.as_str()),
            (
                KeyCall::Delete,
                Method::DELETE,
                "https://station.example/api/desktop/v2/keys/42"
            )
        );
    }

    #[test]
    fn an_operation_the_contract_does_not_define_is_not_sent_anywhere() {
        assert!(call(Method::PUT, "/api/token/", Some(json!({"id": 1}))).is_none());
        assert!(call(Method::GET, "/api/token/search?keyword=x", None).is_none());
        assert!(call(Method::DELETE, "/api/token/../account", None).is_none());
        assert!(call(Method::POST, "/api/token/", Some(json!({"name": "App-1"}))).is_none());
    }

    #[test]
    fn the_key_list_reads_as_one_page_scoped_by_plan() {
        let station = json!({"success": true, "data": {"keys": [
            {"id": "42", "name": "App-1", "planId": "7", "status": "active", "createdAtEpochMs": 1},
            {"id": "43", "name": "App-2", "planId": "9", "status": "disabled", "createdAtEpochMs": 2},
            {"id": "not-a-number", "name": "x", "planId": "9", "status": "active"}
        ]}});
        let (status, first) = key_reply(&KeyCall::List { page: 1 }, 200, &station);
        assert_eq!(status, 200);
        assert_eq!(first["success"], true);
        assert_eq!(
            first["data"]["items"],
            json!([
                {"id": 42, "name": "App-1", "group": "7", "status": 1, "expired_time": -1, "plan_scoped": true},
                {"id": 43, "name": "App-2", "group": "9", "status": 2, "expired_time": -1, "plan_scoped": true}
            ])
        );
        // The station returns everything at once, so a second page is empty
        // and the caller's paging loop stops.
        let (_, second) = key_reply(&KeyCall::List { page: 2 }, 200, &station);
        assert_eq!(second["data"]["items"], json!([]));
    }

    #[test]
    fn a_created_or_revealed_key_comes_back_where_callers_look_for_it() {
        let created = json!({"success": true, "data": {"id": "42", "name": "App-1", "planId": "7", "status": "active", "key": "sk-test"}});
        let (_, value) = key_reply(&KeyCall::Create, 200, &created);
        assert_eq!(
            value["data"],
            json!({"id": 42, "name": "App-1", "key": "sk-test"})
        );

        let (_, value) = key_reply(
            &KeyCall::Reveal,
            200,
            &json!({"success": true, "data": {"key": "sk-test"}}),
        );
        assert_eq!(value["data"]["key"], "sk-test");

        // A success with no key in it is not a key.
        let (status, value) =
            key_reply(&KeyCall::Reveal, 200, &json!({"success": true, "data": {}}));
        assert_eq!((status, value["success"].as_bool()), (502, Some(false)));
    }

    #[test]
    fn a_refused_plan_is_not_mistaken_for_a_lost_session() {
        let refused = json!({"success": false, "code": "plan_not_allowed"});
        let (status, value) = key_reply(&KeyCall::Create, 403, &refused);
        assert_eq!((status, value["success"].as_bool()), (400, Some(false)));

        let expired = json!({"success": false, "code": "session_required"});
        assert_eq!(key_reply(&KeyCall::Create, 401, &expired).0, 401);
        assert_eq!(
            key_reply(
                &KeyCall::Create,
                409,
                &json!({"success": false, "code": "key_limit_reached"})
            )
            .0,
            409
        );
        // success:false under a 200 never reads as success.
        assert_eq!(
            key_reply(
                &KeyCall::Delete,
                200,
                &json!({"success": false, "code": "x"})
            )
            .0,
            502
        );
    }

    #[test]
    fn the_model_list_an_activation_checks_comes_from_the_plans() {
        let plans = json!({"success": true, "data": {"currency": "CNY", "plans": [
            {"id": "7", "models": [{"id": "b"}, {"id": "a"}]},
            {"id": "9", "models": [{"id": "a"}]}
        ]}});
        let (status, models) = catalogue_reply("/api/user/models", 200, &plans);
        assert_eq!((status, &models["data"]), (200, &json!(["a", "b"])));
        assert_eq!(
            catalogue_reply("/api/pricing", 200, &plans).1["success"],
            true
        );
        // An expired session stays an expired session.
        let expired = json!({"success": false, "code": "session_required"});
        assert_eq!(catalogue_reply("/api/user/models", 401, &expired).0, 401);
        assert_eq!(
            catalogue_reply("/api/pricing", 200, &json!({"success": true, "data": {}})).0,
            502
        );
    }
}

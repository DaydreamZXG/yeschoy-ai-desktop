//! First-party account display; no billing writes, raw log projection or storage.
// Independent implementation of the mathematical basis documented by NewAPI:
// https://github.com/yeschoy/new-api/blob/63f3dd8fb908d6c55d5df483530beeebbcc740b6/web/src/features/dashboard/components/overview/easy-savings.ts
// These are reference estimates, not an upstream invoice or cash saved.
use serde::Serialize;
use serde_json::{Map, Value};

use crate::request_diagnostics::{RequestObservation, RequestOutcome};

pub(crate) const RECENT_LOGS_PATH: &str = "/api/log/self?p=1&page_size=100&type=2";
const RECORD_LIMIT: usize = 100;
const MAX_METADATA_BYTES: usize = 64 * 1024;
const MAX_SAFE_INTEGER: f64 = 9_007_199_254_740_991.0;
const MAX_AMOUNT: f64 = 1_000_000_000_000.0;

/// PRD 6.6 用量账单：分页拉取的页大小与页数上限（≥5 页）。
pub(crate) const USAGE_LOG_PAGE_SIZE: usize = 100;
pub(crate) const USAGE_MAX_PAGES: usize = 5;
/// 聚合窗口：30 天（PRD 6.6）。
pub(crate) const USAGE_WINDOW_MS: u64 = 30 * 24 * 60 * 60 * 1000;

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AccountMoney {
    pub currency: &'static str,
    pub balance_amount: String,
    pub consumed_amount: String,
    pub display_rate: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RecentSavings {
    pub status: &'static str,
    pub reason_code: &'static str,
    pub official_amount: String,
    pub site_amount: String,
    pub saved_amount: String,
    pub reference_rate: String,
    pub price_rate: String,
    pub record_limit: usize,
    pub scanned_count: usize,
    pub included_count: usize,
    pub excluded_count: usize,
    pub oldest_at_epoch_ms: u64,
    pub newest_at_epoch_ms: u64,
}

impl Default for RecentSavings {
    fn default() -> Self {
        Self::unavailable("logs_unavailable")
    }
}

impl RecentSavings {
    fn unavailable(reason_code: &'static str) -> Self {
        Self {
            status: "unavailable",
            reason_code,
            official_amount: String::new(),
            site_amount: String::new(),
            saved_amount: String::new(),
            reference_rate: String::new(),
            price_rate: String::new(),
            record_limit: RECORD_LIMIT,
            scanned_count: 0,
            included_count: 0,
            excluded_count: 0,
            oldest_at_epoch_ms: 0,
            newest_at_epoch_ms: 0,
        }
    }
}

/// PRD 6.6 用量明细的单条消费记录（`type=2`）。
/// `tool_id` 为空表示该记录不能归因到客户端管理的任何工具（如网页直用）。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct UsageRecord {
    pub tool_id: String,
    pub model_id: String,
    pub observed_at_epoch_ms: u64,
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub cache_tokens: u64,
    /// 展示货币金额（quota 经 quota_per_unit 与汇率换算）；换算参数缺失时为空串。
    pub amount: String,
}

/// PRD 6.6 用量账单投影：30 天窗口内逐条明细（供前端做工具×模型聚合）。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct UsageLogReport {
    pub status: &'static str,
    pub reason_code: &'static str,
    pub record_count: usize,
    pub scanned_count: usize,
    pub window_days: u32,
    /// 达到分页上限仍未覆盖完整窗口（数据被截断）。
    pub truncated: bool,
    pub oldest_at_epoch_ms: u64,
    pub newest_at_epoch_ms: u64,
    pub records: Vec<UsageRecord>,
}

impl Default for UsageLogReport {
    fn default() -> Self {
        Self::unavailable("logs_unavailable")
    }
}

impl UsageLogReport {
    fn unavailable(reason_code: &'static str) -> Self {
        Self {
            status: "unavailable",
            reason_code,
            record_count: 0,
            scanned_count: 0,
            window_days: (USAGE_WINDOW_MS / (24 * 60 * 60 * 1000)) as u32,
            truncated: false,
            oldest_at_epoch_ms: 0,
            newest_at_epoch_ms: 0,
            records: Vec::new(),
        }
    }
}

fn usage_cache_tokens(row: &Value) -> u64 {
    let Some(metadata) = row
        .get("other")
        .and_then(Value::as_str)
        .filter(|text| !text.is_empty() && text.len() <= MAX_METADATA_BYTES)
    else {
        return 0;
    };
    let Ok(parsed) = serde_json::from_str::<Value>(metadata) else {
        return 0;
    };
    parsed
        .get("cache_tokens")
        .and_then(Value::as_u64)
        .unwrap_or(0)
}

/// 汇总多页 `/api/log/self` 消费日志为 30 天窗口内的明细报告。
/// `now_epoch_ms` 用于窗口裁剪；换算参数缺失时金额置空但明细仍返回。
pub(crate) fn usage_log_report(
    status: Option<&Value>,
    log_pages: &[Value],
    now_epoch_ms: u64,
) -> UsageLogReport {
    let rates = display_rates(status);
    let mut result = UsageLogReport {
        status: "available",
        reason_code: "none",
        ..UsageLogReport::unavailable("logs_unavailable")
    };
    let cutoff = now_epoch_ms.saturating_sub(USAGE_WINDOW_MS);
    let mut oldest: Option<u64> = None;
    let mut newest: Option<u64> = None;
    let mut last_page_full = false;
    for page in log_pages {
        let Some(items) = page
            .get("data")
            .and_then(|data| data.get("items"))
            .and_then(Value::as_array)
        else {
            continue;
        };
        last_page_full = items.len() >= USAGE_LOG_PAGE_SIZE;
        for row in items {
            if row.get("type").and_then(Value::as_u64) != Some(2) {
                continue;
            }
            result.scanned_count += 1;
            let Some(created_at) = row
                .get("created_at")
                .and_then(Value::as_u64)
                .filter(|value| *value > 0 && *value <= 8_640_000_000_000)
                .and_then(|value| value.checked_mul(1000))
            else {
                continue;
            };
            if created_at < cutoff {
                continue;
            }
            let model_id = row
                .get("model_name")
                .and_then(Value::as_str)
                .filter(|value| {
                    !value.is_empty()
                        && value.chars().count() <= 200
                        && !value.chars().any(char::is_control)
                });
            let Some(model_id) = model_id else {
                continue;
            };
            let tool_id = row
                .get("token_name")
                .and_then(Value::as_str)
                .and_then(crate::tool_activation::tool_for_token_name)
                .unwrap_or("");
            let prompt_tokens = row.get("prompt_tokens").and_then(Value::as_u64).unwrap_or(0);
            let completion_tokens = row
                .get("completion_tokens")
                .and_then(Value::as_u64)
                .unwrap_or(0);
            let quota = row.get("quota").and_then(Value::as_i64).unwrap_or(0);
            let amount_text = rates
                .and_then(|(_, unit, rate)| amount(quota as f64 / unit * rate))
                .unwrap_or_default();
            result.records.push(UsageRecord {
                tool_id: tool_id.to_owned(),
                model_id: model_id.to_owned(),
                observed_at_epoch_ms: created_at,
                prompt_tokens,
                completion_tokens,
                cache_tokens: usage_cache_tokens(row),
                amount: amount_text,
            });
            oldest = Some(oldest.map_or(created_at, |value| value.min(created_at)));
            newest = Some(newest.map_or(created_at, |value| value.max(created_at)));
        }
    }
    result.record_count = result.records.len();
    result.oldest_at_epoch_ms = oldest.unwrap_or(0);
    result.newest_at_epoch_ms = newest.unwrap_or(0);
    result.truncated = last_page_full && log_pages.len() >= USAGE_MAX_PAGES;
    if result.records.is_empty() {
        // 有合法 items 结构（可为空数组）但窗口内无记录 → 空历史；
        // 连 items 结构都没有 → 日志不可用。
        if log_pages.is_empty() || !log_pages.iter().any(page_has_items) {
            return UsageLogReport::unavailable("logs_unavailable");
        }
        result.status = "empty";
        result.reason_code = "no_history";
    }
    result
}

fn page_has_items(page: &Value) -> bool {
    page.get("data")
        .and_then(|data| data.get("items"))
        .and_then(Value::as_array)
        .is_some()
}

fn data(value: Option<&Value>) -> Option<&Map<String, Value>> {
    let value = value?;
    (value.get("success")?.as_bool()? && value.get("data")?.is_object())
        .then(|| value["data"].as_object())
        .flatten()
}

fn signed_number(value: Option<&Value>) -> Option<f64> {
    let value = value?;
    let number = value.as_f64().or_else(|| {
        let text = value.as_str()?.trim();
        (!text.is_empty() && text.len() <= 40)
            .then(|| text.parse::<f64>().ok())
            .flatten()
    })?;
    (number.is_finite() && (-MAX_SAFE_INTEGER..=MAX_SAFE_INTEGER).contains(&number))
        .then_some(number)
}

fn number(value: Option<&Value>) -> Option<f64> {
    signed_number(value).filter(|number| *number >= 0.0)
}

fn positive(value: Option<&Value>) -> Option<f64> {
    number(value).filter(|n| *n > 0.0)
}

fn amount(value: f64) -> Option<String> {
    if !value.is_finite() || value.abs() > MAX_AMOUNT || (value != 0.0 && value.abs() < 1e-15) {
        return None;
    }
    if value == 0.0 {
        return Some("0".into());
    }
    Some(
        format!("{value:.15}")
            .trim_end_matches('0')
            .trim_end_matches('.')
            .to_owned(),
    )
}

/// 余额/金额展示所需的换算参数（PRD：价格以 NewAPI 为准，客户端只换算展示）。
/// 返回 (currency, quota_per_unit, rate)；TOKENS 与自定义货币不冒充美元，返回 None。
fn display_rates(status: Option<&Value>) -> Option<(&'static str, f64, f64)> {
    let settings = data(status)?;
    let unit = positive(settings.get("quota_per_unit"))?;
    match settings.get("quota_display_type")?.as_str()? {
        "CNY" => {
            let rate = positive(settings.get("usd_exchange_rate"))?;
            Some(("CNY", unit, rate))
        }
        "USD" => Some(("USD", unit, 1.0)),
        _ => None,
    }
}

pub(crate) fn account_money(status: Option<&Value>, balance: &str, consumed: &str) -> AccountMoney {
    let calculate = || {
        let (currency, unit, rate) = display_rates(status)?;
        Some(AccountMoney {
            currency,
            balance_amount: amount(
                signed_number(Some(&Value::String(balance.into())))? / unit * rate,
            )?,
            consumed_amount: amount(number(Some(&Value::String(consumed.into())))? / unit * rate)?,
            display_rate: amount(rate)?,
        })
    };
    calculate().unwrap_or_default()
}

fn comparable_row(row: &Value, unit: f64, price: f64, reference: f64) -> Option<(f64, f64)> {
    if row.get("type")?.as_u64()? != 2 {
        return None;
    }
    let metadata = row.get("other")?.as_str()?;
    if metadata.len() > MAX_METADATA_BYTES {
        return None;
    }
    let parsed: Value = serde_json::from_str(metadata).ok()?;
    let billing = parsed.as_object()?;
    if billing
        .get("billing_source")
        .is_some_and(|v| v.as_str() != Some("wallet"))
        || billing
            .get("violation_fee")
            .is_some_and(|v| v != &Value::Bool(false))
        || billing.contains_key("subscription_id")
        || billing.contains_key("subscription_plan_id")
        || [
            "tool_surcharges",
            "audio_input_seperate_price",
            "web_search",
            "web_search_call_count",
            "file_search",
            "file_search_call_count",
            "tool_calls",
            "tool_cost",
            "image_generation_call_count",
        ]
        .iter()
        .any(|key| billing.contains_key(*key))
    {
        return None;
    }
    let ratio = positive(billing.get("user_group_ratio"))
        .or_else(|| positive(billing.get("group_ratio")))?;
    let charged = number(billing.get("fee_quota"))
        .or_else(|| number(row.get("quota")))?
        .max(0.0);
    if charged == 0.0 {
        return None;
    }
    let credits = charged / unit;
    let site = credits * price;
    let official = credits / ratio * reference;
    amount(site)?;
    amount(official)?;
    Some((official, site))
}

pub(crate) fn recent_savings(status: Option<&Value>, logs: Option<&Value>) -> RecentSavings {
    let settings = data(status);
    let rates = settings.and_then(|s| {
        Some((
            positive(s.get("quota_per_unit"))?,
            positive(s.get("price"))?,
            positive(s.get("usd_exchange_rate"))?,
        ))
    });
    let Some((unit, price, reference)) = rates else {
        return RecentSavings::unavailable("settings_unavailable");
    };
    let Some(logs) = logs else {
        return RecentSavings::default();
    };
    let Some(page) = data(Some(logs)) else {
        return RecentSavings::unavailable("invalid_logs");
    };
    let Some(items) = page.get("items").and_then(Value::as_array) else {
        return RecentSavings::unavailable("invalid_logs");
    };
    if items.len() > RECORD_LIMIT || page.get("page").and_then(Value::as_u64) != Some(1) {
        return RecentSavings::unavailable("invalid_logs");
    }
    let mut result = RecentSavings {
        scanned_count: items.len(),
        ..RecentSavings::default()
    };
    let (mut official, mut site) = (0.0, 0.0);
    let mut dates = Vec::new();
    for row in items {
        let Some((row_official, row_site)) = comparable_row(row, unit, price, reference) else {
            continue;
        };
        official += row_official;
        site += row_site;
        result.included_count += 1;
        if let Some(date) = row
            .get("created_at")
            .and_then(Value::as_u64)
            .filter(|n| *n > 0 && *n <= 8_640_000_000_000)
            .and_then(|n| n.checked_mul(1000))
        {
            dates.push(date);
        }
    }
    result.excluded_count = result.scanned_count - result.included_count;
    if result.included_count == 0 {
        result.status = if items.is_empty() {
            "empty"
        } else {
            "no_comparable_records"
        };
        result.reason_code = if items.is_empty() {
            "no_history"
        } else {
            "missing_basis"
        };
        return result;
    }
    let (
        Some(official_amount),
        Some(site_amount),
        Some(saved_amount),
        Some(reference_rate),
        Some(price_rate),
    ) = (
        amount(official),
        amount(site),
        amount(official - site),
        amount(reference),
        amount(price),
    )
    else {
        return RecentSavings::unavailable("invalid_logs");
    };
    result.status = "available";
    result.reason_code = "none";
    result.official_amount = official_amount;
    result.site_amount = site_amount;
    result.saved_amount = saved_amount;
    result.reference_rate = reference_rate;
    result.price_rate = price_rate;
    if dates.len() == result.included_count {
        result.oldest_at_epoch_ms = *dates.iter().min().unwrap_or(&0);
        result.newest_at_epoch_ms = *dates.iter().max().unwrap_or(&0);
    }
    result
}

/// 每个工具最近一次经中转完成的请求。
///
/// 数据来自 `/api/log/self` 的消费记录（`type=2`），按客户端为该工具创建的
/// token 名称归因，每个工具只保留最新一条。日志行不含线路，线路由本次读取
/// 使用的 origin 决定。支持多页合并（分页拉取后覆盖更长历史）。
pub(crate) fn recent_requests_from_pages(
    pages: &[Value],
    line_id: &'static str,
) -> Vec<(String, RequestObservation)> {
    let mut latest: Vec<(String, RequestObservation)> = Vec::new();
    for page in pages {
        let Some(items) = page
            .get("data")
            .and_then(|data| data.get("items"))
            .and_then(Value::as_array)
        else {
            continue;
        };
        for row in items.iter().take(RECORD_LIMIT) {
            if row.get("type").and_then(Value::as_u64) != Some(2) {
                continue;
            }
            let Some(tool) = row
                .get("token_name")
                .and_then(Value::as_str)
                .and_then(crate::tool_activation::tool_for_token_name)
            else {
                continue;
            };
            let Some(model_id) =
                row.get("model_name")
                    .and_then(Value::as_str)
                    .filter(|value| {
                        !value.is_empty()
                            && value.chars().count() <= 200
                            && !value.chars().any(char::is_control)
                    })
            else {
                continue;
            };
            let Some(observed_at_epoch_ms) = row
                .get("created_at")
                .and_then(Value::as_u64)
                .filter(|value| *value > 0 && *value <= 8_640_000_000_000)
                .and_then(|value| value.checked_mul(1000))
            else {
                continue;
            };
            let observation = RequestObservation {
                model_id: model_id.to_owned(),
                billing_group: String::new(),
                line_id: line_id.to_owned(),
                outcome: RequestOutcome::Ok,
                http_status: 0,
                observed_at_epoch_ms,
            };
            match latest.iter_mut().find(|(id, _)| id == tool) {
                Some((_, current)) if current.observed_at_epoch_ms >= observed_at_epoch_ms => {}
                Some((_, current)) => *current = observation,
                None => latest.push((tool.to_owned(), observation)),
            }
        }
    }
    latest
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn settings() -> Value {
        json!({"success":true,"data":{"quota_display_type":"CNY","quota_per_unit":500000,"usd_exchange_rate":7,"price":2,"custom_currency_symbol":"USD"}})
    }
    fn row(quota: u64, other: Value) -> Value {
        json!({"type":2,"quota":quota,"created_at":1788598800,"other":other.to_string(),"token_name":"synthetic-private"})
    }
    fn logs(items: Vec<Value>) -> Value {
        json!({"success":true,"data":{"page":1,"page_size":100,"items":items}})
    }

    #[test]
    fn money_tracks_display_currency_without_a_usd_fallback() {
        let mut s = settings();
        let money = account_money(Some(&s), "500000", "1250000");
        assert_eq!(
            (
                money.currency,
                money.balance_amount.as_str(),
                money.consumed_amount.as_str()
            ),
            ("CNY", "7", "17.5")
        );
        s["data"]["usd_exchange_rate"] = json!(1);
        assert_eq!(account_money(Some(&s), "500000", "0").balance_amount, "1");
        s["data"]["quota_display_type"] = json!("USD");
        assert_eq!(account_money(Some(&s), "500000", "0").currency, "USD");
        s["data"]["quota_display_type"] = json!("CUSTOM");
        assert_eq!(account_money(Some(&s), "500000", "0").currency, "");
        assert_eq!(account_money(None, "500000", "0").balance_amount, "");
        assert_eq!(account_money(Some(&settings()), "", "0").currency, "");
        assert_eq!(
            account_money(Some(&settings()), "9007199254740992", "0").currency,
            ""
        );
    }

    #[test]
    fn money_preserves_a_negative_account_balance() {
        let money = account_money(Some(&settings()), "-125000", "120000");
        assert_eq!(money.currency, "CNY");
        assert_eq!(money.balance_amount, "-1.75");
        assert_eq!(money.consumed_amount, "1.68");
    }

    #[test]
    fn website_savings_matches_recorded_ratios_and_current_rates() {
        let s = settings();
        let l = logs(vec![
            row(500000, json!({"group_ratio":0.5})),
            row(
                999999,
                json!({"group_ratio":0.7,"user_group_ratio":"0.25","fee_quota":250000,"cache_tokens":8000000}),
            ),
        ]);
        let result = recent_savings(Some(&s), Some(&l));
        assert_eq!(
            (
                result.official_amount.as_str(),
                result.site_amount.as_str(),
                result.saved_amount.as_str()
            ),
            ("28", "3", "25")
        );
        assert_eq!((result.included_count, result.excluded_count), (2, 0));
        let serialized = serde_json::to_string(&result).unwrap();
        assert!(!serialized.contains("synthetic-private"));
        assert!(!serialized.contains("group_ratio"));
        let mut expensive = s.clone();
        expensive["data"]["price"] = json!(20);
        assert_eq!(
            recent_savings(Some(&expensive), Some(&l)).saved_amount,
            "-2"
        );
        let equal = logs(vec![row(500000, json!({"group_ratio":3.5}))]);
        assert_eq!(recent_savings(Some(&s), Some(&equal)).saved_amount, "0");
    }

    #[test]
    fn savings_excludes_non_comparable_charges_without_faking_zero() {
        let l = logs(vec![
            row(500000, json!({})),
            row(500000, json!({"group_ratio":0})),
            row(
                500000,
                json!({"group_ratio":0.5,"billing_source":"subscription"}),
            ),
            row(
                500000,
                json!({"group_ratio":0.5,"violation_fee":true,"fee_quota":500000}),
            ),
            row(500000, json!({"group_ratio":0.5,"fee_quota":0})),
            row(500000, json!({"group_ratio":0.5,"web_search_call_count":1})),
            row(
                2500,
                json!({"group_ratio":0.5,"billing_source":"wallet","tool_surcharges":[{"name":"web_search","count":1,"price":10}]}),
            ),
            row(
                500000,
                json!({"group_ratio":0.5,"audio_input_seperate_price":true,"audio_input_token_count":10,"audio_input_price":1}),
            ),
        ]);
        let result = recent_savings(Some(&settings()), Some(&l));
        assert_eq!(result.status, "no_comparable_records");
        assert_eq!(result.saved_amount, "");
        assert_eq!(result.excluded_count, 8);
        assert_eq!(
            recent_savings(Some(&settings()), Some(&logs(vec![]))).status,
            "empty"
        );
        assert_eq!(
            recent_savings(Some(&settings()), None).status,
            "unavailable"
        );
    }

    #[test]
    fn savings_rejects_invalid_envelopes_and_bounds_recent_window() {
        let s = settings();
        let valid = row(1, json!({"group_ratio":0.16}));
        assert_eq!(
            recent_savings(Some(&s), Some(&logs(vec![valid.clone(); 100]))).included_count,
            100
        );
        assert_eq!(
            recent_savings(Some(&s), Some(&logs(vec![valid.clone(); 101]))).status,
            "unavailable"
        );
        let mut malformed = valid.clone();
        malformed["other"] = json!("not json");
        let mut oversized = valid.clone();
        oversized["other"] = json!(" ".repeat(MAX_METADATA_BYTES + 1));
        let mut overflow = valid.clone();
        overflow["quota"] = json!(u64::MAX);
        assert_eq!(
            recent_savings(Some(&s), Some(&logs(vec![malformed, oversized, overflow])))
                .included_count,
            0
        );
        let mut invalid = s.clone();
        invalid["data"]["price"] = json!(0);
        assert_eq!(
            recent_savings(Some(&invalid), Some(&logs(vec![valid]))).reason_code,
            "settings_unavailable"
        );
        let bad = json!({"success":false,"data":{"page":1,"items":[]}});
        assert_eq!(
            recent_savings(Some(&s), Some(&bad)).reason_code,
            "invalid_logs"
        );
        assert!(!RECENT_LOGS_PATH.contains("user_id"));
        assert!(!RECENT_LOGS_PATH.starts_with("/api/log/self/"));
    }

    #[test]
    fn recent_requests_attribute_server_logs_to_tools() {
        let logs = logs(vec![
            json!({"type":2,"model_name":"gpt-5.6-sol","created_at":1788598800,"token_name":"野菜API cx-1a2b3c4d5e6f7890-0123456789abcdef"}),
            json!({"type":2,"model_name":"gpt-5.5","created_at":1788599000,"token_name":"野菜API cx-1a2b3c4d5e6f7890-fedcba9876543210"}),
            json!({"type":2,"model_name":"claude-sonnet-5","created_at":1788598900,"token_name":"野菜API cc-1a2b3c4d5e6f7890-0123456789abcdef"}),
            json!({"type":1,"model_name":"ignored","created_at":1788599100,"token_name":"野菜API cx-1a2b3c4d5e6f7890-0123456789abcdef"}),
            json!({"type":2,"model_name":"ignored","created_at":1788599200,"token_name":"别人的 token"}),
        ]);
        let found = recent_requests_from_pages(&[logs], "mainland_optimized");
        assert_eq!(found.len(), 2);
        let codex = found
            .iter()
            .find(|(tool, _)| tool == "codex_desktop")
            .expect("codex row");
        assert_eq!(codex.1.model_id, "gpt-5.5");
        assert_eq!(codex.1.line_id, "mainland_optimized");
        assert_eq!(codex.1.outcome, RequestOutcome::Ok);
        assert!(found.iter().any(|(tool, _)| tool == "claude_code"));
        assert!(recent_requests_from_pages(&[], "mainland_optimized").is_empty());
    }

    #[test]
    fn usage_log_report_aggregates_pages_within_window() {
        let now_ms = 1_789_300_000_000u64; // 2026-09-13 附近
        let fresh = (now_ms / 1000) - 60;
        let stale = (now_ms / 1000) - (31 * 24 * 60 * 60);
        let pages = vec![
            json!({"success":true,"data":{"page":1,"page_size":100,"items":[
                {"type":2,"quota":500000,"prompt_tokens":1200,"completion_tokens":300,"created_at":fresh,"model_name":"gpt-6-astra","token_name":"野菜API cx-1a2b3c4d5e6f7890-0123456789abcdef","other":"{\"group_ratio\":0.5,\"cache_tokens\":400}"},
                {"type":2,"quota":250000,"prompt_tokens":100,"completion_tokens":50,"created_at":fresh - 10,"model_name":"claude-sonnet-4-6","token_name":"野菜API cc-1a2b3c4d5e6f7890-0123456789abcdef"},
                {"type":1,"quota":999,"created_at":fresh,"model_name":"ignored","token_name":"野菜API cx-1a2b3c4d5e6f7890-0123456789abcdef"},
                {"type":2,"quota":100,"created_at":stale,"model_name":"too-old","token_name":"野菜API cx-1a2b3c4d5e6f7890-0123456789abcdef"}
            ]}}),
            json!({"success":true,"data":{"page":2,"page_size":100,"items":[
                {"type":2,"quota":500000,"prompt_tokens":10,"completion_tokens":5,"created_at":fresh - 20,"model_name":"gpt-6-astra","token_name":"web-token"}
            ]}}),
        ];
        let report = usage_log_report(Some(&settings()), &pages, now_ms);
        assert_eq!(report.status, "available");
        assert_eq!(report.record_count, 3);
        assert_eq!(report.scanned_count, 4);
        assert!(!report.truncated);
        assert_eq!(report.window_days, 30);
        let astra = report
            .records
            .iter()
            .find(|r| r.tool_id == "codex_desktop" && r.model_id == "gpt-6-astra")
            .expect("attributed astra row");
        assert_eq!(astra.prompt_tokens, 1200);
        assert_eq!(astra.completion_tokens, 300);
        assert_eq!(astra.cache_tokens, 400);
        assert_eq!(astra.amount, "7"); // 500000/500000*7
        let web = report
            .records
            .iter()
            .find(|r| r.tool_id.is_empty())
            .expect("unattributed web row");
        assert_eq!(web.amount, "7");
        let serialized = serde_json::to_string(&report).unwrap();
        assert!(!serialized.contains("web-token"));
        assert!(!serialized.contains("group_ratio"));
    }

    #[test]
    fn usage_log_report_marks_truncation_and_empty_states() {
        let now_ms = 1_789_300_000_000u64;
        let fresh = (now_ms / 1000) - 60;
        let full_page = |n: usize| {
            json!({"success":true,"data":{"page":n,"page_size":100,"items":
                vec![json!({"type":2,"quota":1,"created_at":fresh,"model_name":"m","token_name":"野菜API cx-1a2b3c4d5e6f7890-0123456789abcdef"}); 100]}})
        };
        let mut pages: Vec<Value> = (1..=USAGE_MAX_PAGES).map(full_page).collect();
        let truncated = usage_log_report(Some(&settings()), &pages, now_ms);
        assert!(truncated.truncated);
        assert_eq!(truncated.record_count, 500);

        // 最后一页不满 → 未截断
        pages.pop();
        pages.push(json!({"success":true,"data":{"page":USAGE_MAX_PAGES,"page_size":100,"items":[
            json!({"type":2,"quota":1,"created_at":fresh,"model_name":"m","token_name":"野菜API cx-1a2b3c4d5e6f7890-0123456789abcdef"})
        ]}}));
        let complete = usage_log_report(Some(&settings()), &pages, now_ms);
        assert!(!complete.truncated);

        let empty = usage_log_report(Some(&settings()), &[logs(vec![])], now_ms);
        assert_eq!((empty.status, empty.reason_code), ("empty", "no_history"));

        let unavailable = usage_log_report(Some(&settings()), &[], now_ms);
        assert_eq!(
            (unavailable.status, unavailable.reason_code),
            ("unavailable", "logs_unavailable")
        );

        // 换算参数缺失：明细仍在、金额置空（不猜能力口径的金额）
        let no_rates = usage_log_report(None, &[logs(vec![json!({
            "type":2,"quota":500000,"prompt_tokens":1,"completion_tokens":2,
            "created_at":fresh,"model_name":"m","token_name":"野菜API cx-1a2b3c4d5e6f7890-0123456789abcdef"
        })])], now_ms);
        assert_eq!(no_rates.status, "available");
        assert_eq!(no_rates.records[0].amount, "");
    }
}

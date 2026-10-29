//! Token enrollment policy: list before minting, reuse only an exact
//! group/model/device scope, and retire superseded keys only after local success.
//! Credentials and transport stay outside this module.
use std::sync::OnceLock;

use std::time::{SystemTime, UNIX_EPOCH};

use reqwest::{Method, Url};
use serde_json::{json, Value};

use super::envelope::{data, server_success};
use super::error::ActivationFailure;
use super::request::ToolActivationRequest;
use crate::application::activation::capability::ModelTransport;
use crate::application::ports::station::StationMethod;
use crate::domain::error::AdapterFailure;
use crate::domain::model::ModelBinding;
use crate::infrastructure::secrets::issuance::IssuanceLog;
use crate::shutdown_coordinator;
use crate::tool_activation::SkippedBinding;
use crate::tool_credentials::{ToolCredential, ToolModelRoute};

/// 中转列表接口一页最多给 100 条（`common.GetPageQuery` 封顶），要多也没用。
const TOKEN_PAGE_SIZE: usize = 100;
/// 最多翻这么多页。中转默认每用户 1000 把密钥，20 页绰绰有余；真有账户超过
/// 这个数，后面的就当没有 —— 宁可多铸一把，也不在接入路径上无限翻页。
const TOKEN_PAGE_LIMIT: usize = 20;
pub(crate) fn now_epoch_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u64::MAX as u128) as u64
}

pub(crate) struct TokenLease {
    pub(crate) id: u64,
    pub(crate) key: String,
    pub(crate) created: bool,
    pub(crate) retire_after_commit: Vec<u64>,
}

/// 服务端用量日志里的 token 名称反查工具。客户端为每个工具创建独立 token，
/// 名称形如 `野菜API cx-<分组哈希>-<随机后缀>`，因此按工具代码前缀归因。
pub(crate) fn tool_for_token_name(name: &str) -> Option<&'static str> {
    [
        "claude_code",
        "claude_desktop",
        "codex_desktop",
        "pi",
        "opencode",
        "dsh_web",
        "workbuddy",
        "dsh_desktop",
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
            log_token_rejection("list", status);
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

/// 中转拒绝一次密钥操作时，只记录操作名和状态码。
///
/// 以前这里还带上中转返回的 `message` —— 那是自由文本，第三方站点可以写任何东西，
/// 包括用户自己的密钥或账户数据；而日志按产品承诺不记密钥、账号和请求体。
/// 排障需要的是「哪一步、什么状态」，`message` 在控制台里本来就完整可读。
fn log_token_rejection(op: &str, status: u16) {
    log::warn!("activation stage=token_api_rejected op={op} status={status}");
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
        "opencode" => "oc",
        "dsh_web" => "ds",
        "workbuddy" => "wb",
        "dsh_desktop" => "dd",
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
        "opencode" => "OpenCode",
        "dsh_web" => "DSH",
        "workbuddy" => "WorkBuddy",
        "dsh_desktop" => "DSHDesktop",
        _ => "Desktop",
    }
}

/// 分组哈希。只是个查找标签，真正比对的永远是中转返回的完整分组名。
///
/// 从 16 位十六进制缩到 8 位，腾出的 8 个字节给了前面的应用名。账户上的分组
/// 是个位数，32 位空间碰撞概率可以忽略；真撞上了也只是两个分组共用一个查找
/// 前缀，而 `owned_token` 随后还会逐个核对完整分组名，不会串。
pub(crate) fn group_hash(group: &str) -> u32 {
    let hash = group.bytes().fold(0xcbf29ce484222325u64, |h, b| {
        (h ^ u64::from(b)).wrapping_mul(0x100000001b3)
    });
    (hash >> 32) as u32 ^ hash as u32
}

/// 0.4.22 及更早铸的名字：`野菜API cx-<16位哈希>`。仍然要认得，否则升级一次
/// 就在账户上留一把没人认领的旧密钥，旁边再铸一把新的。
pub(crate) fn legacy_token_prefix(tool_id: &str, group: &str) -> String {
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
pub(crate) fn token_prefix(tool_id: &str, group: &str) -> String {
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
#[cfg(test)]
pub(crate) fn device_scope_for_tests() -> &'static str {
    device_scope()
}

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
        log_token_rejection("key", status);
        return Err(ActivationFailure::ServerUnavailable);
    }
    data(&value)
        .and_then(Value::as_object)
        .and_then(|value| value.get("key"))
        .and_then(Value::as_str)
        .and_then(normalize_api_key)
        .ok_or(ActivationFailure::ServerUnavailable)
}

/// Build the relay token transport for an authorized session. Used by startup
/// issuance recovery so it talks to the relay through the same validated,
/// origin-bound surface as activation instead of a second HTTP path.
pub(crate) fn station_token_endpoint<'a>(
    provider: &'a crate::adapters::stations::NewApiStationProvider<'a>,
    origin: &'a str,
) -> impl TokenApi + 'a {
    StationTokenApi { origin, provider }
}

pub(crate) async fn acquire_token(
    origin: &str,
    access_token: &str,
    tool_id: &str,
    group: &str,
    model_ids: &[String],
) -> Result<TokenLease, ActivationFailure> {
    let provider = station_api(origin, access_token)?;
    acquire_token_using(
        &StationTokenApi {
            origin,
            provider: &provider,
        },
        origin,
        tool_id,
        group,
        model_ids,
    )
    .await
}

/// 这个错误是整单的，不是某个分组的。
///
/// 分组循环把 `acquire_token` 的每个 `Err` 都记成「这个分组跳过」，其余照常。
/// 对创建被拒、读密钥失败、中转不可用这是对的 —— 换个分组可能就好了。
/// 但 401/403 说的是会话没了，「助手正在退出」说的是不该再动账户：换哪个
/// 分组都一样，接着循环只会对着一个失效的会话再发几轮请求，最后把一句
/// 「请重新登录」藏进 `skipped` 里，界面照常宣布「接入完成」。
pub(crate) fn token_failure_is_global(failure: &ActivationFailure) -> bool {
    matches!(
        failure,
        ActivationFailure::SignedOut
            | ActivationFailure::ConfigurationFailed("assistant_shutting_down")
    )
}

pub(crate) trait TokenApi {
    fn request(
        &self,
        method: Method,
        url: &str,
        body: Option<Value>,
    ) -> impl std::future::Future<Output = Result<(u16, Value), ActivationFailure>> + Send;
}

fn station_api<'a>(
    origin: &'a str,
    access_token: &'a str,
) -> Result<crate::adapters::stations::NewApiStationProvider<'a>, ActivationFailure> {
    crate::adapters::stations::NewApiStationProvider::new(origin, access_token)
        .map_err(|_| ActivationFailure::ServerUnavailable)
}

/// Relay account protocol for token management, reached through the station
/// provider port instead of a second transport implementation. The URL piece
/// used by the error category stays a same-origin `/api/` path.
/// Delete a device's own keys that a previous activation recorded but never
/// finished. Only names the ledger itself wrote for this device are candidates;
/// anything else is left to the normal reuse path. Best-effort: a failure leaves
/// the records in place for the next run and never blocks activation.
pub(crate) async fn release_abandoned_issuances(origin: &str, access_token: &str) {
    let Some(issuance) = issuance_log() else {
        return;
    };
    let ledger = issuance.stale(super::tokens::now_epoch_ms());
    if ledger.is_empty() {
        return;
    }
    let provider = match crate::adapters::stations::NewApiStationProvider::new(origin, access_token)
    {
        Ok(provider) => provider,
        Err(_) => return,
    };
    let api = station_token_endpoint(&provider, origin);
    // The station transport accepts same-origin relative paths only.
    let Ok((status, value)) = api
        .request(Method::GET, "/api/token/?p=1&size=100", None)
        .await
    else {
        return;
    };
    if !(200..300).contains(&status) {
        return;
    }
    let minted: std::collections::BTreeSet<String> = value["data"]["items"]
        .as_array()
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item["name"].as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default();
    let candidates = super::issuance_recovery::provably_abandoned(&ledger, &minted, device_scope());
    let removed = super::issuance_recovery::release(&api, &candidates).await;
    if removed > 0 {
        let names: Vec<String> = candidates
            .iter()
            .map(|record| record.name.clone())
            .collect();
        issuance.forget(&names);
    }
    log::info!(
        "activation stage=abandoned_issuances released={removed} candidates={}",
        candidates.len()
    );
}

/// Where the pre-mint intent record is written. Tests point this at a fixture
/// directory; production resolves it once from the native home directory.
#[cfg(not(test))]
static ISSUANCE_HOME: OnceLock<Option<std::path::PathBuf>> = OnceLock::new();

pub(crate) fn issuance_log() -> Option<IssuanceLog> {
    #[cfg(test)]
    {
        Some(IssuanceLog::at(test_issuance_root()))
    }
    #[cfg(not(test))]
    {
        ISSUANCE_HOME
            .get_or_init(crate::tool_adapters::user_home)
            .clone()
            .map(|home| IssuanceLog::at(&home))
    }
}

/// The intent ledger for tests is a lazily created directory under the temp
/// directory: every token test then exercises the real persistence path without
/// ever touching the user's home directory.
/// Create one relay token for an exact, already-recorded name. Shared by
/// activation and by startup issuance recovery, so recovery proves its path with
/// the same request/response handling instead of a parallel implementation
/// (the only difference is which token api instance it talks to).
pub(crate) async fn create_recorded_token(
    api: &impl TokenApi,
    origin: &str,
    name: &str,
    group: &str,
    model_ids: &[String],
) -> Result<u64, ActivationFailure> {
    if shutdown_coordinator::global().is_shutting_down() {
        return Err(ActivationFailure::ConfigurationFailed(
            "assistant_shutting_down",
        ));
    }
    let (status, value) = api
        .request(
            Method::POST,
            &format!("{origin}/api/token/"),
            Some(token_request(name, group, model_ids)),
        )
        .await?;
    if matches!(status, 401 | 403) {
        return Err(ActivationFailure::SignedOut);
    }
    if !server_success(status, &value) {
        log_token_rejection("create", status);
        return Err(ActivationFailure::ServerUnavailable);
    }
    match created_token_id(&value, name) {
        Some(id) => Ok(id),
        None => {
            // Standard NewAPI returns success with no data/key. Read the exact
            // newly created name back, including its group.
            let tokens = list_tokens(api, origin).await?;
            select_token(&tokens, name, group, model_ids, true)
                .0
                .ok_or(ActivationFailure::ServerUnavailable)
        }
    }
}

#[cfg(test)]
fn test_issuance_root() -> &'static std::path::Path {
    static ROOT: OnceLock<std::path::PathBuf> = OnceLock::new();
    ROOT.get_or_init(|| {
        let root = std::env::temp_dir().join(format!(
            "yeschoy-issuance-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or_default()
        ));
        let _ = std::fs::create_dir_all(&root);
        root
    })
}

#[cfg(test)]
pub(crate) fn issuance_records() -> Vec<crate::infrastructure::secrets::issuance::IssuanceRecord> {
    issuance_log().map(|log| log.records()).unwrap_or_default()
}

pub(crate) struct StationTokenApi<'a> {
    origin: &'a str,
    provider: &'a crate::adapters::stations::NewApiStationProvider<'a>,
}
impl TokenApi for StationTokenApi<'_> {
    async fn request(
        &self,
        method: Method,
        url: &str,
        body: Option<Value>,
    ) -> Result<(u16, Value), ActivationFailure> {
        let op = token_operation(&method, url);
        let path = url
            .strip_prefix(self.origin)
            .filter(|path| path.starts_with("/api/"))
            .ok_or(ActivationFailure::ServerUnavailable)?;
        let method = match method {
            Method::GET => StationMethod::Get,
            Method::POST => StationMethod::Post,
            Method::DELETE => StationMethod::Delete,
            _ => return Err(ActivationFailure::ServerUnavailable),
        };
        crate::application::ports::station::StationProvider::account_json(
            self.provider,
            method,
            path,
            body,
        )
        .await
        .map_err(|_| {
            // Transport failures, timeouts and non-JSON replies (an empty 429)
            // reach here without a status; keep only the step in the log.
            log::warn!("activation stage=token_api_transport_failed op={op}");
            ActivationFailure::ServerUnavailable
        })
    }
}

pub(crate) async fn acquire_token_using(
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

    if shutdown_coordinator::global().is_shutting_down() {
        return Err(ActivationFailure::ConfigurationFailed(
            "assistant_shutting_down",
        ));
    }
    // Persist the intent before the create leaves this process: if the reply is
    // lost, this exact name is the only trustworthy evidence of what may exist.
    // Without a record we do not mint, because it could never be cleaned up.
    let Some(issuance) = issuance_log() else {
        return Err(ActivationFailure::ConfigurationFailed(
            "issuance_record_unavailable",
        ));
    };
    let sequence = issuance
        .next_sequence()
        .map_err(|_| ActivationFailure::ConfigurationFailed("issuance_record_unavailable"))?;
    // Sixteen hex characters as before, now split: this machine's scope, then a
    // nonce. Same shape, so older clients still recognise the name; new clients
    // can tell whose key it is. The nonce is this installation's persisted
    // sequence, so a later run can prove any older nonce was minted by us.
    let name = format!("{prefix}-{}{sequence:08x}", device_scope());
    issuance
        .begin(crate::infrastructure::secrets::issuance::IssuanceRecord {
            name: name.clone(),
            tool_id: tool_id.to_owned(),
            billing_group: group.to_owned(),
            recorded_at_epoch_ms: now_epoch_ms(),
            sequence,
        })
        .map_err(|_| ActivationFailure::ConfigurationFailed("issuance_record_unavailable"))?;
    let id = match create_recorded_token(api, origin, &name, group, model_ids).await {
        Ok(id) => id,
        Err(error) => {
            issuance.finish(&name);
            return Err(error);
        }
    };
    let key = match fetch_token_key(api, origin, id).await {
        Ok(key) => key,
        Err(error) => {
            let _ = api
                .request(Method::DELETE, &format!("{origin}/api/token/{id}"), None)
                .await;
            issuance.finish(&name);
            return Err(error);
        }
    };
    issuance.finish(&name);
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
    if !lease.created {
        return;
    }
    let Ok(provider) = station_api(origin, access_token) else {
        return;
    };
    let _ = crate::application::ports::station::StationProvider::account_json(
        &provider,
        StationMethod::Delete,
        &format!("/api/token/{}", lease.id),
        None,
    )
    .await;
}

pub(crate) async fn delete_created_tokens(
    origin: &str,
    access_token: &str,
    leases: &[(String, TokenLease)],
) {
    futures::future::join_all(
        leases
            .iter()
            .map(|(_, lease)| delete_created_token(origin, access_token, lease)),
    )
    .await;
}

pub(crate) fn retire_superseded_tokens(
    origin: &str,
    access_token: &str,
    leases: &[(String, TokenLease)],
) {
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
        let provider = station_api(&origin, &access_token).ok();
        for id in ids {
            let retired = match &provider {
                Some(provider) => {
                    crate::application::ports::station::StationProvider::account_json(
                        provider,
                        StationMethod::Delete,
                        &format!("/api/token/{id}"),
                        None,
                    )
                    .await
                }
                None => Err(crate::application::ports::station::StationProviderFailure::Transport),
            };
            if matches!(retired, Ok((status, ref value)) if server_success(status, value)) {
                retired_count += 1;
            } else {
                log::warn!("tool_token_cleanup stage=retire_failed");
            }
        }
        log::info!("tool_token_cleanup stage=retire_done retired={retired_count} of={attempted}");
    });
}

pub(crate) async fn revoke_owned_tool_tokens(
    origin: &str,
    access_token: &str,
    tool_id: &str,
    credential: &ToolCredential,
) -> bool {
    if credential.models.is_empty() {
        return true;
    }
    let Ok(provider) = station_api(origin, access_token) else {
        return false;
    };
    let api = StationTokenApi {
        origin,
        provider: &provider,
    };
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
pub(crate) fn surviving_bindings(
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

pub(crate) fn credential_for_models(
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

#[cfg(test)]
#[path = "tokens_tests.rs"]
mod tests;

pub(crate) fn needs_background(tool: &str, credential: &ToolCredential) -> bool {
    // Nothing of ours has to keep running: both read their key from a file.
    if matches!(tool, "workbuddy" | "dsh_desktop") {
        return false;
    }
    credential.has_model_set()
        || matches!(tool, "claude_desktop" | "dsh_web" | "opencode")
        || credential.claude_transport.as_deref() == Some("chat_bridge")
        || credential.codex_transport.as_deref() == Some("chat_bridge")
}

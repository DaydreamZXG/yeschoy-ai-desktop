//! 用户确认过的站点（中转站）来源。
//!
//! 取代过去编译期写死的「只允许 yeschoy.com / ai.yeschoy.io」做法：
//! 任何结构合法的 HTTPS 站点都能接入，但**第一次**必须由用户在桌面助手里
//! 亲手点「信任并继续」。网页拉起的 `yeschoy://connect` 不会再静默完成登录 ——
//! 否则恶意网页可以把用户悄悄接到攻击者的中转上，截走之后所有对话。
use std::collections::{BTreeSet, HashMap};
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::domain::station::StationOrigin;
use crate::station_connect::StationConnectRequest;

/// 参考站点：官方包内置信任，免确认。社区构建同样保留，作为默认示例。
pub(crate) const BUILTIN_TRUSTED: &[&str] = &["https://yeschoy.com", "https://ai.yeschoy.io"];
const MAX_TRUSTED: usize = 64;
const PENDING_TTL: Duration = Duration::from_secs(300);
const FILE_NAME: &str = "trusted-stations.json";

#[derive(Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct TrustFile {
    schema_version: u8,
    origins: BTreeSet<String>,
}

struct State {
    path: Option<PathBuf>,
    origins: BTreeSet<String>,
    pending: HashMap<String, (StationConnectRequest, Instant)>,
}

fn state() -> &'static Mutex<State> {
    static STATE: OnceLock<Mutex<State>> = OnceLock::new();
    STATE.get_or_init(|| {
        Mutex::new(State {
            path: None,
            origins: BTreeSet::new(),
            pending: HashMap::new(),
        })
    })
}

/// 启动时载入持久化列表。文件损坏时当作空列表（只会多问一次，不会放宽）。
pub(crate) fn initialize(data_dir: Option<PathBuf>) {
    let Ok(mut guard) = state().lock() else {
        return;
    };
    let path = data_dir.map(|dir| dir.join(FILE_NAME));
    if let Some(path) = &path {
        if let Ok(text) = std::fs::read_to_string(path) {
            if let Ok(file) = serde_json::from_str::<TrustFile>(&text) {
                if file.schema_version == 1 {
                    guard.origins = file
                        .origins
                        .into_iter()
                        .filter_map(|origin| StationOrigin::parse(&origin).ok())
                        .map(StationOrigin::into_string)
                        .take(MAX_TRUSTED)
                        .collect();
                }
            }
        }
    }
    guard.path = path;
}

/// User authorization is separate from structural identity validation. Callers
/// cannot make a malformed identity trusted, even if a store entry is injected.
pub(crate) fn is_trusted(origin: &str) -> bool {
    let Ok(origin) = StationOrigin::parse(origin) else {
        return false;
    };
    if BUILTIN_TRUSTED.contains(&origin.as_str()) {
        return true;
    }
    state()
        .lock()
        .map(|guard| guard.origins.contains(origin.as_str()))
        .unwrap_or(false)
}

/// Normalize only an explicitly user-typed address. Raw authority validation
/// must precede URL/IDNA normalization, which would otherwise erase userinfo,
/// default ports, encoded hosts, and path traversal. Canonical stored identity
/// still goes through the same pure domain rule as connect and credentials.
fn normalize_trust_input(raw: &str) -> Result<StationOrigin, &'static str> {
    let input = raw.trim();
    let (scheme, rest) = input
        .split_once("://")
        .ok_or("connect_origin_not_acceptable")?;
    let host = rest.strip_suffix('/').unwrap_or(rest);
    if !scheme.eq_ignore_ascii_case("https")
        || host.is_empty()
        || host.chars().any(|c| {
            c.is_control()
                || c.is_whitespace()
                || matches!(c, '@' | ':' | '/' | '?' | '#' | '\\' | '%' | '[' | ']')
        })
    {
        return Err("connect_origin_not_acceptable");
    }
    // URL parsing stays at the input boundary, preserving the existing
    // ASCII-case and Unicode-domain convenience without HTTP dependencies in
    // StationOrigin itself. All fields discarded by origin() were rejected above.
    let url = reqwest::Url::parse(input).map_err(|_| "connect_origin_not_acceptable")?;
    StationOrigin::parse(&url.origin().ascii_serialization())
        .map_err(|_| "connect_origin_not_acceptable")
}

pub(crate) fn trust(origin: &str) -> Result<String, &'static str> {
    let canonical = normalize_trust_input(origin)?.into_string();
    if BUILTIN_TRUSTED.contains(&canonical.as_str()) {
        return Ok(canonical);
    }
    let mut guard = state().lock().map_err(|_| "trust_store_unavailable")?;
    if !guard.origins.contains(&canonical) && guard.origins.len() >= MAX_TRUSTED {
        return Err("trust_store_full");
    }
    guard.origins.insert(canonical.clone());
    persist(&guard);
    Ok(canonical)
}

pub(crate) fn forget(origin: &str) {
    if let Ok(mut guard) = state().lock() {
        guard.origins.remove(origin);
        persist(&guard);
    }
}

pub(crate) fn list() -> Vec<String> {
    state()
        .lock()
        .map(|guard| guard.origins.iter().cloned().collect())
        .unwrap_or_default()
}

fn persist(guard: &State) {
    let Some(path) = &guard.path else { return };
    let file = TrustFile {
        schema_version: 1,
        origins: guard.origins.clone(),
    };
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(text) = serde_json::to_string_pretty(&file) {
        let tmp = path.with_extension("json.tmp");
        if std::fs::write(&tmp, text).is_ok() {
            let _ = std::fs::rename(&tmp, path);
        }
    }
}

/// 暂存一条等待用户确认的连接请求（同一站点只保留最新一条）。
pub(crate) fn hold(request: StationConnectRequest) {
    if let Ok(mut guard) = state().lock() {
        let now = Instant::now();
        guard
            .pending
            .retain(|_, (_, at)| now.duration_since(*at) < PENDING_TTL);
        guard.pending.insert(request.origin.clone(), (request, now));
    }
}

/// 取走一条待确认请求；过期的不返回。
pub(crate) fn take(origin: &str) -> Option<StationConnectRequest> {
    let mut guard = state().lock().ok()?;
    let (request, at) = guard.pending.remove(origin)?;
    (at.elapsed() < PENDING_TTL).then_some(request)
}

/// 冷启动时事件可能早于前端监听，前端挂载后用它补拉。
pub(crate) fn pending_origins() -> Vec<String> {
    state()
        .lock()
        .map(|guard| {
            guard
                .pending
                .iter()
                .filter(|(_, (_, at))| at.elapsed() < PENDING_TTL)
                .map(|(origin, _)| origin.clone())
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_is_trusted_and_unknown_is_not() {
        assert!(is_trusted("https://yeschoy.com"));
        assert!(!is_trusted("https://unknown-station.example"));
    }

    #[test]
    fn trust_canonicalizes_and_rejects_unsafe_origins() {
        assert_eq!(
            trust("https://Relay-Trust.Example/").as_deref(),
            Ok("https://relay-trust.example")
        );
        assert!(is_trusted("https://relay-trust.example"));
        assert!(trust("http://relay.example").is_err());
        assert!(trust("https://1.2.3.4").is_err());
        forget("https://relay-trust.example");
        assert!(!is_trusted("https://relay-trust.example"));
    }

    #[test]
    fn user_typed_trust_input_retains_case_and_idna_convenience() {
        for (input, expected) in [
            (
                " HTTPS://Relay-Trust.Example/ ",
                "https://relay-trust.example",
            ),
            ("https://bücher.example/", "https://xn--bcher-kva.example"),
        ] {
            assert_eq!(normalize_trust_input(input).unwrap().as_str(), expected);
            // Native credentials and deep links must declare canonical identity,
            // not rely on the separate explicitly user-typed input convenience.
            assert!(StationOrigin::parse(input).is_err());
        }
    }

    #[test]
    fn trust_does_not_erase_unsafe_url_fields_or_grant_lookalike_authorization() {
        for origin in [
            "https://user:pass@trust-input.example",
            "https://user@yeschoy.com",
            "https://@yeschoy.com",
            "https://yeschoy.com:443",
            "https://trust-input.example:8443",
            "https://trust-input.example/path",
            "https://trust-input.example/../",
            "https://trust-input.example?x=1",
            "https://trust-input.example#fragment",
            "https://%79eschoy.com",
            "https://trust-input.example\\\\path",
            "https://127.1",
            "https://0x7f000001",
            "https://[::1]",
        ] {
            assert!(trust(origin).is_err(), "accepted {origin:?}");
            assert!(!is_trusted(origin), "trusted {origin:?}");
        }
        assert!(!is_trusted("https://trust-input.example"));
        assert!(!is_trusted("https://yeschoy.com.evil.invalid"));
    }

    #[test]
    fn pending_requests_are_taken_once() {
        let request = StationConnectRequest {
            origin: "https://pending.example".into(),
            code: "c".repeat(40),
        };
        hold(request.clone());
        assert!(pending_origins().contains(&request.origin));
        assert_eq!(take(&request.origin), Some(request.clone()));
        assert_eq!(take(&request.origin), None);
    }
}

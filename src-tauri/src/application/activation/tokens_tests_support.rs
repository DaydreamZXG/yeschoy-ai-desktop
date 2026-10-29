//! Shared in-memory relay for activation tests. Not compiled into release builds.
#![cfg(test)]

use reqwest::{Method, Url};
use serde_json::{json, Value};

use super::error::ActivationFailure;
use super::tokens::TokenApi;

#[derive(Default)]
pub(crate) struct FakeTokens {
    pub(crate) tokens: std::sync::Mutex<Vec<Value>>,
    pub(crate) requests: std::sync::Mutex<Vec<(Method, String)>>,
    pub(crate) fail_keys: std::sync::atomic::AtomicBool,
    /// 列表接口挂掉（网络、限流的 429 空响应），传输层直接报错。
    pub(crate) fail_list: std::sync::atomic::AtomicBool,
    /// 野菜的中转创建密钥时把新密钥回传在 `data` 里；标准 NewAPI 不回。
    pub(crate) echo_created: std::sync::atomic::AtomicBool,
    /// 会话已失效：中转对每个请求都回 401。
    pub(crate) signed_out: std::sync::atomic::AtomicBool,
    /// Number of persisted issuance records observed at each create request.
    pub(crate) intent_at_create: std::sync::Mutex<Vec<usize>>,
}

impl FakeTokens {
    pub(crate) fn seed(&self, token: Value) {
        self.tokens.lock().unwrap().push(token);
    }

    pub(crate) fn list_calls(&self) -> usize {
        self.requests
            .lock()
            .unwrap()
            .iter()
            .filter(|(method, path)| method == Method::GET && path.starts_with("/api/token/?"))
            .count()
    }
}

impl TokenApi for FakeTokens {
    async fn request(
        &self,
        method: Method,
        url: &str,
        body: Option<Value>,
    ) -> Result<(u16, Value), ActivationFailure> {
        // The station transport speaks same-origin relative paths; some callers
        // still pass an absolute origin URL. Normalise to the path form both the
        // real relay and this fake operate on.
        let absolute = format!("https://mock.invalid{url}");
        let parsed = Url::parse(if url.starts_with("https://") {
            url
        } else {
            &absolute
        })
        .unwrap();
        // Record path plus query so a test can prove a list happened (the relay
        // paginates with ?p=&size= and `size` is required for an exact read).
        let path = match parsed.query() {
            Some(query) => format!("{}?{query}", parsed.path()),
            None => parsed.path().to_string(),
        };
        let intent_at_create = (method == Method::POST && path == "/api/token/")
            .then(|| super::tokens::issuance_records().len());
        self.requests
            .lock()
            .unwrap()
            .push((method.clone(), path.clone()));
        if let Some(count) = intent_at_create {
            self.intent_at_create.lock().unwrap().push(count);
        }
        if self.signed_out.load(std::sync::atomic::Ordering::Relaxed) {
            return Ok((401, json!({"success":false,"message":"unauthorized"})));
        }
        let mut tokens = self.tokens.lock().unwrap();
        if method == Method::GET && path.starts_with("/api/token/?") {
            if self.fail_list.load(std::sync::atomic::Ordering::Relaxed) {
                return Err(ActivationFailure::ServerUnavailable);
            }
            // Paginated like the relay: newest first, at most 100 per page.
            let query = parsed
                .query_pairs()
                .map(|(key, value)| (key.into_owned(), value.into_owned()))
                .collect::<std::collections::HashMap<_, _>>();
            let page = query
                .get("p")
                .and_then(|p| p.parse::<usize>().ok())
                .unwrap_or(1)
                .max(1);
            let size = query
                .get("size")
                .and_then(|s| s.parse::<usize>().ok())
                .unwrap_or(10)
                .min(100);
            let mut newest_first = tokens.clone();
            newest_first.sort_by_key(|t| std::cmp::Reverse(t["id"].as_u64().unwrap_or(0)));
            let items = newest_first
                .into_iter()
                .skip((page - 1) * size)
                .take(size)
                .collect::<Vec<_>>();
            return Ok((
                200,
                json!({"success":true,"data":{"items":items,"total":tokens.len(),"page":page,"page_size":size}}),
            ));
        }
        if method == Method::POST && path == "/api/token/" {
            let mut token = body.unwrap();
            assert!(token["name"].as_str().unwrap().len() <= 50);
            assert_ne!(token["group"], "");
            let next_id = tokens
                .iter()
                .filter_map(|t| t["id"].as_u64())
                .max()
                .unwrap_or(0)
                + 1;
            token["id"] = json!(next_id);
            token["status"] = json!(1);
            tokens.push(token.clone());
            if self.echo_created.load(std::sync::atomic::Ordering::Relaxed) {
                token["key"] = json!("sk-****masked");
                return Ok((200, json!({"success":true,"message":"","data":token})));
            }
            // The standard server does not return a key or data here.
            return Ok((200, json!({"success":true,"message":""})));
        }
        let id: u64 = path.split('/').nth(3).unwrap_or("").parse().unwrap_or(0);
        if method == Method::POST && path.ends_with("/key") {
            if self.fail_keys.load(std::sync::atomic::Ordering::Relaxed) {
                return Err(ActivationFailure::ServerUnavailable);
            }
            assert!(tokens.iter().any(|t| t["id"] == id));
            return Ok((
                200,
                json!({"success":true,"data":{"key":format!("synthetic-only-test-token-{id:04}")}}),
            ));
        }
        if method == Method::DELETE {
            tokens.retain(|t| t["id"] != id);
            return Ok((200, json!({"success":true})));
        }
        // In particular `/api/token/search` lands here: the relay matches a
        // keyword without `%` against the whole name and rate-limits the
        // endpoint to ten calls a minute, so the client must never use it.
        panic!("unexpected token operation: {method} {path}");
    }
}

/// A deterministic minted name in the exact on-disk shape:
/// `<token prefix>-<device scope><8 hex nonce>`. Tests must not depend on the
/// scope being random by accident, or a seeded key looks like another machine's.
pub(crate) fn minted_name(prefix: &str, nonce: u32) -> String {
    minted_name_for(prefix, nonce, super::tokens::device_scope_for_tests())
}

pub(crate) fn minted_name_for(prefix: &str, nonce: u32, device: &str) -> String {
    format!("{prefix}-{device}{nonce:08x}")
}

/// A key this machine minted with an exact name/group scope.
pub(crate) fn fake_key(id: u64, name: &str, group: &str) -> Value {
    json!({
        "id": id,
        "name": name,
        "group": group, "status": 1, "expired_time": -1,
        "model_limits_enabled": true, "model_limits": "model-a",
    })
}

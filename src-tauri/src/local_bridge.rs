//! 本地桥共用的骨架：监听、优雅停机、换凭据不打断在途请求。
//!
//! 每个被接入的工具有自己的一座本地桥（Claude Code → `15728/claude-code`、
//! Claude Desktop → `15729/claude-desktop`、Codex → `15731/codex`）。
//! 端口、路由和协议各不相同，**但起停与状态管理完全一样**。
//!
//! 抽出来共用而不是各写一份，是因为这里有一段不显眼但很讲究的逻辑：
//! 用户在应用已经开着的时候重新接入，凭据要换，而**在途请求不能断**
//! —— 所以检查、换状态、起停全压在同一把锁下，状态本身再套一层 `RwLock`
//! 让处理函数随时读到最新的。复制样板不可怕，复制这种东西才可怕。
//!
//! 各座桥自己提供的只有一个 router 工厂（`RouterFactory`）：一行，
//! 把自己的 `dispatch` 挂上去。用函数指针而不是泛型，是为了让这个类型
//! 保持可以直接存进 adapter 结构体里，不必带类型参数传染上去。

use std::{sync::Arc, time::Duration};

use axum::{http::header, Router};
use tokio::{
    net::TcpListener,
    sync::{broadcast, oneshot, Mutex, RwLock},
    task::JoinHandle,
};

use crate::{
    codex_bridge::secure_equal, tool_adapters::AdapterFailure, tool_credentials::ToolCredential,
};

/// 桥起来了、并且真的收到过一次成功请求。接入流程用它把「配置写好了」
/// 和「首次使用验证过了」分成两级。
#[derive(Clone, Debug)]
pub(crate) struct VerificationEvent;

/// 转发到中转站用的 HTTP 客户端。
///
/// `redirect: none` 与 `retry: never` 是有意的：这条路上我们**不替用户重试**
/// ——请求可能已经被上游计费，重发一次就是重复扣费；重定向则可能把凭据
/// 带到另一个域名去。
pub(crate) fn upstream_client() -> Result<reqwest::Client, reqwest::Error> {
    reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .retry(reqwest::retry::never())
        .connect_timeout(Duration::from_secs(15))
        .read_timeout(Duration::from_secs(600))
        .build()
}

/// 一座桥运行时的全部状态。各座桥的 `dispatch` 从这里取。
pub(crate) struct BridgeState {
    pub(crate) credential: ToolCredential,
    pub(crate) local_token: String,
    /// 这座桥的 URL 前缀（`/claude-desktop`、`/codex`……）。
    /// 同一个端口上区分不同工具，也让不带前缀的请求直接 404。
    pub(crate) prefix: &'static str,
    pub(crate) client: reqwest::Client,
    pub(crate) events: broadcast::Sender<VerificationEvent>,
}

/// 处理函数看到的状态句柄。外层 `RwLock` 让换凭据对在途请求透明。
pub(crate) type SharedState = Arc<RwLock<Arc<BridgeState>>>;

/// 各座桥把自己的路由挂上去。
pub(crate) type RouterFactory = fn(SharedState) -> Router;

struct Running {
    shutdown: Option<oneshot::Sender<()>>,
    task: JoinHandle<()>,
    credential: ToolCredential,
    events: broadcast::Sender<VerificationEvent>,
    state: SharedState,
}

#[derive(Clone)]
pub(crate) struct LocalBridgeRuntime {
    address: &'static str,
    prefix: &'static str,
    router: RouterFactory,
    runtime: Arc<Mutex<Option<Running>>>,
}

impl LocalBridgeRuntime {
    pub(crate) fn new(address: &'static str, prefix: &'static str, router: RouterFactory) -> Self {
        Self {
            address,
            prefix,
            router,
            runtime: Arc::new(Mutex::new(None)),
        }
    }

    pub(crate) async fn start(
        &self,
        credential: ToolCredential,
    ) -> Result<broadcast::Receiver<VerificationEvent>, AdapterFailure> {
        let local_token = credential
            .local_gateway_token
            .clone()
            .filter(|value| value.starts_with("ycg-") && value.len() == 68)
            .ok_or(AdapterFailure::SecureStorageUnavailable)?;
        // Opening an already configured app must not interrupt its requests:
        // check, update and start all happen under the same lock as stop.
        let mut slot = self.runtime.lock().await;
        if let Some(running) = slot.as_mut() {
            if !running.task.is_finished() {
                if running.credential != credential {
                    let previous = running.state.read().await.clone();
                    *running.state.write().await = Arc::new(BridgeState {
                        credential: credential.clone(),
                        local_token,
                        prefix: self.prefix,
                        client: previous.client.clone(),
                        events: running.events.clone(),
                    });
                    running.credential = credential;
                }
                return Ok(running.events.subscribe());
            }
        }
        if let Some(mut previous) = slot.take() {
            if let Some(shutdown) = previous.shutdown.take() {
                let _ = shutdown.send(());
            }
            previous.task.abort();
            let _ = previous.task.await;
        }
        let listener = TcpListener::bind(self.address)
            .await
            .map_err(|_| AdapterFailure::LaunchFailed)?;
        let (events, receiver) = broadcast::channel(8);
        let state: SharedState = Arc::new(RwLock::new(Arc::new(BridgeState {
            credential: credential.clone(),
            local_token,
            prefix: self.prefix,
            client: upstream_client().map_err(|_| AdapterFailure::LaunchFailed)?,
            events: events.clone(),
        })));
        let router = (self.router)(state.clone());
        let (shutdown, shutdown_rx) = oneshot::channel::<()>();
        let task = tokio::spawn(async move {
            let _ = axum::serve(listener, router)
                .with_graceful_shutdown(async move {
                    let _ = shutdown_rx.await;
                })
                .await;
        });
        *slot = Some(Running {
            shutdown: Some(shutdown),
            task,
            credential,
            events,
            state,
        });
        Ok(receiver)
    }

    /// 桥现在替哪一份接入转发；没在跑就是 `None`。
    #[cfg(test)]
    pub(crate) async fn serving(&self) -> Option<ToolCredential> {
        let slot = self.runtime.lock().await;
        slot.as_ref()
            .filter(|running| !running.task.is_finished())
            .map(|running| running.credential.clone())
    }

    pub(crate) async fn stop(&self) {
        let runtime = self.runtime.lock().await.take();
        if let Some(mut runtime) = runtime {
            if let Some(shutdown) = runtime.shutdown.take() {
                let _ = shutdown.send(());
            }
            runtime.task.abort();
            let _ = runtime.task.await;
        }
    }
}

/// 过桥时不能带给站点的头。其余的原样带过去。
///
/// 应用自己报的名字（`user-agent`）、会话标记这些，站点是当真的：有的厂商
/// 明说不许改客户端标识，有的接口没有应用的会话标记就直接拒绝，中转站上也有
/// 「只允许某个应用」的渠道。桥只是替应用换一把密钥，不该把它说成别人。
/// 用白名单追不上各个应用每个版本新加的头，所以只列不能带的：
///
/// * `authorization` / `x-api-key`：那是本机桥的令牌，站点的密钥由桥另外放；
/// * 逐跳头和 `content-length`：请求体在桥里可能被改写过；
/// * `content-type` / `accept` / `accept-encoding`：桥按自己发出去的内容重新写，
///   重复带一份会变成两个值；
/// * `cookie`、各种 `proxy-`、`x-forwarded-`：不该越过本机。
const DROPPED_APP_HEADERS: [&str; 17] = [
    "authorization",
    "x-api-key",
    "host",
    "content-length",
    "content-type",
    "accept",
    "accept-encoding",
    "transfer-encoding",
    "connection",
    "keep-alive",
    "te",
    "trailer",
    "upgrade",
    "expect",
    "cookie",
    "forwarded",
    "origin",
];

pub(crate) type AppHeaders = Vec<(header::HeaderName, header::HeaderValue)>;

pub(crate) fn app_headers(headers: &axum::http::HeaderMap) -> AppHeaders {
    headers
        .iter()
        .filter(|(name, _)| {
            let name = name.as_str();
            !DROPPED_APP_HEADERS.contains(&name)
                && !name.starts_with("proxy-")
                && !name.starts_with("x-forwarded-")
        })
        .map(|(name, value)| (name.clone(), value.clone()))
        .collect()
}

pub(crate) fn with_app_headers(
    mut request: reqwest::RequestBuilder,
    headers: &AppHeaders,
) -> reqwest::RequestBuilder {
    for (name, value) in headers {
        request = request.header(name, value);
    }
    request
}

/// 应用自己的会话标记。厂商认得其中几个，但经过翻译的那条路上它们带不过去，
/// 所以拿来当种子：同一段对话里它不变。
const NATIVE_SESSION_HEADERS: [&str; 4] = [
    "x-claude-code-session-id",
    "session_id",
    "conversation_id",
    "x-session-id",
];

/// 有的厂商要求每段对话带一个固定的会话标记，不带就拒绝（OpenCode Go 的
/// `x-opencode-session`）。应用自己带了就用它的；没带的由桥补一个。
///
/// 补的值要「同一段对话每次请求都一样，换一段就变」。应用带着自己的会话标记
/// 就从它算；没有就从这段对话的第一句话算；连第一句话都找不到，退到这次开机
/// 内固定的一个值。
pub(crate) fn session_mark<'a>(
    origin: &str,
    sent: impl IntoIterator<Item = (&'a str, &'a str)>,
    body: &serde_json::Value,
) -> Option<(&'static str, String)> {
    use serde_json::Value;
    use sha2::{Digest, Sha256};

    let name = crate::provider_presets::session_header(origin)?;
    let mut native = None;
    for (sent, value) in sent {
        if sent.eq_ignore_ascii_case(name) {
            return None;
        }
        if native.is_none() && NATIVE_SESSION_HEADERS.contains(&sent) && !value.is_empty() {
            native = Some(value.to_owned());
        }
    }
    // 缓存标记每一轮都在挪位置，不算对话内容。
    fn without_cache_marks(value: &Value) -> Value {
        match value {
            Value::Object(map) => Value::Object(
                map.iter()
                    .filter(|(key, _)| key.as_str() != "cache_control")
                    .map(|(key, value)| (key.clone(), without_cache_marks(value)))
                    .collect(),
            ),
            Value::Array(items) => Value::Array(items.iter().map(without_cache_marks).collect()),
            other => other.clone(),
        }
    }
    let is_user = |turn: &&Value| turn.get("role").and_then(Value::as_str) == Some("user");
    // Chat 和 Anthropic 格式的对话在 `messages` 里，Responses 格式的在 `input` 里。
    let opening = || {
        ["messages", "input"]
            .iter()
            .filter_map(|field| body.get(field))
            .find_map(|turns| match turns {
                Value::String(text) => Some(text.clone()),
                Value::Array(turns) => turns
                    .iter()
                    .find(is_user)
                    .or(turns.first())
                    .map(|turn| without_cache_marks(turn).to_string()),
                _ => None,
            })
    };
    let mark = match native.or_else(opening) {
        Some(seed) => Sha256::digest(seed.as_bytes())
            .iter()
            .take(13)
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>(),
        None => {
            static THIS_RUN: std::sync::OnceLock<String> = std::sync::OnceLock::new();
            THIS_RUN
                .get_or_init(|| uuid::Uuid::new_v4().simple().to_string()[..26].to_owned())
                .clone()
        }
    };
    Some((name, format!("ses_{mark}")))
}

/// 只认本地令牌，不认中转站的 key。
///
/// 这道门是这座桥存在的前提：它监听在 `127.0.0.1` 上，本机任何进程都能连。
/// 令牌用常数时间比较（`secure_equal`），两个位置都收 —— Anthropic SDK 发
/// `x-api-key`，OpenAI 系发 `Authorization: Bearer`。
pub(crate) fn locally_authorized(headers: &axum::http::HeaderMap, local_token: &str) -> bool {
    let bearer = headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "));
    let api_key = headers
        .get("x-api-key")
        .and_then(|value| value.to_str().ok());
    !local_token.is_empty()
        && [bearer, api_key]
            .into_iter()
            .flatten()
            .any(|value| secure_equal(value, local_token))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_app_keeps_its_own_name_and_session_but_not_the_local_token() {
        let mut incoming = axum::http::HeaderMap::new();
        for (name, value) in [
            ("user-agent", "opencode/1.2.3"),
            ("x-opencode-session", "ses_1"),
            ("authorization", "Bearer local-token"),
            ("x-api-key", "local-token"),
            ("content-type", "application/json"),
            ("content-length", "12"),
            ("accept", "text/event-stream"),
            ("accept-encoding", "gzip"),
            ("host", "127.0.0.1:15732"),
            ("cookie", "a=b"),
            ("proxy-authorization", "Basic x"),
            ("x-forwarded-for", "10.0.0.1"),
        ] {
            incoming.insert(name, value.parse().unwrap());
        }
        let kept = app_headers(&incoming);
        let mut names: Vec<&str> = kept.iter().map(|(name, _)| name.as_str()).collect();
        names.sort_unstable();
        assert_eq!(names, ["user-agent", "x-opencode-session"]);

        let request = with_app_headers(
            reqwest::Client::new().post("https://relay.example.com/v1/chat/completions"),
            &kept,
        )
        .bearer_auth("upstream-key")
        .build()
        .unwrap();
        assert_eq!(request.headers()["user-agent"], "opencode/1.2.3");
        assert_eq!(request.headers()["x-opencode-session"], "ses_1");
        assert_eq!(request.headers().get_all("authorization").iter().count(), 1);
        assert_eq!(request.headers()["authorization"], "Bearer upstream-key");
    }

    #[test]
    fn a_missing_session_mark_is_filled_in_and_stays_the_same_within_a_conversation() {
        use serde_json::json;
        let go = "https://opencode.ai";
        let first = json!({"messages": [
            {"role": "system", "content": "be brief"},
            {"role": "user", "content": "fix the build"},
        ]});
        let later = json!({"messages": [
            {"role": "system", "content": "be brief, today is Monday"},
            {"role": "user", "content": "fix the build"},
            {"role": "assistant", "content": "done"},
            {"role": "user", "content": "thanks"},
        ]});
        let other = json!({"input": [{"role": "user", "content": "write a poem"}]});

        let (name, mark) = session_mark(go, [("user-agent", "pi/1.0")], &first).unwrap();
        assert_eq!(name, "x-opencode-session");
        assert!(mark.starts_with("ses_") && mark.len() == 30, "{mark}");
        assert_eq!(session_mark(go, [], &later).unwrap().1, mark);
        assert_ne!(session_mark(go, [], &other).unwrap().1, mark);
        // 缓存标记挪了位置，还是同一段对话。
        let marked = json!({"messages": [{"role": "user", "content": [
            {"type": "text", "text": "fix the build", "cache_control": {"type": "ephemeral"}},
        ]}]});
        let unmarked = json!({"messages": [{"role": "user", "content": [
            {"type": "text", "text": "fix the build"},
        ]}]});
        assert_eq!(
            session_mark(go, [], &marked).unwrap().1,
            session_mark(go, [], &unmarked).unwrap().1
        );
        // 应用带着自己的会话标记：跟着它走，对话内容变了也不变。
        let native = [("x-claude-code-session-id", "abc")];
        assert_eq!(
            session_mark(go, native, &first).unwrap().1,
            session_mark(go, native, &other).unwrap().1
        );
        assert_ne!(session_mark(go, native, &first).unwrap().1, mark);
        // 没有对话内容：这次开机内固定。
        let blank = session_mark(go, [], &json!({})).unwrap().1;
        assert_eq!(session_mark(go, [], &json!({})).unwrap().1, blank);

        // 应用自己带了厂商要的那个：不动它的。
        assert!(session_mark(go, [("X-OpenCode-Session", "ses_1")], &first).is_none());
        // 不要求会话标记的厂商、中转站：什么都不加。
        assert!(session_mark("https://api.deepseek.com", [], &first).is_none());
        assert!(session_mark("https://relay.example.com", [], &first).is_none());
    }
}

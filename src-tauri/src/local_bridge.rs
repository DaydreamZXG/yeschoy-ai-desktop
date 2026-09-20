//! 本地桥共用的骨架：监听、优雅停机、换凭据不打断在途请求。
//!
//! 每个被接入的工具有自己的一座本地桥（Claude Code → `15728/claude-code`、
//! Claude Desktop → `15729/claude-desktop`、Codex → `15730/codex`）。
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

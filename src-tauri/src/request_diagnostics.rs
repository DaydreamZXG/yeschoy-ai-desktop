//! 每个工具最近一次请求结果的投影。
//!
//! 直连改造后本地不再观测请求，写入侧随本地网关一起删除；这里保留投影类型与
//! 清理入口，服务端用量日志接入后由服务端数据填充（见 `outputs/野菜API-直连改造.md`）。
use std::{
    collections::HashMap,
    sync::{Mutex, OnceLock},
};

use serde::Serialize;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
#[allow(dead_code)] // 服务端观测接入后由服务端数据构造这些取值。
pub(crate) enum RequestOutcome {
    Ok,
    Timeout,
    NetworkError,
    UpstreamError,
    InvalidResponse,
    StreamInterrupted,
    UnknownModel,
    PayloadTooLarge,
    LocalBusy,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RequestObservation {
    pub(crate) model_id: String,
    pub(crate) billing_group: String,
    pub(crate) line_id: String,
    pub(crate) outcome: RequestOutcome,
    pub(crate) http_status: u16,
    pub(crate) observed_at_epoch_ms: u64,
}

fn observations() -> &'static Mutex<HashMap<String, RequestObservation>> {
    static OBSERVATIONS: OnceLock<Mutex<HashMap<String, RequestObservation>>> = OnceLock::new();
    OBSERVATIONS.get_or_init(|| Mutex::new(HashMap::new()))
}

pub(crate) fn latest(tool: &str) -> Option<RequestObservation> {
    observations().lock().ok()?.get(tool).cloned()
}

pub(crate) fn clear(tool: &str) {
    if let Ok(mut values) = observations().lock() {
        values.remove(tool);
    }
}

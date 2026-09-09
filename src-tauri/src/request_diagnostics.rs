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

/// 用服务端用量日志里该工具最新的一条消费记录刷新投影。
pub(crate) fn publish(tool: &str, observation: RequestObservation) {
    let Ok(mut values) = observations().lock() else {
        return;
    };
    let newer = values
        .get(tool)
        .is_none_or(|current| observation.observed_at_epoch_ms >= current.observed_at_epoch_ms);
    if newer {
        values.insert(tool.to_owned(), observation);
    }
}

/// 线路标识与账号 origin 一一对应，与激活时写入的线路保持一致。
pub(crate) fn line_for_origin(origin: &str) -> Option<&'static str> {
    match origin {
        "https://yeschoy.com" => Some("mainland_optimized"),
        "https://api.yeschoy.com" => Some("global_accelerated"),
        _ => None,
    }
}

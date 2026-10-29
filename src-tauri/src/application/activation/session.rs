//! In-process admission for the single live activation transaction, progress
//! events and cancellation. The shared write lock lives here so the service and
//! the connection inspector cannot interleave native configuration writes.
use crate::application::activation::request::ToolActivationRequest;
use crate::connectivity_core::request_id_is_valid;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex as StdMutex,
};
use tauri::{Emitter, Manager};

static ACTIVATION_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// The single write lock shared by activation and connection inspection.
pub(crate) fn activation_lock() -> &'static tokio::sync::Mutex<()> {
    &ACTIVATION_LOCK
}
const ACTIVATION_PROGRESS_EVENT: &str = "yeschoy://activation-progress";
const ACTIVATION_PROGRESS_TOTAL: u8 = 7;

#[derive(Default)]
pub(crate) struct ActivationOperationState {
    active: StdMutex<HashMap<String, Arc<ActivationCancellation>>>,
}

#[derive(Default)]
pub(crate) struct ActivationCancellation {
    requested: AtomicBool,
}

impl ActivationCancellation {
    pub(crate) fn request(&self) {
        self.requested.store(true, Ordering::Release);
    }

    pub(crate) fn is_requested(&self) -> bool {
        self.requested.load(Ordering::Acquire)
    }
}

pub(crate) struct ActivationRegistration<'a> {
    request_id: String,
    pub(crate) cancellation: Arc<ActivationCancellation>,
    state: &'a ActivationOperationState,
}

impl Drop for ActivationRegistration<'_> {
    fn drop(&mut self) {
        if let Ok(mut active) = self.state.active.lock() {
            let current = active.get(&self.request_id);
            if current.is_some_and(|value| Arc::ptr_eq(value, &self.cancellation)) {
                active.remove(&self.request_id);
            }
        }
    }
}

impl ActivationOperationState {
    pub(crate) fn begin(&self, request_id: &str) -> Result<ActivationRegistration<'_>, ()> {
        let cancellation = Arc::new(ActivationCancellation::default());
        let mut active = self.active.lock().map_err(|_| ())?;
        // There is one shared activation/configuration transaction. Reject a
        // second click immediately instead of queueing it behind the native
        // lock and making the renderer look frozen.
        if !active.is_empty() {
            return Err(());
        }
        active.insert(request_id.to_owned(), cancellation.clone());
        Ok(ActivationRegistration {
            request_id: request_id.to_owned(),
            cancellation,
            state: self,
        })
    }

    pub(crate) fn cancel(&self, request_id: &str) -> Result<bool, ()> {
        let active = self.active.lock().map_err(|_| ())?;
        Ok(active.get(request_id).is_some_and(|value| {
            value.request();
            true
        }))
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ActivationProgress {
    request_id: String,
    tool_id: String,
    stage: &'static str,
    completed_steps: u8,
    total_steps: u8,
}

/// 进度事件。
///
/// 对运行时泛型，而不是写死 `AppHandle`（即 `AppHandle<Wry>`）：端到端编排
/// 回归要用 `tauri::test::mock_app()` 的真实 `AppHandle<MockRuntime>`，只有
/// 泛型才能让生产和测试跑**同一份**函数。运行时类型由调用方推断，生产代码
/// 不写任何类型参数。
///
/// 投递目标仍是主窗口。真实应用启动时就建了 `main` 窗口；mock 应用没有
/// 窗口系统，窗口表为空，此时 `emit_to("main")` 会返回 `Ok` 却没有任何人
/// 收得到 —— 所以「投递成功」以窗口存在为前提，否则退回应用级广播。
pub(crate) fn emit_activation_progress<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    request: &ToolActivationRequest,
    stage: &'static str,
    completed_steps: u8,
) {
    let payload = ActivationProgress {
        request_id: request.request_id.clone(),
        tool_id: request.tool_id.clone(),
        stage,
        completed_steps,
        total_steps: ACTIVATION_PROGRESS_TOTAL,
    };
    let to_main = app
        .webview_windows()
        .contains_key("main")
        .then(|| app.emit_to("main", ACTIVATION_PROGRESS_EVENT, payload.clone()));
    let delivered = match to_main {
        Some(Ok(())) => true,
        None => app.emit(ACTIVATION_PROGRESS_EVENT, payload).is_ok(),
        // The main window exists but the delivery failed: never silently
        // downgrade to an app-wide broadcast that other windows would see.
        Some(Err(_)) => false,
    };
    if !delivered {
        log::warn!("tool_activation stage=progress_emit_failed");
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ActivationCancelRequest {
    request_id: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActivationCancelResponse {
    request_id: String,
    status: &'static str,
}

#[tauri::command]
pub(crate) fn cancel_tool_activation_v1(
    state: tauri::State<'_, ActivationOperationState>,
    request: ActivationCancelRequest,
) -> Result<ActivationCancelResponse, String> {
    if !request_id_is_valid(&request.request_id) {
        return Err("invalid_activation_cancel_request".into());
    }
    let found = state
        .cancel(&request.request_id)
        .map_err(|_| "activation_state_unavailable")?;
    Ok(ActivationCancelResponse {
        request_id: request.request_id,
        status: if found {
            "cancel_requested"
        } else {
            "not_found"
        },
    })
}

#[cfg(test)]
#[path = "session_tests.rs"]
mod tests;

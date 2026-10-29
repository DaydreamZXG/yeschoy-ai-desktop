//! Activation IPC result DTOs. The schema (keys and `schemaVersion`) is a
//! renderer contract and deliberately stays byte-for-byte compatible.
use serde::Serialize;

use crate::domain::model::ModelBinding;

use super::request::ToolActivationRequest;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolActivationProjection {
    request_id: String,
    schema_version: u8,
    status: &'static str,
    tool_id: String,
    model_id: String,
    billing_group: String,
    observed_at_epoch_ms: u64,
    pub(crate) reason_code: &'static str,
    models: Vec<ModelBinding>,
    /// 这次没写进应用的模型，以及每个是因为什么。
    ///
    /// 一个计费分组铸不出密钥时不再拖垮整单（见 `securing_access` 那个循环），
    /// 但界面必须说得出少了谁 —— 否则用户看到「接入完成」，而列表里那个模型
    /// 其实不能用，比直接失败还糟。
    skipped: Vec<SkippedBinding>,
}

/// 被跳过的一个绑定，带上它自己的原因码。
///
/// 原因码是常量，和失败投影里的 `reason_code` 同一套，所以界面能复用既有文案。
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkippedBinding {
    pub(crate) model_id: String,
    pub(crate) billing_group: String,
    pub(crate) reason_code: &'static str,
}

impl ToolActivationProjection {
    pub(crate) fn new(
        request: &ToolActivationRequest,
        status: &'static str,
        reason_code: &'static str,
    ) -> Self {
        Self {
            request_id: request.request_id.clone(),
            // 5：新增 `skipped`。渲染层的 `exactKeys` 要求键集完全一致，所以
            // 加字段必须升版本，不能在 4 上悄悄多带一个键。
            schema_version: 5,
            status,
            tool_id: request.tool_id.clone(),
            model_id: request.model_id.clone(),
            billing_group: request.billing_group.clone(),
            observed_at_epoch_ms: crate::application::activation::tokens::now_epoch_ms(),
            reason_code,
            models: request.bindings(),
            skipped: Vec::new(),
        }
    }

    /// 挡住整单的那个绑定，通过 `skipped` 传出去，**不动 `model_id`**。
    ///
    /// 第一版是把投影的 `model_id`/`billing_group` 改写成出错的那个绑定 ——
    /// 看着直接，但渲染层有一条反伪造检查：`result.modelId !== input.modelId`
    /// 就整个拒收。于是用户拿到的不是「是 glm-5.3-flash 挡住了」，而是更糟的
    /// 「暂时无法确认接入结果」。投影的 `model_id` 是「这次请求的主语」，是契约，
    /// 不能借用来表达别的意思。界面自己从 `skipped` 里找那一个。
    pub(crate) fn with_skipped(mut self, skipped: Vec<SkippedBinding>) -> Self {
        self.skipped = skipped;
        self
    }
}

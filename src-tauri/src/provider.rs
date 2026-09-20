//! 给 vendored `proxy/` 用的薄壳。
//!
//! 上游 cc-switch 的 `src/provider.rs` 有 1657 行，是它自己那套 provider 模型
//! （数据库存储、表单、多应用配置），野菜用不上。但 vendored 的
//! `proxy/providers/transform_codex_chat.rs` 里写的是
//! `use crate::provider::CodexChatReasoningConfig;` —— 为了让那个文件
//! **一个字节都不用改**，这里按同一路径提供同一个类型。
//!
//! 下面的结构体**逐字抄自上游**（含注释）。字段名与 serde 重命名必须完全一致：
//! 对不上不会编译报错，只会在运行时静默丢字段。同步 `proxy/` 时一并核对。

use serde::{Deserialize, Serialize};

/// Codex Responses -> Chat Completions 的 reasoning 能力描述。
// 目前还没有构造点：用它的 `providers/transform_codex_chat.rs` 尚未 vendored。
// 那个文件搬进来之后，这条 allow 应当去掉 —— 届时 clippy 会自己提醒。
#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct CodexChatReasoningConfig {
    #[serde(rename = "supportsThinking", skip_serializing_if = "Option::is_none")]
    pub supports_thinking: Option<bool>,
    #[serde(rename = "supportsEffort", skip_serializing_if = "Option::is_none")]
    pub supports_effort: Option<bool>,
    #[serde(rename = "thinkingParam", skip_serializing_if = "Option::is_none")]
    pub thinking_param: Option<String>,
    #[serde(rename = "effortParam", skip_serializing_if = "Option::is_none")]
    pub effort_param: Option<String>,
    #[serde(rename = "effortValueMode", skip_serializing_if = "Option::is_none")]
    pub effort_value_mode: Option<String>,
    /// 声明性字段：标注上游 reasoning 的回传位置（reasoning_content / reasoning /
    /// reasoning_details / think_tags）。当前响应侧 `extract_reasoning_field_text`
    /// 靠穷举字段提取、并不读取本字段；保留作文档说明与未来按格式分发（如 think_tags）的预留。
    #[serde(rename = "outputFormat", skip_serializing_if = "Option::is_none")]
    pub output_format: Option<String>,
    /// 运行时字段（不持久化、不进 meta）：当前请求模型在平台侧声明的合法 effort
    /// 档位，由 resolve 按请求模型从供应商 `settings_config.modelCatalog` 的
    /// `reasoningLevels`（逐模型声明，见 #6228）查表填充。仅 "zen" 值映射消费：
    /// Some → 钳到合法档；None → 不发 effort 字段（模型未收录或为 toggle 型）。
    #[serde(skip)]
    pub effort_levels: Option<Vec<String>>,
}

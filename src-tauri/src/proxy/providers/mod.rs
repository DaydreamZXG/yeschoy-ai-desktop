//! 上游 cc-switch 的 provider 转换器，**逐字节原样 vendored**。
//!
//! 规矩见上一层的 `../VENDOR.md`：这里的文件一个字节都不改。

#[allow(dead_code)] // 上游完整 API；桌面端只用到其中一部分。
pub(crate) mod codex_chat_common;
#[allow(dead_code)]
pub(crate) mod codex_responses_sse;

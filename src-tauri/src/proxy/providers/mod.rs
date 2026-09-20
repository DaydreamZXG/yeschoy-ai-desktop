//! 上游 cc-switch 的 provider 转换器，**逐字节原样 vendored**。
//!
//! 规矩见上一层的 `../VENDOR.md`：这里的文件一个字节都不改。

#[allow(dead_code)] // 上游完整 API；桌面端只用到其中一部分。
pub(crate) mod codex_chat_common;
#[allow(dead_code)]
pub(crate) mod codex_chat_history;
#[allow(dead_code)]
pub(crate) mod codex_responses_sse;
// `unused_imports`：上游文件在它自己的 crate 里用得到这些，
// 在我们只取子集的上下文里用不到。**压警告，不改文件** ——
// 改了就等于开始维护一份本地分叉。
#[allow(dead_code, unused_imports)]
pub(crate) mod streaming;
#[allow(dead_code)]
pub(crate) mod streaming_codex_chat;
#[allow(dead_code)]
pub(crate) mod transform;
// `clippy::op_ref`：我们的 clippy（1.95）比上游 CI 用的那版严，
// 在它的测试里挑出一处 `url == &data_url`。**压 lint，不改文件。**
#[allow(dead_code, clippy::op_ref)]
pub(crate) mod transform_codex_chat;

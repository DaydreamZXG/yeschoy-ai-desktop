//! 上游 `proxy/usage/` 的**子集**：只取 `parser`。
//!
//! 上游这里还有 `calculator`（成本计算）与 `logger`（把用量写进它自己的存储）。
//! 野菜的用量与计费走服务端账单，不需要也不应该引入那两个 —— 尤其 `logger`，
//! 它会往 cc-switch 的存储写东西。
//!
//! `parser.rs` 零 crate 内依赖，可以单独拿。vendored 的
//! `providers/transform.rs` 只用到 `usage::parser::TokenUsage`。

#[allow(dead_code)]
pub(crate) mod parser;

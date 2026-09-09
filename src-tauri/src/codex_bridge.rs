//! 共享协议原语。
//!
//! 这里曾承载 Codex 的本地网关（127.0.0.1:15722）。中转站原生支持 Responses 与
//! Anthropic 协议后，网关、守护进程以及配套的协议转换 helper 都已删除，只留下
//! 仍在使用的常量时间比较。

pub(crate) const BASE_URL: &str = "http://127.0.0.1:15722/yeschoy/v1";

pub(crate) fn secure_equal(left: &str, right: &str) -> bool {
    let left = left.as_bytes();
    let right = right.as_bytes();
    let mut difference = left.len() ^ right.len();
    for index in 0..left.len().max(right.len()) {
        difference |= usize::from(
            left.get(index).copied().unwrap_or_default()
                ^ right.get(index).copied().unwrap_or_default(),
        );
    }
    difference == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secure_comparison_requires_exact_value() {
        assert!(secure_equal("sk-example", "sk-example"));
        assert!(!secure_equal("sk-example", "sk-other"));
        assert!(!secure_equal("", "sk-example"));
    }
}

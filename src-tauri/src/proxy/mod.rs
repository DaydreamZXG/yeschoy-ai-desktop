//! 上游 cc-switch 的协议转换层，**逐字节原样 vendored**。
//!
//! 取自 `github.com/farion1231/cc-switch`，见同目录 `VENDOR.md` 里记录的 commit。
//!
//! # 这个目录的规矩
//!
//! **除本文件外，`src/proxy/` 下的每个文件都必须与上游逐字节相同。**
//! 它们由 `vendored_files_are_unmodified` 守着（`VENDOR.md` 里有 sha256）。
//! 需要野菜特有的行为时，写在我们自己的文件里、**包着调用**上游的公开入口，
//! 不要改这里 —— 上次 vendoring 就是把逻辑打进了上游文件，于是每次同步
//! 都要重打一遍补丁。保持原样，再同步就是 `cp`。
//!
//! 模块路径刻意与上游一致（`crate::proxy::…`），这样上游文件里的
//! `use crate::proxy::…` 不用改一个字。

#[allow(dead_code)] // 上游完整 API；桌面端只用到其中一部分。
pub(crate) mod error;
#[allow(dead_code)]
pub(crate) mod json_canonical;
#[allow(dead_code)]
pub(crate) mod providers;
#[allow(dead_code)]
pub(crate) mod sse;
#[allow(dead_code)]
pub(crate) mod tool_media;

#[cfg(test)]
mod tests {
    use sha2::{Digest, Sha256};

    /// 每个 vendored 文件与它的 sha256，与 `VENDOR.md` 里那张表同源。
    ///
    /// 新 vendor 一个文件就在这里加一行，并同步更新 `VENDOR.md`。
    const VENDORED: &[(&str, &str, &str)] = &[
        (
            "error.rs",
            include_str!("error.rs"),
            "5c3a781b02321874dd2c59735c1a5121461ceeba0d6b988318f3747605d6ce88",
        ),
        (
            "json_canonical.rs",
            include_str!("json_canonical.rs"),
            "23715ee65bd5a5bedb55c330247a3b1811f612083e0bd6deaa89bd41109c5772",
        ),
        (
            "sse.rs",
            include_str!("sse.rs"),
            "d74fa5e207fa4de5f47f076b1df9a78e96522e095a2cd8c087d6eebd82217634",
        ),
        (
            "tool_media.rs",
            include_str!("tool_media.rs"),
            "496ce69210d50a689baff3b88daeb45cd015bd35b8a4e24e97a324c4a0389706",
        ),
        (
            "providers/codex_chat_common.rs",
            include_str!("providers/codex_chat_common.rs"),
            "41412157670fedab83a01461a9fc43acfb9a89fbfd1f9ef786929fa3343da0c8",
        ),
        (
            "providers/codex_responses_sse.rs",
            include_str!("providers/codex_responses_sse.rs"),
            "2e4f27b66b51c60244e7a95270bae1ef884433d04bab5684f98fc505302d1057",
        ),
    ];

    /// vendored 文件必须与上游逐字节相同。
    ///
    /// 这条守的是整个策略：只要没人手改这些文件，重新同步就是 `cp`，
    /// 协议转换的边角由上游去磨。一旦有人往里塞野菜的逻辑，下一次同步
    /// 就会静默覆盖掉它 —— 与其那时才发现，不如现在就红。
    ///
    /// 要加野菜行为：写在我们自己的文件里，包着调用上游的公开入口。
    #[test]
    fn vendored_files_are_unmodified() {
        for (name, contents, expected) in VENDORED {
            // 归一化换行再算：Windows 检出默认 autocrlf，文件会是 CRLF，
            // 否则同一份内容在两个平台上得出两个哈希。
            let normalized = contents.replace("\r\n", "\n");
            let digest = Sha256::digest(normalized.as_bytes());
            assert_eq!(
                format!("{digest:x}"),
                *expected,
                "{name} 与上游不一致 —— 要么有人手改了它（请改我们自己的文件），\
                 要么刚同步过上游而忘了更新 VENDOR.md 与这张表"
            );
        }
    }
}

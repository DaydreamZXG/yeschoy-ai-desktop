# vendored 自 cc-switch

来源：`github.com/farion1231/cc-switch`
提交：`06082e189d65e6d6dbadc35dacdac1ce6c79d89a`（main，2026-09-15）
取回日期：2026-09-20

## 规矩

`src/proxy/` 下除 `mod.rs` 与本文件外，**每个文件都与上游逐字节相同**。
`proxy::tests::vendored_files_are_unmodified` 守着这条：改一个字节测试就红。

野菜特有的行为**写在我们自己的文件里、包着调用**上游的公开入口，不要改这里。
上一次 vendoring（`23c12bd3` 之前）把逻辑打进了上游文件，于是每次同步都要
重打一遍补丁 —— 那正是这次要避免的。

## 怎么重新同步

```bash
SHA=<上游新的 commit>
for f in error.rs json_canonical.rs sse.rs tool_media.rs \
         providers/codex_chat_common.rs providers/codex_responses_sse.rs; do
  curl -sSL "https://raw.githubusercontent.com/farion1231/cc-switch/$SHA/src-tauri/src/proxy/$f" \
    -o src-tauri/src/proxy/$f
done
# 更新下面的 sha256 与上面的提交号，然后 cargo test --lib
```

同步后**必须跑全量测试**：上游这些文件自带单测，它们是行为对不对的判据。

## 已 vendored 的文件

| 文件 | sha256 |
| --- | --- |
| `error.rs` | `5c3a781b02321874dd2c59735c1a5121461ceeba0d6b988318f3747605d6ce88` |
| `json_canonical.rs` | `23715ee65bd5a5bedb55c330247a3b1811f612083e0bd6deaa89bd41109c5772` |
| `sse.rs` | `d74fa5e207fa4de5f47f076b1df9a78e96522e095a2cd8c087d6eebd82217634` |
| `tool_media.rs` | `496ce69210d50a689baff3b88daeb45cd015bd35b8a4e24e97a324c4a0389706` |
| `providers/codex_chat_common.rs` | `41412157670fedab83a01461a9fc43acfb9a89fbfd1f9ef786929fa3343da0c8` |
| `providers/codex_responses_sse.rs` | `2e4f27b66b51c60244e7a95270bae1ef884433d04bab5684f98fc505302d1057` |

## 依赖

`thiserror = "2.0"` 是为这批文件加的直接依赖。2.0.18 本来就在
`Cargo.lock` 与本地 registry 缓存里，因此不产生新下载，`--offline` 出包不受影响。

## 与 `cargo fmt` 的关系

两边都用 rustfmt 默认配置（双方都没有 `rustfmt.toml`），所以 `cargo fmt`
跑过之后 vendored 文件哈希不变，不会和守卫测试打架 —— 已实测。

**万一将来某个上游文件不是 rustfmt-clean 的**，`cargo fmt` 会改动它、守卫随即变红。
那时**不要**为了让测试通过就接受 fmt 的改动（那等于开始维护一份本地分叉），
应当把该文件排除在 fmt 之外，或向上游提 issue。

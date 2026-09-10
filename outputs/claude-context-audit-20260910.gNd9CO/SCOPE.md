# Claude 长上下文修复：范围与证据

日期：2026-09-10。基线：当前 `1c77a96b0ed5cc48ee053a66c8f4bd41c409f867` 加已有未提交修复，保留已有直连架构、用户改动和全部历史验收。本轮不打包、不发布、不操作本机已安装应用或真实账户。

## 已核验现状

- 用户确认：不接受 Claude 所有模型统一关闭长上下文，要求按模型能力修复。
- 代码观测：`src-tauri/src/tool_adapters/claude_desktop.rs` 的新建和多模型 profile 均写 `supports1m:false`；共享 `src/model-profiles/catalog.json` 已有 GPT/DeepSeek 上下文数值，但现存十个 Claude 条目未填写。
- 代码观测：`src-tauri/src/claude_bridge.rs::resolve_model` 只匹配完整本地别名；开启 1M 后若请求带 `[1m]`，旧实现会把未还原的别名送往上游。
- 复用依据：继承的 `src-tauri/src/claude_desktop_config.rs` 已有逐模型 `supports1m` 和大小写不敏感的尾部 `[1m]` 解析。其模块依赖完整 CC Switch 数据库，不引入第二套运行时/数据库；将相同小型解析规则放在现有 `tool_model_profile` 所有者内。
- 外部核验：[Anthropic 上下文文档](https://platform.claude.com/docs/en/build-with-claude/context-windows)，2026-09-10 查询。现存表内 Sonnet 4.6/5、Opus 4.6/4.7/4.8/5、Fable 5/5.1、Mythos 5/5.1 为 1M。Haiku 4.5 和 Sonnet 4.5 等不能按这些型号推断成 1M。
- 外部核验：[Claude Code 模型配置](https://code.claude.com/docs/en/model-config) 说明 `[1m]` 是客户端上下文标记，不是另一个上游模型 ID。[官方设置参考](https://code.claude.com/docs/en/settings-reference#modelpicker) 未提供逐行 `contextWindow` 字段；不能凭猜测添加这样的参数，也不能给混合模型列表强加全局 1M 环境变量。

## 本次目标与单一所有者

`catalog.json` 仍是静态模型能力唯一来源，`tool_model_profile` 负责读取与派生。已知窗口至少 1,000,000 tokens 才声明 `supports1m:true`；未知型号与较小窗口不声明支持。只规范明确的 `anthropic/` Claude 命名空间，不凭型号前缀猜未来模型容量。

Claude Desktop 的单模型、新多模型、重接入和已管理 profile 更新均使用该规则；校验读取要验证能力标记；不修改密钥、路由 ID、默认模型或计费分组。旧配置的 `false` 在读取检查时仍可兼容，在新写入/迁移时更新。生成选项不等于强制改动 Claude 已保存的 200K 会话或选择。

转发兼容只把已登记路由的尾部 `[1m]`/`[1M]` 标记剥离后还原原模型 ID，不对未知或相似路由做角色猜测，不改计费密钥逻辑。实际长上下文仍受中转服务上游限制，模型参考元数据不是一次收费长请求验证。

验证只使用纯函数、临时配置和已有合成回归：先复现 profile 标记错误及带 1M 后缀的别名还原失败，再验证多模型差异、未知模型、只更新自有配置、重复迁移、完整回滚、思考强度与其他消费者未回退。不会调用真实客户端启动/退出、系统凭据读写或收费模型接口。

## 治理工具限制

已完整读取 `govern-product-build` 及证据协议、架构规范，并运行仓库审计与 RU-076 review。`governance-review.json` / `.log` 报告 67 个历史当前树相等性错误，包括此前已提交的直连改造和新增审计产物，不是本次上下文测试结果。严格冻结流程仍无法走通；保留失败证据，按上层技能降级要求和用户本轮明确修复授权执行上述有限源码修复，不新造冻结哈希、重写历史证明或宣称全产品就绪。

允许修改范围：共享模型能力表、`tool_model_profile.rs`、`tool_adapters/claude_desktop.rs`、`claude_bridge.rs`，相关只读回归测试及本轮说明。Claude Code 先做只读兼容检查；没有已核验的接口就不新增全局覆盖或臆造字段。源变更与执行结果在完成后另记。

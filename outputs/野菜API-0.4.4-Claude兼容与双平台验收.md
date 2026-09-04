# 野菜API 0.4.4 Claude 兼容与双平台验收

日期：2026-09-04

## 客户端范围

- 本次只修改桌面客户端，不修改 NewAPI、服务器数据库、Nginx、DNS 或线上配置。
- Claude Code 与 Claude Desktop 不再依赖模型名称或应用版本白名单。
- 当前账户模型只要声明 `anthropic` 或 `openai` 端点能力即可进入 Claude 模型列表。
- 同时具备两种能力时优先使用原生 Anthropic Messages；仅有 OpenAI Chat 时由本机兼容层自动双向转换。
- Claude Code 与 Claude Desktop 分别使用 `127.0.0.1:15728` 和 `127.0.0.1:15729`，互不占用同一路由。
- 完整 NewAPI 密钥只写入操作系统安全存储；应用配置中只出现高熵本机令牌或安全凭据助手命令。

## 线上能力快照

2026-09-04 对 `https://yeschoy.com/api/pricing` 的只读检查结果：共 65 个模型，其中 65 个声明 OpenAI Chat、6 个声明 Anthropic Messages、6 个声明 OpenAI Responses。因此 0.4.4 的 Claude 列表不再被旧的 6 个 Anthropic 标记模型限制。

## 自动化验证

- TypeScript 类型检查：通过。
- 前端单元测试：306 项通过，0 项失败。
- Rust 原生测试：315 项通过，0 项失败；覆盖 Anthropic/OpenAI 请求转换、流式事件、工具调用、媒体、推理字段、用量和本机认证头。
- 受控验收：Claude 路由、安全边界、0.4.4 版本一致性与 Windows 工作流结构已通过本地验收。

## macOS 候选包

- 文件：`release/ru032-local/野菜API-0.4.4-macOS-universal.dmg`
- 架构：`x86_64 arm64`
- Developer ID 签名：通过。
- Apple 公证：Accepted，Submission ID `2ba90104-c869-4c51-8bbe-334ec0b84b54`。
- Staple 与 Gatekeeper：通过，来源为 `Notarized Developer ID`。
- SHA-256：`90a5dc6eafefbc64da0d7688359440004c8ce95f46ae129f59a7455f65dcdb28`

## Windows 候选包

- 目标：Windows 10/11 x64，NSIS 安装包。
- 签名状态：明确为未签名内测版，不声称拥有 Windows 发布者签名。
- 工作流在 Windows Server 2022 上重新执行类型检查、前端测试、Rust 测试和 NSIS 构建，只上传精确安装包、SHA-256 与构建回执。
- 本记录生成时，新源码尚未完成受控提交，因此对应 Windows 运行尚未触发。先前运行在任何步骤开始前被 GitHub 账户的 Billing/Spending Limit 门禁阻止；最终源码推送后必须重新触发，并以实际运行结果为准，不能把外部门禁写成构建成功。

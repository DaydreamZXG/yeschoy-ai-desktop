# 野菜API 桌面客户端：工具直连中转站改造

日期：2026-09-09
范围：客户端工具接入链路。删除本地网关，六个工具改为直连中转站；Claude Desktop 保留本地桥。不改服务端、不改计费、不发版。

## 结论

用户反馈的“Codex 一直 Reconnecting”“一键接入按钮禁用”等问题，根因不在单个 bug，而在于客户端里多了一层常驻本地网关（`codex_bridge` 1158 行 + `claude_bridge` 1489 行 + `chat_gateway` 886 行），围绕它又长出守护进程、固定端口、能力令牌、单实例保护、健康检查、退出协调。中转站本身已经原生支持这三种协议，这层网关闭环没有任何协议收益。

本次实测确认后，删除 Codex 与 Chat 两个网关，六个工具改为直连；净删除 2135 行。

## 实测证据（2026-09-09）

对 `https://yeschoy.com` 用真实 key 逐模型验证，覆盖 27 个模型 × 3 端点 + 流式 + 函数调用：

| 端点 | 结果 |
| --- | --- |
| `POST /v1/responses` | 全部 Codex 系模型（gpt-5.6-sol / 5.6-terra / 5.5 / 5.3-codex-spark / codex-auto-review / 6-astra / 5.5-openai-compact）及 glm / kimi / deepseek 系全部通过，含 SSE 流式与 `function_call_arguments.delta` |
| `POST /v1/messages` | 全部 Claude 模型（fable-5 / opus-5 / opus-4-8 / sonnet-5 / fable-5-1）通过，含流式；非 Claude 模型同样可用 |
| `POST /v1/chat/completions` | 全部通过 |

要点：

- 能力表 `supported_endpoint_types` **低估**了中转站：Codex 系模型只标 `openai`，但 `/v1/responses` 实际可用。因此直连判断不再要求 `openai-response` / `anthropic` 标记，只要有 `openai` 即可直连。
- 中转站有反探测保护：`max_tokens ≤ 8` 会被拒。客户端自检请求必须 ≥ 16。
- 用本机 codex CLI 0.146.0 以 `base_url = https://yeschoy.com/v1` + `wire_api = responses` + `env_key` 跑通真实会话。
- Codex 0.146 会主动请求 `{base_url}/models` 并期望 `{"models":[...]}`，中转站返回 OpenAI 的 `{"data":[...]}`，会记一条 `failed to refresh available models` 日志但不影响请求。客户端本地写 `model_catalog_json` 的做法必须保留。

## 改动

**直连**：`prepare_adapter` 统一取中转站 origin；`codex_desktop` / `claude_code` / `pi` / `hermes` / `openclaw` / `dsh_web` 六个适配器写入直连地址与工具级 key。

**删除**：`chat_gateway.rs` 整体删除；`codex_bridge.rs` 只保留被 Claude Desktop 复用的协议转换 helper（1158 → 119 行）；单实例插件、网关守护、固定端口、`ensure_local_gateway_ready` 等一并移除。

**安全**：Codex 配置不再写 `requires_openai_auth`。此前 base_url 是回环地址，官方 ChatGPT 凭据最多发到本机；直连后指向公网域名，必须断开这条路径，否则一旦 Codex 凭据优先级变化，用户官方凭据会被发到中转站。同时修正两处凭据误用（激活校验与 DSH 打开曾用回环令牌请求中转站，必然 401）。

**迁移**：老用户配置里 base_url 指向 `127.0.0.1`，删桥后会断。`connection_recovery::requires_gateway_migration` 会把这类连接判定为“设置已变更”，界面提供“更新接入设置”一键切换。

**保留**：Claude Desktop 仍走 `127.0.0.1:15729`，但桥已重写为**只做别名还原与转发**（1489 → 约 330 行）。原因是实测发现 Claude Desktop 只接受“看起来像 Anthropic 模型”的 route 名，客户端写入的是 `claude-sonnet-5-v<sha>` 这类确定性别名，应用请求时发的也是别名；把别名发给中转站会得到 `model_not_found`。别名到真实模型的还原必须发生在本地，桥因此保留，但不再承担协议转换、模型目录与本地观测。

随网关一起移出编译的还有 CC Switch 的 vendored 转换器模块（`proxy/providers/*` 等，仅在需要时再引入）；约 236 项属于这些模块的测试因此不再参与构建。

## 验证

- `cargo clippy --all-targets -- -D warnings`（1.95 工具链）通过。
- Rust 单元测试 487 项通过（较此前少 24 项，为随网关删除的网关测试）。
- 前端 `tsc --noEmit` 与 476 项单元测试通过。
- 本机 codex CLI 以直连配置完成真实会话。

## 接入页重做（本次一并完成）

原来的主按钮由 `applyPhase × scanPhase × session.loading × connections.loading/error × selectionReadyKey` 五个信号交叉决定标签、提示与禁用状态，存在“都选好了但按钮禁用且无任何说明”的状态（用户实拍反馈）。

- 改为**单一状态 → 单一动作**：`setupBlock` 一次算出标签、提示与点击行为。按钮只在操作进行中禁用；其余状态保持可点，点了要么执行下一步，要么展开需要修改的位置。
- 模型、计费分组、常用模型收进默认折叠的「高级设置」；缺少选择时自动展开。默认路径变成「选应用 → 点接入」。
- 线路选择器保持原有折叠，不放进「高级设置」，避免两层折叠。
- 测试从“按钮禁用”改为断言“按钮说明下一步且不提交”。

## 未完成

1. 七个工具在真机各连一次；Windows 真机验证升级迁移路径与长会话。
2. Claude Desktop 桥的别名映射仍随客户端版本（`claude-desktop-route:v1`）变化；若中转站将来支持模型别名，可再次评估彻底去桥。
3. “最近中转记录”的线路取自本次账户读取使用的 origin（服务端日志行不含线路），切换线路后旧记录会被重新标注。

## 观测改服务端（本次一并完成）

本地观测层随网关删除，改为读取服务端 `/api/log/self?p=1&page_size=100&type=2` 的消费记录：

- 按客户端为每个工具创建的 token 名称归因（名称形如 `野菜API cx-<分组哈希>-<随机后缀>`，见 `token_code`）。
- 每个工具只保留最新一条，映射为既有的 `RequestObservation` 投影；前端文案同步改为“记录来自服务端用量日志”。
- 日志行不含线路与 HTTP 状态，因此 `lineId` 取本次账户读取的 origin，`httpStatus` 为 0（界面不显示）。

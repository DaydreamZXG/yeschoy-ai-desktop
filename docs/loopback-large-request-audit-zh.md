# 本机 AI 请求大附件审计

## 问题与根因

Codex Desktop 通过 `http://127.0.0.1:15722/yeschoy/v1/responses` 发送带 PDF、图片等附件的请求。附件嵌入 JSON 后通常还会经过 Base64 膨胀。原先各本机入口采用不同限制，导致普通文字请求成功，而附件请求在尚未转发到野菜 API 前就被本机拒绝为 HTTP 413。

截图中的 `Failed to buffer the request body: length limit exceeded` 来自 Codex 本机桥原有的 2 MiB 限制，不是朋友版 OAuth 域名、模型、计费组或上游账号配置造成的。

## 入口覆盖审计

| 本机入口 | 覆盖应用 | 修改前请求上限 | 修改后请求上限 |
| --- | --- | ---: | ---: |
| Codex Responses 桥 | Codex Desktop | 2 MiB | 200 MiB（共享） |
| Claude 桥 | Claude Code、Claude Desktop | 8 MiB | 200 MiB（共享） |
| Chat Gateway | Pi、Hermes、OpenClaw、DSH web | 8 MiB | 200 MiB（共享） |
| 主代理路由 | 兼容/高级代理入口 | 200 MiB | 200 MiB（改为引用共享常量） |

所有 AI 请求入口现在引用同一个受限值 `MAX_AI_REQUEST_BODY_BYTES`，避免以后新增或修改某个应用时再次出现 2/8/200 MiB 分叉。

OAuth loopback 回调只承载短小的授权参数，不接收 AI 对话或附件，故不应套用大请求规则。

## 安全与资源审计

- 上限仍为 200 MiB，没有改成无限读取；超过上限会返回明确的 `payload_too_large` 错误。
- Codex 桥改为先校验本机令牌，再读取附件请求；未授权请求不能利用大请求占用内存。
- Claude 和 Chat Gateway 原本已在读取请求体前完成本机鉴权，此顺序保持不变。
- 请求上限与响应/流式缓冲上限已经解耦。此次只放宽客户端上传，Codex 16 MiB、Claude 8 MiB、Chat Gateway 8 MiB 等响应保护不随之放大。
- 服务仍只监听 `127.0.0.1`，没有增加局域网或公网暴露面。
- 对压缩请求不做透明解压，避免压缩炸弹；受管理的客户端继续发送普通 JSON。

## 错误语义审计

本机大请求拒绝不再被归类为“上游返回不可读响应”。HTTP 413 现在有独立结果 `payload_too_large`，提示用户减少或拆分附件。若请求已成功离开本机、随后由 CDN/NewAPI/上游返回 413，同样会被识别为容量问题，但可通过请求观测中的状态与转发阶段区分。

## 回归验证范围

- 超过旧 Codex 2 MiB 的合法 JSON 能被 Codex 请求解析层接收。
- 超过旧 Claude/Chat Gateway 8 MiB 的请求能通过共享读取层。
- 人为设置小上限时，超限请求仍会被拒绝，证明读取保持有界。
- 413 的错误码与提示文案有独立单元测试。
- 完整 Rust 测试、Clippy 和格式检查作为合入前门禁；本次不生成安装包。

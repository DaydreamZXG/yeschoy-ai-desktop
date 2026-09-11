# 修复负余额客户端授权状态

## Goal

让合法欠费账户在完成网页设备授权后正常进入桌面端登录态，并以负数展示真实可用余额；授权轮询遇到瞬时异常时必须明确提示并继续有界重试，不能静默停住。

## Background

- 后端钱包结算允许 `quota` 为负，用于记录欠费；`GET /api/user/self` 会返回该真实值。
- 当前 Rust 客户端在 `src-tauri/src/account_v2.rs:805-863` 使用非负整数解析 `quota`，负余额会使账户摘要不可用，随后被映射为 `invalid_response`。
- 当前渲染边界在 `src/account/session.ts:112-143` 和 `src/account/session.ts:354-359` 再次拒绝或隐藏负余额。
- 远端最新客户端会在轮询瞬时失败时保留 `authorization_pending`、显示 `lastError` 并继续调度下一次轮询；该恢复机制应保留，直到成功、拒绝、过期或取消。

## Requirements

- `balanceQuota` 必须接受规范的有符号十进制整数文本，并保留负值；`usedQuota`、`requestCount` 及用量计数继续只接受非负整数。
- Rust 原生层、TypeScript 投影解码和余额换算必须使用一致的余额符号语义。
- 负余额必须按现有币种和格式化逻辑显示为负金额，而不是 `—`、零或“账户数据异常”。
- 授权轮询瞬时失败或投影解码失败时，界面必须显示连接错误并自动继续轮询；成功、拒绝、过期或取消后必须结束 `authorization_pending`。
- 不修改服务端负余额语义，不截断、归零或隐藏欠费。
- 不扩大桌面会话权限，不改变令牌存储、授权码、过期或轮询协议。

## Acceptance Criteria

- [x] AC1: Rust 账户解析接受 `quota: -1`，同时仍拒绝负数 `used_quota` 和 `request_count`。
- [x] AC2: `decodeAccountProjection` 接受 `status: signed_in` 且 `balanceQuota` 为负数的完整投影，仍拒绝其他非负计数字段中的负数。
- [x] AC3: `quotaToUsd("-125000", "500000")` 返回 `-0.25`，账户页显示对应的负 USD 金额。
- [x] AC4: 从 `authorization_pending` 发起轮询后，命令拒绝或返回瞬时错误时保留授权上下文、显示错误并安排下一次轮询，最终能进入成功、拒绝、过期或取消状态。
- [x] AC5: 正余额、零余额、未登录、授权拒绝、授权过期和已登录状态保留现有行为。
- [x] AC6: 相关 Rust 单元测试、前端单元/组件测试、TypeScript 检查及客户端构建通过。

## Out of Scope

- 修改后端 `/api/user/self` 响应或将负余额截断为零。
- 新增授权协议状态、数据库字段或服务端迁移。
- 重构无关的账户、计费或工作台界面。

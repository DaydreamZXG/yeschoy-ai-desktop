# 野菜API 跨平台内测构建与后端边界记录

记录日期：2026-09-01  
客户端范围：RU-018 仅打包 RU-017 已验证运行时，不修改客户端功能。  
服务器范围：只读核对和交接建议；本次未修改 NewAPI、Nginx、数据库或线上配置。

## 结论

按当前线上公开版本 `v1.0.0-rc.27`，官方客户端第一阶段可以采用“新增桌面专用接口 + Redis 短期授权状态”，默认不新增数据库表，也不复制余额、日志、模型或支付数据。

原因是该版本已经有以下持久化能力：

- `users`：账号、余额/额度和用户分组；
- `user_sessions`：可撤销登录会话、轮换 refresh token 的摘要、失效时间和登录方式；
- `logs`：实际扣费额度、输入/输出 token、模型 ID，并可在 `other` JSON 中保存本次请求实际使用的倍率、缓存和计费参数；
- `tokens`：可限制模型、额度、分组、IP 和有效期的工具密钥；
- 现有定价/选项、充值和支付记录：继续作为唯一业务事实来源。

线上匿名只读核对结果：`https://yeschoy.com/api/status` 与 `https://api.yeschoy.com/api/status` 均返回 `v1.0.0-rc.27`；`/api/pricing` 可用；`/api/desktop/v1/bootstrap` 尚未部署，返回 404。

对应官方源码依据：

- [v1.0.0-rc.27 的 user_sessions 模型](https://github.com/QuantumNous/new-api/blob/v1.0.0-rc.27/model/user_session.go)
- [v1.0.0-rc.27 的自动迁移注册](https://github.com/QuantumNous/new-api/blob/v1.0.0-rc.27/model/main.go)
- [v1.0.0-rc.27 的账户、充值、会话和定价路由](https://github.com/QuantumNous/new-api/blob/v1.0.0-rc.27/router/api-router.go)
- [v1.0.0-rc.27 的消费计费快照字段](https://github.com/QuantumNous/new-api/blob/v1.0.0-rc.27/service/log_info_generate.go)
- [v1.0.0-rc.27 的定价投影](https://github.com/QuantumNous/new-api/blob/v1.0.0-rc.27/controller/pricing.go)

## 给服务端技术人员的最小改造

### 1. 桌面授权

建议新增以下命名空间，不让客户端直接套用浏览器 Cookie，也不把普通 NewAPI 工具密钥当作账号登录凭证：

| 接口 | 用途 | 持久化 |
| --- | --- | --- |
| `POST /api/desktop/v1/device-authorizations` | 创建设备码、用户码和网站确认地址 | Redis，5 分钟 TTL |
| `POST /api/desktop/v1/device-authorizations/token` | 客户端限速轮询；成功后一次性换取桌面会话 | Redis 原子消费 + 现有 `user_sessions` |
| `POST /api/desktop/v1/sessions/refresh` | 轮换桌面 refresh token | 现有 `user_sessions` |
| `DELETE /api/desktop/v1/sessions/current` | 撤销当前设备 | 现有 `user_sessions` |

设备码和用户码只在 Redis 保存不可逆摘要，必须有 TTL、单次消费、失败次数限制和至少 5 秒轮询间隔。用户在野菜 API 官网完成现有账号登录并明确确认设备后，服务端调用现有会话服务创建 `login_method=desktop_device` 的会话。桌面 access token 短期有效，refresh token 只返回一次并由客户端放入系统钥匙串/凭据管理器。

若 access token 需要权限范围，可把 `desktop:read`、`desktop:tool_keys` 写入签名 JWT，并根据 `login_method` 在刷新时重建；无需给 `user_sessions` 加明文密钥或新列。

### 2. 客户端数据投影

建议新增薄聚合层，不让客户端耦合 NewAPI 后台页面响应：

| 接口 | 复用事实来源 | 是否新增表 |
| --- | --- | --- |
| `GET /api/desktop/v1/bootstrap` | 线路、功能开关、最低客户端版本 | 否，现有 options/配置 |
| `GET /api/desktop/v1/me` | `users` | 否 |
| `GET /api/desktop/v1/usage/summary` | `logs` 聚合 | 否 |
| `GET /api/desktop/v1/usage/records` | `logs` | 否 |
| `GET /api/desktop/v1/models` | 模型、渠道能力、`/api/pricing` 投影 | 否 |
| `GET /api/desktop/v1/billing/comparison` | 同一批 `logs` 的官方参考成本与实际净扣费 | 否，优先使用 `logs.other` |
| `POST /api/desktop/v1/tool-keys` | `tokens`，按工具命名并限制模型/额度 | 否 |
| `POST /api/desktop/v1/tool-keys/{id}/rotate` | 原子创建新 token、停用旧 token | 否 |
| `GET /api/desktop/v1/wallet/recharge-url` | 官网钱包/充值页 | 否；支付继续在浏览器完成 |

充值第一版只返回野菜 API 自有 HTTPS 钱包地址并在系统浏览器打开。支付渠道、订单、回调和风控仍由现有 NewAPI 网页流程负责，客户端不接支付回调，不新增支付表。

### 3. 官网价与实际价

服务端负责比较，客户端只显示已校验的归一化回执：

1. 选取同一账号、同一时间段、同一批已结算消费日志；
2. `actualCny` 使用日志的实际净扣费事实，不用宣传倍率反推；
3. `officialUsd` 使用该请求记录的模型、输入/输出、缓存、音频/图片及当时适用的官方参考价格快照；
4. 按已确认固定汇率 `1 USD = 6.75 CNY` 得到 `officialCny`；
5. 返回包含/排除行、覆盖率、价格来源、价格生效时间、价格版本和逐行整数小数位金额；
6. 没有同批证据或存在不支持的计费类型时显示不可比较，不用零值填充。

当前 `logs.other` 已保存实际应用的 `model_ratio`、`group_ratio`、`completion_ratio`、缓存参数、固定模型价和分层表达式等。技术人员应补充 `official_pricing_version`、`official_source_id`、`official_effective_at` 等 JSON 键；官网参考价元数据可放入现有 options，不需要数据库列迁移。

## 什么时候才需要动数据库

默认不动。只有以下审计结果之一成立，才另开迁移：

- 实际生产库没有 `user_sessions`，且不能按当前 NewAPI 自带 AutoMigrate 补齐；
- 没有 Redis，而又必须把待确认设备码跨节点持久化，此时可加一张短寿命 `desktop_device_authorizations` 表；
- 现有消费日志关闭、清洗了 `other`，或法规/对账要求把价格证据做成不可变追加账本，此时才考虑独立 `billing_comparison_snapshots` 表；
- 未来需要独立于会话的设备资产管理、设备昵称、远程推送或合规审计，现有 `user_sessions` 元数据确实不够。

这些条件目前均未被证明。不要预先增加 `desktop_device`、`desktop_session`、余额镜像、消费镜像或支付镜像表。

## 技术改造文件建议

可在 NewAPI 侧新增 `router/desktop-router.go`、`controller/desktop_*.go`、`service/desktop_auth.go`、`service/desktop_projection.go` 和桌面 Bearer 中间件；复用现有 model 与 service。具体文件名可按技术负责人习惯调整，关键是数据库事实来源保持唯一。

改造完成后必须覆盖：设备码过期、重复确认、轮询限速、refresh 重放、会话撤销、多节点 Redis 原子性、账号禁用、工具 key 轮换回滚、同批费用对账、价格版本缺失和部分覆盖。

## 跨平台交付边界

- macOS：通用包必须包含 `x86_64` 与 `arm64` 两个主程序切片，使用现有 Developer ID 签名并公证。本机只能实际启动 Intel 切片；Apple Silicon 原生启动仍需 M 系列机器复验。
- Windows：只在私有仓库手动运行 `windows-2022` 构建，输出 x64 NSIS 内测安装器。当前无 Windows 代码签名证书，SmartScreen 警告是已知限制。
- 两个平台均不启用自动更新，不发布 GitHub Release，不表示账号、DSH 或一键配置功能已经接通。


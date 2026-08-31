# 野菜API 桌面客户端：NewAPI未来接口与改造清单

> 2026-08-31 更新：给后端技术负责人的最新执行入口是同目录《野菜API-后端技术交接-客户端接入.md》。RU-007 已实现客户端公开模型读取、接入预览与最小 bootstrap 契约检测，但未实现账户授权、发 Key 或配置写入。本页保留此前远期功能提案；本轮实际消费的 bootstrap 结构以 `contracts/desktop-bootstrap.v1.schema.json` 为准，不能直接把第 3.3 节远期字段加入这个闭合 v1 响应。服务器仍不由本仓库修改。

状态：后端改造建议，当前未部署、当前不实施
核对日期：2026-08-30
上游源码参考：NewAPI 官方 GitHub HEAD `918427d8ab41f6adaa4113d0496f1f8621855b70`

## 1. 结论

不要让桌面客户端直接拼接NewAPI现有的网页登录、Token CRUD和内部计费结构来“先跑起来”。正确做法是在未来增加一个窄权限、版本化的桌面接口层，底层复用NewAPI现有账号、模型、定价、Token和日志能力，但只暴露官方客户端真正需要的动作。

当前 RU-005 只完成客户端候选版和这份未来交接建议，不修改 NewAPI、服务器、Nginx、数据库、DNS 或任何线上配置。客户端也不会把下文提案当成已经部署的能力。

## 2. 现有源码观察与上线前核验

固定到上述官方 GitHub 提交后，已经观察到：

| 现有路由 | 能力 | 对桌面V1的判断 |
|---|---|---|
| `GET /api/pricing` | 模型、倍率、计费表达式、价格版本 | 可复用内部数据源；不建议客户端自行解释所有计费表达式 |
| `GET /api/models` | 登录用户模型列表 | 可作为模型可见性证据之一 |
| `GET /api/user/self` | 当前用户资料与额度 | 可复用内部查询，但桌面DTO应更窄 |
| `GET /api/user/models` | 当前用户模型 | 可复用内部查询 |
| `/api/user/sessions` | 网站登录会话管理 | 不是桌面设备授权的现成替代品 |
| `/api/token` CRUD及Key读取 | 用户Token管理 | 权限过宽，不宜直接交给官方客户端 |
| `GET /api/log/self`、`/api/log/self/stat` | 用户用量日志与统计 | 可复用查询，但需补设备/工具归因和稳定DTO |
| `/api/user/topup/*`及支付路由 | 充值与支付 | V1只打开第一方钱包网页，不在客户端复制支付 |

源码入口：[API路由](https://github.com/QuantumNous/new-api/blob/918427d8ab41f6adaa4113d0496f1f8621855b70/router/api-router.go)、[认证说明](https://github.com/QuantumNous/new-api/blob/918427d8ab41f6adaa4113d0496f1f8621855b70/docs/authentication.md)、[价格接口](https://github.com/QuantumNous/new-api/blob/918427d8ab41f6adaa4113d0496f1f8621855b70/controller/pricing.go)、[价格结构](https://github.com/QuantumNous/new-api/blob/918427d8ab41f6adaa4113d0496f1f8621855b70/model/pricing.go)

官方认证说明中的网页登录会话是浏览器契约：15 分钟 access token 放在浏览器内存，refresh token 通过 `HttpOnly`、`SameSite=Strict` Cookie 轮换。这不是可以直接复制给桌面端的设备授权契约；客户端不会读取网站 Cookie，也不会把浏览器 access token 当长期桌面凭证。

### 2.1 2026-08-30 线上只读核验结果

本次只发送了不带凭证、不会写数据的请求，观察到：

| 地址与路径 | 只读结果 | 能证明什么 |
|---|---|---|
| `https://yeschoy.com/api/status` | 系统名“野菜API”，版本 `v1.0.0-rc.27` | 大陆优化入口公开状态 |
| `https://api.yeschoy.com/api/status` | 同样为“野菜API”，版本 `v1.0.0-rc.27` | 全球加速入口当前指向同一站点语义 |
| `/api/desktop/v1/bootstrap` | HTTP 404 | 提议的桌面命名空间当前未部署 |
| `/api/user/self`（未登录） | HTTP 401 | 现有用户接口受登录保护；没有把它当桌面授权替代品 |
| `/api/pricing`（公开读取） | 37 个模型，`pricing_version` 为 `a42d372ccf0b5dd13ecf71203521f9d2` | 当前存在公开价格结构，但不是稳定的“当前账号官网价与实际价”桌面投影 |

这些结果只证明核验当时的公开行为，不证明部署提交与官方 GitHub HEAD 完全一致，也不冻结模型数量或价格版本。未来动后端前仍需确认：线上提交、私有定制分支、反向代理前缀、Cookie/Session 行为、用户分组、Token 表差异、日志保留、支付插件和错误响应格式。

## 3. 建议的新接口边界

2026-08-31 只读审计补充：实际镜像标注提交为 `eb48396d5fe97d27772d0cd5e3ca8aa5caa4f3e9`，不是本页先前参考的上游 HEAD。现有 `pricing_version` 是源码兼容常量，不是随改价变化的数据版本；后端当前显示汇率 1 与官网比较汇率 6.75 用途不同。新价格投影必须提供真正的内容版本和明确金额单位。公开模型目录不代表当前账号权限，也不能凭分组说明文字推断流式兼容性。具体证据与交付验收见最新技术交接文档。

统一命名空间建议为 `/api/desktop/v1`。以下路径、字段和状态码全部都是**未来接口提案**，当前未部署，也不是当前已存在的 NewAPI 能力。

### 3.1 设备授权

#### `POST /api/desktop/v1/device-authorizations`

匿名申请一次性授权。请求包含：

- `client_id`: 固定官方客户端标识。
- `app_version`、`platform`、`arch`。
- `device_name`：用户可识别的名称，服务端限制长度并转义。
- 可选设备绑定公钥/挑战值，用于降低设备码被截获后的冒用风险。

返回：

- `device_code`：只给客户端，不能放进浏览器URL。
- `user_code`：可显示给用户。
- `verification_uri`和可选`verification_uri_complete`。
- `expires_in: 600`。
- `poll_interval`：由服务端控制，具体数值待容量测试。
- `authorization_id`和`request_id`。

#### 第一方网站授权页

建议网页路径：`/desktop/authorize`。网站使用现有登录方式；未登录先登录，再展示设备名称、应用名、权限和“允许/拒绝”。URL最多携带非秘密的`user_code`，不得携带账号令牌、网站Cookie、relay key或最终桌面会话。

#### `POST /api/desktop/v1/device-authorizations/token`

客户端轮询。请求体包含`device_code`和可选设备绑定证明。明确状态：

- `authorization_pending`
- `slow_down`
- `approved`
- `access_denied`
- `expired_token`
- `already_used`

批准后只在响应体返回：

- 1小时访问令牌。
- 设备会话刷新凭证。
- `device_id`、会话有效期和允许scope。

设备码单次使用，响应设置`Cache-Control: no-store`，秘密不进入URL和日志。

### 3.2 桌面会话

| 提案接口 | 作用 |
|---|---|
| `POST /api/desktop/v1/session/refresh` | 刷新1小时访问令牌；执行30天不活跃和90天绝对期限 |
| `DELETE /api/desktop/v1/session` | 退出当前设备会话，不删除工具Key |
| `GET /api/desktop/v1/session` | 返回当前设备、授权时间、最后活动、重新授权时间和scope |

桌面scope固定为：

- `profile:read`
- `balance:read`
- `usage:read`
- `models:read`
- `pricing:read`
- `tool_keys:self:manage`
- `session:self:revoke`

不授予改密码/邮箱、支付数据、分组、管理后台、其他设备或其他设备Key权限。网站端仍提供设备列表与撤销；撤销设备时级联撤销该设备全部工具Key。

### 3.3 启动清单

#### `GET /api/desktop/v1/bootstrap`

一次返回稳定、低敏感的启动信息：

- 服务端时间、`schema_version`。
- 当前客户端是否支持、`minimum_client_version`、是否强制升级。
- 两条线路的稳定ID、展示名、根地址和状态；V1两条线路共用账号/Key/模型/价格。
- 模型目录版本和下载方式。
- 价格版本、用量数据更新时间。
- 钱包网页URL、隐私政策URL、服务条款URL、状态页URL。
- 功能开关与维护公告。

根地址只用于诊断和适配器派生；最终工具Base URL仍由客户端的工具协议适配器生成并在写入前展示。

支持`ETag`/`If-None-Match`，但缓存过期时客户端必须明确显示，不得拿旧余额或旧授权伪装在线。

### 3.4 当前账号摘要

#### `GET /api/desktop/v1/me`

只返回桌面首页需要的数据，例如：显示名/掩码账号、余额、已用额度、账号状态、默认分组的展示说明、数据时间。不得返回密码字段、支付资料、邮箱修改能力或管理角色细节。

#### `GET /api/desktop/v1/me/usage-summary`

支持今日、本月和指定时间范围，返回请求数、输入/输出/缓存量、计费金额、货币/额度单位和数据时间。

#### `GET /api/desktop/v1/me/usage`

游标分页，允许按`tool_id`、`model_id`、`tool_key_id`、时间和结果过滤。明细返回稳定字段：时间、工具、模型、token分类、计费金额、结果码、请求ID掩码。不得返回用户提示词、响应或上游敏感正文。

如果当前NewAPI日志无法可靠区分线路，不要先造一个`line_id`；应先决定由网关、请求头或Key元数据怎样产生可信归因。

### 3.5 模型目录与价格

#### `GET /api/desktop/v1/models`

返回当前账号实际可见模型ID及服务端可验证信息：可用状态、支持端点类型、分组限制、维护/下线状态。兼容哪一个桌面工具由版本化野菜目录补充，不能仅凭模型名称猜。

#### `GET /api/desktop/v1/model-catalog`

返回野菜维护的说明性元数据：

- `catalog_version`、发布时间、最低客户端版本。
- 精确模型ID或明确的受控匹配规则。
- Claude Code/Codex/OpenCode/Pi/DSH兼容矩阵及最低/最高已验证版本。
- 推荐、最快、最强、经济等标签及中文说明。
- 官网价格来源URL、来源币种、核对时间。

目录必须签名或通过受信TLS接口和版本校验下发；服务端新模型未收录时可以显示真实ID，但兼容性和推荐保持“尚未验证”。

#### `GET /api/desktop/v1/pricing`

建议由服务端直接生成适合显示的“官网基准价 + 当前用户实际价”投影，而不是让客户端执行`billing_expr`：

```json
{
  "schema_version": 1,
  "pricing_version": "...",
  "effective_at": "...",
  "comparison_fx": {
    "usd_to_cny": "6.75",
    "purpose": "display_only",
    "version": "..."
  },
  "models": [
    {
      "model_id": "...",
      "billing_mode": "token|request|media|tiered",
      "official": { "currency": "USD", "components": [] },
      "actual": { "currency": "CNY", "components": [] },
      "comparable": true,
      "incomparable_reason": null
    }
  ]
}
```

`components`分开表示输入、输出、缓存读、缓存写、图片、音频、按次、长上下文和时段阶梯，并保留原生单位。服务端始终从NewAPI同一份基础价格、分组和特殊计费规则派生，不建立第二张人工数字价表。

#### `POST /api/desktop/v1/pricing/quote`

供成本计算器使用。客户端提交模型、计费维度和预计用量，服务端复用真实计费表达式给出官网估算、野菜估算、适用阶梯和`estimate_only: true`。这可以避免客户端实现不同版本的计费解释器。

长期硬规则：`model_ratio`、`model_price`和基础`billing_expr`表示厂商官网基准；折扣只进入独立分组倍率或特殊规则。接口应返回价格版本和来源状态，以便出现脏数据时停止展示“官网价”结论。

### 3.6 当前设备的工具Key

桌面客户端不直接调用通用`/api/token` CRUD。建议增加只操作“当前会话设备”的窄接口，服务端内部仍可复用NewAPI Token模型。

#### `PUT /api/desktop/v1/tool-keys/{tool_id}`

确保当前设备存在该工具的唯一Key。`tool_id`仅允许五个固定枚举；相同账号、设备、工具重复调用必须幂等，线路和模型变化不新建Key。

请求携带`Idempotency-Key`。返回Key元数据；完整秘密只在首次创建的成功响应中返回一次。若Key已存在而本机秘密丢失，接口不再次明文揭示，客户端应执行安全轮换。

#### `GET /api/desktop/v1/tool-keys`

只返回当前设备五类Key的ID、工具、掩码、创建时间、最后使用、状态和用量摘要，不返回完整Key。

#### 两阶段轮换

| 提案接口 | 作用 |
|---|---|
| `POST /api/desktop/v1/tool-keys/{tool_id}/rotations` | 创建替代Key并返回一次秘密；旧Key仍有效 |
| `POST /api/desktop/v1/tool-key-rotations/{rotation_id}/commit` | 客户端写入并验证成功后撤销旧Key |
| `POST /api/desktop/v1/tool-key-rotations/{rotation_id}/abort` | 写入失败，撤销替代Key并保留旧Key |

轮换过期时间、自动清理和崩溃恢复策略要在容量与安全评审后确定，本文不编造数字。所有操作必须有审计事件和幂等语义。

#### `DELETE /api/desktop/v1/tool-keys/{tool_id}`

撤销当前设备该工具Key。移除本地配置和撤销远端Key是两个独立用户决定；客户端不能把两件事合并成无提示的一次删除。

### 3.7 充值

V1充值按钮打开服务端`bootstrap`返回的第一方钱包URL。浏览器若未登录，执行网站正常登录。桌面access token、refresh token和工具Key均不得放入URL；客户端不处理银行卡、支付宝或支付回调。

如果以后确实需要“从客户端无感进入网站”，应设计一次性、短时、只用于建立网站会话的handoff code，并经过单独安全评审，不能把桌面Bearer token直接拼到链接里。

### 3.8 遥测接收

优先将遥测作为与账号接口隔离的第一方接收服务；若暂时落在NewAPI中，也使用独立路由、存储、权限和保留任务。

#### `POST /api/desktop/v1/telemetry/events`

- 不携带账号access token、网站Cookie、设备ID或工具Key。
- 使用独立、每30天轮换的随机遥测ID。
- 服务端按版本化schema拒绝未知字段和超长字段。
- 只接受固定事件名、阶段、结果、错误码和已脱敏栈帧。
- 限流、防重放、批量大小和滥用防护在压测后确定。
- 普通事件30天、脱敏崩溃90天、不可逆聚合12个月；关闭遥测后本地清队列，并提供原始历史删除请求入口。

客户端是公开软件，不能靠内置“秘密API Key”证明事件可信；防滥用应依赖schema、限流、版本、签名安装包渠道信号和异常检测，而不是把共享秘密写进客户端。

## 4. 数据模型改造建议

表名只是提案，最终应遵循你们NewAPI分支现有命名：

| 实体 | 关键字段 | 关键约束 |
|---|---|---|
| `desktop_device` | user、public_id、display_name、platform、created/last_seen/revoked/reauth_at | public_id随机；网站可见；撤销级联 |
| `desktop_device_session` | device、refresh凭证哈希、scope、last_used、absolute_expiry、revoked | 只存refresh秘密哈希或等价不可逆验证材料 |
| `desktop_device_authorization` | device_code哈希、user_code哈希、状态、过期、poll信息 | 单次使用；过期清理；敏感值不写日志 |
| `desktop_tool_key` | device、tool_id、NewAPI token_id、状态、创建/撤销 | `(device_id, tool_id)`对活动Key唯一 |
| `desktop_tool_key_rotation` | old/new token、状态、创建/提交/中止 | 提交前旧Key有效；操作幂等 |
| `desktop_catalog_version` | version、payload哈希/签名、发布时间、兼容客户端 | 元数据版本化，不重复数字定价 |
| `desktop_audit_event` | actor_device、动作、对象ID、结果、request_id、时间 | 不存完整秘密和配置正文 |

遥测表或存储不应保留可关联到`user_id`/`device_id`的外键。

## 5. 统一响应和错误契约

桌面接口应有稳定封装：

```json
{
  "success": false,
  "error": {
    "code": "MODEL_NOT_AVAILABLE",
    "message_key": "desktop.model_not_available",
    "retryable": false,
    "request_id": "...",
    "safe_details": {}
  }
}
```

要求：

- 客户端用`code`做行为判断，不解析中文错误文字。
- `safe_details`经过白名单，不回显Authorization、Key、Cookie、配置或上游原文。
- 所有写操作支持请求ID和幂等键。
- 429/5xx明确`retryable`和可选`Retry-After`。
- 设备撤销、会话过期、余额不足、模型不可用、Key失效、版本过低必须是不同错误码。
- 时间、金额、比例和token数使用不会引入浮点歧义的序列化约定；具体格式在契约测试中冻结。

## 6. 服务端安全边界

- 桌面会话使用独立中间件和scope，不复用拥有广泛网页权限的通用Session作为长期API令牌。
- access token短时；refresh凭证轮换或采用等价的重放检测机制。
- 设备码、refresh凭证、工具Key和轮换新Key不进入URL、普通日志、遥测或错误正文。
- 授权、轮换、撤销、Key创建和删除写审计事件，但只记对象ID和结果。
- 设备撤销事务必须覆盖会话与该设备所有工具Key；失败时不能返回“撤销成功”。
- 工具Key服务端权限只允许模型调用，不允许访问桌面账号接口。
- 桌面access token不得用作模型调用Key，工具Key也不得刷新桌面会话。
- CORS、CSRF、Origin检查分别按系统浏览器网页授权与原生客户端API场景配置，不能简单全开。
- 任何客户端内置密钥都视为可提取，不作为服务端信任根。

## 7. 更新服务不应放进NewAPI核心权限

Tauri更新建议使用第一方静态下载源/CDN，NewAPI的`bootstrap`只告诉客户端更新通道或最低版本。更新清单和安装包由野菜更新私钥签名，客户端只嵌入公钥；签名私钥不得存入NewAPI数据库或在线管理界面。

发布前必须替换fork中所有CC Switch更新URL、公钥、深链和包身份。即使HTTPS或CDN账号被攻破，客户端仍必须因签名不匹配而拒绝安装。

## 8. 分阶段改造顺序

### 阶段0：只读核验，不改线上

- 确认线上NewAPI提交、分支差异、路由、DTO、认证、分组、定价和日志结构。
- 用测试账号获取脱敏响应样本并形成OpenAPI/JSON Schema草案。
- 确认两条线路是否确实指向同一账号、Key、模型和计费状态。

### 阶段1：设备授权和窄权限会话

- 新表、中间件、scope、网站授权页、撤销级联。
- 先在测试环境完成过期、拒绝、重放、轮询限流和撤销测试。

### 阶段2：设备工具Key

- 用NewAPI内部Token服务封装五类Key的幂等创建、一次秘密返回、两阶段轮换和撤销。
- 补工具/设备归因和审计。

### 阶段3：桌面只读投影

- `bootstrap`、`me`、模型、价格、用量稳定DTO。
- 价格投影与实际账单做样本对账，复杂表达式由服务端执行。

### 阶段4：模型目录与运营元数据

- 把客户端随包目录迁移到签名、版本化接口。
- 建立发布、回滚、来源核对和价格版本联动流程。

### 阶段5：遥测

- 独立接收与存储边界、schema白名单、删除流程、保留任务和告警。
- 上线前用合成事件证明无法上传禁止字段。

### 阶段6：灰度与正式启用

- 用服务端feature flag只允许内部账号，再逐步扩大。
- 老客户端、最低版本、接口降级和紧急关闭均有演练。
- 最后才让正式客户端连接生产，不以mock成功代替真实契约验证。

## 9. NewAPI未来改动的验收清单

- [ ] 桌面设备授权不接触密码和网站Cookie，设备码10分钟、单次使用。
- [ ] access token 1小时，30天不活跃和90天绝对重新授权生效。
- [ ] 桌面scope无法改账号、支付、分组、其他设备或管理员数据。
- [ ] 每个账号 × 设备 × 工具只有一个活动Key，线路/模型切换不重复创建。
- [ ] Key轮换在客户端验证成功前不撤销旧Key。
- [ ] 撤销设备会级联撤销其全部工具Key。
- [ ] 客户端不执行服务端`billing_expr`即可得到基准价和实际价。
- [ ] 基准价确实来自NewAPI受控官网价格字段，折扣没有污染基准字段。
- [ ] 用量可按工具Key和模型查询，无法可靠归因的维度不返回假数据。
- [ ] 所有秘密只在必要响应体出现，不进URL、日志、遥测和错误正文。
- [ ] 充值只走第一方网页，客户端不接触支付资料。
- [ ] 遥测与账号身份隔离，保留和删除任务可验证。
- [ ] 更新签名私钥不在NewAPI或客户端，客户端拒绝未签名/错签更新。
- [ ] 所有接口有版本、错误码、幂等、契约测试和灰度开关。

## 10. 尚需确认后才能冻结的后端问题

1. 你们线上NewAPI具体版本和私有定制差异。
2. 当前登录、Session Cookie、Token存储与多节点部署方式。
3. NewAPI现有Token是否可支持一次秘密返回，还是需要迁移存储模型。
4. 线路归因在网关、请求头还是Key维度实现。
5. 复杂计费表达式生成“官网价/实际价”投影的统一服务端函数。
6. 数据库类型、迁移窗口、回滚办法和灰度环境。
7. 遥测是否单独部署，以及运营、隐私和删除请求的负责人。
8. 更新清单/CDN、签名私钥、Apple和Windows发布凭据的保管人。

这些问题不阻止继续完善客户端离线架构和界面原型，但会阻止完整登录、自动发Key、计费对比、正式遥测和生产发布。

## 11. RU-005 客户端依赖交接矩阵

这张表是客户端接真实后端前的最小交接入口，不表示接口已经存在：

| 客户端页面或能力 | 未来服务端接口 | 服务端必须给出的真相 | 当前候选版行为 |
|---|---|---|---|
| 启动与线路 | `GET /api/desktop/v1/bootstrap` | schema、最低客户端版本、两条线路、功能开关、第一方链接和数据版本 | 使用已确认的两条编译线路；不请求 bootstrap |
| 第一方登录 | 设备授权申请、授权页、token 轮询、session refresh/revoke | 单次设备码、明确 pending/deny/expire/used、窄 scope、撤销级联 | 只显示 backend_upgrade_required，不开放登录按钮 |
| 账户概览 | `GET /api/desktop/v1/me` | 掩码账号、余额、状态、服务端时间 | 全部显示不可用，不显示 0 或样例 |
| 用量 | usage-summary 与游标 usage | 时间范围、工具、模型、token 分类、金额、数据时间、稳定请求 ID | 今日与本月用量不可用 |
| 模型 | models 与签名 model-catalog | 当前账号真实模型 ID、可用状态、兼容性来源、目录版本 | 模型 ID 步骤保持不可用，不猜默认模型 |
| 价格 | pricing 与 pricing/quote | 同一 NewAPI 基准价、当前账号实际价、计费维度、版本、固定展示汇率 6.75 | 官网价与实际价均不显示数字 |
| 工具 Key | 当前设备五类 tool-key 的幂等创建、轮换、提交、中止和撤销 | 一次秘密、设备×工具唯一、两阶段轮换、审计与安全错误码 | 不读取、不获取、不保存任何 Key |
| 精确版本支持 | 签名 compatibility catalog | 工具、精确版本、平台、profile、适配器、证据版本、撤回状态 | 所有发现结果只读；DSH 继续 abstain |
| 遥测 | 隔离的事件接收与删除边界 | 固定 schema、无账号关联、同意、限流、保留、删除、告警 | 完全不上传 |
| 更新 | NewAPI 只提供最低版本或通道提示；安装包与清单走独立签名下载源 | 最低版本、通道；更新私钥不在 NewAPI | 运行时更新器和更新产物都关闭 |

### 11.1 联调前必须交付给客户端的材料

- 固定测试环境 Base URL、服务端提交、OpenAPI/JSON Schema、错误码和脱敏样例。
- 一组普通测试账号及不同用户组、余额、模型可见性和价格规则的测试夹具；不要提供生产账号或生产 Key。
- 设备授权的允许、拒绝、过期、slow_down、already_used、撤销和刷新失败夹具。
- 模型与价格投影对 NewAPI 真实账单的对账证据，尤其是缓存、阶梯、按次和不可比价格。
- 五工具 Key 的创建幂等、秘密丢失、轮换提交、中止、设备撤销和并发冲突夹具。
- 兼容目录签名公钥、版本、撤回与过期策略；DSH 必须带精确 web profile 证据才能从只读变为可配置。
- 遥测若计划启用，先给字段白名单、同意、保留、删除和合成敏感字段拒绝测试；没有这些材料客户端继续关闭。

### 11.2 客户端接入顺序

1. 先只接测试环境 `bootstrap` 与设备授权，不动配置写入。
2. 接 `me`、usage、models、pricing 的只读投影，并验证所有空、部分、过期和错误状态。
3. 接 OS 凭据库与当前设备 tool-key，但仍不向第三方工具写配置。
4. 下发并验证签名精确版本目录，逐工具开启事务化适配器。
5. 完成临时加密回滚快照、外部变更检查、写入、读回、探测、失败回滚与清理后，才开放“应用”。
6. 遥测与自动更新分别经过独立安全验收，不能作为登录或配置联调的顺手附加项。

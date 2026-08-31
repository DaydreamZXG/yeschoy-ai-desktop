# 野菜API：后端技术交接与客户端接入边界

核对日期：2026-08-31。执行分工：本仓库只改桌面客户端；服务器代码、数据库迁移和部署由技术负责人处理。

## 1. 先看这页：已经做了什么，还缺什么

| 能力 | 本轮客户端实际实现 | 服务端后续职责 |
|---|---|---|
| 公开模型目录 | 用户点击后读取真实分组、模型 ID、接口声明、计费类型 | 当前 `/api/status`、`/api/pricing` 已可用，无须为了本轮客户端改动它们 |
| 工具接入预览 | 工具、线路、分组、模型独立选择；Claude Code 检查 `anthropic`，Codex 检查 `openai-response`，OpenCode/Pi 的当前预览配置检查 `openai`；DSH 不猜适配方式 | 下发按账号、分组、模型、协议、流式模式、工具精确版本核验的可用性 |
| 后端接口检测 | 读取 `GET /api/desktop/v1/bootstrap`，区分未部署、结构不兼容、网络错误、已识别契约 | 按下面的 JSON Schema 实现最小能力握手；未实现的能力必须为 false |
| 网站授权、余额、用量、实际价格 | 仍未启用；公开目录不当作当前账号结果 | 实现下文账号授权及只读投影；交付测试环境和脱敏样例后，客户端再接这些能力 |
| 工具 Key、一键写配置 | 仍未启用；不读取、领取、保存或写出 Key | 实现设备绑定 Key 的窄权限服务；客户端的系统凭据库和事务写入仍需单独完成验收 |

**不要把“客户端已识别 bootstrap”当作整套功能已接通。**本轮只实现公开读取、接入预览和能力握手检测，没有完成桌面会话、账户查询或配置落盘。后端补齐后仍需客户端联调，不会仅靠把能力标记改成 true 就自动启用登录。

本轮不登录生产业务账号、不生成临时管理凭证、不创建测试 Key、不调用付费模型；没有服务器修改或部署脚本。

## 2. 实际部署基线，不要拿上游最新 HEAD 直接替换

只读审计确认的镜像标签为 `calciumion/new-api:latest`，版本 `v1.0.0-rc.27`。镜像元数据标注源码提交：

`eb48396d5fe97d27772d0cd5e3ca8aa5caa4f3e9`

镜像摘要：`sha256:60e9c0b709d8e0d4fd768977b8992b64b90f6cbcceb02c85b54f0a8eba8b2610`。

- 正式 NewAPI 使用 PostgreSQL 15。已有 `users`、`tokens`、`logs`、`user_sessions` 等表；本次未发现桌面设备专用表。
- 大陆线路：`https://yeschoy.com`；全球线路：`https://api.yeschoy.com`。两者公开状态返回相同版本和启动时间，所查服务器反代最终指向主 NewAPI 服务。认证跨域、撤销一致性仍需正式联调，不以公开状态相同代替证明。
- `/api/desktop/v1/bootstrap` 在只读核验时为 404；匿名 `/api/user/self`、`/api/user/models`、`/api/log/self/stat` 为 401。
- 公开目录当时有 33 个模型，其中 16 个带阶梯表达式。数量会变，不写死，也不等于任一登录账号的完整模型范围。
- 网页钱包对应当前源码的 `/wallet/` 路由。旧 `/console/topup` 返回 SPA HTML 的 HTTP 200 不能证明那个页面仍存在。

证据入口：[部署提交的路由](https://github.com/QuantumNous/new-api/blob/eb48396d5fe97d27772d0cd5e3ca8aa5caa4f3e9/router/api-router.go)、[鉴权说明](https://github.com/QuantumNous/new-api/blob/eb48396d5fe97d27772d0cd5e3ca8aa5caa4f3e9/docs/authentication.md)、[价格控制器](https://github.com/QuantumNous/new-api/blob/eb48396d5fe97d27772d0cd5e3ca8aa5caa4f3e9/controller/pricing.go)、[价格结构](https://github.com/QuantumNous/new-api/blob/eb48396d5fe97d27772d0cd5e3ca8aa5caa4f3e9/model/pricing.go)、[钱包路由](https://github.com/QuantumNous/new-api/blob/eb48396d5fe97d27772d0cd5e3ca8aa5caa4f3e9/web/src/routes/_authenticated/wallet/index.tsx)。

认证后的余额/用量 DTO 本次仅对照路由、源码及数据库结构，未以真实业务账号调用。上线前需重新核对实际部署提交和测试账号响应。

## 3. 现在客户端实际发出的请求

仅在用户点击“读取模型目录”后，Rust 端对所选固定线路执行以下 GET：

1. `/api/status`：系统名称、版本、后台显示汇率。
2. `/api/pricing`：公开分组说明、分组倍率、模型 ID、模型所属分组、接口声明、计费类型。
3. `/api/desktop/v1/bootstrap`：未来桌面接口的契约识别。

不发送 Authorization、Cookie 或用户资料；不接受前端传入 URL、请求头或请求体；不跟随重定向、不使用代理环境变量。前端仍保持只连接 Tauri IPC 的 CSP。

本地防御限制：每个 HTTP 请求最多 15 秒、连接最多 5 秒、每个响应最多 2 MiB；每个应用进程同时只接受一轮读取。最多 128 个分组、2048 个模型；超限明确报错，不截断成“完整目录”。这是客户端安全边界，不是本站容量或速度承诺。需要调整时修改客户端契约、测试及限制说明。

没有持久缓存、不使用现有 `pricing_version` 判定新旧，不自动重试。刷新、切换线路或离开页面会清除/失效旧结果；观察时间是客户端完成时间，不是服务端生效时间。

下列内容不会传给界面：原始错误响应、计费表达式、模型价格数值、密钥字段、远端提供的跳转 URL。分组说明只按普通文本显示，**不解析中文描述来决定协议或流式兼容性**。

## 4. 最小握手：本轮已经消费的唯一新接口契约

`GET /api/desktop/v1/bootstrap`，匿名读取，HTTP 200，`Content-Type: application/json`。

机器契约：`contracts/desktop-bootstrap.v1.schema.json`。

正例：`contracts/fixtures/desktop-bootstrap/recognized.json`；不兼容例：同目录 `incompatible.json`。

```json
{
  "success": true,
  "data": {
    "schema_version": 1,
    "service": "yeschoy-desktop",
    "contract_id": "desktop-bootstrap-v1",
    "minimum_client_version": "0.1.0",
    "capabilities": {
      "device_authorization": false,
      "account_read": false,
      "usage_read": false,
      "models_read": false,
      "pricing_read": false,
      "tool_keys_manage": false
    }
  }
}
```

规则：

- 示例全 false 是“只实现握手”的合法状态；不能复制成全 true 冒充功能完成。
- `minimum_client_version` 是无前导零的三段正式版本号；高于当前客户端版本时客户端拒绝识别为可用契约。
- v1 是闭合结构，必须逐字段匹配。额外字段、缺失字段、未知能力或新 schema 版本都按不兼容处理。
- 404 表示尚未部署；HTML、重定向、5xx、超时不伪装成授权成功。
- 不在这个匿名接口中返回账号、Key、Cookie、设备码、刷新凭证或后台配置。
- 现阶段不把目录、价格、钱包链接塞入这个闭合响应。将来新增字段先升级契约并同步客户端，而不是直接在生产扩充 v1。
- “契约已识别”只代表格式与最低版本满足，客户端仍不创建会话、不启用账户与应用配置按钮。

## 5. 技术负责人待改清单：按依赖实施

下列接口是后续实现要求，**不是本轮已经完成的客户端调用器**。具体 DTO、错误码、幂等与迁移由双方在测试环境联调前确认；不要直接把示例当生产实现。

### P0-A：第一方网站授权与桌面设备会话

- `POST /api/desktop/v1/device-authorizations`：申请设备码，返回用户码、第一方授权页、过期时间和轮询间隔。
- 第一方 `/desktop/authorize` 网页：复用网站登录，显示设备与权限，用户明确允许/拒绝。不读取客户端密码，不让桌面读取网站 Cookie。
- `POST /api/desktop/v1/device-authorizations/token`：处理 pending、slow_down、approved、denied、expired、already_used；设备码单次使用。
- `POST /api/desktop/v1/session/refresh`：刷新独立桌面会话；`DELETE /api/desktop/v1/session`：退出当前设备。
- 复用现有用户身份和鉴权失效机制，但不直接把网页 JWT/PAT 包装成永久桌面凭证。当前网页 JWT 是 15 分钟，Refresh Cookie 是网页边界，不是桌面设备授权。
- 按既定桌面需求：访问令牌 1 小时、30 天不活跃和 90 天绝对期限；设备码 10 分钟。若拟调整，先列出原因和兼容影响，不改成隐式默认值。
- 桌面 scope 限制为资料、余额、用量、模型和价格读取，以及当前设备工具 Key 管理、当前设备退出；禁止改账号安全设置、支付配置、其他设备和管理后台。
- 设备列表与撤销在网站提供；设备撤销必须覆盖其会话与工具 Key，记录不含秘密的审计事件。

验收：普通账号授权、拒绝、过期、轮询限流、设备码重放、刷新重放、跨账号和跨设备访问、密码变更后的失效均有测试；秘密不进入 URL、日志和遥测。

### P0-B：账户、真实模型与价格的只读投影

| 新接口建议 | 可复用的 NewAPI 数据 | 必须补齐的稳定语义 |
|---|---|---|
| `GET /api/desktop/v1/me` | `/api/user/self`、users | 掩码账号、余额、已用额度、状态、货币/额度单位、数据时间；不携带支付资料和管理能力 |
| `GET /api/desktop/v1/me/usage-summary` | `/api/log/self/stat`、logs | 今日/月/时间范围、请求数、输入/输出/缓存量、金额与时间；不要把现有 rpm/tpm 当请求总数 |
| `GET /api/desktop/v1/me/usage` | `/api/log/self` | 分页、模型、工具/Key 归因、金额、结果、时间；不返回提示词、响应正文或管理员日志信息 |
| `GET /api/desktop/v1/models` | `/api/user/models?group=...`、abilities | 当前账号 × 选定分组的真实可用模型，而不是匿名目录 |
| `GET /api/desktop/v1/model-catalog` | models 元数据与受控兼容目录 | 工具精确版本、平台、协议、stream/non-stream 限制、证据版本、推荐依据、撤回状态；未知保持未知 |
| `GET /api/desktop/v1/pricing` | 基础价格、分组及用户特殊规则 | 官网基准价、账号实际价、原生计费单位、是否可比、真实内容版本及生效时间 |
| `POST /api/desktop/v1/pricing/quote` | 现有计费引擎 | 复杂表达式/阶梯由服务端执行；返回估算标识、适用条件及版本，不让客户端另写计费引擎 |

四个必查问题：

1. **线路 ≠ 分组。** 当前有分组仅支持流式 Chat。模型接口声明是模型级信息，不足以证明某分组可用的所有协议；需要结构化返回 `group_id + model_id + endpoint_type + stream_mode` 的限制。没有证据不能按名字猜 Claude/Codex 兼容性。
2. **公开价 ≠ 当前账号实际价。** 已部署价格控制器会结合用户组应用 `GroupGroupRatio` 等特殊规则。匿名倍率不能代替该账号的最终倍率。
3. **汇率与账本单位分离。** 已观察到后台 `USDExchangeRate=1`、人民币显示、`quota_per_unit=500000`；官网价比较约定固定汇率 6.75。必须返回明确的单位和换算用途，不能拿 6.75 乘本站余额。对普通价、缓存、阶梯和充值样例做账单对账后才开放价格比较。
4. **`pricing_version` 不是改价版本。** 当前价格控制器的顶层值和 model/pricing 的兼容标记是源码常量。请新建真正随影响价格的基础价、分组/特殊规则和表达式变化的内容版本；不能用这些常量做长期缓存或对账凭据。

验收：两个普通测试账号、不同组/特殊折扣、无模型/被禁用模型、余额不足、空日志、分页和跨账号隔离；每种计费维度有输入、返回结果与实际账单的对账样例。

### P0-C：当前设备的工具 Key

- `PUT /api/desktop/v1/tool-keys/{tool_id}`：确保当前设备该工具存在唯一活动 Key；只接受 Claude Code、Codex、OpenCode、Pi、DSH 对应固定枚举。
- `GET /api/desktop/v1/tool-keys`：只返回当前设备的掩码和元数据，不返回秘密。
- 轮换采用准备、客户端写入验证、提交/中止两个阶段；验证新 Key 配置前不能撤销旧 Key。
- 重复请求须幂等。线路或模型变化不重复发 Key；分组变更如何更新已有 Key 的绑定与权限，需要明确定义，不能让客户端猜。
- Key 与设备、工具和 NewAPI token_id 关联，便于用量归因与设备撤销；不靠可改的 token_name 充当唯一关联键。
- 桌面边界只在首次发放/轮换返回秘密；本地秘密遗失走轮换，不调用现有通用 `POST /api/token/:id/key` 重新显示 Key。
- 不把模型调用 Key 当账户管理凭证，也不把桌面会话给第三方工具调用模型。

验收：重复创建、并发创建、跨设备访问、旧 Key 保留、写入失败中止、响应丢失重试、撤销设备级联、秘密不进入日志。幂等重试如何恢复首次响应必须有明确安全规则，不能只写一个 Idempotency-Key 头就算完成。

### P1：充值、遥测与更新

- 充值继续打开第一方网站钱包。客户端不处理支付订单和回调，也不把桌面凭证拼到钱包 URL。
- 遥测接收与账号权限隔离，先明确字段白名单、同意、删除与保留策略；客户端当前仍不上传。
- 更新签名与下载发布独立于 NewAPI。更新私钥不进入业务后台；当前客户端更新器仍关闭。
- 这些能力不要顺手混进 P0 的登录联调；分别验收。

## 6. 后端交付给客户端的材料

- [ ] 固定测试环境地址、服务端 Git 提交和构建镜像摘要，不提供生产管理员密码或生产 Key。
- [ ] P0 已实现接口的 OpenAPI/JSON Schema、稳定错误码、脱敏成功/空/失败样例。
- [ ] 普通测试账号与用户分组、特殊价格、余额和模型可见性夹具。
- [ ] 设备授权/刷新/撤销和 Key 生命周期的重放、并发与幂等测试结果。
- [ ] 真实价格内容版本与账单对账样例，明确官网价 6.75 与本站计费/充值单位的区别。
- [ ] 结构化工具兼容矩阵；DSH 未给精确 profile 证据前继续标记未验证。
- [ ] 数据库迁移、回滚和灰度步骤由技术负责人评审执行；桌面仓库不包含生产执行脚本。

建议顺序：先按本页最小 Schema 开测试环境握手，再交付授权和账号读取，最后交付 Key 生命周期及配置写入联调。**本页不是生产上线批准。**

## 7. 本地验证入口

- `pnpm typecheck`
- `pnpm test:candidate`：既有候选版回归 + 新目录契约、工具协议、界面竞态和交接一致性测试。
- `cargo test --locked --manifest-path src-tauri/Cargo.toml --lib`：固定目的地、匿名 HTTP、重定向/体积限制、真实解析器和 bootstrap 识别测试；使用本机合成 HTTP 服务器，不调用生产。
- `python scripts/ci/run-rust-tests.py`：拒绝空测试、忽略/过滤测试和旧 crate。

联调时必须区分：合成测试通过、真实公开接口读取成功、测试账号流程通过、生产发布批准。前一项不自动证明后一项。

### 2026-08-31 本轮验证记录

- 本地 `pnpm typecheck`、30 项候选版前端测试、15 项既有 RU-005/RU-006 回归通过。
- macOS 本机 13 项原生测试全部通过，无忽略或过滤；Rust 格式、Clippy（警告视为错误）、原生二进制与前端构建通过。未据此宣称 Windows 或 Intel macOS 已复测。
- 浏览器检查了配置入口、读取失败与重试、禁用应用按钮、DSH 未验证状态，以及默认宽度和桌面最小 860px 宽度。浏览器没有 Tauri 原生接口，因此这不是在线目录读取或账号端到端验收；成功目录与切换线路的竞态由合成测试覆盖。
- RU-007 冻结契约校验为 0 错误、0 警告；**正式工作包验收报告未通过**。验收工具的 `vitest-json@1` 适配器要求 npm 的 `package-lock.json`、精确 Vitest 版本和非符号链接安装布局，当前仓库使用 pnpm，工具在执行场景前拒绝运行。没有更换包管理器、放宽隔离或手工标记通过。
- 前端构建仍有浏览器兼容数据库陈旧及大于 500 kB 的 chunk 提示；本轮未扩展为依赖升级或全量性能优化。
- 当前仅本地实现与验证，未提交、未推送、未生成安装包，服务器和生产业务数据未修改。

正式工作包报告：`.product-governance/execution/reports/844c10cbf87bca3c.7ae8d67e9c1415981843.json`。解决验收适配器与 pnpm 的兼容性后，须重新运行并绑定最终源码；不能把独立测试通过替代这份报告的状态。

### 后续修正：RU-008

以上为 RU-007 当轮记录，失败报告保留不改。RU-008 通过新冻结决定接替其验收：保留 pnpm、原有 30 项 Vitest 测试和 13 项原生测试，新增 6 个 `pytest-json@1` 场景，在源码只读隔离下直接执行真实 TypeScript 模块与 React 组件。首轮正式报告已通过：`.product-governance/execution/reports/ce53d3336e8c469b.d7b996e29b1fbe3f57df.json`。后续源码变更以执行登记中 RU-008 最新通过报告为准。

这项修正没有更改全局技能、包管理器、业务运行时、服务器或秘密处理方式。Developer ID Application 身份已在本机构建钥匙串识别；候选包签名、公证、真机安装与正式发布仍需各自证据，不能由证书存在或测试通过推导。

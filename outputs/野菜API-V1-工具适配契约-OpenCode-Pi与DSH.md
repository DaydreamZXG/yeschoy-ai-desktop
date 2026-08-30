# 野菜API V1 工具适配契约：OpenCode、Pi 与 DSH

状态：架构草案，未冻结，不授权实现  
日期：2026-08-29  
适用产品：野菜API 官方桌面客户端 V1  
当前范围：客户端方案与只读核验；不修改 NewAPI、服务器、Nginx、DNS 或线上配置

## 1. 本轮结论

2026-08-29，产品负责人确认三款工具都采用“不把野菜API Key 明文写进普通配置文件”的安全接入方向，但接入层级不同：

| 工具 | 安全取 Key 方式 | 当前判断 |
|---|---|---|
| Pi | `models.json` 原生支持 `apiKey: "!command"` | 已确认目标；可复用同一个签名凭据助手，仍需跨平台实测 |
| OpenCode | 野菜认证插件的 `auth.loader` 在运行时调用凭据助手，只把 Key 放入进程内存 | 已确认目标；必须锁定兼容版本并做插件级实测 |
| DSH | 用 profile/plugin 机制把默认本地凭据提供方替换为野菜的系统凭据库提供方 | 已确认目标；开发与验证量最大 |

三者共同遵守已经确认的安全边界：完整工具 Key 只保存在 macOS Keychain 或 Windows 用户凭据库；普通 JSON/YAML、SQLite、日志、遥测、诊断包和插件代码都不得包含完整 Key。安全组件缺失或工具版本不兼容时阻止一键配置，不回退到明文。

这不是“已经兼容”的结论。OpenCode 插件、DSH 凭据提供方、最低版本、Windows/macOS 安装路径和工具级请求都还没有实现或验收。

## 2. 证据与事实分层

| 类型 | 已观察事实 | 已确认目标中仍待实现或验证 |
|---|---|---|
| OpenCode 官方资料 | 全局配置为 `~/.config/opencode/opencode.json`；`/connect` 默认把凭据存入 `~/.local/share/opencode/auth.json`；自定义 provider 支持 `baseURL`、模型和 `apiKey`；插件 API 有 provider 认证 loader | 野菜认证插件能否在全部目标版本稳定加载、调用 helper 并覆盖普通认证，需要实机证明 |
| Pi 官方资料 | `~/.pi/agent/models.json` 支持 OpenAI Chat Completions、Responses、Anthropic Messages；`apiKey` 和 headers 支持 `!command`、环境变量和字面量；命令在请求时解析 | 野菜具体字段、命令转义、最低版本和 Windows/macOS helper 路径需要实测 |
| DSH 官方资料 | settings 只保存 credential reference；默认本地 provider 把值保存到 `$DSH_HOME/.credentials.yaml`；Cordis profile 可装外部插件并按 row ID 替换 credential provider | 野菜要开发一个完整的 OS-vault credential provider；不能把默认 `.credentials.yaml` 当成合规实现 |
| CC Switch 固定源码 | 已有 OpenCode provider 节点读写与插件数组管理；Pi 只管理 `models.json.providers`，保留未知字段、检测外部变更、原子写入且不碰 `auth.json` | 上游会保存字面 API Key，且没有 DSH 适配器；不能原样发布 |
| 本机只读探测 | 当前规划机没有发现 `opencode` 和 `pi`；发现 DSH `0.1.0-rc.6`，其 `--dump-config` 显示 `credentials` row 使用 `@deepseek-ai/dsh-credentials-local` | 本机版本不是 V1 支持矩阵，也不能替代 Windows/macOS 测试 |

主要来源：

- [OpenCode Providers](https://opencode.ai/docs/providers/)
- [OpenCode Config](https://opencode.ai/docs/config/)
- [OpenCode 插件接口源码](https://github.com/anomalyco/opencode/blob/dev/packages/plugin/src/index.ts)
- [Pi 自定义模型与 provider](https://github.com/earendil-works/pi/blob/main/packages/coding-agent/docs/models.md)
- [Pi provider 与认证](https://github.com/earendil-works/pi/blob/main/packages/coding-agent/docs/providers.md)
- [DSH provider 配置](https://github.com/deepseek-ai/deepseek-harness/blob/master/docs/user/guide/providers.zh.md)
- [DSH 凭据服务](https://github.com/deepseek-ai/deepseek-harness/blob/master/packages/credentials/credentials/README.md)
- [DSH 组合与 profile](https://github.com/deepseek-ai/deepseek-harness/blob/master/docs/architecture.md)
- CC Switch 固定提交 `3217f72596f2d1c0f879f0a05f83803825d9809f`

本轮外部快照锚点：OpenCode `dc4449df0d52199704ea4989a5a993ebbc605612`、Pi `853a80d26c90a14c1886f0ebb8ffaae133ca2185`、DSH `cd5ef8148158c3a752a658978873241fdf8e2bbc`。这些只是 2026-08-29 的核验锚点，不自动成为未来发布基线。

## 3. 三款工具共同契约

### 3.1 目标身份

每张配置收据至少记录：

- `tool_id`：`opencode`、`pi` 或 `dsh`
- PATH 默认命中的规范化可执行路径
- 原始版本与解析后的语义版本
- 原生配置目录、目标文件和适配器契约版本
- DSH 额外记录 profile 名、`DSH_HOME` 和 composed-config 哈希
- Windows 额外记录 native、WSL 及发行版身份；不能把两套配置混为一套

发现多处安装、无法执行 `--version`、配置目录不唯一或 DSH profile 不明确时，只读展示并停止写入。

### 3.2 线路与协议派生

线路是产品 ID，不允许页面或适配器自由拼 URL：

| 线路 ID | OpenAI 兼容 Base URL |
|---|---|
| `mainland_optimized` | `https://yeschoy.com/v1` |
| `global_cf` | `https://api.yeschoy.com/v1` |

V1候选协议：

- OpenCode：`@ai-sdk/openai-compatible`，走 `/v1/chat/completions`。
- Pi：`openai-completions`；只有模型目录明确标记并完成实测后才改用 `openai-responses`。
- DSH：`llm-pi-ai` 自定义 route，`api: openai-completions`。

这些是客户端候选映射，不证明线上两条线路已经通过相应协议与流式响应测试。

### 3.3 模型事实

- provider ID 固定为 `yeschoy`，展示名固定为“野菜API”。
- 模型 ID 按服务端版本化目录的精确字符串写入，不做模糊匹配或静默别名替换。
- 模型名称、上下文、输出上限、图片能力、推理能力和工具兼容性来自版本化野菜模型目录；缺字段时宁可保守隐藏能力。
- 用户选中的模型 ID 必须展示；配置预览同时展示官网基准价和野菜实际价。
- 价格只用于客户端展示，不把展示价写入第三方工具的计费字段作为账单权威。

### 3.4 配置事务

三款工具复用 Claude/Codex 契约中的事务规则：精确字段计划、用户确认、加密临时快照、写前二次比较、原子写入、读回校验、工具级最小请求、成功后删除快照。产品不保留历史备份。

未知字段、注释和非野菜 provider 必须保留。若外部程序在预览后修改文件，立即停止并返回 `external_change_conflict`。

## 4. OpenCode 适配契约

### 4.1 当前事实

- 全局配置候选路径为 `~/.config/opencode/opencode.json`；还存在远程、项目、环境内容和 managed settings 等更高或不同层级，不能只看一个文件就宣称配置生效。
- 官方 `/connect` 会把 API Key 存到 `~/.local/share/opencode/auth.json`。
- 自定义 provider 使用 `provider.<id>`，`npm` 可选 `@ai-sdk/openai-compatible`，`options.baseURL` 指向兼容端点，模型由 `models` 映射提供。
- 顶层 `model` 与 `small_model` 使用 `provider/model` 形式。
- 官方配置只原生提供字面量、环境变量或文件内容替换，没有文档化的 `!command` 配置值。
- 插件 API 的认证 hook 提供 `provider`、`loader` 与认证方法，可在 provider 初始化时生成运行时 options。

### 4.2 目标配置形状

以下只是非秘密示意，不是可直接复制的最终模板：

```json
{
  "$schema": "https://opencode.ai/config.json",
  "plugin": ["file:///STABLE_SIGNED_PATH/yeschoy-opencode-auth.js"],
  "provider": {
    "yeschoy": {
      "npm": "@ai-sdk/openai-compatible",
      "name": "野菜API",
      "options": {
        "baseURL": "https://yeschoy.com/v1"
      },
      "models": {
        "EXACT_MODEL_ID": {
          "name": "MODEL_DISPLAY_NAME"
        }
      }
    }
  },
  "model": "yeschoy/EXACT_MODEL_ID"
}
```

不得写 `provider.yeschoy.options.apiKey`，不得创建 `{file:...}` 秘密文件，也不得把完整 Key 写入 OpenCode `auth.json`。

### 4.3 已确认的野菜认证插件方向

逻辑名称：`yeschoy-opencode-auth`。

- 插件只注册 provider `yeschoy` 的认证 loader。
- loader 调用签名的 `yeschoy-credential-helper`，固定请求 `tool_id=opencode`。
- helper stdout 只进入 OpenCode provider 的内存 `apiKey` option；插件不得记录、缓存或回写该值。
- 插件代码、配置参数和错误信息都不含账号 ID、Key ID、Key 前后缀或 Key 长度。
- helper 非零退出、插件未加载或版本不兼容时，provider 不可用，并返回稳定脱敏错误码。
- 插件和 helper 必须随野菜客户端签名发布；禁止运行时从未固定版本的 npm 包下载执行。

这是已确认的目标架构，但仍待实现和验证。冻结前必须证明 loader 在每个支持版本和安装方式中确实生效，且插件故障不会让 OpenCode退回其他同名凭据。

### 4.4 字段所有权

| 字段 | 所有权 |
|---|---|
| `/provider/yeschoy` | 野菜拥有整个精确节点，但保存时仍需字段级生成和读回 |
| `/plugin` 中精确的野菜插件项 | 野菜只拥有该数组元素，不拥有整个数组 |
| `/model` | 用户确认“设为默认模型”后由野菜拥有，值为 `yeschoy/<model_id>` |
| `/small_model` | 只有用户选择或目录明确给出轻量模型后才拥有；否则不写 |
| `~/.local/share/opencode/auth.json` | OpenCode拥有；野菜默认只做冲突检查，不存 Key |
| 其他 provider、MCP、agent、权限、主题、项目配置 | 用户或 OpenCode拥有，不得修改 |

如果项目级配置、managed settings、`OPENCODE_CONFIG_CONTENT` 或已有 `auth.json` 条目覆盖野菜 provider，状态为 `external_override`，不能显示“已配置成功”。

### 4.5 从 CC Switch 的复用边界

可复用 provider 节点读写、JSON5 解析、插件数组去重、文件锁和原子写入。必须改造：上游 provider 表单会保存 `options.apiKey`，并且当前适配只处理 provider 节点，不足以安全安装认证插件、检查配置层级和设置小白选择的默认模型。

## 5. Pi 适配契约

### 5.1 当前事实

- 配置目录默认为 `~/.pi/agent`，可由 `PI_CODING_AGENT_DIR` 或明确设置覆盖。
- 自定义 provider 位于 `models.json.providers.<provider_id>`。
- `apiKey` 与自定义 headers 原生支持 `!command`；命令在每次请求时解析，Pi 不替任意命令做 TTL 或失败回退。
- `settings.json` 的 `defaultProvider`、`defaultModel` 决定新会话默认模型。
- `/login` 和 `auth.json` 属于 Pi 原生认证；CC Switch 当前实现也明确不读取或修改它们。

### 5.2 目标配置形状

```json
{
  "providers": {
    "yeschoy": {
      "name": "野菜API",
      "baseUrl": "https://yeschoy.com/v1",
      "api": "openai-completions",
      "apiKey": "!STABLE_SIGNED_HELPER get --tool pi",
      "authHeader": true,
      "models": [
        {
          "id": "EXACT_MODEL_ID",
          "name": "MODEL_DISPLAY_NAME",
          "reasoning": true,
          "input": ["text"],
          "contextWindow": 128000,
          "maxTokens": 16384
        }
      ]
    }
  }
}
```

示例中的能力数字不是产品默认值；最终必须来自模型目录。helper 的真实绝对路径和跨平台引用格式尚未冻结。

### 5.3 字段所有权

| 文件与字段 | 所有权 |
|---|---|
| `models.json.providers.yeschoy` | 野菜拥有精确节点 |
| `settings.json.defaultProvider` | 用户确认设为默认后，野菜写 `yeschoy` |
| `settings.json.defaultModel` | 用户确认设为默认后，野菜写精确模型 ID |
| `auth.json` | Pi拥有；野菜不读取完整 Key、不复制、不修改、不删除 |
| 其他 provider、settings、extensions、skills、sessions | 用户或 Pi拥有 |

移除时只删除仍与收据匹配的 `providers.yeschoy`，并只在 `defaultProvider == "yeschoy"` 且默认值仍与收据匹配时清理对应默认字段。不得影响 Pi 登录或其他 provider。

### 5.4 helper 与错误行为

- `apiKey` 只保存以 `!` 开头的非秘密命令。
- helper 每次调用从 OS vault 读取 `tool-key/pi`；无缓存、无 stale key 回退。
- Windows 命令行转义、空格路径、Unicode 路径和 `.cmd` shim 必须用真实进程测试，不靠字符串猜测。
- 外部 `--api-key`、同 provider 的 `auth.json` 凭据或其他高优先级来源若覆盖 helper，必须显示 `external_override`。
- helper 不存在、不可执行或 Key 缺失时显示“安全组件不可用”，不把 Key 改写成字面量。

### 5.5 从 CC Switch 的复用边界

Pi 是三款中最适合直接复用的适配器基础。当前实现已经有 1 MiB 上限、JSON5 解析、provider 节点精确增删、内容 revision、并发冲突、原子私有写入和失败回滚；还会保留未知字段并避开 `auth.json`。

必须改造：上游允许字面 `apiKey` 并把完整 provider 配置保存到自身数据库；野菜版必须生成 helper 命令、剥离任何导入的完整 Key、增加 `settings.json` 默认模型的字段级事务，并避免把秘密复制到 SQLite 或 UI。

## 6. DSH 适配契约

### 6.1 当前事实

- `$DSH_HOME` 默认为 `~/.dsh`；`settings.yaml` 保存 provider 与默认模型的用户层配置。
- `llm-pi-ai` 支持自定义 OpenAI 兼容 route，配置包含 `apiKeyEnv`、`api`、`baseURL` 和 `models`。
- `agent-default-model` section 保存默认 provider、模型及可选推理强度。
- DSH 的凭据 service 让配置只保存引用，并在每次模型请求时解析当前值。
- 默认 `@deepseek-ai/dsh-credentials-local` 会把托管值保存到 `$DSH_HOME/.credentials.yaml`；它不是野菜“只进系统凭据库”规则的合规实现。
- DSH profile 允许安装外部插件，并用 `cordis.patch.yml` 或后置 patch 按 row ID 替换 `credentials` provider。

### 6.2 非秘密 settings 目标

```yaml
llm-pi-ai:
  providers:
    yeschoy:
      apiKeyEnv: YESCHOY_DSH_API_KEY
      api: openai-completions
      baseURL: https://yeschoy.com/v1
      models:
        - id: EXACT_MODEL_ID
          name: MODEL_DISPLAY_NAME

agent-default-model:
  provider: yeschoy
  model: EXACT_MODEL_ID
```

`YESCHOY_DSH_API_KEY` 是 credential reference，不是要求用户设置环境变量，更不是允许把 Key 放进 `.env`。野菜凭据 provider 必须拦截该引用并从 OS vault 返回 `tool-key/dsh`。

### 6.3 已确认的野菜 DSH 凭据 provider 方向

逻辑包名：`@yeschoy/dsh-credentials-keyring`。

- 通过 DSH 官方 profile/plugin 机制安装，不修改 DSH 上游源码。
- 在目标 profile 的 composed config 中替换精确 `id: credentials` row。
- 对 `YESCHOY_DSH_API_KEY` 从 `com.yeschoy.desktop` 的 OS vault 读取；每次 `resolve()` 都取当前值。
- `describe()` 只返回 configured/source/writable 等状态，不返回值。
- `set()`、`unset()` 只能通过野菜客户端受控流程触发，不能把值写入 `.credentials.yaml`。
- 必须完整实现 DSH credential service 对 reference、record、事件和生命周期的契约，或安全委托非野菜引用；不能为了一个 Key 破坏用户其他 provider/OAuth 凭据。
- 包必须固定版本、完整性哈希和发布签名；V1不得在配置时执行未固定版本的远程安装脚本。

这是已确认的目标架构，但完整 credential service、非野菜凭据兼容和跨版本行为仍待实现与验证。

目前尚未证明 DSH 支持“链式 credential providers”。因此，文档不能假装只插一个拦截器就完成；冻结前必须选定并验证“完整替代 provider”或官方支持的组合方式。

### 6.4 字段与 profile 所有权

| 位置 | 所有权 |
|---|---|
| `settings.yaml` 的 `llm-pi-ai.providers.yeschoy` | 野菜拥有精确 route |
| `settings.yaml` 的 `agent-default-model` | 用户确认设为默认后，野菜拥有匹配字段 |
| profile/home patch 中精确 `credentials` row 变更 | 野菜安全组件拥有，但必须保留原 row 身份以便可验证移除 |
| 野菜凭据插件安装记录 | 野菜拥有固定包与版本记录 |
| `.credentials.yaml` | DSH默认 provider拥有；野菜不得把自己的 Key 写进去 |
| 其他 profile、provider、settings 和 credential records | 用户或 DSH拥有 |

DSH V1 已确认只正式支持 `web` profile。精确 DSH 版本通过完整矩阵后按正常 V1 支持展示，不加 Developer Preview 产品标签，但高级详情和诊断必须保留真实上游版本号。DSH 尚未通过矩阵不阻止整体 V1 发布；此时 `web` 也只显示“已发现，当前版本暂未支持自动配置”，不创建凭据、不安装插件、不写配置，后续只能通过签名客户端更新增加支持。`headless`、`sdk`、`sdk-minimal`、`acp` 及其他 profile 第一版始终只做只读发现，不复用 `web` 的健康状态。

### 6.5 从 CC Switch 的复用边界

CC Switch 当前没有 DSH app type、检测、writer、provider UI 或事务逻辑，不能声称“fork 后自然就有”。可复用的只有通用路径发现、版本探测、原子写入、配置事务、凭据助手、状态模型和 UI壳。DSH 适配是独立新增模块。

## 7. 验证门槛

### 7.1 静态检查

- 三款普通配置、野菜 SQLite、日志、遥测和诊断导出均不存在完整 Key。
- OpenCode provider 没有 `options.apiKey`；Pi 只有 `!helper`；DSH settings 只有 credential reference。
- 配置保存后未知字段和非野菜数组元素不变。
- 插件/helper 的固定路径、签名或哈希可验证。

### 7.2 工具级最小请求

每个受支持版本在空临时项目执行固定、无用户内容、极短输出请求，并检查：

- PATH 实际命中的正是配置收据中的工具。
- 实际 provider 为 `yeschoy`，实际模型 ID 与用户选择一致。
- 请求经过目标线路和预期协议。
- 认证失败、余额不足、模型不存在、协议不兼容、TLS、DNS、超时能被区分。
- helper/plugin/provider 的 stdout、stderr、日志和崩溃信息均不泄露 Key。

### 7.3 平台矩阵

至少覆盖 macOS Intel/Apple Silicon 与 Windows 10/11 x64。WSL 未正式确认前只读发现。每个平台都要覆盖安装路径含空格/中文、工具多版本、只读配置、损坏配置、外部并发修改、工具运行中、系统凭据库锁定和客户端升级后 helper 路径变化。

## 8. 统一状态

除 Claude/Codex 契约的状态外，三款工具增加：

| 状态 | 小白文案 |
|---|---|
| `security_component_missing` | 安全组件缺失，暂不能配置；可修复或升级 |
| `plugin_incompatible` | 当前工具版本与野菜安全组件不兼容 |
| `credential_source_conflict` | 工具中已有其他凭据正在覆盖野菜配置 |
| `profile_ambiguous` | 发现多个 DSH 使用方式，需要先确认配置目标 |
| `profile_dependency_missing` | DSH profile 缺少已固定版本的野菜凭据插件 |
| `managed_plaintext_detected` | 发现野菜 Key 曾以明文保存；立即停止并引导轮换 |

出现 `managed_plaintext_detected` 时，客户端不得把发现的值复制到日志或 UI，也不能直接继续使用；应先由服务端轮换该工具 Key，再在用户确认后清理精确旧字段。

## 9. 仍阻止本契约冻结的事项

1. OpenCode 认证 loader 的精确支持版本、插件安装位置和配置优先级实测；产品已确认精确版本白名单策略，当前候选集合见[工具版本支持与验证矩阵草案](./野菜API-V1-工具版本支持与验证矩阵.md)。
2. Pi `!command` 在候选精确版本、macOS、Windows native 与未来 WSL 的真实转义和 helper 路径。
3. DSH 野菜凭据 provider 的完整 service 设计，以及非野菜 credential reference/record 的兼容策略。
4. 三款工具设置默认模型时，遇到已有用户默认值的最终确认文案。
5. OpenCode 已有 `auth.json` 同名条目、项目配置或 managed settings 覆盖时，是只提示手动清理，还是在用户确认后调用官方注销流程。
6. 首批模型清单、能力字段和每款工具的精确最小验证命令。
7. 两条实际部署线路对 chat completions、模型发现和流式响应的只读契约验证。

## 10. 已确认的 DSH V1范围

2026-08-29，产品负责人确认：

> DSH V1 只正式支持 `dsh web` 的 `web` profile；其他 profile 第一版只发现并提示“暂未支持自动配置”。

由此产生的硬边界：

- DSH 适配器的 `capabilities()` 只有在目标身份明确为 `web` profile 且版本进入支持矩阵时，才返回可配置。
- 其他 profile 不共享 `web` 的配置收据、插件安装状态、凭据可用状态或健康结果。
- “只发现”必须是零写入：不得为了探测而运行 `dsh plugin add`、创建 profile、生成 patch 或修改 settings。
- 后续增加另一个 profile 时，需要新的已接受范围决策、独立样本和完整验收，不能自动继承 `web` 的兼容声明。
- 该范围收敛不改变安全边界：DSH 的野菜 Key 绝不写入 `.credentials.yaml`、`.env` 或普通配置。

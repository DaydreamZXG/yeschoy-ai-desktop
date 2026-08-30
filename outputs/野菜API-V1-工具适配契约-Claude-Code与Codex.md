# 野菜API V1 工具适配契约：Claude Code 与 Codex

状态：架构草案，未冻结，不授权实现  
日期：2026-08-29  
适用产品：野菜API 官方桌面客户端 V1  
当前范围：只读核验和客户端方案；不修改 NewAPI、服务器、Nginx、DNS 或线上配置

## 1. 本轮结论

Claude Code 与 Codex 都可以做成真正面向小白的“一键配置”，而且目标方案不需要把野菜API Key 明文写入 `settings.json`、`config.toml` 或 `auth.json`。

已经确认的共同方案是：

1. 工具配置文件只保存线路、模型、野菜 provider 标识和一个不含秘密的“凭据助手”命令。
2. 野菜API 工具 Key 保存于 macOS Keychain 或 Windows 用户凭据库。
3. Claude Code 通过官方 `apiKeyHelper` 取 Key；Codex 通过官方 `model_providers.<id>.auth.command` 取 bearer token。
4. 凭据助手只在工具发起请求时把对应“设备 × 工具”Key 输出给该工具进程，不把 Key 留在配置文件、普通数据库、日志或遥测中。
5. 如果某个旧版本工具不支持安全凭据助手，V1 将其标为“版本过旧，暂不支持一键配置”，不降级为明文写 Key。

产品负责人已经确认“不支持安全读取时只提示升级，绝不明文回退”。但最低支持版本、凭据助手的稳定安装路径和实际线上 NewAPI 协议仍需验证，因此本文仍不能标记为冻结。

## 2. 证据与事实分层

| 类型 | 已确认内容 | 不能据此声称的内容 |
|---|---|---|
| 用户已确认的目标 | 官方客户端、小白流程、系统凭据库、无明文降级、按设备和工具隔离 Key、无历史备份 | 不能证明任一工具版本已经通过实机适配 |
| CC Switch 源码现状 | 有跨平台 CLI 发现、`--version` 探测、原子文件写入、Codex 多文件回滚、配置路径覆盖等可复用基础 | 不能证明这些代码原样满足野菜API字段所有权和秘密存储要求 |
| Claude Code 官方文档 | 用户配置位于 `~/.claude/settings.json`；支持 `env`、`apiKeyHelper`、自定义 Base URL 和模型环境变量 | 不能证明用户电脑上的旧版本支持今天文档中的全部字段 |
| Codex 官方文档 | 用户配置位于 `~/.codex/config.toml`；自定义 provider 支持 `base_url`、`env_key`、`auth.command`；`wire_api` 仅支持 `responses` | 不能证明本机旧版本或野菜线上服务已经通过端到端测试 |
| NewAPI 固定源码快照 | `/v1/messages`、`/v1/responses` 已注册；Token 中间件会把 Claude 的 `x-api-key` 转为 bearer 认证 | 不能证明当前部署实例与固定源码快照完全一致 |

主要来源：

- [Claude Code 设置文件与作用域](https://code.claude.com/docs/en/settings)
- [Claude Code 环境变量](https://code.claude.com/docs/en/env-vars)
- [Claude Code 身份验证与凭据优先级](https://code.claude.com/docs/en/team)
- [Claude Code LLM Gateway 配置](https://code.claude.com/docs/en/llm-gateway)
- [Claude Code 模型配置](https://code.claude.com/docs/en/model-config)
- [OpenAI Docs：Codex 配置参考](https://developers.openai.com/codex/config-reference/)
- [OpenAI Docs：Codex 身份验证](https://developers.openai.com/codex/auth/)
- CC Switch 固定提交 `3217f72596f2d1c0f879f0a05f83803825d9809f`
- NewAPI 固定提交 `ac381acf4bf41204b97bb26b4c58c83275877a2e`

## 3. 两个适配器共用的 V1 契约

### 3.1 适配目标必须有明确身份

一次配置不是只记录“Claude”或“Codex”，而是绑定以下目标：

- 工具 ID：`claude-code` 或 `codex`
- 运行环境：`macos-native`、`windows-native`，或未来明确选择的 `wsl:<distro>`
- PATH 默认命中的可执行文件规范化路径
- `--version` 返回的原始版本和解析版本
- 实际配置目录与配置文件路径
- 适配器契约版本

同一电脑发现多处安装时，客户端展示“你平常终端实际使用的版本”，并将其他安装标为冲突候选。无法确定 PATH 默认目标、配置目录或 WSL 发行版时，只读展示，不执行写入。

### 3.2 只修改声明拥有的字段

每个适配器维护一组精确 JSON Pointer 或 TOML 路径。未声明的字段、未知字段、注释、其他 provider、权限、hooks、MCP 和用户偏好都属于用户或工具，不得重写。

野菜API本地只保存一张不含秘密的“配置收据”：

- 工具目标身份与适配器版本
- 操作 ID、配置时间
- 客户端生成字段的路径和值哈希
- 线路 ID、模型 ID
- 写后文件哈希和上次验证时间

配置收据不是配置备份，不保存被替换字段的旧值，也不能用于任意历史恢复。它只用于判断当前字段是否仍是野菜API生成的值，以及识别外部改动。

### 3.3 已存在同名字段时的规则

如果目标字段已有不同值：

1. 向普通用户显示“这台工具已连接到其他服务”，不展示大段 JSON/TOML。
2. 高级详情展示将被替换的字段名和掩码值，不展示完整旧 Key。
3. 用户明确确认后，字段才转为野菜API拥有。
4. 由于产品已决定不保留历史备份，事务完成后不能承诺未来恢复被替换的旧 provider 值。
5. 用户不确认时保持原文件不变。

移除野菜配置时，仅删除仍与配置收据匹配的野菜字段。字段已经被用户或其他工具修改时，进入“外部变更冲突”，禁止静默删除。

### 3.4 配置事务

```text
发现工具和版本
→ 解析目标配置并检查权限
→ 检查已有字段、外部凭据和冲突
→ 生成新文件但不写入
→ 展示线路、模型、文件和字段摘要
→ 获取或轮换当前设备的工具 Key，并写入系统凭据库
→ 创建加密的事务临时快照
→ 再次比较文件身份、修改时间和内容哈希
→ 原子写入
→ 重新读取并执行 JSON/TOML 语法及字段回验
→ 在空临时目录执行工具级最小连通性验证
→ 成功：记录配置收据并删除临时快照
→ 失败：恢复原文件；恢复成功后删除临时快照
```

写入前后只要检测到文件被外部修改，事务就停止并显示冲突。恢复失败时保留加密的未完成事务快照和操作 ID，下一次启动先恢复，不允许继续覆盖同一目标。

### 3.5 凭据助手边界

暂定逻辑名称：`yeschoy-credential-helper`。最终文件路径和打包方式尚未冻结。

助手输入只允许固定枚举：

- `tool_id`: `claude-code` 或 `codex`
- 非秘密的本机 Key 引用
- 可选的协议输出类型：`api_key` 或 `bearer_token`

助手行为：

- 从 `com.yeschoy.desktop` 的系统凭据命名空间读取当前设备对应工具的 Key。
- 只向标准输出写一行 Key；标准错误只允许稳定、脱敏的错误码。
- 不打印账号、Key ID、完整路径、调用参数、Key 长度或前后缀。
- 不把 Key 写入剪贴板、文件、SQLite、日志、遥测或崩溃上下文。
- 系统凭据库不可用、条目不存在或设备已撤销时以非零状态退出。
- 客户端诊断只读取退出码和允许的错误码，绝不采集标准输出。

该机制降低配置文件泄露风险，但安全边界仍是本机用户账户。已经控制用户会话、能够注入目标进程或拥有管理员权限的恶意软件，不在桌面配置助手能够完全抵御的范围内。按设备和工具隔离 Key、快速撤销和轮换用于缩小此类事件的影响面。

## 4. Claude Code 适配契约

### 4.1 发现与版本

目标发现顺序：

1. 查找登录 shell / PATH 默认命中的 `claude`。
2. 对定位到的真实可执行文件执行有 10 秒上限的 `claude --version`。
3. PATH 未命中时只读扫描已审计的常见安装目录。
4. 区分“未安装”和“可执行存在但无法运行”。
5. 解析当前运行环境中的 `CLAUDE_CONFIG_DIR`；未设置时使用默认目录。

默认配置路径：

| 平台 | 配置目录 | 用户设置文件 |
|---|---|---|
| macOS | `~/.claude` | `~/.claude/settings.json` |
| Windows 原生 | `%USERPROFILE%\.claude` | `%USERPROFILE%\.claude\settings.json` |
| 自定义 | `CLAUDE_CONFIG_DIR` | `<CLAUDE_CONFIG_DIR>/settings.json` |

官方文档说明 Windows 的 `~/.claude` 即 `%USERPROFILE%\.claude`，设置 `CLAUDE_CONFIG_DIR` 后设置、会话历史和插件都会迁移到该目录。多个候选目录无法确定哪个是实际运行目标时禁止写入。

最低支持版本：**尚未冻结**。当前工作机观测到 `Claude Code 2.1.233`，而当前官方文档中部分新字段要求更高版本；这证明不能把“本机装了 Claude”直接等同于“支持当前适配契约”。

### 4.2 线路派生

| 线路 | 写入 `ANTHROPIC_BASE_URL` 的值 | 有效消息端点 |
|---|---|---|
| 大陆优化 | `https://yeschoy.com` | `https://yeschoy.com/v1/messages` |
| Cloudflare 全球加速 | `https://api.yeschoy.com` | `https://api.yeschoy.com/v1/messages` |

Claude Code 的 Base URL 写根地址，不附加 `/v1`。官方 Gateway 示例也是把 `ANTHROPIC_BASE_URL` 指向服务根，由工具请求 `/v1/messages` 和相关端点。

### 4.3 野菜API拥有的字段

目标字段集合：

- `/apiKeyHelper`
- `/env/ANTHROPIC_BASE_URL`
- `/env/ANTHROPIC_MODEL`
- `/env/ANTHROPIC_DEFAULT_SONNET_MODEL`
- `/env/ANTHROPIC_DEFAULT_OPUS_MODEL`
- `/env/ANTHROPIC_DEFAULT_HAIKU_MODEL`
- 只有模型目录明确需要时才启用的其他模型映射字段

默认不写：

- `ANTHROPIC_API_KEY`
- `ANTHROPIC_AUTH_TOKEN`
- `CLAUDE_CODE_OAUTH_TOKEN`
- `.credentials.json`
- `~/.claude.json`
- permissions、hooks、plugins、MCP、theme 和其他用户设置

建议生成形状如下；命令路径和模型映射只是结构示例，不是可复制的正式配置：

```json
{
  "apiKeyHelper": "<signed-yeschoy-helper> get --tool claude-code",
  "env": {
    "ANTHROPIC_BASE_URL": "https://yeschoy.com",
    "ANTHROPIC_MODEL": "<selected-model-id>",
    "ANTHROPIC_DEFAULT_SONNET_MODEL": "<catalog-sonnet-mapping>",
    "ANTHROPIC_DEFAULT_OPUS_MODEL": "<catalog-opus-mapping>",
    "ANTHROPIC_DEFAULT_HAIKU_MODEL": "<catalog-haiku-mapping>"
  }
}
```

`settings.json` 是严格 JSON；注释和尾逗号会使文件无效。写入必须使用结构化 JSON 合并，不能用字符串替换。

### 4.4 模型映射

`ANTHROPIC_MODEL` 决定主会话模型，但 Claude Code 还可能使用 Sonnet、Opus、Haiku 等角色别名。不能只按模型名字猜测映射。

服务端/签名模型目录最终需为每个 Claude Code 兼容模型提供：

- 主模型 ID
- Sonnet 角色映射
- Opus 角色映射
- Haiku / 后台任务角色映射
- 已验证 Claude Code 版本范围
- 不支持的能力和已知限制

在兼容矩阵未确认前，不把任意 NewAPI 模型标为 Claude Code 可用，也不自动把三个角色全部映射成同一个模型。若产品最终选择“所有角色同模型”的简化策略，需要单独确认其成本和能力影响。

### 4.5 凭据优先级冲突

Claude Code 当前官方优先级中，`ANTHROPIC_AUTH_TOKEN` 和 `ANTHROPIC_API_KEY` 都高于 `apiKeyHelper`。因此：

- 若这两个变量已存在于 `settings.json`，客户端必须提示冲突并在用户确认后移除，不能留下它们覆盖野菜助手。
- 若变量来自 shell、系统环境或第三方启动器，客户端不能擅自修改；状态显示“外部凭据正在覆盖野菜配置”，并提供定位说明。
- 不修改 Claude `/login` 保存的 OAuth 凭据；`apiKeyHelper` 生效时自然优先于普通 `/login` 凭据。

### 4.6 写后验证

至少执行：

1. JSON 重新解析。
2. 逐字段读回，确认 Base URL、模型和 helper 命令与计划一致。
3. 确认配置文件和配置收据中不存在完整 Key。
4. 通过 helper 进行不回显秘密的存在性检查。
5. 在空临时目录启动一次版本适配器规定的非交互最小请求，只要求固定短文本，不读取用户项目，不调用工具。
6. 区分配置错误、凭据助手失败、401、余额不足、模型不可用、协议不兼容、网络超时和上游错误。

官方文档称多数设置会在运行中重载，但模型、环境和已存在会话可能有启动期行为。V1保守提示“新开一个 Claude Code 会话后生效”，不声称正在运行的旧会话已切换。

## 5. Codex 适配契约

### 5.1 发现与版本

发现规则与 Claude Code 相同，但目标命令为 `codex` 和 `codex --version`。

默认配置路径：

| 平台 | 配置目录 | 主配置 | 登录凭据 |
|---|---|---|---|
| macOS | `~/.codex` | `~/.codex/config.toml` | `~/.codex/auth.json` 或系统 keyring |
| Windows 原生 | `%USERPROFILE%\.codex` | `%USERPROFILE%\.codex\config.toml` | `auth.json` 或系统凭据库 |
| 自定义 | `CODEX_HOME` | `<CODEX_HOME>/config.toml` | `<CODEX_HOME>/auth.json` 或系统 keyring |

OpenAI Docs 明确指出用户级 provider 配置必须放在用户 `config.toml`，项目 `.codex/config.toml` 不能覆盖 `model_provider` 和 `model_providers` 等机器级 provider 字段。

最低支持版本：**尚未冻结**。当前工作机观测到 `codex-cli 0.146.0`；CC Switch 固定源码已针对 0.149 的 provider 校验行为做了适配。野菜API必须通过真实版本矩阵确认 `auth.command` 的最低版本，不能假定 0.146 已支持。

### 5.2 线路派生

| 线路 | 写入 provider `base_url` 的值 | 有效 Responses 端点 |
|---|---|---|
| 大陆优化 | `https://yeschoy.com/v1` | `https://yeschoy.com/v1/responses` |
| Cloudflare 全球加速 | `https://api.yeschoy.com/v1` | `https://api.yeschoy.com/v1/responses` |

Codex 的自定义 provider 使用 Responses 协议。OpenAI Docs 当前配置参考将 `wire_api` 的唯一支持值列为 `responses`；因此 V1不通过本地代理把 Chat Completions 假装成原生 Codex 能力。

### 5.3 野菜API拥有的字段

目标字段集合：

- 顶层 `model`
- 顶层 `model_provider`
- 完整 `[model_providers.yeschoy]` 表
- 完整 `[model_providers.yeschoy.auth]` 子表

默认不写或不改：

- `auth.json`
- `cli_auth_credentials_store`
- OpenAI/ChatGPT 官方登录缓存
- 其他 `model_providers.*` 表
- MCP、sandbox、approval、profiles、telemetry 和项目配置
- 明文 `experimental_bearer_token`
- 全局或 shell 的永久 API Key 环境变量

建议生成形状如下；helper 路径和模型 ID 仍是占位：

```toml
model = "<selected-model-id>"
model_provider = "yeschoy"

[model_providers.yeschoy]
name = "野菜API"
base_url = "https://yeschoy.com/v1"
wire_api = "responses"

[model_providers.yeschoy.auth]
command = "<signed-yeschoy-helper>"
args = ["get", "--tool", "codex"]
timeout_ms = 5000
refresh_interval_ms = 300000
```

`yeschoy` 是提议的稳定 provider ID；发布前还需在受支持 Codex 版本上验证它不与保留 ID 冲突。OpenAI Docs 当前说明 `openai`、`ollama` 和 `lmstudio` 不能被自定义覆盖；CC Switch 源码还为更新版本防御性保留了 Amazon Bedrock 相关 ID。

### 5.4 官方登录保护

Codex CLI 和 IDE 扩展会共享登录缓存。OpenAI Docs 说明缓存可能位于 `auth.json`，也可能位于操作系统 keyring。

野菜API采取以下规则：

- 配置野菜 provider 时完全不读出、复制、覆盖或删除官方登录秘密。
- 不把野菜 Key 写进 `auth.json`。
- 不改变用户选择的 `file`、`keyring` 或 `auto` 登录缓存策略。
- 删除野菜 provider 时不执行 `codex logout`。
- 用户的 OpenAI官方登录仍保留；当 `model_provider = "yeschoy"` 时请求由野菜 provider 的 `auth.command` 取得工具 Key。

这比 CC Switch 为兼容旧行为而在 `auth.json`、`experimental_bearer_token` 和 provider 切换之间迁移 Key 的路径更窄，也更适合只有一个第一方中转服务的官方客户端。

### 5.5 TOML 合并规则

- 使用 TOML AST / `toml_edit` 一类结构化编辑方式，尽量保留注释、顺序和未知字段。
- 若 `[model_providers.yeschoy]` 已存在但不是野菜配置，显示 provider ID 冲突并停止。
- 写入前校验完整文件和目标表，不接受重复键、错误类型或不兼容的认证组合。
- 不同时设置 `auth`、`env_key`、`experimental_bearer_token` 或 `requires_openai_auth`；OpenAI Docs 明确这些认证方式互斥。
- `model_provider` 指向用户其他 provider 时，须在预览中明确会切换当前 provider；其他 provider 表保持不变。

### 5.6 写后验证

至少执行：

1. TOML 重新解析。
2. 读回 `model`、`model_provider`、野菜 provider 和 auth 子表。
3. 确认 `config.toml`、配置收据及 `auth.json` 中未新增野菜完整 Key。
4. helper 仅做存在性和权限检查，不在诊断输出中显示 stdout。
5. 在空临时目录通过版本适配器规定的 Codex 非交互命令发送固定短请求。
6. 确认实际请求走 `/v1/responses`，并区分认证、余额、模型、协议、TLS、超时和服务端错误。

V1保守提示“新开 Codex 会话后生效”。不尝试强制关闭用户正在运行的 Codex 进程。

## 6. 状态模型

两个适配器对外统一返回以下状态，不把底层异常压成一个“失败”：

| 状态 | 小白文案 | 是否允许一键配置 |
|---|---|---|
| `not_found` | 未发现该工具 | 否；只给安装说明 |
| `installed_supported` | 已发现，可以配置 | 是 |
| `installed_unsupported` | 版本过旧或尚未验证 | 否；给升级说明，不自动升级 |
| `installed_broken` | 已安装，但当前无法运行 | 否；进入诊断 |
| `target_ambiguous` | 发现多套安装，需要选择 | 否，选择后继续 |
| `config_missing` | 尚未配置 | 是，可创建 |
| `config_invalid` | 原配置格式有误 | 否；不覆盖，先诊断 |
| `foreign_config` | 当前连接到其他服务 | 用户确认替换后允许 |
| `managed_healthy` | 野菜API已配置并验证 | 可重新验证或改线路/模型 |
| `managed_stale` | 配置存在，但验证已过期 | 可重新验证 |
| `external_override` | 其他设置正在覆盖野菜API | 否，先解决冲突 |
| `external_change_conflict` | 配置已被其他程序修改 | 否，重新读取后再决定 |
| `credential_unavailable` | 系统凭据不可用或Key缺失 | 否，不降级明文 |
| `auth_failed` | 工具Key已失效 | 可安全轮换 |
| `balance_insufficient` | 余额不足 | 否；打开钱包 |
| `model_unavailable` | 当前模型不可用 | 否；要求重新选择，不静默替换 |
| `protocol_incompatible` | 当前模型不支持这个工具 | 否；显示兼容模型 |
| `network_failed` | 当前线路无法连接 | 可换线路后重新配置 |
| `rollback_required` | 上次配置未完成，正在恢复 | 否 |
| `recovery_failed` | 自动恢复失败，需要人工处理 | 否；保留操作 ID 和加密快照 |

只有工具级最小请求成功后才显示 `managed_healthy`。直接 HTTP 探测成功但工具调用失败时，必须显示“线路可达，工具配置仍有问题”。

## 7. 从 CC Switch 保留什么、改掉什么

### 7.1 建议保留

- Tauri/Rust 跨平台基础。
- 登录 shell、PATH 默认命中和常见目录兜底发现。
- Windows `.cmd`、App Execution Alias、PATH 合并和 WSL 路径处理经验。
- JSON/TOML 结构化解析。
- 临时文件加 rename 的原子写入。
- Codex 多文件写入前捕获状态、失败恢复和并发代次保护的思路。

### 7.2 必须改造

| CC Switch 当前方向 | 野菜API V1方向 |
|---|---|
| 通用多 provider 管理器 | 只配置野菜API第一方服务 |
| Claude provider 快照可整体写回 `settings.json` | 只 patch 明确拥有的 JSON Pointer |
| Codex 为兼容多种 provider 可在 `auth.json` 或 `experimental_bearer_token` 搬运 Key | 不动官方登录；只用 provider `auth.command` 从系统凭据库取 Key |
| 允许本地代理完成协议转换 | V1要求目标工具的原生协议端点；Codex 使用 Responses |
| 模型目录与 provider 表可由本地 provider 数据生成 | 模型事实来自 NewAPI和版本化野菜目录 |
| 备份、云同步和多项目切换能力 | V1不提供历史备份、云备份或通用 provider 切换 |
| 面向懂配置的用户展示 JSON/TOML | 默认只说“工具、线路、模型、价格、是否可用”；高级详情才显示字段 |

因此，“直接 fork”仍然成立，但复用的是经过重新审计的配置内核和跨平台经验，不是把 CC Switch 换 Logo 后发布。

## 8. 仍然阻止适配契约冻结的事项

1. Claude Code `apiKeyHelper` 与 Codex `auth.command` 的精确支持版本和 Windows/macOS 实机样本；产品已确认精确版本白名单策略，候选验证集合见[工具版本支持与验证矩阵草案](./野菜API-V1-工具版本支持与验证矩阵.md)。
2. 凭据助手在签名、自动更新、用户移动 macOS App、Windows安装路径变化后的稳定可执行路径。
3. 实际部署的两条野菜线路是否都支持 Claude `x-api-key`、`/v1/messages`、Codex bearer、`/v1/responses` 及流式返回。
4. 首批模型清单及 Claude Code 的主模型/Sonnet/Opus/Haiku 映射。
5. 每个支持版本的非交互工具级最小验证命令、超时和安全参数。
6. WSL 是 V1正式适配目标还是只读发现/后续支持，需要在平台矩阵中明确。
7. 已存在同名字段、其他 credential helper 或外部环境变量时，小白文案和确认流程的最终交互稿。

## 9. 已确认的产品硬规则

2026-08-29，产品负责人确认：

> Claude Code 和 Codex 的野菜工具 Key 都只保存在系统凭据库，通过官方凭据助手按需提供；不支持该机制的旧版本只提示升级，绝不为了兼容而把 Key 明文写进配置文件。

因此，此项不再是待选方案。后续适配、测试和发布只能证明某一版本是否满足规则，不能用“兼容旧版本”为理由放宽规则。OpenCode、Pi 和 DSH 的同标准收敛另见对应适配契约草案。

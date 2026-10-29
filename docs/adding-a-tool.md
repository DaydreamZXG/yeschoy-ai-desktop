# 新增一个可一键配置的应用

> 核对基准：2026-10-30，v0.5.1，`main`@`fb7e8ea5` + 工作区改动。接入链路已经迁到四层骨架，**旧版本文里「改 `tool_activation.rs` 的接入流程分支」的说法已失效**。架构总览见 [`architecture.md`](architecture.md)。

## 两条接入路径，先选一条

| | 主干（一键接入） | 独立接入 |
| --- | --- | --- |
| 定义 | `TARGETS` 里的目标 | 自己的 `#[tauri::command]` 三件套 |
| 触发 | 「我的应用」里选应用 → 选模型 / 计费分组 → 预览 → 确认 | 设置页里专门的卡片 |
| 令牌 | 由接入用例铸造、复用、退役 | 无，或由工具自己登录 |
| 恢复 | 写进 `connection_recovery` 加密基线，可「恢复原设置」 | 自己的 revert 命令 |
| 写锁 | 与所有接入共用 `activation_lock` | 无 |
| 现状 | 8 个工具（见下表） | 仅 `cursor_agent` |

**默认选主干。** 只有当一个工具**必须替用户做它自己的登录 / 开关操作**（例如 Cursor Desktop 的自定义服务要写它的 `state.vscdb` 并在界面里勾选）时，才退到独立接入——因为主干的产品不变量是「你确认之前我不写任何东西」，而这类工具有一部分状态藏在应用自己手里。

---

## 路径 A：主干（推荐）

### Rust

| 文件 | 改什么 |
| --- | --- |
| `tool_adapters/<id>.rs`（新文件） | 实现 `prepare_*` 生成 `FileChange` 计划、格式读回校验、`SharedCommit` / `SharedRollback` |
| `tool_adapters/mod.rs` | `TARGETS` 加条目（**tool id 的唯一来源**） |
| `application/activation/lifecycle.rs` | `prepare_adapter` 的 match 加分支持；若需本地桥或运行时，加进 `HelperRuntimes` 与启停分发 |
| `application/connections/mod.rs` | `CONNECTION_TOOLS` 加条目（连接检查 / 恢复） |
| `connection_recovery.rs` | 备份与恢复涉及的文件集合 |
| `exit_restore.rs` | 退出时还原的列表 |
| `open_connection.rs` / `config_reveal.rs` | 「打开使用」/「打开配置文件夹」 |
| `tool_credentials.rs` | 该工具的令牌命名与作用域 |
| `desktop_app_discovery*` / `tool_discovery*` | 本机检测 |
| `manual_locations.rs` | 用户手动指定位置 |
| `app_installation/catalog.rs` | 安装引导链接 |

> 注意：**不要再往 `tool_activation.rs` 加分支**——它现在只有 3 个 `#[tauri::command]` 门面，接入逻辑在 `application/activation/service.rs`。

### 前端

| 文件 | 改什么 |
| --- | --- |
| `configuration/activation.ts` | `ACTIVATION_TOOL_IDS` 加 id（**必须与 `TARGETS` 一致**） |
| `workbench/appCatalog.ts` | 展示清单：从「即将支持」移到正式列表 |
| `features/activation/previewRules.ts` / `applicationChoices.ts` | 预览页的分组与展示名 |
| `installation/toolInstallGuide.ts` | 安装引导文案 |
| `settings/AdvancedConnectionDetails.tsx`、`QuitAssistant.tsx` | 高级连接详情、退出助手 |
| `i18n/locales/{zh,en}.json` | `yeschoyCatalog.appDescription.<id>` 等，**两边同时改** |
| `dev/mockIpc.ts` | 假数据场景（否则 uxaudit 页看不到） |

### 必跑

```bash
pnpm typecheck && pnpm test:unit
cargo fmt --check --manifest-path src-tauri/Cargo.toml
cargo clippy --locked --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
python3 scripts/ci/run-rust-tests.py
```

新增适配器务必补：写入即读回一致、用户既有字段被保留、回滚能精确还原原始字节。

---

## 路径 B：独立接入（Cursor 式）

只有在上面「必须替工具做登录 / 开关」的情况下才用。参照 `tool_adapters/cursor_agent.rs`：

1. **不**进 `TARGETS`，**不**进 `ACTIVATION_TOOL_IDS`——否则两端一致性检查与预览页会把它当成主干目标。
2. 在 `tool_adapters/<id>.rs` 里自带 `prepare` / `rollback` / `undo`，并在文件头用 `//!` 写清**为什么**不走主干（这是后来人最需要的一句话）。
3. 命令走独立命名：`<id>_config_read_only` / `<id>_apply_v1` / `<id>_revert_v1`，在 `lib.rs` 的 `generate_handler!` 注册。
4. 端点之类由原生侧固定的取值，用构建期常量（如 `env!("YESCHOY_AUTHORIZATION_PAGE_ORIGIN")`），**不接受渲染层传来的任意 URL**。
5. 前端挂在 `settings/` 下做成一张卡片（如 `CursorConfigCard.tsx`），并在 `SettingsView.tsx` 挂载；`dev/mockIpc.ts` 补对应假数据。
6. **必须**在 `docs/architecture.md` 第 3.1 节和本文件这里同时登记，说明它不参与主干的恢复与写锁。这是路径 B 唯一的代价：**它不在「恢复原设置」的覆盖范围内。**

---

## 当前已接入的工具

| tool id | 路径 | 说明 |
| --- | --- | --- |
| `claude_code` | A | `~/.claude/settings.json`，本地桥 15728 |
| `claude_desktop` | A | 第三方推理配置，本地桥 15729 |
| `codex_desktop` | A | `~/.codex/config.toml`，本地桥 15731（Responses↔Chat） |
| `pi` | A | `~/.pi/agent/`，JSONC 最小编辑 |
| `opencode` | A | `~/.config/opencode/opencode.json(c)`，本地桥 15732 |
| `workbuddy` | A | `~/.workbuddy/models.json` |
| `dsh_web` / `dsh_desktop` | A | `~/.dsh/profiles/{web,desktop}` |
| `cursor_agent` | **B** | `~/.cursor/cli-config.json`；Cursor Desktop 只读概览 |

> 表中 A 行 8 个 id 必须与 `tool_adapters/mod.rs` 的 `TARGETS` 和 `configuration/activation.ts` 的 `ACTIVATION_TOOL_IDS` 完全一致。

---

## 待办：Codex CLI 作为独立目标

`codex_cli` 还没做。注意它和 `codex_desktop` **共用** `~/.codex/config.toml`，如果将来要加：

- 走路径 A，但两个 id 必须被视为同一组连接——任意一个恢复时两边一起恢复，并在界面上说明；
- 或者放弃独立 id，继续让 `codex_desktop` 同时覆盖 CLI（这是当前的实际行为）。

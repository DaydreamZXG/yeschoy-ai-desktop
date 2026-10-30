# 当前架构（yeschoy-ai-desktop）

> 基线：2026-10-30，v0.5.1，`main`@`fb7e8ea5` **加上工作区里尚未提交的改动**。
> 本文是**只读梳理**的产物：代码一行没改，数字全部来自本机实跑（见第 7 节）。
> 目的不是描述「重构前的样子」或「重构计划」，而是回答一个问题：**今天的代码实际长什么样、边界在哪、哪里还欠着**。
>
> 历史过程文档见 `architecture-refactor.md`（改造前观察与安全边界）和 `architecture-refactor-final.md`（改造过程与验收报告）。两份都在回答「这一轮改了什么」，本文回答「改完之后是什么」。有冲突时以本文和代码为准。

---

## 1. 一句话概括

Tauri 2 桌面应用 = **一个 React 渲染层**（按功能域分成 9 个页面目录 + 一个 `features/activation` 领域切片）+ **一个 Rust 原生层**（命令门面 → 用例 → 领域 / 端口 → 适配器 / 基础设施）。

原生层正在从「平铺的 60 多个模块」演进成**模块化单体 + Ports & Adapters**：

- 新的四层骨架（`domain` / `application` / `adapters` / `infrastructure`）已经建好，**接入链路已经真正跑在上面**；
- 其余领域（账户、安装、站点接入、发现、更新、桥）**仍是平铺模块**，没有迁移；
- 两种结构目前**并存**，这是当前架构最重要的一个事实，也是主要的技术债来源。

---

## 2. 系统全景

```text
┌──────────────────────────── 渲染层 (src/, React 18 + TS strict) ────────────────────────────┐
│                                                                                              │
│  App.tsx  ── AppView 状态机（无路由库：home/account/setup/models/announcements/…）    │
│    │                                                                                         │
│    ├─ workbench/       首页、我的应用、账户、模型、公告、Repair、侧边栏                       │
│    ├─ configuration/   预览 / 接入任务 / 连接管理 / 计费分组 / 模型选择                       │
│    ├─ features/activation/   ← 从预览页下沉的领域切片（hooks + 纯规则 + 展示件）              │
│    ├─ account/         登录会话、余额用量投影、站点深链与信任弹窗                             │
│    ├─ settings/ diagnostics/ installation/ update/ station/                     │
│    └─ shared/api/      统一严格解码的只读 IPC 出口                                             │
│                                                                                              │
│  约束：拿不到文件内容与路径；所有 IPC 响应 invoke<unknown> 后严格解码（多一个字段即判无效）   │
└───────────────────────────────────────┬──────────────────────────────────────────────────────┘
                                        │  Tauri IPC（命令名 = Rust 函数名，载荷 camelCase）
┌───────────────────────────────────────┴────────────────── 原生层 (src-tauri/src/) ───────────┐
│                                                                                              │
│  ① 命令门面层（#[tauri::command]，共 47 个注册项）                                            │
│     lib.rs 的 generate_handler! 是唯一注册表；命令分散在 14 个模块 + 门面文件                  │
│                                                                                              │
│  ② 用例层（新骨架，已接入）                                                                   │
│     application/activation/  service · tokens · capability · lifecycle · session · request      │
│                              · result · error · issuance_recovery · envelope                   │
│     application/connections/ 连接检查 / 恢复 / 退出恢复                                        │
│     application/ports/       ConfigurationPlan · SharedCommit · SharedRollback · ConfigFiles     │
│                              StationProvider                                                  │
│                                                                                              │
│  ③ 领域层 domain/            纯规则：model · credential · recovery · error · configuration      │
│                              · station（仅 std + serde）                                       │
│  ④ 适配器层 adapters/stations/ NewApiStationProvider；tool_adapters/<tool>.rs 各工具格式       │
│  ⑤ 基础设施层 infrastructure/  config（安全 IO + 唯一字节事务）· process（有界进程执行）        │
│                              · home（主目录）· secrets（签发清单）                              │
│                                                                                              │
│  ⑥ 仍为平铺模块的独立领域（未迁移）                                                           │
│     account_v2 · account_finance · app_installation · station_connect* · station_trust         │
│     station_probe · tool_discovery* · desktop_app_discovery* · model_catalog                   │
│     connection_recovery · tool_credentials · app_update · *_bridge · proxy/（vendored）        │
└──────────────────────────────────────────────────────────────────────────────────────────────┘
```

---

## 3. 接入链路（系统的主干，也是唯一完成分层的地方）

这是全应用最关键的一条路径，也是唯一一条真正跑在新骨架上的路径。按调用顺序：

```text
渲染层
  configuration/ConfigurationPreviewView.tsx（只做展示与交互）
    ← features/activation/useActivationSelection · useActivationTargets · useLineSpeedtest
    ← useAccountConnectionReading · useActivationCelebration
    ← configuration/activation.ts（接入任务 useActivationTask、ACTIVATION_TOOL_IDS）
         │
         ▼ invoke("configure_desktop_tool_v2")
原生层
  tool_activation.rs            门面：3 个命令，只做 Tauri 参数注入 + 错误串转换
         │
         ▼
  application/activation/service.rs   configure_desktop_tool(&ActivationContext, request)
         │  ActivationContext 显式传入 AppHandle / 各 State / 各运行时，不读全局
         ▼
    request       校验请求（request_is_valid）
    capability    模型能力与 Codex 传输判定（probe_requested 是唯一开关点）
    tokens        令牌枚举 / 复用 / 铸造 / 退役 + 签发清单（经 StationProvider）
    lifecycle     工具差异表、启停、桌面重载、补偿顺序
    session       单事务准入 / 取消 / 进度事件
         │
         ├──► application/ports/station.rs ──► adapters/stations/new_api.rs（只允许同源 /api/）
         ├──► application/ports/configuration.rs ──► infrastructure/config/transaction.rs
         │        └──► infrastructure/config/files.rs（安全 IO：ensure_safe_target / 有界读写 / owner-only）
         └──► tool_adapters/<tool>.rs（各工具的格式实现，互不依赖）
```

**失败补偿顺序是硬语义**：文件与凭据 → 运行时重启 → 登录态归还。这条顺序写在 `lifecycle` 里，不在各适配器里。

**`Session` 是唯一可以直接碰 Tauri 的地方**（发进度事件）。其余用例只接收显式依赖，所以端到端测试能用 `tauri::test::mock_app()` 把同一份 `configure_desktop_tool` 跑到第一个真实关口。

### 3.1 工具矩阵（两端必须一致的 tool id）

| tool id | 应用 | 写入位置 | 本地桥 |
| --- | --- | --- | --- |
| `claude_code` | Claude Code | `~/.claude/settings.json` | `127.0.0.1:15728` |
| `claude_desktop` | Claude Desktop | 第三方推理配置 | `127.0.0.1:15729` |
| `codex_desktop` | Codex 桌面版 / CLI | `~/.codex/config.toml` | `127.0.0.1:15731`（Responses↔Chat） |
| `pi` | Pi | `~/.pi/agent/` | — |
| `opencode` | OpenCode | `~/.config/opencode/opencode.json(c)` | `127.0.0.1:15732` |
| `workbuddy` | WorkBuddy | `~/.workbuddy/models.json` | — |
| `dsh_web` / `dsh_desktop` | DeepSeek Harness | `~/.dsh/profiles/{web,desktop}` | —（web 起本地工作台） |
| `cursor_agent` ★ | Cursor（CLI） | `~/.cursor/cli-config.json` | — |

唯一来源：Rust `tool_adapters/mod.rs` 的 `TARGETS`（8 项）× 前端 `configuration/activation.ts` 的 `ACTIVATION_TOOL_IDS`（8 项），两端**当前一致**。

> ★ `cursor_agent` 是**刻意设计成第二条接入路径**的工具，不在 `TARGETS` 里：走自己的 3 个命令（`cursor_config_read_only` / `cursor_apply_v1` / `cursor_revert_v1`），不经过接入用例、不共享事务、不受 `ACTIVATION_TOOL_IDS` 约束。原因是 Cursor Desktop 的开关与密钥握在它自己手里（详见 6 节）。它的契约写在 `tool_adapters/cursor_agent.rs` 的 `ipc` 模块头和 [`adding-a-tool.md`](adding-a-tool.md) 的「路径 B」。

---

## 4. 渲染层结构

没有路由库，`AppView` 联合类型 + `App.tsx` 的 `useState` 就是全部路由。8 个视图：

```text
home · account · setup · models · diagnostics · feedback · announcements · settings
```

| 目录 | 近似行数 | 职责 |
| --- | --- | --- |
| `workbench/` | 14.4k | 首页、我的应用、账户、模型、公告、Repair、侧边栏与主样式 |
| `configuration/` | 10.5k | 预览、接入任务、连接管理、计费分组、模型选择 |
| `account/` | 3.1k | 登录会话、余额用量、站点深链与信任弹窗 |
| `features/activation/` | 2.2k | **新的领域切片**：14 个 hooks / 纯规则 / 展示件 |
| `theme/` | 3.0k | 主题 CSS（`new-api*`、`system`、`motion`） |
| `settings/` `diagnostics/` `installation/` | 2.5k / 2.6k / 2.2k | 设置、诊断与反馈、安装引导 |
| `model-profiles/` | 1.1k | 模型能力目录（`catalog.json`，与 Rust 共用） |
| `components/` `station/` `update/` `distribution/` `candidate/` `shared/` `i18n/` `dev/` | 合计约 3k | 基础组件、站点探测、更新、发行分轨、共享 API、文案、仅开发用的假数据 |

**当前仍在改进的两点**：

1. `ConfigurationPreviewView.tsx`（1879 行）已经下沉了很多逻辑，但**仍直接持有** `useRef` / `useCallback` 状态组合，并把 `connectionLifecycleMode` 等一整套 `features/activation/previewRules` 的符号**原样 re-export**。这个 re-export 块是给旧 import 站点留的兼容层——它说明下沉还没收尾。
2. `workbench-v2.css` 单文件 7.6k 行，是样式的唯一集散地。样式没有 Tailwind 管线，全部靠语义 class + `src/tokens.css` 变量，由 `designSystem.test.ts` 守着（字号 ≥ 12px、间距必须用 `var(--space-*)`）。

---

## 5. 原生层：新旧两种结构的对照

### 5.1 已分层的部分（接入链路）

见第 3 节。依赖方向是单向的：

```text
命令 → application → domain / ports
                     ▲        ▲
            adapters ┘        └ infrastructure
```

`domain/` 只依赖 std + serde，**可以用独立 `rustc --test` 编译**，这是这条边界的硬证明。

**真正的成果**（对比改造前）：

| 指标 | 改造前 | 现在 |
| --- | --- | --- |
| `tool_activation.rs` | 5154 行（IPC + 准入 + 令牌 + 探测 + 分发 + 回滚全在一个文件） | **101 行**（3 个命令门面） |
| `ConfigurationPreviewView.tsx` | 2890 行 | **1879 行** |
| 配置写入事务 | 7 个适配器各写一份 | 统一 `SharedCommit` / `SharedRollback` + 唯一字节事务 |
| JSONC 写入 | 整份重排，注释丢失 | `tool_adapters/jsonc.rs` 原文最小编辑，失败才回退重排 |
| 第三方站点凭据 | 只认 `yeschoy.com`，第三方站点接入后凭据被判无效 | `StationOrigin` 统一校验，历史 origin 保持可读 |
| 令牌拒绝日志 | 记录服务器返回的自由文本（可能夹带密钥） | 只记 `op=… status=…` |
| Codex 付费探测 | 接入时默认发真实请求 | 用户显式开启（`probe_requested`，默认关） |

### 5.2 仍是平铺模块的部分

这些模块**没有** `domain` / `application` / `adapters` 之分，直接互相 `use crate::…`：

| 领域 | 模块 | 规模 |
| --- | --- | --- |
| 账户 | `account_v2.rs`、`account_finance.rs` | 3.4k + 0.8k |
| 安装 | `app_installation/`（含 `zip_package`、`platform`、`tests`） | 2.9k |
| 站点接入 | `station_connect*`、`station_trust`、`station_probe`、`oauth_pkce` | 约 3.9k |
| 发现 | `tool_discovery*`、`desktop_app_discovery*`、`manual_locations`、`windows_cli_locations`、`shell_environment` | 约 4.3k |
| 恢复与凭据 | `connection_recovery`、`tool_credentials`、`exit_restore`、`shutdown_coordinator` | 约 4.5k |
| 本地桥 | `local_bridge`、`claude_bridge`、`codex_responses_bridge`、`opencode_bridge` | 约 4.0k + `proxy/` |
| 其余 | `app_update`、`model_catalog`、`tool_model_profile`、`health_snapshot`、`residue_cleanup`、`prompt_injection`、`connectivity*`、`service_catalog*` | — |

### 5.3 `*_core.rs` 的约定

`connectivity_core`、`desktop_app_discovery_core`、`service_catalog_core`、`tool_discovery_core`、`tool_selection_core` 是各领域「**可移植的纯逻辑一半**」：不碰进程、文件系统与 Tauri，被同名的平台模块包一层。这是分层之前一次性做出来的、成本最低的一条边界，值得保留并命名为统一约定。

### 5.4 `proxy/` 是封死的

`src-tauri/src/proxy/` 从 cc-switch 逐字节 vendored，除 `mod.rs` / `VENDOR.md` 外与上游完全一致，由 `vendored_files_are_unmodified` 用 sha256 守着。野菜自己的协议逻辑只能写在**外面包一层**（`codex_responses_bridge.rs`、`provider.rs`）。本次梳理确认该目录**零改动**。

---

## 6. 已知问题与欠账（按优先级）

### ✅ 已修复（2026-10-30）：旧骨架残留的 import 注释已经过期

原先 `adapters/stations/mod.rs`、`application/ports/station.rs`、`adapters/stations/new_api.rs` 与 `infrastructure/secrets/issuance.rs` 上有 6 处 `#[allow(dead_code)]`，其中两条注释直接说错了事实：

```rust
#[allow(dead_code)]
// Wired into activation in the station-provider integration step.   ← 早就接完了
// Activation integration wires this provider next.                  ← 早就接完了
```

`NewApiStationProvider` 已经在 `tokens.rs` 与 `capability.rs` 里真实使用，`StationProvider::account_json` 也已被调用。处理方式：

- 删掉 `ports/station.rs` 整体 `#![allow(dead_code)]` 与 `adapters/stations/mod.rs` 的 `#[allow(dead_code, unused_imports)]`，以及 `new_api.rs` 的 impl 级豁免——**实测确无未使用告警**，说明这是纯注释债务，没有隐藏的僵尸代码；
- 保留 `origin()`、`STALE_AFTER`、`now_epoch_ms()` 上真正必要的单点豁免，但把注释改成如实的「预留，当前无调用方」；
- `cargo clippy --all-targets -- -D warnings` 与 852 项原生测试均通过，行为零变化。

> 教训：`#[allow(dead_code)]` 会把「注释与代码不一致」压成静默状态。新加豁免时，注释必须写**为什么现在没有调用方**，而不是「以后会接上」。

### ✅ 已拆解（2026-10-30）：`connection_recovery` 的互指三角

改造前是 `connection_recovery → tool_credentials → tool_adapters` 反指回去，其中 recovery 还直接 reach 进 `tool_adapters::dsh_web`。三条边现在都断了：

| 原来的边 | 现在 |
| --- | --- |
| `connection_recovery → tool_credentials::ToolCredential` | 类型下沉到 **`domain/credential.rs`**；凭据库与恢复变成**兄弟**，不再互相拥有 |
| `connection_recovery → tool_adapters::dsh_web::*`（格式化恢复） | **`domain/recovery.rs`** 定义 `RecoveryHooks`（`owns` / `strip_owned` / `restore_document`），DSH 实现它，`tool_adapters::recovery_hooks(tool_id)` 做查表 |
| `connection_recovery → tool_adapters::user_home()` | 搬到 **`infrastructure/home.rs`**（旧路径仍转发，30 处调用点不动） |

recovery 的生产代码现在只剩对 `tool_adapters` **聚合注册表**的调用（`recovery_hooks` / `lock_for_restore`），不再点名任何一个具体适配器。这层间接是刻意的：recovery 拥有日志与合并顺序，工具拥有自己的文件格式，谁都不该知道对方的名字。

`RecoveryHooks` 的契约写在 trait 文档里并逐条说明为什么安全：`owns` 只能按路径判断、必须幂等、可以答 `None` 回落到通用合并、且不得抹掉用户后来的修改。未知工具得到 `NoHooks`，行为与改造前完全一致。

> 仍在 P3 的：`connection_recovery.rs` 本身仍有 2.3k 行。拆分方向变清晰了（日志编解码 / 合并 / 补偿可以各自独立），但那是另一个量级的改动。

### ✅ 已归位（2026-10-30）：`tool_adapters/common.rs` 的三类混杂内容已拆开

原 561 行里混着三样不属于「工具适配器」的东西，现在各回各家：

| 内容 | 去向 | 理由 |
| --- | --- | --- |
| 进程执行（`run_bounded` / `ProcessResult` / `ProcessFailure` / `cli_runtime_*`） | **`infrastructure/process/`**（新） | 发现、安装、备份也跑 CLI；这里不该知道任何工具格式 |
| 模型目录谓词（`validate_catalog` / `catalog_matches` / `default_matches`） | **`domain/model.rs`** | 纯规则，只依赖 std + serde |
| 文件 IO（`snapshot` / `atomic_write` / `FileTransaction` …） | 原本就转发到 `infrastructure/config`，现在只是不再经过 `common::` 的间接层（内部仍保留转发，兼容既有调用） | 真实归属一直是 config |

外部调用点已改成直连真实归属：`app_installation/platform.rs`、`tool_discovery.rs`、`tool_adapters/{codex_desktop,dsh_web,terminal_launch}.rs`。**`common.rs` 从 561 行降到 221 行**，剩下的只有测试夹具（`temporary_working_directory`、`test_support::Script`）与转发声明。

进程执行的 4 个测试随之迁入 `infrastructure::process::process_tests`（`--list` 已确认在新位置运行），**没有删除、没有 `#[ignore]`**，全量仍是 852 通过。

### ✅ 已定性（2026-10-30）：Cursor 是「独立接入」，不走主干

`cursor_agent` 有完整的读写回三件套，但不走 `ConfigurationPlan` / `SharedCommit`、不参与激活锁、不写 `connection_recovery` 基线、不受「两端 tool id 一致」约束。

**结论是保留为第二条接入路径，而不是并入 `TARGETS`。** 理由是 Cursor Desktop 把服务地址与密钥存在它自己的 `state.vscdb` 和钥匙串里、启用还要在它界面勾选；硬塞进主干等于替用户做它的登录与开关，违反「确认前不写任何东西」的精神。Cursor 这里只写 CLI 的 `~/.cursor/cli-config.json`，端点固定取构建期常量。

落地方式（契约已写死在文档里，不再靠口头约定）：

- [`tool_adapters/cursor_agent.rs`](../src-tauri/src/tool_adapters/cursor_agent.rs#L539) 的 `ipc` 模块头用 `//!` 说明了「为什么不走主干」以及将来要并入需要补哪三件事；
- [`docs/adding-a-tool.md`](adding-a-tool.md) 改成「路径 A 主干 / 路径 B 独立接入」二选一，并给出各自的文件清单；
- 代价被明确写出来：**路径 B 不在「恢复原设置」的覆盖范围内**，只有它自己的 revert 命令。

### ✅ 已补齐（2026-10-30）：安装流程的只读预览

`app_installation` 原先只有 `manage_app_installation_v2` 一个命令，用户要看到「装什么、从哪来、什么模式」只能靠 `inspect`——而它和 `start`/`cancel`/`confirm` 共用一个命令，属于接入链路早就改掉的那种「一条命令承担全部语义」。

新增 **`installation_preview_read_only`**：

- 入参只有 `requestId` + `toolId`，出参是闭合词汇：平台、架构、安装模式、是否有官方包、是否编入镜像、是否有引导页。**不含 URL、路径、主机名**；
- 原生侧是纯查表，不碰网络、文件、钥匙串，也不进入安装状态机——产品不变量第 1 条因此在安装侧也成立；
- 未知工具返回稳定原因码 `invalid_installation_preview`，不会退化成「空的合法预览」；
- 前端 `decodeInstallationPreview` 严格校验（字段集合必须完全一致、取值必须在白名单、`packageAvailable` 与 `guideAvailable` 不能同时为假），走 `invokeDecoded` 统一出口；`dev/mockIpc.ts` 同步了假数据。

原生 2 个测试 + 前端 9 个测试守着它，其中一条明确断言「只调用只读命令」。

### P3 — 可维护性：剩余大文件

| 文件 | 行数 | 备注 |
| --- | --- | --- |
| `account_v2.rs` | 3373 | 账户会话 / 授权 / 公告 / 钱包全在一个文件 |
| `tool_adapters/dsh_web.rs` | 2661 | DSH 格式 + profile 锁 + 工作台启动 |
| `connection_recovery.rs` | 2361 | 见 P3 的拆分方向 |
| `desktop_app_discovery.rs` | 1817 | 平台扫描 |
| `workbench/workbench-v2.css` | 7600 | 样式集散地 |
| `configuration/DailyUse.test.tsx` | 2290 | 测试文件本身过大 |

### P3 — 工程：未跟踪的嵌套检出

`yeschoy-ai-desktop/`（112 MB）是工作区里一份**未跟踪的嵌套副本**。`vitest.config.ts` 已通过 `include` 限定根仓库源码来隔离它（这条改动是对的），但副本本身仍占空间、会让 `git status` 噪音变大、也会让编辑器索引变慢。建议用户确认后删除或加进 `.gitignore`。

---

## 7. 本次实跑验证

| 检查 | 命令 | 结果 |
| --- | --- | --- |
| 前端类型 | `pnpm typecheck` | ✅ 通过 |
| 前端单测 | `pnpm test:unit` | ✅ **80 文件 / 735 项全通过** |
| 原生测试 | `python3 scripts/ci/run-rust-tests.py` | ✅ **846 通过，0 失败，3 个已声明真机测试忽略** |
| 原生格式 | `cargo fmt --check` | ✅ 通过 |
| 原生 lint | `cargo clippy --locked --all-targets -- -D warnings` | ✅ 通过（本次梳理后补跑） |
| vendored 完整性 | `git status src-tauri/src/proxy/` | ✅ 零改动 |
| tool id 一致性 | 人工比对 `TARGETS` × `ACTIVATION_TOOL_IDS` | ✅ 8 × 8 一致 |

> 第 6 节标记 ✅ 的项已在本次梳理后完成修复；上表的 fmt / clippy 反映修复后的状态。
> 未对任何真实工具配置、钥匙串或付费接口做过写操作——符合产品不变量。

---

## 8. 建议的下一步（按收益排序）

1. **`account_v2.rs` 按用例拆分**（2–3 天）：它是下一个最该上四层骨架的领域。
2. **删掉 `ConfigurationPreviewView` 的 re-export 兼容块**（半天）：完成预览页的下沉收尾（目前只剩测试还在从预览页导入这些符号）。
3. **拆 `connection_recovery.rs`**（2–3 天）：日志编解码 / 合并 / 补偿可以各自独立，当前 2.3k 行。

---

## 9. 扩展指引

**新增一个中转站（Station Provider）**：在 `adapters/stations/` 实现 `StationProvider`（只允许同源 `/api/` 相对路径），用 `StationOrigin` 表示站点身份，首次信任仍走既有确认弹窗。工具适配器与配置格式无需改动。

**新增一个 AI 工具**：见 `docs/adding-a-tool.md`（约 20 个文件的联动清单）。关键点：`tool_adapters/mod.rs` 的 `TARGETS` 与前端 `ACTIVATION_TOOL_IDS` 必须同时加，且前端展示信息在 `workbench/appCatalog.ts`。

**新增一个领域到四层骨架**：参照 `application/activation` 的切法——纯规则进 `domain/`，用例进 `application/<domain>/`，外部能力定义成 `application/ports/<x>.rs`，具体实现在 `adapters/` 或 `infrastructure/`，**门面只保留 `#[tauri::command]` 包装**。

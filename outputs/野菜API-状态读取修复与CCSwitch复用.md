# 接入状态读取问题与 CC Switch 复用检查

当前状态：按用户确认的提交 `1c77a96b0ed5cc48ee053a66c8f4bd41c409f867` 为新基线完成限定代码修复和模拟数据回归。未操作用户本机 Codex/Claude，未进行 Windows/macOS 安装版端到端验证，未打包、提交、推送或发布本次修改。不能把单元回归等同于用户设备问题已消失。

## 修改前已确认的代码事实

1. 首页的应用扫描与接入状态查询是两条独立路径。`src/workbench/AppLibraryView.tsx` 在扫描失败或 `connections.error` 时显示同一条黄色提示；接入列表尚无成功结果时又将数量计算为 0，并将找到的应用显示为“待接入”。这会把未知状态误呈现为未配置，不能据此推断网关故障或配置丢失。
2. `src/configuration/connections.tsx` 手写刷新与请求序号控制，每次窗口 focus 都可再次调用原生命令。错误处理中仅保存布尔值，丢失了查询超时、操作占用和响应校验失败之间的区别。
3. `src-tauri/src/tool_activation.rs` 的 `manage_tool_connections_v1` 与接入操作共用 `ACTIVATION_LOCK`，当前等锁上限 8 秒；前端查询总等待上限为 15 秒。取得锁后会同步读取恢复存储与系统凭据。`inspect_connection` 部分路径会对同一工具读取两次凭据。
4. 未取得反馈设备的关联错误码，尚不能确定此次提示实际由哪一分支触发。不得将某一推测写成已复现根因。

## CC Switch 原实现与当前运行边界

- `src-tauri/src/commands/provider.rs` 保留 `get_current_provider` 等入口；`src-tauri/src/services/provider/mod.rs` 保留 `ProviderService::current`、`read_live_settings` 等功能。
- 原 `current` 路径读取 CC Switch 的 settings/SQLite 供应商状态；它不是野菜加密恢复记录的直接替代品，也不单独证明远端模型可用。
- `src/lib/query/queryClient.ts` 与 `src/lib/query/queries.ts` 保留已有 TanStack Query 查询缓存机制。
- 当前 `src-tauri/src/lib.rs` 没有注册这些供应商命令和 `AppState`，而是注册 `tool_activation`、账户及安装等野菜命令。新首页也没有复用原查询缓存路径。
- 原 `useProvidersQuery` 同样有捕获异常后返回空值的处理，不应未经检查整体照搬。应复用缓存、请求去重、保留成功数据等基础机制，纠正错误语义。

## 已实施的限定修复

1. 复用现有 QueryClient 管理接入状态读取，不增加新的 provider 数据库、缓存实现或第三套接入记录。
2. 首次读取失败显示“接入状态待确认”，不显示已确认的 0 或“待接入”；保留成功快照时明确标注为上次确认结果。
3. 合并重复检查；恢复操作后，晚到的检查结果不能覆盖恢复结果。
4. 用封闭错误码与请求关联 ID 区分等待超时、操作占用、无效响应和调用失败，不展示原始异常、密钥或本机路径。
5. 采用仓库已有的 blocking-worker 模式承载系统凭据读取，保持与写入操作的互斥，复用已有等待界限并避免重复读取凭据。
6. 保留直连端点、计费、模型配置、原 IPC 数据格式和加密恢复机制；不迁移 CC Switch 数据库，不操作用户实际安装的 Codex/Claude。

另补齐单应用读取失败的边界：当一项投影为 `unavailable`、其他应用读取成功时，不再将该项显示成“待接入”；接入页阻止使用默认模型覆盖未知设置，提供重新读取按钮。只有成功确认的 `not_connected` 才作为未接入展示。

读操作采用原有的 8 秒等锁与 15 秒总预算，系统凭据读取可能无法由操作系统即时取消。超时后工作线程仍持锁直到退出，避免与配置写入并发；新的读取不会再启动第二个系统凭据任务。这是保护数据一致性的取舍，不是保证所有设备都能在 15 秒内读取成功。

## 本次验证及边界

- 前端：11 个测试文件、190 项通过，报告 `outputs/ru076/frontend-results.json`。重跑命令：`pnpm exec vitest run --dir src src/configuration src/workbench --exclude 'release/**' --exclude 'work/**' --exclude 'tests/**' --reporter=json --outputFile=outputs/ru076/frontend-results.json`。覆盖首次失败、超时、离线但可读本机状态、重复 focus / 手动刷新合并、StrictMode 重放、后台刷新保留快照、恢复与旧查询竞态、错误脱敏、单应用未知状态和日常接入交互。
- 原生：`cargo +1.94.0 test --manifest-path src-tauri/Cargo.toml --lib ru076_ --locked --offline`，3 项通过，仅注入模拟读取器，不调用真实钥匙串、不接触用户应用。验证工作线程不阻塞当前线程运行时、超时后互斥锁仍保护读写、异常映射为固定错误码。
- `pnpm exec tsc --noEmit` 与 `git diff --check` 通过。
- 全仓库 `cargo fmt --check` 存在直连改造提交中已有的格式差异，未批量改动无关文件。本次修改的 Rust 文件单独执行格式化。
- 一次未加目录限制的 Vitest 调用因旧 DMG 暂存目录的 Applications 符号链接递归而失败；后续使用仓库既有的 `--dir src` 和排除目录约定完成测试，没有改动应用目录。
- 截图不能区分当时是锁忙、系统存储慢、IPC 返回丢失或格式不匹配。本次修正可复现的代码缺陷并增加安全诊断编号，不伪称已在反馈设备复现根因。若该设备继续失败，应提供新版“查看诊断信息”中的错误码和编号，不需提供密钥或配置文件。

## 治理工具限制与用户确认

按 `govern-product-build` 规范登记了范围有限的 `RU-076`，但 review 尚未通过。修复前 review 报告：`.product-governance/reports/RU-076.review.json`。

现有 `RU-075` 的验收快照对应提交 `04382354f93d597ab1ae1b46c5b8af7fcfe2e43a`；当前提交为 `1c77a96b0ed5cc48ee053a66c8f4bd41c409f867`。校验指出 45 处已验收文件与后来提交的工作树不同，包含直连改造、Windows 安装器及 UI 调整。这是过时验收证据与当前代码的差异，不是 45 个已确认功能缺陷。

用户在明确获知该限制后，回复“对的”，确认以当前提交为新基线保留直连并复用现有组件继续修复，登记为 `DEC-121` / `E-USER-097`。按技能无法干净衔接时的回退流程执行限定修复与模拟数据回归；未回滚 DeepSeek 改造，未修改技能校验器、历史通过记录或冻结哈希。`RU-076` 仍未机械封存，本次直接测试报告不冒充治理验证器签发的已通过报告。后续发布前仍需独立处理治理衔接及 Windows/macOS 安装版验证。

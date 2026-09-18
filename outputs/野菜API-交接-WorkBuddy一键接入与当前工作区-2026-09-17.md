# 野菜API 桌面端交接：WorkBuddy 一键接入与当前工作区

> 已过期。请改用 `outputs/野菜API-交接-桌面助手当前状态-2026-09-17.md`（HEAD `5d8e35ae`）。下面的脏工作区说明和旧 HEAD 不要再执行。

日期：2026-09-17  
仓库：`/Users/zxg/.codex/worktrees/c485/xiaobai-ai-desktop`  
当前分支：`codex/yeschoy-0.4.14-updater`  
当前 HEAD：`65b3e497`（`chore: align WorkBuddy closed-set comments`）  
当前应用版本：`0.4.20`  
相对 `origin/codex/yeschoy-0.4.14-updater`：本地超前 9 个提交，**未推送**。

## 1. 收尾后的状态

脏工作区已经拆提交。源码工作树应只剩本交接文档未跟踪。不要 `git reset --hard`。

已落地：

- WorkBuddy 一键接入（直接写 `~/.workbuddy/models.json`，不走本地网关，不强制重启）。
- Codex 历史接管改为提交配置后再改写 session。
- 安装镜像公网失败后走直连 DNS。
- DSH 源码构建校验流水线。
- 审查缺陷：WorkBuddy 整对象恢复、`availableModels` 清理、已运行不重复拉起、Windows 身份认官方 NSIS/`腾讯` 同时拒绝 Helper 与 Contoso。

未做、发布前仍要做：

- 推送、升版本、四端打包、公证/签名、双通道发布。
- Windows/macOS 隔离真机 smoke（热加载、请求 URL、退出恢复）。
- `useCustomProtocol` 与能力字段需真机确认，不要再猜。

## 2. 本轮目标与结论

目标是把 WorkBuddy 加入野菜API桌面助手，让不会手工配置的用户也能一键接入，同时遵守以下边界：

1. 直接修改 WorkBuddy 官方配置，不使用本地网关。
2. 不要求退出或重启 WorkBuddy；配置热加载后，由用户在新对话中选择野菜模型。
3. 不强行改 WorkBuddy 默认模型，不强行切换已经打开的对话。
4. 保留用户原有模型、未知字段、偏好以及配置根结构。
5. 失败时自动回滚；退出野菜助手或退出账号时能按现有恢复体系处理。
6. 不碰真实用户配置做测试，全部使用隔离 fixture/临时目录。

当前代码已经覆盖上述范围，并把 WorkBuddy 纳入应用发现、接入、打开、恢复、退出和界面闭集。

## 3. WorkBuddy 接入是怎么工作的

### 3.1 写入位置

配置文件：

```text
~/.workbuddy/models.json
```

适配器同时兼容两种已知结构：

```json
[
  { "id": "model-id" }
]
```

以及：

```json
{
  "models": [{ "id": "model-id" }],
  "availableModels": ["model-id"],
  "其他未来字段": "保留"
}
```

如果 JSON 损坏、`models` 不是数组，或者写入后读回校验失败，流程会失败关闭，不会猜测修复或覆盖整份配置。

### 3.2 野菜API负责的字段

对用户本次选择的模型，野菜API会维护：

- `id`
- `name`
- `vendor`（固定为 `野菜API`）
- `url`
- `apiKey`
- `useCustomProtocol`
- `supportsToolCall`
- `supportsImages`
- `supportsReasoning`
- `maxInputTokens`
- `maxOutputTokens`

请求地址由当前野菜API线路生成：

```text
{origin}/v1/chat/completions
```

本地 `127.x` 或 `localhost` HTTP 地址会被拒绝，WorkBuddy 不依赖野菜助手常驻，也不经过本地转发网关。

### 3.3 保留与恢复规则

- 只替换 ID 与本次已选模型相同的记录，避免重复模型。
- 保留其他厂商模型和不归野菜API管理的记录。
- 保留匹配模型上的未知字段，例如用户自定义温度。
- 每个模型使用对应模型和计费分组签发的独立 key，不共用一个错误的通用 key。
- 写入使用现有 `FileTransaction`：先取快照、原子写入、读回验证，失败自动回滚。
- 恢复记录沿用现有加密 recovery receipt；退出恢复和下次启动清理逻辑已经包含 WorkBuddy。

### 3.4 运行中的应用

- WorkBuddy 被定义为热加载型工具，`needs_background("workbuddy", ...) == false`。
- 一键接入不会关闭 WorkBuddy。
- 接入成功后会打开或聚焦实际检测到的那份 WorkBuddy 安装。
- 已有对话可能仍使用旧模型，所以界面明确提示：新建对话，并在模型选择器中选择野菜模型。
- 没有编造 WorkBuddy 的默认模型字段，也没有强行改用户当前会话。

## 4. 安装与应用发现边界

当前已做：

- macOS Bundle ID：`com.tencent.workbuddy.mac`
- Windows 包/产品身份：`Tencent.WorkBuddy` / `WorkBuddy`
- Windows 可执行文件：`WorkBuddy.exe`
- Windows 发现要求 GUI 可执行文件且产品、公司身份匹配腾讯，避免误开同名 helper。
- 未安装时提供 WorkBuddy 官方入口：`https://www.workbuddy.cn/`

当前**没有做**：

- 没有镜像 WorkBuddy 安装包。
- 没有自动下载并静默安装 WorkBuddy。
- 没有为网络受限用户提供自有 CDN 安装源。
- 没有在真实 Windows WorkBuddy 安装上做端到端验证；Windows 目前只有路径和身份 fixture 测试。

因此产品文案应称为“官方引导安装 + 安装后的一键接入”，不能声称已经实现 WorkBuddy 全自动安装。

## 5. 关键代码位置

| 范围 | 文件 | 作用 |
| --- | --- | --- |
| WorkBuddy 配置适配器 | `src-tauri/src/tool_adapters/workbuddy.rs` | 兼容两种 JSON 结构，生成模型配置，原子提交、校验与回滚 |
| 适配器注册 | `src-tauri/src/tool_adapters/mod.rs` | 注册 WorkBuddy 模块 |
| 接入主流程 | `src-tauri/src/tool_activation.rs` | 请求校验、模型支持、专属 token、配置准备、打开应用、恢复路径 |
| 打开/重开 | `src-tauri/src/open_connection.rs` | 校验现有设置并打开准确安装路径 |
| 凭据 | `src-tauri/src/tool_credentials.rs` | WorkBuddy 模型路由和凭据结构 |
| 恢复 | `src-tauri/src/connection_recovery.rs` | WorkBuddy recovery receipt 和自动恢复 |
| 应用发现 | `src-tauri/src/desktop_app_discovery.rs` | 平台上的实际发现流程 |
| 发现规则 | `src-tauri/src/desktop_app_discovery_core.rs` | macOS Bundle ID、Windows 包名/文件/产品/公司身份规则 |
| 安装入口 | `src-tauri/src/app_installation/catalog.rs` | 未安装时打开官方 WorkBuddy 网站 |
| 模型能力 | `src-tauri/src/tool_model_profile.rs` | 新增/传递 `max_output_tokens` 等能力数据 |
| 退出恢复 | `src-tauri/src/exit_restore.rs` | WorkBuddy 加入退出时恢复闭集 |
| 退出账号 | `src-tauri/src/account_v2.rs` | 清理 WorkBuddy 请求诊断状态 |
| 前端接入 | `src/configuration/activation.ts`、`src/configuration/ConfigurationPreviewView.tsx`、`src/configuration/preview.ts` | 工具 ID、配置预览和激活流程 |
| 前端文案 | `src/configuration/copy.ts` | 热加载、完成态和恢复文案 |
| 工具卡片 | `src/workbench/appCatalog.ts` | WorkBuddy 名称、说明和官方图标 |
| 图标 | `src/assets/icons/official-workbuddy.svg` | 随包本地资源，不依赖运行时网络 |
| 退出设置 | `src/settings/QuitAssistant.tsx` | 六个支持工具的精确白名单 |

全局查漏建议：

```bash
rg -n 'workbuddy|WorkBuddy' src src-tauri/src tests
rg -n 'toHaveLength\(5\)|五个|5 个|5个' src src-tauri/src
```

第二条用于防止增加第六个工具后还残留旧的计数断言或文案。

## 6. 已完成验证

截至本交接文档生成前，最后一轮结果：

- TypeScript 类型检查：通过。
- Rust 全量库测试：283 passed，0 failed。
- 前端全量测试：37 个测试文件，463 passed，0 failed。
- `git diff --check`：通过。
- WorkBuddy 三个治理工作包：均通过。

验证命令：

```bash
pnpm typecheck

cd src-tauri
env -u ANTHROPIC_BASE_URL -u ANTHROPIC_AUTH_TOKEN \
  RUSTUP_TOOLCHAIN=1.94.0-x86_64-apple-darwin \
  cargo test --lib

cd ..
pnpm vitest run --dir src --exclude 'release/**' --exclude 'work/**' --exclude 'tests/**'

python3 -m pytest tests/work_packages/ru083/test_workbuddy.py -q
python3 -m pytest tests/work_packages/ru084/test_workbuddy_lifecycle.py -q
python3 -m pytest tests/work_packages/ru085/test_workbuddy_regression.py -q

git diff --check
```

前端测试必须保留 `--dir src`。否则 Vitest 可能扫描 Rust `target` 或发布输出中的重复测试文件，结果不具有可比性。

本机曾遇到 Rust 1.95 的 `rustfmt` 组件冲突，所以最终稳定验证使用了上面的 `1.94.0-x86_64-apple-darwin`。不要在不确认差异的情况下全仓运行格式化。

## 7. 治理与可审计证据

WorkBuddy 被拆为三个 release unit：

| Release Unit | 内容 | 冻结哈希 | 最终成功报告 |
| --- | --- | --- | --- |
| RU-083 | WorkBuddy 直接接入核心 | `sha256:4938d75e9924ee18d0c0c82fe25275b7662ae8e18a08097078b4e9d88d430ae2` | `.product-governance/execution/reports/0e5709cdcaef56f9.50266d34fd7d0a83e1fa.json` |
| RU-084 | 退出恢复和账号生命周期 | `sha256:6a78bfb77b267db41e10cb7c007cc7255626a5e3b090da77ed15c12a913c8397` | `.product-governance/execution/reports/cc4eabbf5cde6a7b.34af3bcf546c06e34d30.json` |
| RU-085 | 六工具闭集与回归一致性 | `sha256:dc02c4dfc91757f9f81a16b6306cb7456ee83a951a06b0b9c4a7d9a8db5ca9f2` | `.product-governance/execution/reports/93c0e83ddde9e4bf.fe4f8e5a022edf6a5f9a.json` |

对应测试：

- `tests/work_packages/ru083/test_workbuddy.py`
- `tests/work_packages/ru084/test_workbuddy_lifecycle.py`
- `tests/work_packages/ru085/test_workbuddy_regression.py`

## 8. 仍需真实设备确认的风险

自动化通过不等于真实客户端已经验收。发布前至少要确认：

1. Windows 真机能够找到官方 WorkBuddy，而不是同名 helper。
2. Windows WorkBuddy 正在运行时，写入后不退出应用即可在新对话模型选择器里看到野菜模型。
3. macOS 官方 WorkBuddy 同样能热加载；本地只观察过安装形态，没有修改真实默认配置做测试。
4. 分别用“配置不存在、根为数组、根为对象、已有同 ID、含未知字段、畸形 JSON”六种 fixture/一次性测试账户验证。
5. 退出野菜助手时能恢复接入前设置；恢复失败时提示自动修复，不要求用户手改 JSON。
6. 退出账号后没有留下野菜API管理的 WorkBuddy 配置或诊断状态。
7. 已有 WorkBuddy 对话不强制换模型；新对话选择后请求确实出现在野菜API最近中转记录里。

真实设备测试前先完整备份 `~/.workbuddy/models.json`，最好使用一次性系统账户或单独的临时 Home 目录，不要直接拿主力配置做首次试验。

## 9. 建议的接手顺序

1. 重跑 §6 的类型、Rust、前端和三个工作包测试。
2. 在 Windows 与 macOS 各做一次隔离的真实 WorkBuddy 接入/恢复 smoke。
3. 处理真实设备发现的问题；不得为“跑通”而改用本地网关或强制重启。
4. 推送后再升版本、生成四个构件：主版 Windows、主版 macOS、朋友版 Windows、朋友版 macOS。
5. 完成安装器验收、macOS 公证、Tauri 更新签名后，才发布到双通道并切换永久下载链接。

## 10. 当前打包与发布入口

当前项目使用本地双通道流程，权威说明在：

```text
deploy/self-update/README.md
```

四个永久下载地址已经固定，网页不应每次跟着版本号修改：

- 主版 Windows：`https://ergou.qzz.io/releases/official/yeschoy-windows-x86_64-installer.exe`
- 主版 macOS：`https://ergou.qzz.io/releases/official/yeschoy-macos-universal-installer.dmg`
- 朋友版 Windows：`https://ergou.qzz.io/releases/partner/yeschoy-windows-x86_64-installer.exe`
- 朋友版 macOS：`https://ergou.qzz.io/releases/partner/yeschoy-macos-universal-installer.dmg`

本地构建入口：

```bash
node deploy/self-update/build-local.mjs windows official /绝对路径/release/internal/本次目录
node deploy/self-update/build-local.mjs windows partner  /绝对路径/release/internal/本次目录
node deploy/self-update/build-local.mjs macos official   /绝对路径/release/internal/本次目录
node deploy/self-update/build-local.mjs macos partner    /绝对路径/release/internal/本次目录
```

macOS 之后还要按 README 分阶段执行 `notarize-local.mjs`，Apple 返回 Accepted、staple 与 Gatekeeper 验证通过后，才能执行 `macos-finalize`。Windows 当前没有 Authenticode 证书，不能对外声称安装包已通过微软代码签名；Tauri updater 签名仍然必须通过。

发布前必须严格确认：

- `package.json`、`src-tauri/Cargo.toml`、`src-tauri/tauri.conf.json` 版本一致。
- official 与 partner 分开构建，不复用错误版别的最终 exe/app。
- 不上传整个工作目录或任何本地私钥目录。
- 使用 `publish_variant.py`，不要回到已退役的 0.4.15 单通道发布命令。
- 公网探针成功不替代 Windows/macOS 真机安装与自动更新验收。

## 11. 接手验收清单

- [ ] 确认 WorkBuddy 未安装时显示官方安装引导，不声称自动安装。
- [ ] 确认已安装时一键接入能直接写入模型列表。
- [ ] 确认不需要本地网关和野菜助手后台常驻。
- [ ] 确认运行中的 WorkBuddy 不会被关闭。
- [ ] 确认不更改默认模型，不强切旧对话。
- [ ] 确认每个模型的 key、模型 ID、计费分组一一对应。
- [ ] 确认原有模型、未知字段和根结构保留。
- [ ] 确认畸形/篡改配置失败关闭并能自动恢复。
- [ ] 确认退出助手和退出账号都覆盖 WorkBuddy。
- [ ] 确认 Windows 真机发现、打开、热加载、恢复均通过。
- [ ] 确认 macOS 真机热加载、恢复均通过。
- [ ] 确认四端打包、公证/签名、双通道发布和永久链接探针通过。

## 12. 官方参考

- WorkBuddy 官网：`https://www.workbuddy.cn/`
- WorkBuddy 模型说明：`https://www.workbuddy.cn/docs/workbuddy/From-Beginner-to-Expert-Guide/Function-Description/Model`
- WorkBuddy CLI 模型说明：`https://www.workbuddy.cn/docs/cli/models`
- WorkBuddy 更新日志：`https://www.workbuddy.cn/docs/workbuddy/Changelog`

最后提醒：源码已提交、未推送。不要继续扩大功能。下一步是隔离真机 smoke，通过后再推送、升版本和四端发布。

# 新增一个可一键配置的应用（OpenCode / Codex CLI 实施清单）

这一轮没来得及把 OpenCode 和 Codex CLI 做完。原因是加一个应用要同时改大约 22 个文件，包括写配置、备份恢复、退出还原这些核心逻辑，赶工容易出事。下面是照着现有 WorkBuddy 适配器梳理出的改动清单。

## 要改的地方（以 `workbuddy` 为模板）

Rust：
- `tool_adapters/<id>.rs`（新文件）：生成配置、校验现有配置
- `tool_adapters/mod.rs`：`TARGETS` / `CONNECTION_TOOLS`
- `tool_activation.rs`：接入流程分支
- `connection_recovery.rs`：备份和恢复用到的文件集合、本地端口
- `exit_restore.rs`：退出时还原的列表
- `open_connection.rs`：「打开使用」
- `config_reveal.rs`：「打开配置文件夹」
- `tool_credentials.rs`：给每个工具单独签发的令牌
- `account_v2.rs`：令牌命名白名单
- `desktop_app_discovery*.rs` 或 `tool_discovery*.rs`：本机检测
- `manual_locations.rs`：用户手动指定位置
- `app_installation/catalog.rs`：安装引导链接

前端：
- `src/configuration/activation.ts`：`ACTIVATION_TOOL_IDS`
- `src/configuration/preview.ts`、`ConfigurationPreviewView.tsx`
- `src/workbench/appCatalog.ts`：从「即将支持」移到正式列表
- `src/installation/toolInstallGuide.ts`
- `src/settings/AdvancedConnectionDetails.tsx`、`QuitAssistant.tsx`
- `src/i18n/locales/*.json`：`yeschoyCatalog.appDescription.<id>` 等
- `src/dev/mockIpc.ts`

## OpenCode（估 1–1.5 天）

- 配置文件：`~/.config/opencode/opencode.json`。在 `provider` 里写一个 `@ai-sdk/openai-compatible` 条目，`options.baseURL` 指向本地转发或站点的 `/v1`，`models` 写选中的模型。
- 合并写入时只改我们自己的 provider 键，保留用户已有的其他配置。恢复时只删我们的键。
- 检测：`opencode` 可执行文件（PATH 和常见安装位置）。

## Codex CLI（估 0.5–1 天）

- 和 Codex 桌面版**共用** `~/.codex/config.toml`，现有 `codex_desktop` 适配器已经写好了 `model_provider`。
- 主要是加一个 `codex_cli` 目标：检测 `codex` 命令，复用 codex_desktop 的写入和恢复逻辑。「打开使用」改成弹出终端命令说明，和 Claude Code、Pi 一样。
- 注意：两个目标共用一个文件，恢复时不能互相踩。最简单的办法是把它们当作同一组连接，任意一个恢复就两边一起恢复，并在界面上说明。

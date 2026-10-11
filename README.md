# 野菜API 桌面助手

本地桌面配置助手：把常用的 AI 编程和办公应用接到兼容的 API 中转。选应用、选模型，助手替你把配置写进去；不想用了，一键恢复成原来的设置。

[野菜API](https://yeschoy.com) 是开箱默认的连接源；长期目标是让更多兼容站点也能被一键接入（选站连接，而不是手填 Base URL 和 API Key）。

[English](#english)

## 能接入哪些应用

| 应用 | 形态 | 助手做什么 |
| --- | --- | --- |
| Claude Code | 命令行 | 写入 `~/.claude/settings.json`，经本地转发连接 |
| Claude Desktop | 桌面应用 | 写入第三方推理配置，经本地转发连接 |
| Codex | ChatGPT 桌面应用 | 写入 `~/.codex/config.toml`，经本地转发连接；不支持 Responses 的模型在本地转换协议 |
| WorkBuddy | 桌面应用 | 写入 `~/.workbuddy/models.json`，WorkBuddy 自动加载 |
| DeepSeek Harness | 桌面应用 | 写入 `~/.dsh/profiles/desktop`，密钥放进 DSH 自己的凭据文件 |
| DSH web | 命令行 + 浏览器 | 写入 `~/.dsh/profiles/web`，由助手启动本地工作台 |
| Pi | 命令行 | 写入 `~/.pi/agent/models.json` |

支持 macOS 和 Windows。

## 它怎么保证不乱改你的电脑

- **确认之前不写任何东西。** 预览页只读，点"接入"才动文件。
- **每次写入前记下原样。** 原配置加密保存在本机（另留一份仅本人可读的应急副本，以防系统钥匙串丢失）。"恢复原设置"只撤回助手写的那几项，你后来自己改的内容原样保留。
- **写完立刻读回校验。** 读回不一致就整体回滚，不会留下改了一半的配置。
- **密钥尽量不进配置文件。** 账号凭据保存在系统钥匙串（macOS 钥匙串 / Windows 凭据管理器）。只有应用只能从自己的配置文件里读密钥时（Codex、WorkBuddy、DeepSeek Harness）才会写进去；这类文件别放进网盘同步目录。
- **不发收费的测试请求。** 接入校验只看配置和本地连接，第一次真实请求的结果显示在"最近连接结果"里。

## 下载

- [GitHub Releases](https://github.com/DaydreamZXG/yeschoy-ai-desktop/releases)（安装包直链）
- [野菜API 官网下载页](https://yeschoy.com/download)

应用内会自动检查更新。

## 开发

需要 Node.js 22、pnpm 10 和 Rust 1.95（见 `rust-toolchain.toml`）。

```bash
pnpm install
pnpm dev             # 启动开发版
pnpm build           # 打包

pnpm typecheck
pnpm test:unit       # 前端测试（vitest）
cd src-tauri && cargo test --lib   # Rust 测试
```

目录大致如下：

- `src/`：界面（React + TypeScript），文案在 `src/i18n/locales/`
- `src-tauri/src/tool_adapters/`：每个应用一份适配器，负责生成、写入、读回和撤销配置
- `src-tauri/src/connection_recovery.rs`：原配置的加密保存与恢复
- `src-tauri/src/*_discovery*.rs`：在本机查找已安装的应用
- `src-tauri/src/proxy/`：Codex 的 Responses ↔ Chat 协议转换

## 致谢与许可

以 [MIT 许可](LICENSE) 发布。感谢 [CC Switch](https://github.com/farion1231/cc-switch)（MIT，Jason Young）；部分 proxy 文件仍与上游一致，见 [`VENDOR.md`](src-tauri/src/proxy/VENDOR.md)。

---

## English

A local desktop config helper that connects common AI coding and office apps to a compatible API relay. Pick an app and a model; it writes the configuration for you. One click restores the original settings.

[Yeschoy (野菜API)](https://yeschoy.com) is the default connection source out of the box. Longer term, other compatible stations should be connectable the same way (pick a station — not paste Base URL and API Key by hand).

Supported apps: Claude Code, Claude Desktop, Codex (ChatGPT desktop), WorkBuddy, DeepSeek Harness, DSH web and Pi, on macOS and Windows.

- Nothing is written until you confirm.
- Original settings are saved (encrypted, on this machine) before every write. Restoring undoes only what the assistant wrote and keeps your later edits.
- Every write is read back and rolled back as a whole if it does not match.
- Account credentials live in the system keychain. A key is written into an app's own config file only when that app can read it from nowhere else (Codex, WorkBuddy, DeepSeek Harness); keep those files out of synced folders.

### Download

- [GitHub Releases](https://github.com/DaydreamZXG/yeschoy-ai-desktop/releases) (direct installer links)
- [Yeschoy download page](https://yeschoy.com/download)

### Build

`pnpm install && pnpm dev`. Tests: `pnpm test:unit` and `cargo test --lib` in `src-tauri`.

Released under the [MIT License](LICENSE). Thanks to [CC Switch](https://github.com/farion1231/cc-switch) (MIT, Jason Young); some proxy files remain identical to upstream — see [`VENDOR.md`](src-tauri/src/proxy/VENDOR.md).

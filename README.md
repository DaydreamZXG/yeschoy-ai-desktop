# 野菜 YesChoy

> 以前叫“野菜API 桌面助手”。装过旧版的人升级后，登录和已经接好的应用都还在，不用重新登录或重新接入。

本地桌面配置助手：把常用的 AI 编程和办公应用接到兼容的 API 中转。选应用、选模型，助手替你把配置写进去；不想用了，一键恢复成原来的设置。

填上站点网址在浏览器里授权，或粘贴站点地址和密钥，就能连任意 OpenAI 兼容的中转站。[野菜API](https://yeschoy.com) 是内置的预设站点之一。

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
| OpenCode | 命令行 | 写入 `~/.config/opencode/opencode.json`，经本地转发连接；密钥留在系统钥匙串 |
| Cline | 命令行 + 编辑器扩展 | 写入 `~/.cline/data/settings/providers.json`（两者共用），只动自己那一项；经本地转发连接，密钥留在系统钥匙串 |
| Hermes Agent | 命令行 | 只换 `~/.hermes/config.yaml` 里的 `model:` 一块，注释和别的设置原样保留；经本地转发连接，密钥留在系统钥匙串 |
| OpenClaw | 命令行 | 只动 `~/.openclaw/openclaw.json` 里自己那一家和主型号；经本地转发连接，密钥留在系统钥匙串 |

支持 macOS 和 Windows。

## 它怎么保证不乱改你的电脑

- **确认之前不写任何东西。** 预览页只读，点"接入"才动文件。
- **每次写入前记下原样。** 原配置加密保存在本机（另留一份仅本人可读的应急副本，以防系统钥匙串丢失）。"恢复原设置"只撤回助手写的那几项，你后来自己改的内容原样保留。
- **写完立刻读回校验。** 读回不一致就整体回滚，不会留下改了一半的配置。
- **密钥尽量不进配置文件。** 账号凭据保存在系统钥匙串（macOS 钥匙串 / Windows 凭据管理器）。只有应用只能从自己的配置文件里读密钥时（Codex、WorkBuddy、DeepSeek Harness）才会写进去；这类文件别放进网盘同步目录。
- **不发收费的测试请求。** 接入校验只看配置和本地连接，第一次真实请求的结果显示在"最近连接结果"里。

## 下载

- [野菜下载页（yeschoy.app）](https://yeschoy.app/download)（自动识别系统，优先 GitHub 已发布版本，否则走官网备用地址）
- [GitHub Releases](https://github.com/DaydreamZXG/yeschoy-ai-desktop/releases)

应用内会自动检查更新：自己编译的公开版本走 GitHub Releases，官方安装包走官方更新源（见 [`docs/OPEN_SOURCE.md`](docs/OPEN_SOURCE.md)）。下载页源码与部署说明见 [`docs/download-page.md`](docs/download-page.md)。

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

## 社区与隐私

- 提问与 Bug：[SUPPORT.md](./SUPPORT.md)
- 隐私说明：[PRIVACY.md](./PRIVACY.md)
- 安全披露：[SECURITY.md](./SECURITY.md)
- 开源发布清单（含 secret scanning、社区 Release 步骤）：[`docs/OPEN_SOURCE.md`](docs/OPEN_SOURCE.md)

## 致谢与许可

以 [MIT 许可](LICENSE) 发布。致谢 [CC Switch](https://github.com/farion1231/cc-switch)（MIT）· 哪些代码来自它，见 [`THIRD_PARTY_NOTICES.md`](THIRD_PARTY_NOTICES.md)。

---

## English

A local desktop config helper that connects common AI coding and office apps to a compatible API relay. Pick an app and a model; it writes the configuration for you. One click restores the original settings.

YesChoy (野菜, formerly the 野菜API desktop assistant) connects to any OpenAI-compatible relay station: enter the station address and authorize in the browser, or paste the address and an API key. [Yeschoy (野菜API)](https://yeschoy.com) is one of the built-in station presets. Upgrading from an earlier version keeps your sign-in and connected apps.

Supported apps: Claude Code, Claude Desktop, Codex (ChatGPT desktop), WorkBuddy, DeepSeek Harness, DSH web, Pi, OpenCode, Cline, Hermes Agent and OpenClaw, on macOS and Windows.

- Nothing is written until you confirm.
- Original settings are saved (encrypted, on this machine) before every write. Restoring undoes only what the assistant wrote and keeps your later edits.
- Every write is read back and rolled back as a whole if it does not match.
- Account credentials live in the system keychain. A key is written into an app's own config file only when that app can read it from nowhere else (Codex, WorkBuddy, DeepSeek Harness); keep those files out of synced folders.

### Download

- [YesChoy download page (yeschoy.app)](https://yeschoy.app/download) (OS-aware; prefers published GitHub releases)
- [GitHub Releases](https://github.com/DaydreamZXG/yeschoy-ai-desktop/releases)

See [`docs/download-page.md`](docs/download-page.md) for hosting notes. In-app auto-update: community builds use GitHub Releases; official packages use the official feed (see `docs/OPEN_SOURCE.md`).

### Build

`pnpm install && pnpm dev`. Tests: `pnpm test:unit` and `cargo test --lib` in `src-tauri`.

Community docs: [SUPPORT.md](./SUPPORT.md), [PRIVACY.md](./PRIVACY.md), [SECURITY.md](./SECURITY.md), [`docs/OPEN_SOURCE.md`](docs/OPEN_SOURCE.md).

Released under the [MIT License](LICENSE). Thanks to [CC Switch](https://github.com/farion1231/cc-switch) (MIT) — see [`THIRD_PARTY_NOTICES.md`](THIRD_PARTY_NOTICES.md) for exactly which code comes from it.

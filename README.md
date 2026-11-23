# YesChoy

**九位性格各异的 AI 同学，点一下就去你电脑里干活。**

不用背命令、不用改配置文件，也不用搞懂什么协议转发。填上你用的站点网址，挑一位顺眼的同学，点一下「派她上场」，她就把 Claude Code、Codex、OpenCode 这些应用全给你接好；不想用了，一键还原，什么都不留痕。

> 以前叫“野菜API 桌面助手”。装过旧版的人直接升级就行，登录和已经接好的应用都还在。

[English](#english)

## 认识一下这群同学

在 YesChoy 里，每个模型家族都有自己的脾气和擅长领域。戳一戳她们还会搭话，谁顺眼就叫谁值班：

| 同学 | 背后模型 | 擅长 | 一句话 |
| --- | --- | --- | --- |
| **橘子** | Claude | 写代码、长任务、讲得清楚 | 「别急，你把事说清楚，剩下的我来收尾。」 |
| **阿青** | GPT | 什么都会、写文案、查资料 | 「啥活都能接，稳着呢。」 |
| **小星** | Gemini | 看图、读长文、反应快 | 「眼睛借我，图和长文都交给我扫！」 |
| **小黑** | Grok | 消息快、敢说、联网 | 「就这？行吧，看我的。」 |
| **小鲸** | DeepSeek | 便宜、写代码、中文好 | 「便宜归便宜，活可一点不含糊！」 |
| **小千** | Qwen | 什么都会、中文好、会思考 | 「想不通的、查不到的，都丢给我！」 |
| **小月** | Kimi | 读超长文档、总结、写报告 | 「……两百万字？放桌角吧，我醒了就看。」 |
| **小谱** | GLM | 写代码、写文档、稳重 | 「放心交给我，这波奔着满分去。」 |
| **小豆** | Doubao | 聊天、快、接地气 | 「说句大白话：这事包我身上。」 |

站点里要是添了别的新模型，还会多出一位刚转学来的「转校生」照样能打。

## 三步就能用

1. **连上你的站点**：输入站点网址，浏览器点一下允许就能连上（也支持直接粘密钥，或直连主流 AI 厂商）。[野菜API](https://yeschoy.com) 是内置预设站点之一，任意兼容的站点都能连。
2. **挑一位同学**：点开她的名片，能直接看到型号、价格方案和折扣，不藏着掖着。
3. **派她进应用**：选好你电脑上装的 AI 应用，点一下就接好并自动打开；想换人、想看今天花了多少、想一键还原，都在界面里点两下搞定。

支持 macOS 和 Windows。

## 支持哪些应用

| 应用 | 形态 | 接入方式 |
| --- | --- | --- |
| Claude Code | 命令行 | 自动写入 `~/.claude/settings.json`，经本地转发连接 |
| Claude Desktop | 桌面应用 | 自动写入第三方推理配置，经本地转发连接 |
| Codex | 桌面应用 / 命令行 | 自动写入 `~/.codex/config.toml`，经本地转发连接；不支持 Responses 的模型自动在本地转协议 |
| WorkBuddy | 桌面应用 | 自动写入 `~/.workbuddy/models.json`，打开即生效 |
| DeepSeek Harness | 桌面应用 | 自动写入 `~/.dsh/profiles/desktop`，密钥存入 DSH 凭据文件 |
| DSH web | 命令行 + 浏览器 | 自动写入 `~/.dsh/profiles/web`，自动拉起本地工作台 |
| Pi | 命令行 | 自动写入 `~/.pi/agent/models.json` |
| OpenCode | 命令行 | 自动写入 `~/.config/opencode/opencode.json`，经本地转发连接，密钥留在系统密码库 |
| Cline | 命令行 + 编辑器扩展 | 只改 `~/.cline/data/settings/providers.json` 里自己那一项，密钥留在系统密码库 |
| Hermes Agent | 命令行 | 只换 `~/.hermes/config.yaml` 的 `model:` 一块，注释和其它设置原样保留 |
| OpenClaw | 命令行 | 只改 `~/.openclaw/openclaw.json` 里自己那一家和主型号，密钥留在系统密码库 |

## 放心，不乱动你的电脑

- **你没点确认，一个字都不写。** 看名片、挑方案都是只读的，点了接入才动文件。
- **改之前先留底，随时一键还原。** 原配置加密存在本机（另留一份仅本人可读的应急副本）。「改回原样」只撤掉 YesChoy 写的那几项，你自己改过的设置和聊天记录全留着。
- **写完立刻核对。** 读回来不对就整套回滚，绝不留改了一半的配置。
- **密钥尽量不落明文。** 账号登录和密钥存在系统密码库（macOS 钥匙串 / Windows 凭据管理器）；只有应用本身只认自己配置文件时（Codex、WorkBuddy、DeepSeek Harness）才会写入对应配置，这类文件别放进网盘同步目录。
- **不偷跑收费请求。** 接入检查只看配置和本地通道，不替你乱花钱。

## 下载

- [YesChoy 下载页（yeschoy.app）](https://yeschoy.app/download)（自动识别系统，优先 GitHub 已发布版本，否则走官网备用地址）
- [GitHub Releases](https://github.com/DaydreamZXG/yeschoy-ai-desktop/releases)

客户端内会自动检查更新：社区自编译版本走 GitHub Releases，官方安装包走官方更新源（见 [`docs/OPEN_SOURCE.md`](docs/OPEN_SOURCE.md)）。下载页源码与部署说明见 [`docs/download-page.md`](docs/download-page.md)。

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

**Nine AI classmates with real personalities — one click to put them to work in your apps.**

Skip the terminal commands, config files, and protocol headaches. Enter your station's web address, pick whichever classmate fits the job, and hit go: YesChoy wires up Claude Code, Codex, OpenCode, and more for you — and restores everything back with one click whenever you want.

> Formerly the 野菜API desktop assistant. Upgrading keeps your sign-in and connected apps intact.

### Meet the Cast

- **Juzi (Claude)** — Coding, long tasks, clear explanations. *"No rush. Tell me what you want, I'll handle the rest."*
- **Aqing (GPT)** — All-rounder, writing, lookups. *"Whatever it is, I've got it covered."*
- **Xiaoxing (Gemini)** — Vision, long reads, fast replies. *"Drop the screenshots and wall-of-text on me, I read fast!"*
- **Xiaohei (Grok)** — Fresh news, blunt answers, web-connected. *"That's it? Fine, watch this."*
- **Xiaojing (DeepSeek)** — Budget-friendly, strong coding & Chinese. *"Cheap? Sure. Sloppy? Never!"*
- **Xiaoqian (Qwen)** — All-rounder, Chinese, reasoning. *"Weird questions, tricky lookups — toss them all here!"*
- **Xiaoyue (Kimi)** — Massive docs, summaries, reports. *"…Two million words? Drop it on my desk, I'll read when I wake up."*
- **Xiaopu (GLM)** — Coding, docs, methodical. *"Leave it to me. Aiming for a clean 100."*
- **Xiaodou (Doubao)** — Chatty, quick, down-to-earth. *"Straight up: consider it handled."*

### How It Works

1. **Connect your station** — Enter any OpenAI-compatible station URL and authorize in the browser (or paste an API key / connect a provider directly). [Yeschoy (野菜API)](https://yeschoy.com) is included as a preset.
2. **Pick a classmate** — Check models, pricing plans, and discounts right on her card.
3. **Send her in** — Choose an app on your machine and click once. Supported on macOS and Windows: Claude Code, Claude Desktop, Codex, WorkBuddy, DeepSeek Harness, DSH web, Pi, OpenCode, Cline, Hermes Agent, and OpenClaw.

### Safe by Default

- Nothing is written until you confirm.
- Original settings are backed up (encrypted, locally) before every write. Restoring only undoes what YesChoy touched and keeps your own edits.
- Every write is verified on read-back and rolled back as a whole if anything mismatches.
- Credentials stay in the system keychain; a key is written to an app's config only when that app cannot read it anywhere else (Codex, WorkBuddy, DeepSeek Harness — keep those out of synced cloud folders).

### Download

- [YesChoy download page (yeschoy.app)](https://yeschoy.app/download) (OS-aware; prefers published GitHub releases)
- [GitHub Releases](https://github.com/DaydreamZXG/yeschoy-ai-desktop/releases)

See [`docs/download-page.md`](docs/download-page.md) for hosting notes. In-app auto-update: community builds use GitHub Releases; official packages use the official feed (see `docs/OPEN_SOURCE.md`).

### Build

`pnpm install && pnpm dev`. Tests: `pnpm test:unit` and `cargo test --lib` in `src-tauri`.

Community docs: [SUPPORT.md](./SUPPORT.md), [PRIVACY.md](./PRIVACY.md), [SECURITY.md](./SECURITY.md), [`docs/OPEN_SOURCE.md`](docs/OPEN_SOURCE.md).

Released under the [MIT License](LICENSE). Thanks to [CC Switch](https://github.com/farion1231/cc-switch) (MIT) — see [`THIRD_PARTY_NOTICES.md`](THIRD_PARTY_NOTICES.md) for exactly which code comes from it.

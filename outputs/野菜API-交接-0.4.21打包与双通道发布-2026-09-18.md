# 野菜API 0.4.21 打包与双通道发布交接（给 Codex）

日期：2026-09-18  
任务：在本机把 **0.4.21** 打成主版/朋友版的 macOS + Windows 安装包，公证 macOS，签 Tauri 更新，按双通道发布。  
不要改产品功能代码。版本已经是 0.4.21，**不要再升版本号**。

仓库：`https://github.com/DaydreamZXG/yeschoy-ai-desktop`  
分支：`codex/yeschoy-0.4.14-updater`  
HEAD：`a7b70ca2`（`fix: open Claude Code in a chosen project folder, never $HOME`）已在 origin。  
本机工作树：`/Users/zxg/.codex/worktrees/c485/xiaobai-ai-desktop`

完整流程以 `deploy/self-update/README.md`「当前本地流程」为准。0.4.16 起**不用** GitHub Actions 候选工作流。

---

## 1. 产品上已经测过、不要回退

作者用带选文件夹的 0.4.21 主版 macOS 候选包测过，反馈「没问题，很漂亮」。包含：

- WorkBuddy 一键接入；`url` 为完整 `/v1/chat/completions` 且 `useCustomProtocol: true`
- 自定义显示名 `野菜:<id>`（WorkBuddy 自己会拼 id）
- Codex 只接 `openai-response`；Chat 模型灰着并提示去 Claude
- 已接入卡片点模型名换模型；未保存时主按钮是「保存并应用」
- 退出对话框先说这一次会怎样
- Claude Code 不再因 `ANTHROPIC_*` 环境变量失败；从助手打开会 unset 这些变量
- Claude Code / Pi **必须选项目文件夹**，禁止用家目录；取消则落到 `Documents/野菜工作区`

不要恢复：WorkBuddy 镜像安装、本机 127.0.0.1 协议网关、把 Chat 模型写进 Codex。

Chat 渠道要在 Codex 里能用，是中转（new-api）的 Responses→Chat，不在这次打包范围。new-api 的 `max_tokens` 下限已在 `feat/yecai-console-landing` 的 `e6152580c`，客户端不管部署。

---

## 2. 本机已有的 macOS 主版包（注意别用错）

| 目录 | 提交 | 能否公证发布 |
| --- | --- | --- |
| `release/internal/0.4.21-2026-09-18/` | 环境变量修复**之前** | **否** |
| `release/internal/0.4.21-envfix/` | 选文件夹**之前** | **否** |
| `release/internal/0.4.21-workspace/` | `a7b70ca2`，作者测过 | **可以拿去公证**（或从当前 HEAD 重打一份干净目录） |

推荐新建：

```text
release/internal/0.4.21-release-YYYYMMDD/
```

若沿用 `0.4.21-workspace` 做公证，先确认 `official-stage/野菜API.app` 的版本是 0.4.21，且 Info.plist 与当前 HEAD 一致。不要公证 envfix / 2026-09-18 那两份。

---

## 3. 必须按顺序做（禁止并行两版）

密钥已在本机：`~/Library/Application Support/yeschoy-release/updater-v2`（或 `~/.config/yeschoy-release/updater-v2`）。**不要 init 覆盖、不要提交、不要上传私钥。**

Developer ID：`Developer ID Application: Guangzhou Xiaobai Intelligent Technology Co., Ltd. (F9NG3F8Y6K)`。

```bash
cd /Users/zxg/.codex/worktrees/c485/xiaobai-ai-desktop
git checkout codex/yeschoy-0.4.14-updater
git pull --ff-only origin codex/yeschoy-0.4.14-updater
# 确认 HEAD 是 a7b70ca2 或其后仅文档提交

# 主版 macOS（若重打）
node deploy/self-update/build-local.mjs macos official /绝对路径/release/internal/0.4.21-release-日期

# 公证（分阶段，等 Apple Accepted）
node deploy/self-update/notarize-local.mjs /绝对路径/release/internal/0.4.21-release-日期/macos official submit-app
# 随后 status-app → app-to-dmg → status-dmg → verify-dmg
# 通过后再 macos-finalize

node deploy/self-update/build-local.mjs macos-finalize /绝对路径/release/internal/0.4.21-release-日期 official
```

然后**同一目录、换 partner，再打 macOS 朋友版**（登录域 `https://ai.yeschoy.io`）。不要和主版并行。

Windows：

```bash
node deploy/self-update/build-local.mjs windows official /绝对路径/release/internal/0.4.21-release-日期
node deploy/self-update/build-local.mjs windows partner /绝对路径/release/internal/0.4.21-release-日期
```

Windows **没有 Authenticode**，不能写「已通过微软签名」。Tauri 更新签名必须过。Rust 1.94、Zig RC 当次编译 Windows 资源，不能复用上一版。`ANTHROPIC_*` 和代理环境变量建议 unset，避免测试/探测被带偏。

每版三个精确构件：EXE、DMG、`.app.tar.gz` 及各自 `.sig`。先 `publish_variant.py` 对本地临时 public 根做真实验签，再上传服务器 incoming。不要上传整个工作目录或密钥目录。

发布（服务器，版本 **0.4.21**）：

```sh
python3 publish_variant.py publish --root /srv/yeschoy-download/public \
  --variant official --lock /opt/yeschoy-download/publish.lock \
  --candidate /opt/yeschoy-download/incoming/本次目录/candidate \
  --version 0.4.21 --notes /opt/yeschoy-download/incoming/本次目录/release-notes.txt \
  --channel-config /opt/yeschoy-download/incoming/本次目录/update-channels.json
```

朋友版再调一次 `--variant partner`。探针：`probe_variant_release.py --version 0.4.21 ...`

永久下载名不要改网页：

- 主版 Windows / macOS、朋友版 Windows / macOS 四条 `https://ergou.qzz.io/releases/{official|partner}/yeschoy-*-installer.*`

---

## 4. 不要做的事

- 不要改 `package.json` / Tauri / Cargo 版本（已是 0.4.21）
- 不要 `git reset --hard`、不要 force push
- 不要合 `main` 或 `product/yeschoy-v1`，除非作者另说
- 不要用 GitHub Actions「Yeschoy Signed Update Candidate」发 0.4.21
- 不要并行打 official 和 partner
- 不要公证/发布 envfix、2026-09-18 那两份旧 mac 包
- 不要声称 Windows 已 Authenticode
- 不要动 WorkBuddy 镜像安装、本地网关、Codex Chat 闸门

---

## 5. 建议更新说明（可改）

```
0.4.21：WorkBuddy 一键接入野菜；接入后可直接点模型名更换；Codex 仅使用支持 Responses 的模型，其余会提示可去 Claude；Claude Code 从助手打开时不再被旧的 ANTHROPIC 环境变量挡住，并会让你选择项目文件夹。
```

---

## 6. 一句话

代码已在 `codex/yeschoy-0.4.14-updater` @ `a7b70ca2`。Codex 只需从这份 HEAD 完成 0.4.21 四端本地签名构建、macOS 公证、双通道发布；不要再改功能。

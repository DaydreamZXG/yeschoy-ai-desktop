# 交接：直连改造收尾 + Windows 交叉构建

日期：2026-09-09
交接对象：接手继续构建/验证的同事（Codex）
当前分支：`codex/yeschoy-0.4.14-updater`（本地已有 9 个提交，**未推送**）

## 1. 已完成并真机验证

| 事项 | 状态 | 证据 |
| --- | --- | --- |
| 六个工具直连中转站（Codex / Claude Code / Pi / Hermes / OpenClaw / DSH） | ✅ | Codex 发消息并收到回复；中转站日志有 `gpt-5.6-sol` 记录 |
| Claude Desktop（保留本地别名转发） | ✅ | 中转站日志有 `野菜API cd-…` 多条记录 |
| 接入不再卡死 | ✅ | `~/Library/Logs/野菜API/yeschoy.log` 显示 `enter→opened` 2.2 秒 |
| 恢复原设置 | ✅ | 配置 / 模型目录 / 钥匙串凭据全部清干净 |
| 服务端观测（最近中转记录） | ⚠️ 未确认 | token 名与归因规则一致，但界面是否显示未回报 |

本次提交（`git log --oneline --reverse 261f5d65..HEAD`）：

```
23c12bd3 refactor: connect tools directly to the relay
bc7caa71 refactor: shrink the Claude Desktop gateway to alias forwarding
22ed652b feat: read recent relay requests from the server usage log
88292c79 feat: make the setup action explain itself instead of going silent
132c281f docs: record the setup page rework and the local verification build
0f22b91a feat: log an activation timeline and unstick the running-app action
f4a50496 feat: log activation outcomes, not just stages
acb744bb ci: let the internal Windows workflow build the friend variant
```

## 2. macOS 产物（已完成，可直接用）

目录 `release/local-0.4.15/`：

| 文件 | SHA-256 | 状态 |
| --- | --- | --- |
| `野菜API-0.4.15-macOS-x64.dmg` | `b7c851744276f67282c20d73d23de58a3499f7c55d3563fd0d61090c772a0334` | Developer ID 签名 + 公证 Accepted + 已 staple + Gatekeeper 通过 |
| `野菜API-0.4.15-macOS-x64-朋友版.dmg` | `b83ff28ebc9f7dd3c3eefb6cba239fe9f335098283ba87af694cb41ffef1469e` | 同上 |

签名身份 `Developer ID Application: xiangguo Zheng (BRG82P5ZB7)`，公证用 keychain profile `yeschoy-notary`。均为 **x64**（本机 Intel）。

## 3. 待办：Windows 交叉构建

### 3.1 环境现状（本机已就绪）

- `cargo-xwin` 0.23.1：`~/.local/bin/cargo-xwin`
- xwin 缓存 5.4G：`~/Library/Caches/cargo-xwin`（含 `windows-msvc-sysroot`）
- 交叉编译目标缓存 3.6G：`release/local-0.4.15/target-windows`（从上次 0.4.15-partner 构建复制而来，可省大量编译时间）
- `x86_64-pc-windows-msvc` 已安装（必须配 `RUSTUP_TOOLCHAIN=1.95.0`；裸 `1.95` 那个工具链在本机损坏）
- **makensis 已下载并解压**：`/tmp/makensis-bottle/makensis/3.12/bin/makensis`
  - 来源：Homebrew `makensis` 3.12 **sonoma** bottle
  - SHA-256：`9b482291c76d7965a7c535ee2fddbca5f76a0e018100f6f4950ab92c25967e1d`
  - 运行必须设 `NSISDIR=/tmp/makensis-bottle/makensis/3.12/share/nsis`（否则报 `reading stub .../Cellar/makensis/...`）
  - `/tmp` 会被清理，如已丢失按 §3.3 重新下载

### 3.2 卡点：缺 `llvm-rc`（必须先解决）

上一轮构建会在这里失败：

```
thread 'main' panicked at tauri-winres-0.3.5/src/lib.rs:536:
called `Result::unwrap()` on an `Err` value: NotAttempted("llvm-rc")
```

原因：`tauri-build` → `tauri-winres` → `embed-resource` 在 `*-windows-msvc` 目标下探测 `llvm-rc`（[embed-resource `non_windows.rs`](https://docs.rs/embed-resource)），本机没有。

**解法（二选一）**：

1. **Homebrew `llvm` 23.1.0 bottle 里的 llvm-rc**（上一轮就是这么做的，文档记为“真实 llvm-rc 23.1.0”）：

```bash
TOKEN=$(curl -s "https://ghcr.io/token?service=ghcr.io&scope=repository:homebrew/core/llvm:pull" | python3 -c "import sys,json;print(json.load(sys.stdin)['token'])")
mkdir -p /tmp/llvm-bottle && curl -sL -H "Authorization: Bearer $TOKEN" -o /tmp/llvm-bottle/llvm.tar.gz "https://ghcr.io/v2/homebrew/core/llvm/blobs/sha256:0b0168dc611a9d77aaa62d094178297f8a861d647cc59c43a5dc3f76bd6eb7b2"
tar xzf /tmp/llvm-bottle/llvm.tar.gz -C /tmp/llvm-bottle
```

   期望 SHA-256：`0b0168dc611a9d77aaa62d094178297f8a861d647cc59c43a5dc3f76bd6eb7b2`（356 MB）

   运行时设置（llvm-rc 动态链接 bottle 里的 libLLVM，install name 指向 Cellar，必须给回退路径）：

```bash
export RC=/tmp/llvm-bottle/llvm/23.1.0/bin/llvm-rc
export DYLD_FALLBACK_LIBRARY_PATH=/tmp/llvm-bottle/llvm/23.1.0/lib
```

   自检：`"$RC" -V /?` 输出应以 `OVERVIEW: LLVM Resource Converter` 开头（`embed-resource` 就是靠这行判断类型）。

2. 或用 mingw-w64 的 `windres`（`embed-resource` 同样支持，靠 `-V /?` 输出以 `GNU windres` 开头识别），但上一轮没有验证过这条路径。

### 3.3 构建（每个版本一次）

```bash
export CARGO_TARGET_DIR=$PWD/release/local-0.4.15/target-windows
export RUSTUP_TOOLCHAIN=1.95.0
export RC=/tmp/llvm-bottle/llvm/23.1.0/bin/llvm-rc
export DYLD_FALLBACK_LIBRARY_PATH=/tmp/llvm-bottle/llvm/23.1.0/lib
export YESCHOY_AUTHORIZATION_PAGE_ORIGIN=https://yeschoy.com      # 朋友版改 https://ai.yeschoy.io
pnpm exec tauri build --runner cargo-xwin --target x86_64-pc-windows-msvc --no-bundle
```

产物：`$CARGO_TARGET_DIR/x86_64-pc-windows-msvc/release/yeschoy-desktop.exe`

注意：切换 `YESCHOY_AUTHORIZATION_PAGE_ORIGIN` 会让 `build.rs` 重编主 crate（依赖不重编，很快）。两个版本必须**分别构建**，不能复用同一个 exe。

### 3.4 打包（makensis + 已入库配方）

```bash
NSISDIR=/tmp/makensis-bottle/makensis/3.12/share/nsis \
/tmp/makensis-bottle/makensis/3.12/bin/makensis \
  -DAPP_EXE=release/local-0.4.15/target-windows/x86_64-pc-windows-msvc/release/yeschoy-desktop.exe \
  -DOUTPUT_EXE=release/local-0.4.15/野菜API-0.4.15-Windows-x64-内测版.exe \
  -DAPP_ICON=src-tauri/icons/icon.ico \
  -DAPP_VERSION=0.4.15 \
  src-tauri/windows/installer.nsi
```

朋友版输出名：`野菜API-0.4.15-partner-ai-yeschoy-Windows-x64-内测版.exe`

配方 `src-tauri/windows/installer.nsi` 是当前用户级简体中文安装、带开始菜单/桌面快捷方式和卸载入口，`installer-hooks.nsh` 负责升级前安全退出旧进程。

### 3.5 验收

- `file` 确认 `PE32+ executable (GUI) x86-64`
- 记录 SHA-256；确认安装包与 exe 的架构一致
- 在交付记录里如实标注：**未做 Authenticode 签名、未在真实 Windows 机器验证、依赖机器已有 WebView2 Runtime**
- 用户侧 smoke（如能拿到 Windows 机器）：安装 → 启动 → 接入 Codex → 发一条消息 → 回接入页看“最近中转记录”

## 4. 其余待验证（优先级从高到低）

1. **另外五个工具各连一次**：Claude Code、Pi、Hermes、OpenClaw、DSH（Codex 与 Claude Desktop 已过）
2. **升级迁移路径**：用旧版（本地桥）接入 → 升级到新包 → 界面应显示“设置已变更”，点“更新接入设置”后直连生效。这条必须在发更新前过一遍
3. **Windows 真机**：安装、启动、接入、长会话
4. **“最近中转记录”**：确认接入后发消息，界面能出现完整模型 ID（服务端日志归因）

## 5. 遗留与风险

- **版本号仍是 0.4.15**，与线上相同。要测“升级”必须先升到 0.4.16，否则更新通道不会触发。
- **两个版本共用更新通道**（`ergou.qzz.io/updates/stable.json`）：朋友版用户收到更新后会被换成主版本（授权页变回 yeschoy.com）。长期并行需给朋友版单独清单。
- **Claude Desktop 的别名映射**随客户端版本前缀 `claude-desktop-route:v1` 变化；若中转站将来支持模型别名，可再评估彻底去桥。
- **Windows 无代码签名**：安装包未签名，但自动更新包是 Tauri minisign 签名的，更新链路安全，只有首次安装会被 SmartScreen 拦。建议：短期保持未签名 + 下载页写清“更多信息 → 仍要运行” + 公布 SHA-256；中期有海外主体可上 Azure Trusted Signing（约 $10/月，CI 友好），否则买 EV 证书 + 云 HSM，或上架 Microsoft Store（$19 一次性，商店签名，无提示）。
- CI 额度已用尽（`acb744bb` 给内部 Windows 工作流加了“朋友版”开关，等额度恢复后可用）。
- 本机日志位置：`~/Library/Logs/野菜API/yeschoy.log`（超过 4 MB 自动重置）。

## 6. 常用自检

```bash
# 原生测试 + 静态检查
cd src-tauri && RUSTUP_TOOLCHAIN=1.95.0 cargo clippy --all-targets -- -D warnings
cd src-tauri && env -u ANTHROPIC_BASE_URL -u ANTHROPIC_AUTH_TOKEN RUSTUP_TOOLCHAIN=1.95.0 cargo test --lib

# 前端
pnpm typecheck && pnpm test:unit
```

注：跑 Rust 测试前要清掉 `ANTHROPIC_BASE_URL` 等环境变量，否则 `claude_code` 适配器会因检测到外部覆盖而失败。

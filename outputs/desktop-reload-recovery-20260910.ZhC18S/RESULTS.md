# 桌面换模：启动确认与失败恢复

2026-09-10。结论：限定源码修复与隔离回归通过；不是发布或真实安装包验收。没有打包、上传、提交或推送，没有启动、关闭、修改本机真实 Codex、Claude、野菜应用，也没有读取真实凭据或发送收费模型测试消息。

## 修了什么

1. 继续复用现有桌面接入流程与保存确认。Codex/Claude 正在运行时，用户确认已保存后，助手先请求精确安装路径对应的应用退出，再配置、重开。不把退出工作交给用户找进程；也不强制结束仍显示的 Windows 编辑窗口/保存提示。
2. 桌面配置事务不再在“启动命令发出前”就完成。启动命令和系统启动观察成功后，才完成恢复记录并进入旧密钥清理步骤。
3. 加密 pending 记录新增本次操作前的文件和工具凭据快照。换模中断后，恢复的是紧邻的上一次配置，而不是最初接入前的配置；原始基线仍保留给明确的断开/恢复操作。只保留一个在途检查点，不增加历史备份集合。
4. 启动或后续完成步骤失败，先确认新进程已经停止，再撤回本次文件、凭据和辅助连接；回退成功才重新打开旧配置。退出、文件恢复、凭据恢复、辅助连接恢复、旧应用重开失败分别呈现，不假报成功，也不循环自动重启。
5. 新增“自动修复并重试”主要操作。恢复重试继续使用保存确认，不偷偷继承旧的退出授权。“打开使用”发现未完成事务时也会引导到接入修复，不再要求先手动恢复原设置。
6. 复用现有三方合并：不覆盖外部冲突字段，允许保留无关修改及其他 Claude 配置条目。只读系统状态检查与启动等待有界，不把未观察到启动当成功。

## 关键分支

| 情况 | 行为 |
| --- | --- |
| 用户尚未确认保存 | 不退出应用、不写配置 |
| 正常换模、观察到系统启动 | 完成记录；界面只说明配置与启动检查完成 |
| 部分文件写入后中断 | 下一次明确接入先恢复上一次模型和凭据 |
| 新启动失败且可停止 | 回退本次更改，尝试重新打开旧配置 |
| 新应用无法退出 | 不覆盖活跃配置、不撤销仍可能使用的新密钥，保留待恢复记录 |
| 文件或凭据恢复失败 | 保留未完成记录；展示真实失败与修复重试入口 |
| 配置恢复但旧应用重开失败 | 明示启动未确认，不声称已能使用 |
| 取消/账户状态变化/完成记录失败 | 与启动故障区分，回退后不把模型切换标为成功 |

## 验证

- 修复前运行 `desktop_failed_switch_recovers_previous_connection_not_factory_settings` 得到失败：恢复成 factory 而不是 a；修复后通过。那次完整原始 stdout 在本任务工具记录中，本目录没有伪造补写失败日志。
- 前端全量：42 文件、533 用例通过，见 `frontend-full-final.json`。覆盖两种桌面应用的重复换模、对话框解除阻塞、保存授权、修复入口、搜索与滚动及相邻模块。
- 原生定向：71 用例通过。`connection_recovery` 21、`tool_activation` 16、`desktop_lifecycle` 3、`desktop_launch` 4、Claude 适配器 11、Codex 适配器 16。包括部分文件提交、恢复重放、合成凭据加密、外部冲突、停止拒绝、启动观察超时、失败顺序。见两个 `native-*-final.log`。
- Rust Clippy `--lib --tests -- -D warnings`、rustfmt、TypeScript 检查通过。
- macOS Intel 编译/Clippy，Apple Silicon `cargo check --target aarch64-apple-darwin --lib` 通过。
- Windows x64 `cargo-xwin check --target x86_64-pc-windows-msvc --lib --tests` 通过。复用 RU-056 的 **check-only** 空资源兼容脚本，使用独立且已 gitignore 的目标目录；没有链接或构建可分发程序，不能据此宣称 Windows 资源、安装程序、窗口行为实测通过。该脚本禁止用于打包。
- Vite 生产 renderer 构建通过。浏览器数据陈旧与大 chunk 警告仍存在，不属于本轮换模修复；未为消除警告升级依赖。
- 最终 `git diff --check` 通过。`working-tree-vs-HEAD.diff` 包含当前工作树相对 HEAD 的相关文件差异，也包含此前未提交修复，不能全部归因于本轮。

## 证据边界与尚需真机验证

- 启动观察：Windows 为精确路径进程的可见、未被系统判挂起的窗口；macOS 为精确 bundle/path 的进程已结束系统启动阶段。**不证明越过应用内部 Logo、完成登录或模型请求可用**，成功文案已明确这一点。
- 本轮未运行真实 Windows/macOS 目标应用或系统凭据恢复；正式发包前仍需在隔离机器/VM验证实际退出、系统权限提示、配置加载、失败恢复和连续换模。模拟和交叉编译不能替代这项验收。
- 外部软件改动了同一个模型/凭据字段时不会强制覆盖；重试也不会绕过冲突保护。持续的系统权限、只读文件或外部冲突需要处理对应原因，不能保证仅重复点击就修复。未知未来客户端私有格式也不在保证范围内。
- 老版 pending 日志没有新增的上一次凭据检查点时，仍按旧版安全恢复语义读取；不会猜测不存在的模型或密钥。新检查点完成后删除，旧版程序不保证读取新版在途记录。
- 按 `govern-product-build` 审计明确了事务所有者、失败状态和非目标；既有 RU-075 历史验证状态漂移导致本次 RU-076 review 的 146 项错误仍未通过（`governance-review.json`）。保留失败证据，未改写历史冻结或声称正式发布验收通过。本次按用户明确授权限定修复，不创建另一套配置系统。

## 复跑命令

在仓库根目录执行：

```sh
pnpm typecheck
pnpm exec vitest run --dir src --maxWorkers=2 --minWorkers=1
RUSTUP_TOOLCHAIN=stable cargo clippy --manifest-path src-tauri/Cargo.toml --locked --offline --lib --tests -- -D warnings
RUSTUP_TOOLCHAIN=stable cargo test --manifest-path src-tauri/Cargo.toml --locked --offline --lib connection_recovery::tests -- --test-threads=1
RUSTUP_TOOLCHAIN=stable cargo test --manifest-path src-tauri/Cargo.toml --locked --offline --lib tool_activation::tests -- --test-threads=1
RUSTUP_TOOLCHAIN=stable cargo test --manifest-path src-tauri/Cargo.toml --locked --offline --lib tool_adapters::desktop_lifecycle::tests -- --test-threads=1
RUSTUP_TOOLCHAIN=stable cargo test --manifest-path src-tauri/Cargo.toml --locked --offline --lib tool_adapters::desktop_launch::tests -- --test-threads=1
RUSTUP_TOOLCHAIN=stable cargo test --manifest-path src-tauri/Cargo.toml --locked --offline --lib tool_adapters::claude_desktop::tests -- --test-threads=1
RUSTUP_TOOLCHAIN=stable cargo test --manifest-path src-tauri/Cargo.toml --locked --offline --lib tool_adapters::codex_desktop::tests -- --test-threads=1
RUSTUP_TOOLCHAIN=stable cargo check --manifest-path src-tauri/Cargo.toml --locked --offline --target aarch64-apple-darwin --lib
```

Windows check-only 命令使用本目录 `llvm-rc-CHECK-ONLY.sh` 作为 RC，`CARGO_TARGET_DIR` 指向本目录 `windows-check-target`，`XWIN_CROSS_COMPILER=clang`，执行 `cargo-xwin xwin check --manifest-path src-tauri/Cargo.toml --locked --offline --target x86_64-pc-windows-msvc --lib --tests`。不将该环境复用于安装包构建。

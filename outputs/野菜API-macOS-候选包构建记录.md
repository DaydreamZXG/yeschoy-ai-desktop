# 野菜API macOS 本机候选构建记录

状态快照：2026-08-31 16:07（Asia/Shanghai）。这不是正式发布批准。

## 当前结论

本机已识别有效的 Developer ID Application 签名身份，前端和原生 release 编译已成功；首次代码签名正在等待 macOS 钥匙串授权，尚未取得签名完成证据。

当前没有完成核验的安装包，不安装、不启动、不分发本次中间产物。公证认证尚未配置，也没有向 Apple 提交任何公证产物。

## 源码与构建环境

- 构建源码提交：`c67c683a6baa01236e6a53083253aa8fab92acb5`，分支 `product/yeschoy-v1`；开始构建时 Git 工作区干净。未推送 GitHub。
- 系统：macOS 15.7.9（24G830），Intel x86_64；不代表 Apple Silicon、Universal 或 Windows 验证。
- Node：24.16.0；pnpm：10.12.3；Tauri CLI：2.8.0。
- Rust/Cargo：stable 1.97.1；使用 `RUSTUP_TOOLCHAIN=stable` 与 `--locked`。
- `pnpm-lock.yaml` SHA-256：`40016cef2ca79be59e608ea6b7cc8b2bd36bd5db71d8273c6d8314bf19aaf91c`。
- `src-tauri/Cargo.lock` SHA-256：`a169e30dcb54526a13bd067c1a9db1fd530ce75bfb624306af2ef109cf02fb33`。
- 独立构建目录：`release/ru008-local-gtjOAI/build`。保留此前 `src-tauri/target/release/bundle` 下的旧文件，没有把旧安装包当作本次产物。
- 构建配置：`src-tauri/tauri.candidate.conf.json`，额外启用 `bundle.macOS.hardenedRuntime: true`，仅 `--bundles app`，自动更新产物保持关闭。
- 构建进程显式取消公证认证、导入证书与更新私钥相关环境变量，只使用已经安装在钥匙串中的签名身份；没有导出私钥。

## 已完成验证

- `pnpm typecheck` 通过。
- `pnpm test:candidate`：7 个文件、30 项测试通过。
- 当前原生 Rust 测试：13 项通过，无忽略、过滤或空测试集。
- RU-005 / RU-006 / RU-008 pytest 合计 21 项通过。
- Rust Clippy（all-targets、warnings as errors）通过。
- RU-008 正式验收已通过，历史 RU-007 失败报告保留；最新记录以 `.product-governance/execution/work-package-runs.json` 为准。验收执行真实 TypeScript/React 模块，但其中原生传输与发布配置检查不是在线请求、产物签名或公证的替代证据。
- Tauri renderer 构建和 Rust release 编译通过；签名命令已启动。
- 编译产物架构确认为 Mach-O x86_64；生成的 plist 中应用名为“野菜API”、标识为 `com.yeschoy.desktop`、版本为 `0.1.0`。

构建仍有前端依赖数据过期及大 chunk 警告；本次没有升级依赖或将警告视为兼容性验证通过。

## 安装前必须处理的发现

1. **钥匙串授权待完成。** `codesign` 使用身份指纹 `952554E26C708644686E7687E3FE7E2BFEF08D67`、Team ID `BRG82P5ZB7`，命令包含 `--options runtime`，但尚未退出成功。需要用户在 macOS 系统授权弹窗中操作，不能在聊天中提交密码。最终还需核验签名、hardened runtime 和安全时间戳，不能从命令参数推断已成功。
2. **继承的 URL 协议尚未清理。** `src-tauri/Info.plist` 和本次生成的 app plist 仍声明 `CC Switch Deep Link` / `ccswitch://`。可能与原版 CC Switch 的系统链接处理产生冲突。当前运行时没有注册深链接处理插件；这不等于该系统注册安全或正确。需要在后续明确覆盖此文件的打包元数据修复范围内移除声明、添加回归检查、重新构建并复核实际 plist；不能只修改已签名产物，不能顺势启用新的账号回调或配置导入功能。
3. **公证待完成。** `yeschoy-notary` 钥匙串配置不存在。用户需在本机安全保存 Apple 公证认证后，才可提交确切产物；密码不进入仓库、日志、客户端或 NewAPI。Apple 接受与 stapler 验证之前，不称作已公证。

尚未生成最终 DMG、产物 SHA-256 和大小；尚未进行完整内容/秘密扫描、最终签名校验、Gatekeeper、公证票据、安装与原生界面测试。不得使用当前源配置或单元测试结果填补这些证据。

## 范围边界

不访问或修改服务器、NewAPI、Nginx、业务账户、API 密钥、用户工具配置、付款、遥测、自动更新、GitHub CI 或公开发布。登录、凭据管理及配置应用仍未启用。即使后续本机签名和公证完成，也不能据此宣布完整商业 V1 已就绪。

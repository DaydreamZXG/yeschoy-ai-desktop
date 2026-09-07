# 野菜API 0.4.10 双平台内测包

打包时间：2026-09-07（Asia/Shanghai）

源代码提交：ffcfe898f79ff5069d7c4651a1bf0a779aa1a753

## 产物

### macOS

- 文件：release/internal/0.4.10/macos/野菜API-0.4.10-macOS-universal.dmg
- 大小：11,993,405 bytes
- SHA-256：c62a36bff71cc7180b956ae298eefcfb4105ab73b5874f8a3ebe1d734ea1c8a3
- 架构：Intel x86_64 + Apple Silicon arm64
- 签名：Developer ID Application，Team ID BRG82P5ZB7
- 苹果公证：Accepted，Submission ID ead04218-08b2-4d96-9a8b-66a0ae61d6c0
- 已完成公证票据装订、Gatekeeper、代码签名、镜像完整性和挂载后应用校验。

### Windows

- 中文安装程序：release/internal/0.4.10/windows/野菜API-0.4.10-Windows-x64-内测版.exe
- 安装程序大小：2,869,598 bytes
- 安装程序 SHA-256：f3d19f5f0c6dcdb570678906286a7c6ea621631a73fc82b3426c41a8c8e2ee5e
- x64 主程序：release/internal/0.4.10/windows/野菜API-0.4.10-Windows-x64.exe
- 主程序 SHA-256：45299274990706fce6ccb47ef80df9de982b146f22f1c3dce8a71f2a97fb2119
- 目标：x86_64-pc-windows-msvc
- 安装方式：当前用户级简体中文 NSIS；包含开始菜单、桌面快捷方式和卸载入口。
- 限制：未做 Authenticode 签名、SmartScreen 信誉和真实 Windows 机器安装验收；依赖机器已有 Microsoft Edge WebView2 Runtime。

## 验证

- TypeScript 类型检查通过。
- Renderer：37 个测试文件、453 个测试通过。
- Rust：477 个测试通过。
- 0.4.10 工作区运行代码、锁文件及版本元数据与 Windows 候选提交逐项一致。
- 双平台制品验收：2 项通过。
- 完整机器可读收据：release/internal/0.4.10/release-receipt.json

GitHub Actions 原生 Windows 运行 34045326082 因仓库账户计费或 Actions 消费额度问题在任何步骤开始前被拒绝。因此本次 Windows 包采用本地 cargo-xwin 交叉构建与固定哈希的临时 llvm-rc、NSIS 工具完成，并如实保留为未签名内测包。

本次没有创建 Tag、GitHub Release、公开下载、更新清单或自动更新制品，也没有改动服务器、数据库和用户配置。

正式验收批次：RU-059/WP-RU059-STANDALONE-CROSS-PACKAGING@2026-09-06T16:59:51.578615+00:00

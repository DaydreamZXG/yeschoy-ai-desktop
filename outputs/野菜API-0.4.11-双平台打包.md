# 野菜API 0.4.11 双平台内测包

本次产物来自同一份已验证源码，仅保存在本地；没有上传 Release、没有推送代码，也没有改动更新源。

## macOS

- 文件：`release/internal/0.4.11/macos/野菜API-0.4.11-macOS-universal.dmg`
- SHA-256：`9a7df65d9690281a56187e85fa7a1a060b990d832b70446b950d9b75f4adb8f7`
- 架构：Intel x86_64 + Apple Silicon arm64
- 状态：Developer ID 已签名、Apple 公证已通过、票据已装订、Gatekeeper 已验收

## Windows

- 安装程序：`release/internal/0.4.11/windows/野菜API-0.4.11-Windows-x64-内测版.exe`
- SHA-256：`73d2003cba3cf9b4f67cf03843b4e61684253af538769b659dee59246651a419`
- 架构：Windows x64
- 安装界面：简体中文，当前用户安装，不要求管理员权限
- 状态：在 macOS 上通过锁定的 `cargo-xwin` 与 NSIS 工具链交叉构建；尚未在真实 Windows 机器上冒烟测试，也没有 Windows 代码签名

## 验证结果

- 前端测试：454 项通过
- Rust 测试：479 项通过
- TypeScript 类型检查、Rust 格式检查、Clippy、生产构建：通过
- Prettier 检查：3 个既有源码文件存在排版差异，未在冻结打包阶段改动业务源码；不影响本次可执行产物
- 冻结打包单：`RU-062`；双平台安装包验收与变更范围审计由该打包单执行
- 校验方式：下载或复制安装包后，可按本文列出的 SHA-256 核对文件完整性
- Apple `stapler validate` 已在审计沙箱之外分别对应用与 DMG 执行并通过；沙箱内继续以签名、Gatekeeper 和公证提交 ID 做只读复验

完整机器可读记录见 `release/internal/0.4.11/release-receipt.json`。

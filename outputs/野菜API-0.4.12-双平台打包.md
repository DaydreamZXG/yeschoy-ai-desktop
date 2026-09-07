# 野菜API 0.4.12 双平台内测包

本次产物来自同一份已验证源码，仅保存在本地；没有上传 Release、没有推送代码，也没有改动更新源。

## macOS

- 文件：`release/internal/0.4.12/macos/野菜API-0.4.12-macOS-universal.dmg`
- SHA-256：`3ceefcf9168188b84c5abe6a1e6fde53aa8fa565fe42c51fe72bb6fcd0420eb5`
- 架构：Intel x86_64 + Apple Silicon arm64
- 状态：Developer ID 已签名、Apple 公证已通过、票据已装订、Gatekeeper 已验收

## Windows

- 安装程序：`release/internal/0.4.12/windows/野菜API-0.4.12-Windows-x64-内测版.exe`
- SHA-256：`569043a156c5b1b8f6fe1454fcd5cb8910df9ce5d7c5baff16c1a3aa66a348a6`
- 架构：Windows x64
- 安装界面：简体中文，当前用户安装，不要求管理员权限
- 状态：在 macOS 上通过锁定的 `cargo-xwin` 与 NSIS 工具链交叉构建；尚未在真实 Windows 机器上冒烟测试，也没有 Windows 代码签名

## 验证结果

- 前端测试：455 项通过
- Rust 测试：479 项通过
- TypeScript 类型检查、Rust 格式检查、Clippy、前端生产构建：通过
- Apple Silicon 与 Windows x64 编译检查：通过
- 冻结打包单：`RU-066`；双平台安装包验收与变更范围审计由该打包单执行
- 校验方式：下载或复制安装包后，可按本文列出的 SHA-256 核对文件完整性

完整机器可读记录见 `release/internal/0.4.12/release-receipt.json`。

# 设置页账户遗留文案清理

## 本次变更

- 删除设置页中“账户功能暂未开放”及其重复的“查看账户 / 检查连接”按钮。
- 保留应用侧边栏账户入口、`AccountView` 与 `desktop-account-session@v5` 行为。
- 不修改自动更新、第三方客户端下载、安装、接入、发布或服务端行为。

## 当前能力边界

- 助手自动更新：当前 Tauri 壳未注册 updater 插件或发布更新端点；国内源 `updates/stable.json` 当前返回 204，尚未上线。
- 客户端下载：Codex Desktop 与 Claude Desktop 已有下载/安装流程，国内目录已发布 macOS 与 Windows 构件；这说明下载源可用，不等同于全部平台的干净机器安装验收已经完成。
- 其他工具：Claude Code、Pi、DSH、Hermes 与 OpenClaw 当前仍是引导安装，不应描述为自动下载。

## 验证

- `src/settings/SettingsView.test.tsx`
- `tests/work_packages/ru063/test_acceptance.py::test_settings_removes_stale_account_unavailable_boundary`
- TypeScript 类型检查与相关格式检查

本单元不打包、不上传、不发布更新，也不修改远端服务。

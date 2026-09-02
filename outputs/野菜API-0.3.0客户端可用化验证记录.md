# 野菜API 0.3.0 客户端可用化验证记录

## 本次结果

本次交付把 0.2.0 中仅展示或占位的账户、模型和接入流程改成了客户端可执行功能。改动只发生在桌面客户端；没有修改线上 NewAPI、数据库、Nginx 或服务器配置。

源码提交：`a6bd4924abacd073ee0d82a79f6eba26f029eee6`

## 已修复的问题

1. 账户登录态现在由整个应用共享。大陆优化和全球加速使用同一份安全会话；切换线路只会重新拉取当前线路的数据，不会主动清除登录态。线路暂时不可用时，界面保留最近一次已确认的登录账户，不再错误显示“未登录”。只有服务端明确返回 401/403、会话过期或用户主动退出时才清除登录态。
2. 模型与价格页面直接复用已登录账户的模型列表，不再维护一套容易失败的公开模型列表。汇率从 NewAPI `/api/status` 的 `usd_exchange_rate` 读取，不再写死 6.75。
3. 价格对比使用 NewAPI 持续维护的定价元数据：官网参考价按 `model_ratio × 2 USD × usd_exchange_rate` 计算，野菜API 实际价再乘当前用户组的 `group_ratio`；输入和输出分别计算，缺少模型倍率、输出倍率、用户组倍率或汇率时不展示价格，避免猜测。
4. Claude Desktop 与 Codex Desktop 的“一键接入”已开放。客户端会先用 `/api/user/models` 和 `/api/pricing` 校验模型是否属于账户且支持目标协议，然后通过 NewAPI 已有 Token 接口创建或复用专用密钥，并原子写入本地配置。
5. Claude Desktop 写入 `~/.claude/settings.json` 中由野菜API负责的环境变量；Codex Desktop 写入 `~/.codex/config.toml` 的 `yeschoy` provider 和 `~/.codex/auth.json` 的密钥。其他配置字段会保留。配置文件写入失败时会回滚；不会保留额外备份文件。
6. API 密钥只在 Rust 原生层、系统安全存储和目标工具配置文件之间流转，不返回网页渲染层，也不会写入操作记录。

## 客户端使用的现有接口

- 设备授权：`/api/desktop/v2/device-authorizations`、`/api/desktop/v2/device-authorizations/token`
- 会话：`/api/desktop/v2/sessions/refresh`、`/api/desktop/v2/sessions/current`
- 账户与用量：`/api/user/self`、`/api/log/self/stat`
- 模型与价格：`/api/user/models`、`/api/pricing`、`/api/status`
- 工具专用密钥：`/api/token/search`、`/api/token/`、`/api/token/:id/key`

## 验证证据

- 前端单元与交互测试：24 个测试文件，277 项通过。
- Rust 原生测试：39 项通过。
- RU-024 验收测试：3 项通过。
- TypeScript 类型检查、Prettier、Rustfmt 和 `git diff --check` 通过。
- Windows GitHub Actions：运行 `33604114617` 成功，包含前端测试、Rust 原生测试和 Windows x64 NSIS 构建。
- macOS：通用二进制同时包含 `x86_64`、`arm64`；Developer ID 签名验证通过；苹果公证提交 `6c7427db-7783-4414-ba7f-f2e59fd0891c` 状态为 Accepted；Gatekeeper 判定为 `Notarized Developer ID`。

## 安装包

- macOS：`release/ru024-local/野菜API-0.3.0-20260902-macOS-universal-已签名公证.dmg`
  - SHA-256：`aadbc77486c7274bef47142e64cc27099c67072b49b3a0999d309c0dae3a7d53`
- Windows：`release/ru024-local/windows/野菜API-0.3.0-20260901-Windows-x64-内测版.exe`
  - SHA-256：`9379168bb2566412fb2561d79ff3bee407ea3e4f35a9e79db0633be6be2d9c30`

## 仍需真实账号确认

自动化验证覆盖了会话保持、动态汇率、模型协议校验、配置合并、失败回滚和无密钥泄露；本轮没有取得用户的网页登录会话，因此仍需要在安装版里用一个真实账户完成一次网页登录，并分别点击 Claude Desktop、Codex Desktop 的一键接入作为上线前冒烟测试。Windows 内测包按既定决策暂未购买代码签名证书，首次安装可能出现系统信誉提示。

DSH/DSH Desktop 不在 RU-024 的实现范围内，下一发布单元再按其真实配置格式接入，不在本版伪装成可用。

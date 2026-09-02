# 野菜API 0.4.0 真实一键接入验证记录

日期：2026-09-02  
发布单元：RU-027  
验收运行：`RU-027/WP-RU027-VERIFIED-ONE-CLICK-ADAPTERS@2026-09-02T10:54:56.379325+00:00`  
范围：仅桌面客户端；未修改 NewAPI、服务器、Nginx 或线上数据库。

## 本版结论

0.4.0 将“写入配置”和“接入成功”拆成两个状态。只有所选目标工具使用所选模型完成真实请求并返回响应后，界面才显示“接入完成”。写入、启动、请求或回读任一步失败，都会显示明确失败状态，并尝试恢复本次操作前的本地配置、钥匙串记录以及本次新建的 NewAPI 令牌。

密钥只保存在操作系统安全凭据存储中，不返回给前端，不写入普通配置文件，也不作为子进程命令行参数传递。

## 五类目标

| 目标 | 配置方式 | 完成条件 |
| --- | --- | --- |
| Claude Code | 合并 `~/.claude/settings.json`，通过 `apiKeyHelper` 读取钥匙串 | Claude Code 自身发出验证消息并取得非空回复 |
| Claude Desktop | 建立官方第三方网关配置及本机回环代理 | 用户在 Claude Desktop 发消息，代理观察到所选模型的真实成功响应 |
| Codex Desktop | 合并 `~/.codex/config.toml` 的独立 provider，保留官方登录资料 | 桌面应用内置 Codex 运行时完成真实只读验证请求，随后启动应用 |
| Pi | 合并 `~/.pi/agent/models.json` 与 `settings.json`，通过 helper 读取钥匙串 | Pi 自身使用所选 provider/model 取得非空回复 |
| DSH web | 合并 `~/.dsh/settings.yaml`，密钥仅在启动进程环境中注入 | 启动 DSH web，通过其 RPC 创建会话、发送消息并取得助手回复 |

模型列表不是静态猜测：客户端读取登录账户当前可用模型及 NewAPI 维护的 endpoint 元数据。Claude 目标只列出 `anthropic` 模型，Codex Desktop 只列出 `openai-response` 模型，Pi 与 DSH 只列出 `openai` 模型。

## 本机验证

- TypeScript 类型检查：通过。
- 前端单元测试：24 个文件、279 项测试通过。
- Rust 原生单元测试：51 项通过。
- Rust 全目标检查：通过；仅有未使用分支的编译警告。
- RU-027 四项验收：通过。
- 本机发现：Claude Code 2.1.233、Claude Desktop 1.40609.1、Codex Desktop 26.825.51511、DSH 0.1.0-rc.6 均能唯一识别；本机没有安装 Pi，因此 Pi 的真实账号请求仍需在装有 Pi 的验收机上做发布前设备验证。

## macOS 安装包

- 文件：`release/ru027-local/野菜API_0.4.0_universal.dmg`
- 架构：`arm64` + `x86_64`
- SHA-256：`80c6379f7dd38c8ac36aaf7c8bb70fe592de7f93de1da4cd6987c64876a3c225`
- 签名：Developer ID Application，Team ID `BRG82P5ZB7`，Hardened Runtime。
- Apple 公证：Accepted，Submission ID `2e159fa6-5093-40cf-8a48-a944884f5bec`。
- Gatekeeper：Accepted，来源 `Notarized Developer ID`。
- DMG 校验：`hdiutil verify` 通过；公证票据已装订并验证。

首次运行时，如果这台电脑已有开发版写入的钥匙串项目，macOS 可能要求确认一次访问权限；正式签名版本选择“始终允许”后不会把密钥暴露给界面。

## 尚未冒充为已完成的事项

- Windows x64 源码由现有 Windows CI 做原生编译与安装包验证；macOS 本机缺少 Windows `llvm-rc`，不以跨平台失败冒充 Windows 已验收。
- Pi 在本机未安装，所以当前证据覆盖配置合并、密钥边界、失败回滚和验证命令，但不覆盖真实 Pi 二进制的账号请求。
- Claude Desktop 的官方第三方网关需要本客户端的本机回环代理保持运行；退出野菜API 后，Claude Desktop 连接应明确视为不可用，重新打开野菜API 会恢复代理。

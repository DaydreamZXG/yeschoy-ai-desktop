# 野菜API 0.4.0 真实一键接入验证记录

日期：2026-09-02  
发布单元：RU-027  
验收运行：`RU-027/WP-RU027-VERIFIED-ONE-CLICK-ADAPTERS@2026-09-02T11:53:20.718904+00:00`  
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
- Rust 原生单元测试：52 项通过。
- Rust 全目标检查：通过；仅有未使用分支的编译警告。
- RU-027 四项验收：通过。
- 本机发现：Claude Code 2.1.233、Claude Desktop 1.40609.1、Codex Desktop 26.825.51511、Pi 0.84.4、DSH 0.1.1-rc.2 均能识别。Pi 使用的是迁移后的官方包 `@earendil-works/pi-coding-agent`；DSH 使用 npm `latest`/`next` 当前共同指向的正式候选版。
- Pi 0.84.4 实机验证：能读取客户端生成的 `models.json` / `settings.json`、执行签名客户端的钥匙串 helper，并把真实 Chat Completions 请求送达 NewAPI。所测模型随后均被 NewAPI 拒绝，明确返回 `admin-only` 用户组没有可用渠道；因此不能把本次结果表述为“模型回复成功”。失败后配置、专用令牌和钥匙串记录均已清理，账号登录会话已恢复为完整 JSON。
- DSH 0.1.1-rc.2 实机验证：`dsh web --host 127.0.0.1 --port 0 --no-open` 正常启动并返回受限回环地址；`session.create` 和 `session.models` RPC 均返回 `ok: true`。最新版新增的默认自动打开浏览器行为已用 `--no-open` 抑制，只允许客户端在真实验证完成后打开一次。
- Windows x64 CI：前端、Rust 原生测试、NSIS 打包和产物上传均通过；安装器 SHA-256 为 `1806b8b9d42ecdbf9630a2ee347faa01d71c7109bfa246d8ad737d9f2d086f7c`。

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

- Windows 安装器尚未代码签名，内测运行时可能触发 SmartScreen；CI 通过不等于完成 Windows 签名发布。
- Pi 与 DSH 已证明本地配置、凭证边界、版本发现、进程/RPC 和到达 NewAPI 的请求链路；但当前 `Root User / admin-only` 账号组没有可用模型渠道，尚未取得最终模型回复。要完成端到端回复验收，需要先由服务器维护者修复该账号组的渠道可用性或提供一个有可用 Chat 模型的测试账号。
- Claude Desktop 的官方第三方网关需要本客户端的本机回环代理保持运行；退出野菜API 后，Claude Desktop 连接应明确视为不可用，重新打开野菜API 会恢复代理。

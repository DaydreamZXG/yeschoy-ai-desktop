# 野菜API 桌面客户端 V1 候选版发布清单

状态：客户端候选构建准备完成；不是正式发布批准

核对日期：2026-08-30

## 1. 当前结论

仓库提供 `pnpm build:candidate` 作为候选安装包构建入口。该入口启用 Tauri bundle，但明确保持 `createUpdaterArtifacts: false`。它不会生成或启用自动更新包，也不会证明任何产物已完成代码签名、公证、恶意软件扫描、真机兼容或公开分发审核。

当前可以做本机开发、自动化验收和受控候选构建。不得把尚未取得下列证据的文件命名为“正式版”“安全更新”或“已签名版”。

## 2. 每次候选构建的固定前置检查

在干净检出上执行：

```text
pnpm install --frozen-lockfile
pnpm typecheck
pnpm test:candidate
pnpm build:renderer
cargo test --manifest-path src-tauri/Cargo.toml --lib
pnpm build:candidate
```

要求：

- 前端、Rust 和治理验收全部通过；警告单独记录，不能用构建成功替代测试。
- macOS 产物在 macOS 构建机生成；Windows 产物在 Windows 构建机生成。跨平台可编译不替代原生安装、卸载、启动和系统提示检查。
- 记录源码提交、锁文件哈希、Rust/Node/pnpm/Tauri 版本、构建平台与架构。
- 对每个候选安装文件记录 SHA-256、大小、构建时间和分发范围。
- 安装后复核应用名“野菜API”、包标识 `com.yeschoy.desktop`、版本 `0.1.0`、两条固定线路和五款固定工具。
- 确认安装包没有内置 API Key、网站 Cookie、账号令牌、更新私钥、签名私钥、证书密码或测试账号。

## 3. macOS 直接分发门槛

Apple 的当前直接分发要求以官方文档为准：

- 使用适当的 Developer ID Application 身份签名；普通开发证书或临时签名不能充当公开分发证据。
- 启用并验证 hardened runtime，带安全时间戳签名。
- 提交 Apple 公证，检查 notary log，成功后把票据 stapled 到分发物。
- 在计划支持的 Apple Silicon 与 Intel 目标上至少验证安装、首次打开、重启、卸载和两项本机只读命令。
- 私钥与公证凭据只进入受保护的本机构建钥匙串或 CI 密钥库，不进入仓库、日志、产物或 NewAPI。

官方依据：

- https://developer.apple.com/documentation/security/notarizing-macos-software-before-distribution
- https://developer.apple.com/support/developer-id/
- https://v2.tauri.app/distribute/

用户已经拥有个人 Apple Developer 账号，但这不能替代实际证书、签名、公证和真机证据。发布者名称与隐私取舍必须在生成公开包前单独核对，客户端不推断账号可用证书类型。

## 4. Windows 候选与公开分发门槛

当前没有已验证的 Windows 代码签名证书或远程签名服务，因此：

- 可以生成并在明确知情的受控测试者范围内分发“未签名候选测试包”。
- 下载页、文件名和测试说明必须显著标记未签名；不得显示“已验证发布者”。
- 测试者可能看到 Microsoft Defender SmartScreen 警告；企业策略或 Smart App Control 可能阻止继续，客户端不能承诺一定可以绕过。
- 公开分发前优先确定 Microsoft Store 或一致的受信代码签名路径，并在 Windows 构建环境验证签名、时间戳、安装、升级、卸载和发布者显示。
- 签名凭据只进入受保护的签名服务、证书存储或 CI 密钥库，不进入仓库、产物、NewAPI 或客户端设置。

官方依据：

- https://learn.microsoft.com/en-us/windows/apps/package-and-deploy/smartscreen-reputation
- https://v2.tauri.app/distribute/sign/windows/
- https://v2.tauri.app/distribute/windows-installer/

## 5. 自动更新门槛

当前配置固定关闭自动更新产物与运行时更新器。未来启用前必须同时具备：

- 独立更新签名密钥对；客户端只固化公钥，私钥仅在受保护构建环境使用。
- 第一方 HTTPS 更新清单与下载源，明确平台、架构、版本、哈希、签名和强制升级策略。
- 正确签名、错签、篡改、旧版本、同版本、断网、下载中断、安装失败和回滚/前向修复测试。
- Windows 和 macOS 平台代码签名与 Tauri 更新签名分别验证；两者不能相互替代。
- 发布撤回、密钥轮换、私钥丢失和紧急停更流程。

Tauri 当前文档明确要求更新包签名校验且不能关闭；私钥不能放进客户端：

- https://v2.tauri.app/plugin/updater/

## 6. 分发等级

| 等级 | 允许范围 | 必须满足 |
|---|---|---|
| 本机候选 | 开发者自己的电脑 | 自动化测试、原生构建、无秘密扫描 |
| 受控测试 | 明确知情的少量测试者 | 平台安装测试、哈希、已知限制、Windows 未签名警告或 macOS 已签名公证 |
| 公开正式版 | 普通小白用户 | 全部平台签名/公证、更新安全、隐私与遥测、后端契约、真实登录和配置事务证据 |

## 7. 当前仍阻止正式发布的事项

- `/api/desktop/v1` 设备授权、账号、用量、模型、价格、钱包和工具 Key 接口尚未部署验证。
- OS 凭据库、工具原生 credential helper、精确版本签名白名单和配置事务尚未实现。
- macOS Developer ID 签名与公证证据尚未产生。
- Windows 受信代码签名路径尚未确定；未签名包只适合受控测试。
- 自动更新私钥托管、公钥、清单、错签与篡改测试尚未完成。
- 遥测接收、字段白名单、同意、保留与删除机制尚未部署，因此客户端保持不上传。
- 完整 NFR、真机矩阵、安装升级、无障碍、安全和生产演练尚未完成。

这些事项不阻止继续开发和体验客户端候选版，但阻止把它称为完整商业 V1 或公开正式版。

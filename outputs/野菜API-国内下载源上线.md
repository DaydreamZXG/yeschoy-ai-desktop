# 野菜 API 国内下载源上线记录

## 已上线

`https://ergou.qzz.io/apps/catalog.json` 已公开 schema 2 下载清单，当前含 6 个原厂安装包槽位：Codex 与 Claude Desktop 的 macOS、Windows x64、Windows ARM64。

| 应用 | 平台 | 当前版本 | 公开校验状态 |
| --- | --- | --- | --- |
| Claude Desktop | macOS Universal | 1.46388.4 | 服务器已完成原生签名校验 |
| Claude Desktop | Windows x64 | 1.46388.4.0 | 客户端安装前执行 Windows 原生校验 |
| Claude Desktop | Windows ARM64 | 1.46388.4.0 | 客户端安装前执行 Windows 原生校验 |
| Codex | macOS Apple Silicon | 26.901.51231 | 服务器已完成原生签名校验 |
| Codex | Windows x64 | 26.901.5280.0 | 客户端安装前执行 Windows 原生校验 |
| Codex | Windows ARM64 | 26.901.5280.0 | 客户端安装前执行 Windows 原生校验 |

这些对象均是从代码中固定的厂商官方下载地址取得的原始字节。客户端仍会核对清单结构、来源、应用、平台、架构、包身份、发布者、大小和 SHA256，并在安装前交给 macOS Gatekeeper/代码签名或 Windows App Installer/签名链判断。国内源不可用、目录为空或文件校验失败时，会自动改走厂商官网。

## 远程停用与恢复

发布器不再要求额外的批准文件。运营方可以在服务器上原子发布一个合法空清单作为总开关；客户端无需升级便会自动退回官网。

2026-09-06 已在正式下载源做过一次完整演练：6 个槽位变为 0，健康状态变为未发布，6 个不可变对象与 3,110,376,973 字节原包均保留；随后逐槽恢复为 6 个，健康状态重新变为已发布。

## 实际验收结果

- 6 个公开对象的最终大小与 SHA256 全部重新计算并匹配清单。
- 6 个 HTTPS 地址均返回 `206` 且支持 1 字节 Range 请求，可继续断点下载。
- ZIP、DMG、MSIX 分别返回客户端允许的 `application/zip`、`application/x-apple-diskimage`、`application/vnd.ms-appx`。
- 发布器回归 64 项通过；原生安装链相关 Rust 测试 29 项通过；安装界面测试 20 项通过。
- macOS Apple Silicon、Windows x64、Windows ARM64 均完成离线跨平台 Clippy 编译检查。
- 共享入口配置哈希保持不变；未改数据库、NewAPI、用户账号或现有应用设置。

## 仍不能冒充完成的部分

- 本单元没有构建或发布新的野菜客户端安装包，用户必须拿到包含这套客户端逻辑的新版本后才会优先使用国内源。
- 野菜自身的自动升级频道仍未发布；`/updates/stable.json` 与 `/updates/beta.json` 继续保持未启用。
- 跨平台编译与公开下载可达，不等于 Windows/Apple Silicon 干净电脑上的首次安装验收；正式发布前仍要各跑一次真机安装。
- 原厂包可下载并通过本机信任校验，不等于第三方服务登录、额度或模型调用一定可用，这些需独立验证。

机器可读的部署事实记录在 `deploy/vendor-sync/deployment-receipt-ru053.json`。

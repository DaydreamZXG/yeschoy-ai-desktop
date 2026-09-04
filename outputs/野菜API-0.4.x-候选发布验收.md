# 野菜API 0.4.1 候选发布验收

日期：2026-09-04。

## 候选结论

0.4.1 客户端已具备两条当前线路、账户登录与余额用量、服务端汇率和计费分组价格、七种工具识别与安全接入事务，以及 macOS 通用安装包。当前结论是“可进入受控发布与三平台实机验收”，不是“所有工具和平台已完成真实账户验证”。

## macOS 安装包

- 文件：`release/ru028-local/野菜API-0.4.1-macOS-universal.dmg`
- 架构：Intel x86_64、Apple arm64
- 大小：9,635,286 字节
- SHA-256：`19e290f716daf99e11f28f362812230ac4235d1319decf7703ef73149bdce9bd`
- Apple 公证：Accepted，提交号 `d9fd7672-46cc-44f4-a31d-f8b0c7d57884`
- 票据：已附加并验证
- Gatekeeper：accepted，Notarized Developer ID

## 发布边界

- `api.yeschoy.com` 作为后续恢复项，不进入本版运行线路。
- 自动更新不在本版启用，不显示虚假的“已是最新”状态。
- Windows x64 使用手动 CI 生成未签名 NSIS 安装包；当前源码推送后必须重新执行并下载新产物。
- 没有修改服务器、NewAPI、数据库或线上配置，也没有自动安装第三方客户端。

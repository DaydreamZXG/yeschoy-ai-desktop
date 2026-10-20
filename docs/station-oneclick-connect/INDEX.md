# 站长傻瓜接入（收进桌面仓文档）

- 总览：[README.md](./README.md)
- **New API 一键补丁（已验收）**：[A-一键补丁脚本.md](./A-一键补丁脚本.md) · [A/站长一页说明.md](./A/站长一页说明.md) · [A/VERIFIED.md](./A/VERIFIED.md)
- 可下载草案：`newapi-desktop-connect-patch-0.1.1-20261020.tgz`（同目录，含 sha256）
- Sub2API / Sidecar / AI 提示词：见同目录其它 md

配套：`connect/yeschoy-connect.ts` 已支持「refresh cookie → Bearer 发码」，请用 `pnpm build:connect` 生成的 `public/connect.js`（或已重建版本）部署到 `https://yeschoy.com/connect.js`。

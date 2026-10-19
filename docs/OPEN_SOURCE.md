# 开源发布准备清单

## 构建方式

| 构建 | 怎么来 | 更新源 | 安装包镜像 | 远程模型目录 |
|---|---|---|---|---|
| 公开（社区）构建，默认 | `pnpm build` | GitHub Releases `latest.json`（需构建时提供 `YESCHOY_COMMUNITY_UPDATER_PUBKEY`，否则自动更新关闭，界面提示去 GitHub 下载） | 关闭，只走官方下载源 | 关闭，用内置目录 |
| 官方 / 合作方包 | `deploy/self-update/build-local.mjs`（会设 `YESCHOY_DISTRIBUTION=official\|partner`） | `src-tauri/update-channels.json` 里的官方源 | `YESCHOY_APP_MIRROR_HOST`、`YESCHOY_APP_MIRROR_ADDR` | `YESCHOY_MODEL_CATALOG_URL` |

可选的构建期变量：`YESCHOY_GITHUB_REPO`（默认 `DaydreamZXG/yeschoy-ai-desktop`）。

## 发官方包之前要做的事（只做一次）

1. 在 GitHub 仓库 Settings → Secrets and variables → Actions → **Variables** 里新建 `DEPLOY_HOST`，值填下载服务器地址。发版流程不再把 IP 写在源码里。
2. 如果官方包要继续用安装包镜像和远程模型目录，把 `YESCHOY_APP_MIRROR_HOST`、`YESCHOY_APP_MIRROR_ADDR`、`YESCHOY_MODEL_CATALOG_URL` 作为 env 加到 `yeschoy-release.yml` 的 windows 和 macos 构建步骤里（值从仓库变量读取）。不加也能用，只是没有镜像。

## 站点接入

- 任何 HTTPS 中转站都能接入。站点第一次出现时，桌面端会弹窗请用户确认，确认过的站点保存在应用数据目录的 `trusted-stations.json`。
- 站点没有部署桌面接口时，用户可以粘贴 sk- 令牌：桌面端会验证令牌并列出可用模型。
- 站点想做到一键登录，按 `docs/patch/new-api-desktop-connect/` 打补丁即可。

## 清理 git 历史（不在真实仓库上执行）

真实仓库不改写历史。开源时建议另建一个新的公开仓库，用全新历史：

```bash
scripts/oss/clean-history.sh ../yeschoy-oss product/yeschoy-v1
# 人工检查 ../yeschoy-oss，确认干净后再自己 push 到新的公开仓库
```

脚本只导出文件快照，删除内部目录，扫描内部地址和密钥形态（只列出位置），然后生成单个初始提交。它**不会推送**。

## 历史里出现过的密钥

| 密钥 | 位置 | 时间 | 处理 |
|---|---|---|---|
| Context7 API key `ctx7sk-4ddd****` | `deplink.html`（来自上游 CC Switch #286） | 2025-11-25 引入，2026-09-29 删除（219a8c50） | 这是上游项目的 key，不是我们的，已经随 CC Switch 公开。用全新历史开源就不会带上它。如果想负责任一点，可以通知上游作废。 |
| ccswitch 深链示例里的 `sk-…` | `deplink.html` | 同上 | 示例值，同上 |
| 测试夹具里的 `sk-…`、`AKIA…`、JWT 形态字符串 | `src-tauri/src/**` 的测试 | — | 都是假值 |

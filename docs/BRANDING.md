# 官方包与开源 / 第三方站隔离（BRANDING）

本文约定「野菜」官方桌面包与社区 / 开源构建、以及各中转站网页入口之间的边界。目标：**通道分开、身份分开、广告可关且不挡路**。

相关实现：`YESCHOY_DISTRIBUTION` / `VITE_YESCHOY_DISTRIBUTION`（见 `src-tauri/build.rs`、`deploy/self-update/build-local.mjs`、`src/distribution/identity.ts`）。开源清单见 [`OPEN_SOURCE.md`](./OPEN_SOURCE.md)。

## 两种构建

| | **官方「野菜」包** | **社区 / 开源包**（默认） |
|---|---|---|
| 怎么打 | `deploy/self-update/build-local.mjs`，设 `YESCHOY_DISTRIBUTION=official\|partner` 与 `VITE_YESCHOY_DISTRIBUTION=official` | `pnpm build` / CI；**不**设上述变量（默认为 `community` / OSS UI） |
| 应用内更新 | 编入 `src-tauri/update-channels.json`，主源为 `ergou.qzz.io/updates/{official\|partner}/stable.json` | GitHub Releases（需 `YESCHOY_COMMUNITY_UPDATER_PUBKEY`）；否则关闭自动更新并引导去 GitHub |
| 安装包镜像 / 远程模型目录 | 可由官方流水线编入（`YESCHOY_APP_MIRROR_*`、`YESCHOY_MODEL_CATALOG_URL`） | 默认关闭；fork **不要**填 ergou 的目录地址 |
| 默认中转站预填 | 可预填参考站 `https://yeschoy.com`（用户仍可改） | **不预填**官方站；由用户粘贴自己的站或走网页深链 |
| 轻量官网入口 | 设置页底部一行链到 `https://yeschoy.app`，**可关掉** | **无**官方广告；可选「参考实现」归因（见下） |

`OFFICIAL_BUILD` 若在外部脚本中使用，应映射为：`YESCHOY_DISTRIBUTION=official`（或 `partner`）且 `VITE_YESCHOY_DISTRIBUTION=official`。

## `ergou.qzz.io` 仅官方国内回源

- **允许**：官方 / 合作方包的自动更新 feed、官方流水线发布的安装包与模型目录回源。
- **禁止**：社区包、第三方站页面、`connect.js` 嵌入、站长文档把用户导向 `ergou.qzz.io` 做更新或冒充官方下载。
- 公开下载页优先：GitHub Releases → 必要时再回落 ergou（见 [`download-page.md`](./download-page.md)）。

第三方不得：

1. 冒充「官方野菜」安装包或更新通道；
2. 把官方中转站（如 `yeschoy.com` / `api.yeschoy.com`）写死为用户唯一默认站；
3. 在分发包中编入官方 `update-channels.json` 或 ergou 公钥通道（除非自己是授权合作方流水线）。

## 站点接入：网页深链，不嵌官网

- 推荐：各站网页提供「打开桌面助手」按钮，走 `yeschoy://connect?...`（协议与补丁见 [`station-connect-protocol.md`](./station-connect-protocol.md)、[`station-connect-embed.md`](./station-connect-embed.md)）。
- **`https://yeschoy.app`** 是产品官网 / 下载落地页；各中转站自己的「打开桌面助手」按钮是站内入口。**二者互不替换、互不嵌入**——官网不代替站按钮，站按钮也不应 iframe 或冒充官网。
- 客户端对第三方站只展示该站自己的域名与站名；不出现「官方推荐某某站」类置顶。

## 轻量广告（仅官方包）

- **位置**：设置页底部一行小字（例如「由野菜开源 · 官网」），链到 `https://yeschoy.app`。
- **可关**：用户点「不再显示」后写入本地偏好（`localStorage`），之后不再出现。
- **不做**：弹窗、启动页霸屏、聊天 / 模型列表 / 计费路径里的推广。

开源包底部若出现「参考实现：野菜API」一类链接，属于归因软提示，且在用户已接入第三方站或已登录时应隐藏（见 `showOssSoftTraffic`）。

## 检查清单（发版 / 审 PR）

- [ ] 社区构建未设置 `YESCHOY_DISTRIBUTION=official|partner`。
- [ ] 官方包才编入 ergou 更新通道；社区包 updater 指向 GitHub 或关闭。
- [ ] 第三方文档与 `public/connect.js` 不出现 ergou 更新 CDN（禁止语境除外）。
- [ ] OSS 默认不预填官方站域名；深链 `yeschoy://connect` 逻辑保留。
- [ ] 官方轻量官网行可关；主题仍为米灰 / 叶绿 + 近黑 / 薄荷绿（勿用朱红暖纸）。

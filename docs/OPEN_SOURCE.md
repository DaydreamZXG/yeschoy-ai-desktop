# 开源发布准备清单

品牌与官方/开源隔离约定见 [`BRANDING.md`](./BRANDING.md)。

## 构建方式

| 构建 | 怎么来 | 更新源 | 安装包镜像 | 远程模型目录 |
|---|---|---|---|---|
| 公开（社区）构建，默认 | `pnpm build` | GitHub Releases `latest.json`（需构建时提供 `YESCHOY_COMMUNITY_UPDATER_PUBKEY`，否则自动更新关闭，界面提示去 GitHub 下载） | 关闭，只走官方下载源 | 关闭，用内置目录 |
| 官方 / 合作方包 | `deploy/self-update/build-local.mjs`（会设 `YESCHOY_DISTRIBUTION=official\|partner`） | `src-tauri/update-channels.json` 里的官方源 | `YESCHOY_APP_MIRROR_HOST`、`YESCHOY_APP_MIRROR_ADDR` | `YESCHOY_MODEL_CATALOG_URL` |

可选的构建期变量：`YESCHOY_GITHUB_REPO`（默认 `DaydreamZXG/yeschoy-ai-desktop`）。

## 发官方包之前要做的事（只做一次）

1. 在 GitHub 仓库 Settings → Secrets and variables → Actions → **Variables** 里新建 `DEPLOY_HOST`，值填下载服务器地址。发版流程不再把 IP 写在源码里。
2. 远程模型目录：在同一页 **Variables** 里新建 `YESCHOY_MODEL_CATALOG_URL`，官方仓库填 `https://ergou.qzz.io/apps/model-catalog.json`。`yeschoy-release.yml` 的 windows/macos 打包步骤只在官方仓库（`DaydreamZXG/yeschoy-ai-desktop`）读取它；fork 或变量为空时不编入地址，客户端只用内置目录。**其他站点不要填 ergou 的地址**，要用远程目录请换成自己的 https 地址并自己发布。本地 `build-local.mjs` 同样读这个环境变量（必须 https://，空值不编入）。
   - 客户端行为：启动时拉取 `<地址>` 和 `<地址>.sha256`，sha256 不符、schema 不合法、`verifiedAt` 不比当前新、网络失败时都保留当前目录（缓存或内置），不会变空。
   - 发布：`node scripts/ops/publish-model-catalog.mjs --to /srv/yeschoy-download/public/apps`（在下载服务器上执行），发布前把 `catalog.json` 的 `verifiedAt` 改成新日期。
3. 如果官方包要用安装包镜像，把 `YESCHOY_APP_MIRROR_HOST`、`YESCHOY_APP_MIRROR_ADDR` 作为 env 加到上述打包步骤里（值从仓库变量读取）。不加也能用，只是没有镜像。

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

## 合作方商标图（partners）

历史树里曾有 `assets/partners/logos` / `banners`（多来自上游 CC Switch 展示用物料）。产品代码**不引用**这些路径。

**策略（本仓库已落地）：**

1. 公开树只保留 `assets/partners/README.md`，说明不附带商标二进制。
2. 官方/合作方发行若需展示物料，放在已获授权的**私有发行资源**里，不要提交回本公开仓库。
3. 用 `scripts/oss/clean-history.sh` 导出新历史时，会删除上述 logos/banners 目录。

## GitHub Secret scanning / Push protection（仓库设置，需维护者点一次）

本仓库若对社区可见，请在 GitHub 打开防护（文档步骤；**不能**靠合并本 PR 自动打开）：

1. 打开 <https://github.com/DaydreamZXG/yeschoy-ai-desktop/settings/security_analysis>
2. 启用 **Secret scanning**
3. 启用 **Push protection**（阻止把已知密钥形态推进仓库）
4. （建议）启用 **Dependabot alerts** / security updates

当前工作树里的 `sk-` / `AKIA` / JWT 形态字符串应只出现在测试夹具且为假值；若 scanning 报警，先当真实密钥轮换，再改夹具为明显假值。

## Git 历史说明（不在本 PR 改写真实仓库）

真实 `DaydreamZXG/yeschoy-ai-desktop` **不强制** `git filter-repo`。开源对外「干净起点」推荐：

```bash
scripts/oss/clean-history.sh ../yeschoy-oss product/yeschoy-v1
# 人工检查 ../yeschoy-oss 后，再决定是否 push 到新公开仓库或替换默认分支
```

历史中曾出现的 Context7 / 示例密钥见上文「历史里出现过的密钥」。启用 secret scanning 后，即使旧提交仍在，推送新提交时的密钥会被挡住。

## 社区 Release 手工步骤（本 PR 不自动发版）

社区轨要让用户从 GitHub Releases 装上包，并（可选）让社区构建能打到 `latest.json`，按下列顺序**人工**操作。合并文档 PR ≠ 发版。

1. **确认** `product/yeschoy-v1`（或选定发版提交）已包含本开源文档与 partners 清理；版本号已在 `package.json` / Tauri 配置中 bump。
2. **打开** 上文 Secret scanning / Push protection。
3. **用社区构建**打安装包（不要走官方 `YESCHOY_DISTRIBUTION=official|partner` 镜像变量，除非有意发官方包）：
   - 本地：`pnpm build`（需要签名/公证时再准备对应证书与 `YESCHOY_COMMUNITY_UPDATER_PUBKEY`）
   - 或按需触发仓库里的发版 workflow，但 **Release 勾选为非草稿**、面向 public。
4. **创建 GitHub Release**（网页或 `gh release create`）：
   - Tag：如 `v0.x.y`（与包内版本一致）
   - 标题与说明用中文/英文简述变更
   - 上传各平台安装包与校验文件
   - **不要**标成 Draft；需要时勾选 Latest
5. **更新清单（可选，为应用内检查更新）**：
   - 生成/上传 Tauri updater 用的 `latest.json`（及签名），URL 与社区构建时写入的公钥一致
   - 若未提供 `YESCHOY_COMMUNITY_UPDATER_PUBKEY`，社区包会关闭自动更新，界面引导用户去 GitHub 下载——这样也符合「能装上」的最低目标
6. **验证**：匿名浏览器打开 Release 页可下载；用干净机器安装后能完成一次「选应用 → 预览 → 取消」且不误连官方镜像。
7. **Catalog 热更新**与社区 Release 无关；模型标签 publish 另做，勿与本步骤绑死。

官方 ergou / partner 发版仍按原有 `deploy/self-update` 与 `yeschoy-release.yml` 流程，与社区轨分离。

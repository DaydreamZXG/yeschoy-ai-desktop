# Sub2API 站长：野菜「一键连」怎么搞（小白版）

> **状态**：可审阅草案（2026-10-20，Asia/Shanghai）。**未合入仓库发版、未部署生产。**  
> **对象**：跑的是 [Wei-Shaw/sub2api](https://github.com/Wei-Shaw/sub2api)（或其私改 fork），不是 New API。  
> **一句话**：**不能**直接跑 New API 那套一键补丁脚本；现实只有两条路——**挂 Sidecar**，或**自己（/ 让 AI）在 Sub2API 里做出同样协议的接口**。

---

## 先说结论（站长只看这段也行）

| 你想怎么做 | 能不能用 | 白话 |
|---|---|---|
| 跑 `install-newapi-desktop-connect.sh`（① New API 补丁） | ❌ **不行** | 那是给 New API 目录结构写的，套到 Sub2API 上会认错文件、直接失败或改坏站 |
| 旁边挂「野菜连接小助手」+ 改反代（② Sidecar） | ✅ **推荐** | 主站程序尽量不动；要改的是反代几条路径 |
| 在 Sub2API 源码里自己实现发码/兑码（自研接口） | ✅ 可以 | 没有现成补丁包；适合会 Go/Vue 或愿意把下面提示词丢给 AI 的人 |
| 只塞 `connect.js` 按钮 | ❌ **不够** | 按钮会唤起，但没有发码兑码，客户端连不上账 |

用户最终要看到的，和 New API 一样：

1. 在你站 HTTPS 域名登录；
2. 打开 `https://你的域名/desktop/connect`，点「打开桌面助手」；
3. 跳到 `yeschoy://connect?origin=…&code=…`；
4. 客户端兑码后能看余额 / 建令牌——**不只是弹一下窗口**。

---

## 为什么 New API 补丁脚本套不上？（必读）

New API（QuantumNous/new-api）和 Sub2API（Wei-Shaw/sub2api）**不是同一套程序**，只是都能当「中转站」用。

| 对比 | New API | Sub2API |
|---|---|---|
| 仓库 | 常见 `controller/`、`model/`、`router/`、`middleware/` 铺在仓库根附近 | `backend/` + `frontend/` 单体仓；后端是 `handler` / `service` / `repository` + **Ent** + **Wire** |
| 登录与鉴权 | Cookie + 常要 `New-Api-User` 头；补丁里专门加了桌面 Bearer | 自己的会话 / JWT / API Key 体系，**路径和头都不一样** |
| 「我是谁 / 余额 / 令牌」API | 约定好的 `/api/user/self`、`/api/token/` 等 | **没有**同名同形接口；Sidecar 或自研时要按 Sub2API 真实 API 做「会话桥」 |
| ① 一键脚本在干什么 | 往固定文件塞 `desktop_connect.go`、改 `api-router` / `UserAuth` | 这些文件在 Sub2API **根本不存在** → 脚本会拒绝或乱改 |

所以：

- **① 一键补丁** = 只服务 New API；Sub2API 站长请关掉这个念头。
- **② Sidecar** = 协议仍是野菜固定的 `/api/desktop/v2/*`，但回源时要走 **Sub2API profile**（和 New API profile 分开做）。
- **自研** = 在你这份 Sub2API 里，亲手（或让 AI）实现同一套 OpenAPI，再映射到你站已有的用户/令牌逻辑。

> 小白口诀：**补丁脚本认的是「New API 的房间号」；你住的是「Sub2API 另一栋楼」，钥匙不能通用。**

---

## 路线 A（推荐）：挂 Sidecar

细节步骤、反代路径、验收清单，先看总览里的  
[C-一键挂Sidecar.md](./C-一键挂Sidecar.md)。

Sub2API 站长额外记住这三点：

1. **compose 里的上游地址**写成你的 Sub2API 内网地址（不要写 `new-api:3000` 这个名字也没关系，变量语义是「主站根 URL」）。
2. **「会话桥」必须按 Sub2API 调**：发码前用浏览器 Cookie（或站内会话）问 Sub2API「当前用户是谁」；兑码后桌面 Bearer 要能换到 Sub2API 能认的访问方式（常见是映射成用户 API Key / 站内 token）。  
   → 野菜正式镜像落地前，可能要单独的 **`UPSTREAM_PROFILE=sub2api`**（草案；未发版）。
3. **反代别漏**：  
   - 必进 Sidecar：`/desktop/connect`、`/api/desktop/v2/`  
   - 若 Sidecar 代读账：还要把桌面用的「用户信息 / 令牌」类路径指过去（具体路径以你站 OpenAPI 为准，**不要照抄 New API 的 `/api/user/self` 除非你站真有这条**）。  
   漏了就会出现：**连上了，余额是空的**。

当前诚实状态：

- Sidecar **设计有了，官方镜像还没得 `docker pull`**（见 C 文档「落地前还缺什么」）。
- 在镜像出来之前：可以先用路线 B（自研 / AI），或等野菜发 Sub2API profile。

---

## 路线 B：自研接口（或丢给 AI）

在你自己的 Sub2API 仓里实现与野菜约定一致的路径（对外路径不要改名）：

| 谁调用 | 方法 | 路径 |
|---|---|---|
| 浏览器（已登录） | POST | `/api/desktop/v2/connect-codes` |
| 桌面端 | POST | `/api/desktop/v2/connect-codes/exchange` |
| 桌面端 | GET | `/api/desktop/v2/bootstrap` |
| 桌面端 | POST / DELETE | `/api/desktop/v2/sessions/refresh`、`/sessions/current` |

安全底线（和 New API 一样，不能省）：

- 连接码 TTL ≤ 5 分钟、一次性、绑定 HTTPS `origin`；
- 长期 token **禁止**进 URL；
- 兑码校验 Host 与绑定 origin 一致；
- token 只存哈希；日志不打明文 code/token；
- 页面路径固定 **`/desktop/connect`**，可加载 `https://yeschoy.com/connect.js`。

对照材料（野菜桌面仓，只读参考）：

- `docs/station-connect-api.openapi.yaml`
- `docs/station-connect-protocol.md`
- `docs/station-connect-page.md`
- `public/desktop/connect.html`
- （**不要**把 `docs/patch/new-api-desktop-connect/*.go.txt` 当可粘贴代码——那是 New API 草稿。）

### 发给 AI 的提示词（整段复制）

把工程切到独立分支后，粘贴下面整段；允许 AI 改 `backend/` / 必要前端路由；**禁止**让它推生产、动计费核心、乱删库。

```text
你是资深 Go（Gin + Ent + Wire）和 Vue 工程师。我的项目是 Wei-Shaw/sub2api 或其 fork，
不是 New API。请实现「野菜 / YesChoy 桌面一键连接」最小能力。

【重要】
- 禁止套用 New API 的 desktop_connect 补丁文件或 install-newapi-desktop-connect.sh。
- 那些文件的目录（controller/、model/main.go、UserAuth、New-Api-User）在我仓不存在。
- 请按我仓真实结构：backend/internal/handler、service、repository、server/router、ent、Wire；
  前端在 frontend/（若需 /desktop/connect 页面或路由）。

【目标用户体验】
1. 用户在本站 HTTPS 域名已登录；
2. 打开 https://<本站>/desktop/connect ，点「打开桌面助手」；
3. 浏览器 POST 同源 /api/desktop/v2/connect-codes 拿到一次性 code；
4. 跳转 yeschoy://connect?origin=<canonical-https-origin>&code=<code>；
5. 桌面 POST /api/desktop/v2/connect-codes/exchange 换会话；
6. 之后桌面带 Authorization: Bearer <accessToken> 能读到「当前用户 / 余额或额度 /
   可用模型或令牌」——请映射到我站现有 API，不要假装实现 New API 的
   /api/user/self 路径（除非我站本来就有完全兼容的同名接口）。

【硬性约束】
- 只改我这份代码，不推 upstream。
- 改动面尽量小：不重构计费、订阅、网关调度、主题。
- 连接码：TTL ≤ 300 秒、一次性、绑定 origin；长期 token 禁止进 URL。
- 兑码校验请求 Host 与绑定 origin 一致，否则 403（可用 code: connect_origin_mismatch）。
- token 只存 SHA-256 哈希；日志禁止打印明文 code/token。
- device_authorization 可先关闭（bootstrap 里 device_authorization_available=false）。
- /desktop/connect 路径固定；页面可加载 https://yeschoy.com/connect.js，
  按钮 data-yeschoy-connect；支持 ?auto=1。
- 有 Redis 用 Redis 存连接码，否则进程内 Map，并注明多副本限制。
- 需要新表就走 Ent schema + 生成；更新 Wire 依赖注入。

【对照材料（契约，不是可粘贴的 New API 代码）】
- docs/station-connect-api.openapi.yaml
- docs/station-connect-protocol.md
- docs/station-connect-page.md
- public/desktop/connect.html

【请你按顺序做】
A. 扫描我仓：路由注册、登录中间件、用户/API Key 模型、前端路由与静态资源如何嵌入。
B. 实现 /api/desktop/v2/*（issue / exchange / bootstrap / refresh / logout）可编译可跑。
C. 说明桌面 Bearer 如何映射到我站已有鉴权（新建「桌面会话」表，或映射用户 API Key）。
D. 提供 /desktop/connect（后端静态页或 Vue 路由二选一，行为对齐参考 HTML）。
E. 给出：本地/compose 重建命令、curl 验收、git 回滚文件列表。
F. 自检：未登录发码失败；登录发码成功；码一次性；Bearer 能读到用户信息；
   错误 origin 兑码失败。

【输出】
1. 变更文件列表（路径 + 一句目的）
2. 与 New API 补丁的差异说明（避免后人再套错）
3. 我需要手动执行的命令
4. 代码以可合并为准，不要长篇空文档
```

### 编译报错时追加（短版）

```text
编译/运行报错如下（原样粘贴）。请只修野菜桌面连接相关文件，对齐我仓现有
handler/service/ent/wire 写法，不要扩大范围，不要引入 New API 文件布局。
修完列出 diff 并给出重新 build 命令。

（在此粘贴错误日志）
```

### 后端已好、只差按钮时追加

```text
后端 /api/desktop/v2 已可用。请只加 /desktop/connect 页面或前端路由：
加载 https://yeschoy.com/connect.js，使用 data-yeschoy-connect；
未登录提示先登录；支持 ?auto=1。不要改 API 与数据库。
```

---

## 站长验收（做完 A 或 B 都勾）

- [ ] `https://域名/desktop/connect` 未登录会提示先登录。
- [ ] 登录后能唤起已安装的野菜客户端。
- [ ] 客户端显示已连接该站（不是只闪一下）。
- [ ] 能看到用户信息 / 余额或额度（说明桥或自研鉴权没漏）。
- [ ] 同一连接码不能兑第二次；过期失败。
- [ ] 故意改深链 origin，兑码失败。
- [ ] **确认没对 Sub2API 目录跑过 New API 的 install 脚本**（若误跑：用脚本备份 / `git checkout` 回滚）。

---

## 和 New API 文档怎么分工

| 文件 | 给谁 |
|---|---|
| [A-一键补丁脚本.md](./A-一键补丁脚本.md) + `A/install-*.sh` | **仅 New API** |
| [C-一键挂Sidecar.md](./C-一键挂Sidecar.md) | New API / Sub2API 都能看；Sub2API 回源要换 profile |
| [AI-提示词.md](./AI-提示词.md) | **仅 New API** 源码改造 |
| **本文件** | **Sub2API** 选型 + 自研/AI 提示词 |

---

## 给产品/运营的一句话

> Sub2API 站长：别发 New API 补丁脚本。主推「挂 Sidecar」；能改源码的给「自研 + AI 提示词」。按钮脚本可以官方托管，但发码兑码必须在站侧（或 sidecar）按同一协议落地。

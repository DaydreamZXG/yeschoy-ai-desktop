# 站长傻瓜接入：野菜「一键连」桌面助手

> **状态**：① New API 一键补丁已在干净上游验收（2026-10-20，Asia/Shanghai），见 `A/VERIFIED.md`；未部署生产站；发版渠道另开。  
> **目的**：让第三方站长（New API / Sub2API 等）用尽量少的步骤开通「网页一点 → 唤起野菜桌面」。  
> **对照仓库**：`DaydreamZXG/yeschoy-ai-desktop` 里已有协议与补丁草案，本目录只做**站长侧接入方案**，不写完整产品代码。

---

## 用户最终看到什么

1. 用户在你的站点登录。
2. 打开 `https://你的域名/desktop/connect`，点「打开桌面助手」。
3. 浏览器跳到 `yeschoy://connect?origin=…&code=…`，野菜客户端兑码登录。
4. 之后在客户端里能看到余额、建令牌、连应用——**不只是「唤起」一下**。

只塞一段 `connect.js` **不够**：站点（或旁边的小服务）必须能**发码、兑码、认桌面会话**。  
官网脚本只负责按钮：`https://yeschoy.com/connect.js`。

---

## 怎么选（一张表）

| 你的情况 | 选哪条 | 大概体感 |
|---|---|---|
| **New API**，能改源码 / 自己编译，私改要保留 | **① 一键补丁脚本** | 像「打补丁再重建」，私改还在 |
| **New API 或 Sub2API**，不想动主程序，反代能改 | **② 一键挂 Sidecar** | 旁边挂容器，主站镜像不动 |
| **New API**，不会改但有 Cursor / ChatGPT / Claude | **③ 发给 AI 的提示词** | 复制粘贴，让 AI 按 New API 补丁改 |
| **Sub2API**（Wei-Shaw/sub2api） | **看 Sub2API 专页** | ① 脚本套不上；用 Sidecar 或自研/AI |

New API 站长：①②③ 选一条做完即可。  
**Sub2API 站长：不要跑 ①**；打开 [Sub2API-傻瓜接入.md](./Sub2API-傻瓜接入.md)。

**不推荐作为主推**：整包换你们打好的镜像（会盖掉站长私改）。有需要再当「零代码极简站」第四选项。

---

## ① 快速试跑（草案，勿对生产）

```bash
cd station-oneclick-connect/A
./install-newapi-desktop-connect.sh --src /path/to/your/new-api --dry-run
./install-newapi-desktop-connect.sh --src /path/to/your/new-api --yes
# 然后按脚本打印的说明自行 docker compose build && up
```

详情见 [A-一键补丁脚本.md](./A-一键补丁脚本.md)。

## 共同前提（三条都要满足）

1. **HTTPS 域名**（不能用裸 IP、不能长期 http）。
2. 用户网页登录的 Cookie，和发码接口在**同一个站点源**（同源）。
3. 固定页面路径：**`/desktop/connect`**（桌面端写死了，别改成别的路径）。
4. 至少实现协议里这几条（详见仓库文档）：

| 谁调用 | 方法 | 路径 |
|---|---|---|
| 浏览器（已登录） | POST | `/api/desktop/v2/connect-codes` |
| 桌面端 | POST | `/api/desktop/v2/connect-codes/exchange` |
| 桌面端 | GET | `/api/desktop/v2/bootstrap` |
| 桌面端 | POST / DELETE | `/api/desktop/v2/sessions/refresh`、`/sessions/current` |

5. 桌面用 `Authorization: Bearer <accessToken>` 访问现有：
   `/api/user/self`、`/api/user/models`、`/api/token/`、`/api/log/self`、`/api/pricing`  
   （原版 New API 的 `UserAuth` 还要 `New-Api-User` 头，桌面不带 → **必须加 Bearer 中间件，或由 Sidecar 代发**。）

6. 安全底线（不能省）：
   - 连接码 TTL ≤ 5 分钟、一次性、绑定 origin；
   - 长期 token **禁止**进 URL；
   - 兑码校验 Host 与绑定 origin 一致。

---

## 仓库里已经有什么（对照用，勿当「已上线补丁包」）

| 路径 | 内容 |
|---|---|
| `docs/patch/new-api-desktop-connect/README.md` | New API 最小补丁说明（草案） |
| `docs/patch/new-api-desktop-connect/desktop_connect.go.txt` | Go 草稿（`.txt`，未编译未测） |
| `docs/station-connect-protocol.md` | 深链与协议 |
| `docs/station-connect-api.openapi.yaml` | 机器可读契约 |
| `docs/station-connect-embed.md` | `connect.js` 嵌入 |
| `docs/station-connect-page.md` | `/desktop/connect` 页面约定 |
| `public/desktop/connect.html` | 参考连接页 |
| `public/connect.js` / `connect/yeschoy-connect.ts` | 官方按钮脚本源码 |

本方案目录：

| 文件 | 内容 |
|---|---|
| [A-一键补丁脚本.md](./A-一键补丁脚本.md) | ① New API 一键打补丁（**可跑草案**：`A/install-newapi-desktop-connect.sh`） |
| [C-一键挂Sidecar.md](./C-一键挂Sidecar.md) | ② 一键挂旁路服务（设计 + compose 骨架；Sub2API 也适用） |
| [AI-提示词.md](./AI-提示词.md) | ③ New API 复制给 AI 的完整提示词 |
| [Sub2API-傻瓜接入.md](./Sub2API-傻瓜接入.md) | **Sub2API**：为何不能套 ①、Sidecar / 自研选型、可复制 AI 提示词 |

---

## 站长验收（做完任一方案后勾一下）

- [ ] 浏览器打开 `https://域名/desktop/connect`，未登录会提示先登录。
- [ ] 登录后点「打开桌面助手」，能唤起已安装的野菜客户端。
- [ ] 客户端显示已连接该站点（不是只弹一下窗口）。
- [ ] 能看到余额 / 用户信息；能列出或创建令牌（说明 Bearer 或代理没漏）。
- [ ] 同一连接码不能兑第二次；过期码会失败。
- [ ] 故意改深链里的 origin，兑码应失败（origin 校验）。

---

## 明确不做（本草案范围外）

- 不推上游 QuantumNous/new-api。
- 可下载草案 tarball 见同目录 `newapi-desktop-connect-patch-*.tgz`；正式 Release/镜像另开任务。
- 不 merge、不 deploy、不改生产站。
- 不把 New API ① 脚本冒充成 Sub2API 方案；Sub2API 见 [Sub2API-傻瓜接入.md](./Sub2API-傻瓜接入.md)（Sidecar 或自研/AI）。

---

## 给产品/运营的一句话

> 要一键连：New API 选「打最小补丁 / 挂 Sidecar / 丢给 AI」；Sub2API 别跑 New API 脚本，选「挂 Sidecar」或「自研 + AI 提示词」。按钮脚本官方托管，协议固定，私改站点也不用换整包镜像。

# ③ 复制给 AI 的提示词（适合：不会改代码，但有 AI 编程助手）

> **本页仅针对 New API。** 若你的站是 **Sub2API（Wei-Shaw/sub2api）**，不要用下面提示词，改看 [Sub2API-傻瓜接入.md](./Sub2API-傻瓜接入.md)。

> 把下面「完整提示词」整段复制到 Cursor / ChatGPT / Claude / Trae 等，  
> 并附上仓库里的补丁草案文件（或让 AI 自己打开这些路径）。  
> **不替代测试**：AI 改完后仍要按总览验收清单点一遍。

---

## 使用步骤（站长）

1. 打开你的 **New API 源码**工程（最好在 Git 分支上：`git checkout -b feat/yeschoy-desktop-connect`）。
2. 把野菜桌面仓里的对照文件放进工作区（或 clone 一份只读参考）：
   - `docs/patch/new-api-desktop-connect/README.md`
   - `docs/patch/new-api-desktop-connect/desktop_connect.go.txt`
   - `docs/station-connect-api.openapi.yaml`
   - `docs/station-connect-page.md`
   - `public/desktop/connect.html`
3. 新建对话，粘贴下方**完整提示词**。
4. 若 AI 问「是否执行」：允许它改源码与加文件；**不要**让它直接推生产、删库、改已有计费逻辑。
5. AI 完成后：你本地编译 → 部署到预发 → 勾验收清单。
6. 失败时把报错 + 相关文件发回同一对话，追加一句：「按 OpenAPI 与补丁 README 修，不要扩大改动面。」

---

## 完整提示词（复制从这里开始）

```text
你是资深 Go + 前端工程师。请在我当前的 New API（QuantumNous/new-api 或其 fork）代码库里，
实现「野菜 / YesChoy 桌面一键连接」最小能力。我不会手写补丁，请你直接改代码并给出验证步骤。

【目标用户体验】
1. 用户在本站 HTTPS 域名已登录；
2. 打开 https://<本站>/desktop/connect ，点击「打开桌面助手」；
3. 浏览器 POST 同源 /api/desktop/v2/connect-codes 拿到一次性 code；
4. 跳转 yeschoy://connect?origin=<canonical-https-origin>&code=<code>；
5. 桌面客户端再 POST /api/desktop/v2/connect-codes/exchange 换会话；
6. 之后桌面用 Authorization: Bearer <accessToken> 能调用：
   /api/user/self、/api/user/models、/api/token/、/api/log/self、/api/pricing
   （不要要求桌面再带 New-Api-User 头）。

【硬性约束】
- 不要推 upstream，只改我这份代码。
- 改动面尽量小：不要重构计费、渠道、前端主题。
- 连接码：TTL ≤ 300 秒、一次性、绑定 origin；长期 token 禁止进 URL。
- 兑码时校验请求 Host 与连接码绑定的 origin 一致，否则 403 connect_origin_mismatch。
- token 只存 SHA-256 哈希；日志禁止打印明文 code/token。
- 设备码登录可不做（bootstrap 里 device_authorization_available=false 即可）。
- /desktop/connect 路径必须固定；页面可加载 https://yeschoy.com/connect.js，
  按钮使用 data-yeschoy-connect；支持 ?auto=1 自动尝试发码。
- 若与现有文件冲突，优先保持我站私改行为，把桌面连接做成增量。

【对照材料（工作区里已有或我会贴上）】
- docs/patch/new-api-desktop-connect/README.md  （设计说明）
- docs/patch/new-api-desktop-connect/desktop_connect.go.txt  （Go 草稿，需按我仓的 model/middleware/router 签名改到可编译）
- docs/station-connect-api.openapi.yaml
- docs/station-connect-page.md
- public/desktop/connect.html  （参考页，可按本站静态资源方式安置）

【请你按顺序做】
A. 先扫描我仓：Go 模块路径、router 注册处、UserAuth 中间件、AutoMigrate、静态文件/前端如何托管。
B. 落地可编译的 controller + 必要 model + Bearer 兼容中间件；在 api-router 注册 /api/desktop/v2/*。
C. 连接码存储：有 Redis 用 Redis（GETDEL），否则进程内 sync.Map；说明多副本限制。
D. 提供 /desktop/connect 页面（或前端路由等价物），行为对齐参考 HTML。
E. 写出：本地/容器重建命令、curl 验收示例、回滚方法（git 文件列表）。
F. 自检清单：未登录发码失败；登录发码成功；码只能兑一次；Bearer 能拉 /api/user/self；
   故意错误 origin 兑码失败。

【输出格式】
1. 变更文件列表（路径 + 一句目的）
2. 关键风险点（若有）
3. 我需要手动执行的命令
4. 不要生成无关文档长文；代码以可合并为准
```

（提示词到这里结束）

---

## 给 AI 的「短版」追加（编译报错时用）

```text
编译/运行报错如下（原样粘贴）。请只修桌面连接相关文件，对齐我仓现有 middleware 与 model.DB 用法，
不要扩大范围。修完列出 diff 文件并给出重新 build 命令。

（在此粘贴错误日志）
```

---

## 给 AI 的「只做前端按钮」追加（后端已就绪时）

```text
后端 /api/desktop/v2 已可用。请只加 /desktop/connect 页面与「打开桌面助手」按钮：
加载 https://yeschoy.com/connect.js，使用 data-yeschoy-connect；未登录提示先登录；
支持 ?auto=1。不要改 API 与数据库。
```

---

## 站长验收（AI 改完后你亲自点）

同 [README.md](./README.md) 验收清单。另加：

- [ ] `git diff` 没有动到计费/支付等无关目录（若有，让 AI 撤回）。
- [ ] 预发环境测过再上生产。

---

## 和 ①② 的关系

| 关系 | 说明 |
|---|---|
| ③ → ① | AI 改完后，野菜可把「成功案例」收成一键脚本的版本矩阵 |
| ③ vs ② | ③ 改的是主站源码；不想动主站就不要用 ③，改用 Sidecar |
| ③ 失败 | 版本过旧或结构差异大 → 停手，改推荐 ②，或等官方补丁包支持你的版本 |

---

## 风险提示（务必给站长看）

- AI 可能「顺手」大改；用独立分支 + code review。
- 不要把生产数据库连接串、管理员 Token 贴进公共 AI 网页版；优先本地/私有模型或已隔离的 IDE Agent。
- 提示词不能保证一次成功；**验收比生成重要**。

# ② 一键挂 Sidecar（适合：主站不能改 / 有私改镜像不想动）

> 草案。描述「主 New API 进程不动，旁边挂一个小服务」的目标形态。  
> **本文件不含可上线镜像源码**；只给站长步骤设计与 compose 骨架，供审阅。

---

## 一句话

你的 New API **继续用现在的镜像**；再跑一个「野菜连接小助手」容器。  
用 Nginx / Caddy / Traefik 把「连接相关路径」指到小助手，其余流量仍进 New API。

---

## 适合谁 / 不适合谁

| 适合 | 不适合 |
|---|---|
| 镜像是别人编的，改不了源码 | 连反代都改不了（只有面板点点点且无自定义路由） |
| 私改很多，不敢合补丁 | 希望连接与主站完全同库同事务（更适合 ①） |
| Sub2API 等非 New API（① 的 Go 补丁套不上） | 期望「零维护一辈子」——上游登录变了仍要升 sidecar |

---

## 「养」到底是什么（对站长 vs 对野菜）

| 谁 | 日常 |
|---|---|
| **站长** | 装一次；偶尔 `docker compose pull && up -d` 跟版本 |
| **野菜团队** | 发 sidecar 版、对齐协议、修 Cookie/登录兼容、写升级说明 |

站长侧可以做到接近一键；**不是完全不用人管**，但通常比反复合源码补丁省心。

---

## 站长操作（目标 UX）

### 0. 准备

- 已有 HTTPS 反代（必须能加几条 `location` / route）。
- 知道 New API 在内网的地址，例如 `http://new-api:3000`。
- 准备一个 sidecar 能用来「代表用户」访问 New API 的方式（见下文「会话桥」——这是设计关键，不能省）。

### 1. 一条命令 / 一段 compose（示意）

```bash
# 假想：下载最小 compose 片段并合并进现有栈（落地后换真实 URL）
curl -fsSL https://yeschoy.com/station/yeschoy-connect-sidecar.compose.yaml -o yeschoy-connect-sidecar.yaml
docker compose -f docker-compose.yml -f yeschoy-connect-sidecar.yaml up -d
```

### 2. 反代：必须转发的路径（漏了会「连上了没余额」）

**连接与页面（进 Sidecar）：**

```text
/desktop/connect
/api/desktop/v2/
```

**读账与令牌（二选一，推荐 A）：**

- **A（推荐）**：下列路径也进 Sidecar，由 Sidecar 校验桌面 Bearer 后再回源 New API：  
  `/api/user/self`、`/api/user/models`、`/api/token/`、`/api/log/self`、`/api/pricing`  
  （仅当请求带桌面 `Authorization: Bearer` 时；浏览器 Cookie 会话可继续直打 New API——用规则区分。）
- **B**：主站自己已有 Bearer 中间件（等于做了 ①）→ 这些路径仍打 New API。

> 小白口诀：**唤起靠连接路径；看得见余额靠「用户信息那几条」也别漏。**

### 3. 示例：Caddy 片段（示意）

```caddy
# 示意，按站长实际域名改
handle /desktop/connect* {
  reverse_proxy yeschoy-connect-sidecar:8080
}
handle /api/desktop/v2/* {
  reverse_proxy yeschoy-connect-sidecar:8080
}
# 可选：桌面 Bearer 走 sidecar（具体匹配规则落地时写进官方片段）
handle /api/user/* {
  reverse_proxy yeschoy-connect-sidecar:8080
}
handle {
  reverse_proxy new-api:3000
}
```

Nginx / Traefik 官方文档落地时各给一份「复制即用」片段。

### 4. 验收

同总览验收清单。重点多测一项：

- 客户端已连接后，**余额数字能出来**；若不能，先查反代是否漏了 `/api/user/self` 等。

---

## Sidecar 内部要干什么（设计说明，非实现）

```text
浏览器 --Cookie--> Sidecar: POST /connect-codes
                 Sidecar 用该 Cookie 问一下 New API「你是谁」
                 → 发一次性 code（Redis/内存）

桌面 --code--> Sidecar: POST /exchange
                 → 发 access/refresh（只存哈希）
                 → 之后桌面带 Bearer 访问

桌面 --Bearer--> Sidecar: /api/user/self 等
                 Sidecar 认 Bearer → 换成 New API 能懂的调用
                 （例如：站内系统令牌 / 用户令牌 / 经主站约定的头）
                 → 回源 New API → 把 JSON 原样返回桌面
```

### 「会话桥」三种档次（审阅时请拍板产品默认）

| 档 | 做法 | 站长配置 | 风险 |
|---|---|---|---|
| **L1 只读桥** | 发码时用 Cookie 调 New API `/api/user/self`；兑码后 Sidecar 为该用户创建（或复用）一个 **sk- 系统访问令牌** 存在 Sidecar 侧，桌面 Bearer 映射到这个 sk- | 可能要管理员令牌一次 | 实现相对直接；令牌权限要收紧 |
| **L2 Cookie 透传** | 难：桌面没有浏览器 Cookie | 不推荐 | — |
| **L3 主站打补丁** | 等于回到 ① | — | 失去「主站不动」 |

**草案默认推荐 L1**：站长在 compose 里填一次 `NEW_API_BASE` + 可选 `ADMIN_ACCESS_TOKEN`（仅用于代建用户令牌），文档写清权限最小化。

---

## 环境变量（设计清单）

| 变量 | 含义 |
|---|---|
| `NEW_API_BASE` | 内网 New API 根 URL，如 `http://new-api:3000` |
| `PUBLIC_ORIGIN` | 对外 HTTPS 源，如 `https://api.example.com`（绑码用） |
| `REDIS_URL` | 可选；没有则进程内 map（多副本必开 Redis） |
| `ADMIN_ACCESS_TOKEN` | L1 桥用；权限最小化说明见发版文档 |
| `LISTEN` | 默认 `:8080` |

**禁止**把管理员令牌写进公开仓库或发给用户浏览器。

---

## compose 骨架（可审阅，非发行版）

```yaml
# yeschoy-connect-sidecar.yaml — 示意骨架
services:
  yeschoy-connect-sidecar:
    image: ghcr.io/daydreamzxg/yeschoy-connect-sidecar:0.1.0   # 落地后才存在
    restart: unless-stopped
    environment:
      NEW_API_BASE: http://new-api:3000
      PUBLIC_ORIGIN: https://api.example.com
      # ADMIN_ACCESS_TOKEN: 请用 secrets，不要写进 git
    # ports:  # 生产建议只进反代网络，不暴露公网端口
    #   - "8080:8080"
    networks:
      - default
```

---

## 升级与回滚

- **升级**：改 image tag → `compose pull && up -d`；看 changelog 是否要改反代。
- **回滚**：指回旧 tag；或反代去掉 sidecar location，站点恢复「仅贴 Key」能力（一键连暂时消失，主站不受损）。

---

## 风险话术

- 「主站镜像可以不动；要改的是反代几条路径。」
- 「最常见故障：连接成功但无余额 → 用户 API 没进 sidecar / 桥没配好。」
- 「多机部署必须配 Redis，否则连接码不同步。」

---

## 落地前还缺什么（给野菜团队）

1. 实现 sidecar 服务（发码/兑码/bootstrap/sessions + L1 回源桥）。
2. 官方镜像 + 签名/校验和；compose 与 Caddy/Nginx/Traefik 三份片段。
3. 与 New API 常见登录 Cookie 名/头做兼容表；登录方式变异时的升级策略。
4. 安全评审：管理员令牌范围、速率限制、日志不打 code/token。
5. Sub2API 适配说明（路径与「我是谁」API 不同，可能要第二 profile）。

**本文件审阅通过 ≠ 镜像已可 docker pull。**

# ① 一键补丁脚本（适合：有私改、能自己编译 New API）

> **状态**：已在干净 QuantumNous/new-api 验收（2026-10-20，Asia/Shanghai）。可下载草案包；**未部署生产站、未推 QuantumNous 上游。** 详见 `A/VERIFIED.md`、`A/站长一页说明.md`。  
> **对照**：`yeschoy-ai-desktop/docs/patch/new-api-desktop-connect/`  
> **本目录实现**：[`A/install-newapi-desktop-connect.sh`](./A/install-newapi-desktop-connect.sh)

---

## 一句话

在你**现有的 New API 源码目录**里自动拷入最小连接代码、挂路由、加连接页，然后你照旧 `docker compose build` / 编译重启。  
**你的私改还在**，只多一块「桌面一键连」。

---

## 适合谁 / 不适合谁

| 适合 | 不适合 |
|---|---|
| 自己维护 New API fork 或改过前端/计费 | 完全不能编译、只有别人给的封闭镜像 → 看 ② Sidecar |
| 愿意重建一次容器 | 仓里已经有完整桌面体系（如野菜自有站）→ 不用再打 |
| 希望连接逻辑跟主站同进程、同库 | 想让 AI 代改 → 可先看 ③，再用本脚本验收 |
|  | **Sub2API（Wei-Shaw/sub2api）→ 禁止用本脚本**，见 [Sub2API-傻瓜接入.md](./Sub2API-傻瓜接入.md) |

---

## 站长操作（傻瓜式）

### 0. 准备

- 服务器上有一份 New API **源码**（能 `go build` 或有 Dockerfile）。
- 已有 HTTPS 反代到该站。
- 建议先 `git commit` 或打个 tag，方便回滚。

### 1. 拿到草案包

当前草案在本仓库旁路目录（产品落地后会挂到官网或 GitHub Release）：

```bash
# 假设你已把 station-oneclick-connect/A 拷到机器上
cd /path/to/station-oneclick-connect/A
chmod +x install-newapi-desktop-connect.sh
```

### 2. 先看、再打

```bash
# 只探测，不改文件（推荐先跑）
./install-newapi-desktop-connect.sh --src /path/to/your/new-api --dry-run

# 确认无误后真正打补丁
./install-newapi-desktop-connect.sh --src /path/to/your/new-api --yes
```

| 参数 | 含义 |
|---|---|
| `--src` | New API 源码根目录（必填） |
| `--dry-run` | 只打印将改哪些文件，不写入 |
| `--skip-frontend` | 不注册 `/desktop/connect`（你已自己做了） |
| `--yes` | 跳过确认 |
| `--undo` | 用最早一份 `desktop-connect-backup-*.tgz` 回滚 |

### 3. 脚本替你做的事

脚本**幂等**（跑两次不炸），顺序：

1. **探测**：读 `go.mod` / `VERSION` / 目录结构；已是完整桌面栈则**拒绝覆盖**。
2. **备份**：打补丁前打 `desktop-connect-backup-<时间戳>.tgz`。
3. **拷文件**（并按你的 module 路径改 import）：
   - `model/desktop_session.go`
   - `controller/desktop_connect.go` + 连接页 embed
   - `middleware/desktop_bearer.go`
   - `router/desktop_connect.go`
4. **改现有文件**（带 `YESCHOY_DESKTOP_CONNECT` 标记，方便辨认）：
   - `router/api-router.go`：挂 `DesktopBearerAuth` + `registerDesktopConnectRoutes`
   - `model/main.go`：`AutoMigrate` 增加 `DesktopSession`
   - `middleware/auth.go`：桌面会话已识别时 `UserAuth` 直接放行
   - `router/web-router.go`：注册 `GET /desktop/connect`
5. **打印重建与验收说明**（不自动 `docker up`）。

### 4. 重建与验收

```bash
cd /path/to/your/new-api
docker compose build && docker compose up -d
# 或按你站点习惯 go build / systemctl restart
```

然后按总览 [`README.md`](./README.md) 里的**验收清单**勾一遍。

### 5. 回滚

```bash
./install-newapi-desktop-connect.sh --src /path/to/your/new-api --undo --yes
# 然后重新 build && up
```

或：`git checkout` 被改文件，并删除脚本新增的 `desktop_*.go` / `desktop_bearer.go` / `desktop_connect.html`。

---

## 补丁到底改了什么

| 块 | 作用 |
|---|---|
| 发码 `POST …/connect-codes` | 网页登录态发一次性码 |
| 兑码 `POST …/connect-codes/exchange` | 桌面换 access/refresh |
| bootstrap / sessions | 能力声明、续期、退出 |
| Bearer 中间件 | 连上之后**读得动账** |
| `/desktop/connect` + `connect.js` | 用户点的那一页（脚本加载 `https://yeschoy.com/connect.js`） |

细节与安全要求：`docs/patch/new-api-desktop-connect/README.md`。

**人肉不加脚本**：约 3～5 人日。  
**有本脚本后站长侧目标**：备份 → 跑脚本 → 重建 → 测按钮，**< 1 小时**（不含编译排队）。

---

## 目录结构（草案包）

```
A/
  install-newapi-desktop-connect.sh   # 站长入口
  patch_helpers.py                    # 标记插入（api-router / migrate / auth / web）
  payload/
    VERSION
    model/desktop_session.go
    controller/desktop_connect.go
    controller/desktop_connect_page.go
    controller/desktop_connect.html
    middleware/desktop_bearer.go
    router/desktop_connect.go
    web/desktop/connect.html          # 同上 HTML，便于单独挂静态
```

---

## 风险与话术

- 「不会覆盖你的计费/套餐私改，只增加桌面连接相关文件与几处注册。」
- 「第一次请 `--dry-run`。」
- 「编译失败先 `--undo`；连上但无余额 → 多半 Bearer / UserAuth 跳过没挂上。」
- 「野菜自有完整桌面栈的仓，脚本会拒绝，避免误伤。」

---

## 已验收 / 仍缺（给野菜团队）

已完成：干净上游 dry-run → 打补丁 → `go build` → 发码兑码 Bearer 冒烟；`--undo`；完整栈拒绝。  
仍缺：挂官网/Release + sha256；小白演练录像；生产站灰度。

**已验收 ≠ 已对客户站上线。** 发版渠道与生产部署另开任务。

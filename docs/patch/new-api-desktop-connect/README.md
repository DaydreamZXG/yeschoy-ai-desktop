# New API「桌面一键连接」最小补丁（草案，未部署）

> 状态：**草案**。没有在任何生产站点上部署，也没有并入上游。
> 目标：一个普通的 [QuantumNous/new-api](https://github.com/QuantumNous/new-api) 站点只加这个补丁，
> 就能让用户在网页上点一个按钮，桌面助手自动登录。

不打这个补丁也能用：桌面助手已经支持「粘贴站点网址 + sk- 令牌」的兜底方式（见下文）。
补丁只是让体验从「复制令牌」变成「点一下」。

## 补丁新增的接口（全部在 `/api/desktop/v2` 下）

| 方法 | 路径 | 鉴权 | 作用 |
|---|---|---|---|
| GET | `/bootstrap` | 无 | 声明能力与各接口路径（客户端严格校验路径，见 `account_v2.rs` `fetch_bootstrap_read`） |
| POST | `/connect-codes` | 网页登录态（`UserAuth`） | 签发一次性连接码，TTL ≤ 300 秒，绑定站点来源 |
| POST | `/connect-codes/exchange` | 无（凭连接码） | 桌面端用连接码换会话：`accessToken` / `refreshToken` / `sessionId` |
| POST | `/sessions/refresh` | 无（凭 refreshToken） | 轮换 refreshToken，签发新 accessToken |
| DELETE | `/sessions/current` | 桌面 Bearer | 退出登录，吊销会话 |

另外需要一个中间件：让 `Authorization: Bearer <desktop access token>` 能通过现有的
`/api/user/self`、`/api/user/models`、`/api/token/`、`/api/log/self`、`/api/pricing`。
原版 New API 的 `UserAuth` 要求同时带 `New-Api-User` 头，桌面端不带。

设备码登录（`/device-authorizations*`）在补丁里**可选**，`device_authorization_available=false` 即可；
连接码 + 网页按钮已经够用。

## 数据

一张表 `desktop_sessions`：

| 列 | 说明 |
|---|---|
| id (uuid) | sessionId |
| user_id | 归属用户 |
| access_hash / access_expires_at | accessToken 的 SHA-256，15 分钟有效 |
| refresh_hash / refresh_expires_at | refreshToken 的 SHA-256，30 天有效，每次刷新轮换 |
| revoked_at | 退出或管理员吊销 |
| device_platform / client_version / created_at / last_seen_at | 审计 |

连接码存 Redis（没有 Redis 时存内存 map 加锁），键 `desktop:connect:<sha256(code)>`，值为
`{user_id, origin, expires_at}`，**兑换时原子删除**（GETDEL），保证一次性。

## 安全要点（必须）

1. 连接码只出现在 `yeschoy://connect?origin=…&code=…` 里；长期凭据永远不进 URL。
2. 兑换时校验：请求 `Host` == 连接码绑定的 origin 的 host；否则 403 `connect_origin_mismatch`。
3. 所有 token 只存哈希；日志里只打 sessionId。
4. `/connect-codes` 必须是 POST + 同源 Cookie + CSRF（复用 New API 现有的 `New-Api-User` 头校验即可）。
5. 速率限制：复用 `middleware.CriticalRateLimit()`。
6. 桌面端第一次见到某个站点会弹确认框（「要连接这个新站点吗？」），站点无法绕过。

## 网页按钮（站点只需要放这一段）

```html
<button type="button" id="open-desktop">打开桌面助手</button>
<script>
document.getElementById("open-desktop").onclick = async () => {
  const r = await fetch("/api/desktop/v2/connect-codes", {
    method: "POST",
    credentials: "include",
    headers: { "Content-Type": "application/json",
               "New-Api-User": localStorage.getItem("uid") || "" },
    body: "{}",
  });
  const { success, data } = await r.json();
  if (!success) return alert("请先登录站点");
  location.href = "yeschoy://connect?origin=" + encodeURIComponent(location.origin)
                + "&code=" + encodeURIComponent(data.code);
};
</script>
```

建议放在 `/desktop/connect` 页面（桌面端「检测并连接」会打开 `<站点>/desktop/connect?auto=1`），
`auto=1` 时页面加载后自动点一次按钮。

## 草案代码

`desktop_connect.go.txt` 是 Go 草稿（后缀 `.txt`，避免被误编译），按上面的设计写成，
**未编译、未测试**，落地时需要：

- 对齐目标 New API 版本的 `model.DB`、`common.RedisEnabled`、`middleware` 函数签名；
- 在 `router/api-router.go` 注册路由（文件末尾有示例）；
- 在 `model/main.go` 的 `AutoMigrate` 里加 `DesktopSession`；
- 补单测：连接码一次性、过期、Host 不匹配、refresh 轮换后旧 token 失效。

估计工作量：一人 3–5 天（含测试与前端 `/desktop/connect` 页面）。

## 参考实现

野菜 API 的 fork（私有）已经实现了设备码与会话体系（`controller/desktop.go`、
`service/desktop_device_authorization.go`，约 900 行），但依赖 fork 自己的 `auth_session` 体系，
不能直接搬到上游。本草案刻意做成**自带最小会话表**，不依赖那套体系。

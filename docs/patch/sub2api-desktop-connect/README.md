# sub2api 的 desktop-connect 参考补丁

让一个 [Wei-Shaw/sub2api](https://github.com/Wei-Shaw/sub2api) 站点支持桌面客户端的一键连接：用户在浏览器里点一下就登录，客户端里能看到余额、价格方案、密钥、用量和公告。

不打补丁也能用：客户端支持“粘贴站点网址 + 密钥”。补丁把“复制密钥”变成“点一下”。

**状态**：参考实现。没有并入上游，没有在线上站点部署过。验证过什么、没验证什么见[最后一节](#验证情况)。

## 怎么打

补丁是对上游 `v0.2.25`（提交 `348997267`，2026-11-24）生成的：

```bash
cd sub2api
git checkout v0.2.25
git apply --check /path/to/desktop-connect.patch
git apply /path/to/desktop-connect.patch
```

然后照常构建（Docker 镜像或 `make build`）。不需要改数据库，不需要新的配置项。

上游以后的版本如果改了下面几个文件，`git apply` 可能失败。新文件不受影响，其余几处可以对着补丁手动加：

| 文件 | 加什么 |
|---|---|
| `backend/internal/handler/handler.go`、`handler/wire.go` | `Desktop *DesktopHandler` 字段和 `NewDesktopHandler` |
| `backend/internal/service/wire.go` | `NewDesktopConnectService` |
| `backend/internal/repository/wire.go` | `NewDesktopConnectCodeStore` |
| `backend/internal/server/router.go` | 一行 `routes.RegisterDesktopRoutes(...)` |
| `backend/cmd/server/wire_gen.go` | 在 `backend/cmd/server` 里跑一次 `wire` 重新生成 |
| `frontend/src/router/index.ts`、`i18n/locales/*/misc.ts` | `/desktop/connect` 页面的路由和文案 |

## 站点要满足的条件

- 对外是 HTTPS、标准端口、域名（不是 IP）。客户端只连这样的站，补丁也只给这样的地址发连接码。
- 反向代理要把用户访问的域名原样放在 `Host` 里。连接码绑定在这个域名上。
- 要有 Redis（sub2api 本来就需要）。连接码存在 Redis 里，多实例也能用。
- 要在后台「渠道管理」里配好渠道，并把渠道关联到分组。客户端的方案和模型列表来自模型广场，没有渠道的分组在模型广场里看不到，客户端里也不会出现。没配渠道的站，用户仍可以用“粘贴网址 + 密钥”。

## 加了什么

一个页面：`/desktop/connect`。客户端打开它（带 `?auto=1` 时自动连接）。没登录就先去登录，登录后向站点要一个一次性连接码，再跳到 `yeschoy://connect?origin=…&code=…` 交给客户端。

`/api/desktop/v2` 下的接口：

| 方法 | 路径 | 谁能调 | 作用 |
|---|---|---|---|
| GET | `/bootstrap` | 任何人 | 声明约定和各接口路径 |
| POST | `/connect-codes` | 浏览器的登录 | 发一次性连接码 |
| POST | `/connect-codes/exchange` | 持有连接码 | 换成客户端的登录 |
| POST | `/sessions/refresh` | 持有续登录凭证 | 续登录 |
| DELETE | `/sessions/current` | 客户端的登录 | 退出 |
| GET | `/account` | 客户端的登录 | 用户名、余额 |
| GET | `/plans` | 客户端的登录 | 能用的分组和每个模型的价格 |
| GET / POST | `/keys` | 客户端的登录 | 列出、新建密钥 |
| POST | `/keys/:id/reveal` | 客户端的登录 | 取回密钥原文 |
| DELETE | `/keys/:id` | 客户端的登录 | 删除密钥 |
| GET | `/usage/summary`、`/usage/records` | 客户端的登录 | 近 30 天用量 |
| GET | `/notices` | 客户端的登录 | 公告 |

## 它怎么对应到 sub2api

- **登录**：用 sub2api 现成的登录会话（JWT + 续登录凭证）。没有新表。
- **价格方案**：用户能建密钥的分组就是方案。价格来自模型广场的同一份数据，已经乘上分组倍率；管理员给这个用户单独设的倍率优先，和计费一致。月卡分组的高峰倍率、分时段倍率、长上下文分档都照实给出；按次计费的模型按上下文分档时，每档的按次价也给出。
- **官方价**：模型广场里的官方参考价，客户端据此显示折扣。
- **钱**：一律按美元报，和 sub2api 自己页面显示的单位一致。站点如果把余额当人民币用，客户端显示的符号会和网页不一样，这一点要站长自己改 `desktop_data_handler.go` 里的 `money`。
- **密钥**：和控制台里新建的一样，受站点原有的数量上限约束。
- **用量**：只读本人的记录，汇总和明细都只算最近 30 天。
- **公告**：读控制台公告。
- **后端模式**：站点开了后端模式时，和 `/api/v1` 的用户接口一样，只有管理员能用。开之前发出、还没用掉的连接码，普通用户也换不了。

## 安全

- 连接码是 32 字节随机数，300 秒过期，只能用一次，绑定站点域名。Redis 里只存它的 SHA-256。
- 连接码一出示就作废，再检查别的。在别的域名上出示过的码，回到正确的域名也不能再用。
- 改过密码之后，之前发出的连接码全部失效。
- 换码、续登录、发码、建密钥都有限流，用的是 sub2api 原有的限流器，Redis 不可用时拒绝请求。方案和两个用量接口和 `/api/v1/usage` 一样，走控制台的重查询限流。
- 长期凭证不进网址。

## 没做的

- 客户端专属价（`appOnly` 方案和设备凭证 `app-tokens`）。上游 sub2api 没有对应的计费概念，`/plans` 里全部是普通方案，`bootstrap` 也不声明 `app_tokens_path`。
- 人民币显示，见上面“钱”。
- 设备码登录和本机回环 OAuth。`bootstrap` 里都声明为不可用。

## 验证情况

验证过的：

- 补丁能干净地打在 `v0.2.25` 上；`go build ./...`、`go vet` 通过；前端 `vue-tsc` 和 `eslint` 通过。
- 补丁自带 16 个测试（`go test -tags unit ./internal/handler ./internal/service -run Desktop`），`internal/server`、`internal/server/routes`、`cmd/server` 原有测试也都通过。
- 这套接口的原版一直跑在 yeschoy.us 上（那里多了客户端专属价和人民币显示），客户端日常就是连它。

没验证的：

- 打了补丁的原版 sub2api 还没有在线上站点跑过，也还没用真客户端连过。
- 浏览器把连接码交给客户端这一步，依赖客户端已在系统里注册 `yeschoy://`。

## 许可

补丁是 sub2api 的衍生作品，和 sub2api 一样按它的许可证（LGPL-3.0）提供，不适用本仓库的 MIT 许可。

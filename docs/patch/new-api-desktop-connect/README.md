# new-api 的 desktop-connect 参考补丁

让一个 [QuantumNous/new-api](https://github.com/QuantumNous/new-api) 站点按
[`desktop-connect-v1`](../../desktop-connect.md) 约定接入桌面客户端：用户在浏览器里点一下就登录，
客户端里能看到余额、价格方案、密钥、用量和公告。

不打补丁也能用：客户端支持“粘贴站点网址 + 密钥”。补丁把“复制密钥”变成“点一下”。

**状态**：参考实现。没有并入上游，没有在线上站点部署过。验证过什么、没验证什么见[最后一节](#验证情况)。

## 怎么打

补丁是对上游提交 `185e1e51`（2026-11-22）生成的：

```bash
cd new-api
git apply --check /path/to/desktop-connect.patch
git apply /path/to/desktop-connect.patch
```

然后照常构建。不需要改数据库，不需要新的配置项。

上游的路由文件变动后，`git apply` 可能在三处小改动上失败。两个新文件
（`controller/desktop_connect.go`、`controller/desktop_connect_test.go`）不受影响，
其余三处可以对着补丁手动加：

| 文件 | 加什么 |
|---|---|
| `router/api-router.go` | `/desktop/connect` 页面和 `/api/desktop/v2` 路由组 |
| `middleware/access_token_routes.go` | 11 条登录后接口，全部登记为 `accessTokenSessionRule` |
| `router/access_token_scope_test.go` | 3 条不用登录的接口加进豁免表 |

## 站点要满足的条件

- 对外是 HTTPS、标准端口、域名（不是 IP）。客户端只连这样的站，补丁也只给这样的地址发连接码。
- 反向代理要把用户访问的域名原样放在 `Host` 里。连接码绑定在这个域名上。
- 多实例部署要开 Redis。不开 Redis 时连接码存在进程内存里，只适合单实例。

## 加了什么

一个页面：`GET /desktop/connect`。客户端打开它（带 `?auto=1` 时自动连接）。没登录就让用户先去登录，
登录后向站点要一个一次性连接码，再跳到 `yeschoy://connect?origin=…&code=…` 把它交给客户端。

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

## 它怎么对应到 new-api

- **登录**：用 new-api 现成的登录会话，登录方式记为 `desktop`。用户在控制台的“登录会话”里能看到它，
  也能在那里踢掉。没有新表。
- **价格方案**：一个用户能用的分组就是一个方案（`auto` 分组不算）。价格按分组倍率换算好，
  单位是每百万 token；按次计费的模型给每次价格。按表达式计费的模型不给单一价格。
  new-api 里没有单独的“官方价”，补丁把分组倍率为 1 时的价格当作官方价报出去，客户端据此显示折扣。
  站长改过模型自身的倍率或价格时，这个“官方价”就是站点自己的基础价，不是上游的标价。
- **钱**：站点设置显示人民币时给人民币（按站点的汇率），其他情况给美元。客户端不接触额度单位。
- **密钥**：和控制台里按默认值建的一样，不限额、不过期。新建数量受站点原有上限约束；
  列表给出本人名下全部密钥，上限后来调低也不会漏。
- **用量**：只读本人的消费记录，最多 30 天；站点对用户开放的日志范围更短时以站点为准。
- **公告**：读控制台公告。公告没有标题，取正文第一行当标题。

## 安全

- 连接码是 32 字节随机数，300 秒过期，只能用一次，绑定站点域名。站点只存它的 SHA-256。
- 连接码一出示就作废，再检查别的。在别的域名上出示过的码，回到正确的域名也不能再用。
- 只有浏览器的登录能要连接码。客户端的登录不能，个人访问令牌也不能。
- 登录后的接口只接受登录会话，不接受个人访问令牌。
- 长期凭证不进网址。页面把访问凭证只放在内存里，带 `no-store`、禁止被嵌入、带随机数的内容安全策略。
- 换码、续登录、发码、建密钥都有限流，用的是 new-api 原有的限流器。
- 续登录沿用 new-api 的做法：旧凭证在很短的并发窗口内还认，窗口之外再用会让整个会话作废。

## 没做的

- 客户端专属价（`appOnly` 方案和设备凭证）。new-api 没有对应的计费概念，`/plans` 里全部是普通方案。
- 订阅方案。`subscription` 一律为 `false`。
- 设备码登录和本机回环 OAuth。`bootstrap` 里都声明为不可用。
- `exchange` 请求里的 `deviceId`、`platform`、`clientVersion` 不校验也不保存。

## 验证情况

验证过的：

- 补丁能干净地打在 `185e1e51` 上；`go build ./...`、`go vet` 通过；`golangci-lint` 在新增代码上没有报告。
- 补丁自带 7 个测试（`go test ./controller -run TestDesktop`），以及 `router`、`middleware` 原有测试。
- 本机实测（Linux、SQLite、Redis、本地 HTTPS）：浏览器里打开 `/desktop/connect` 发出连接码；
  真客户端连上后读到余额、方案价格、公告；从客户端接入 OpenCode，经站点发出一条请求，
  用量和扣费入账；把应用改回原样时站上的密钥被删；退出登录。

没验证的：

- 线上站点，MySQL 和 PostgreSQL，多实例。
- 浏览器把连接码交给客户端这一步在 Windows 和 macOS 上没有试过。Linux 上浏览器发出了跳转，
  交给客户端这一下是在测试环境里手动完成的。

## 许可

补丁是 new-api 的衍生作品，和 new-api 一样按 AGPL-3.0 提供，不适用本仓库的 MIT 许可。

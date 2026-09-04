# 野菜API 0.4.2 线路恢复验收

日期：2026-09-04。范围仅限桌面客户端，没有修改 NewAPI、Nginx、DNS、数据库或服务器代码。

## 线路结论

- 大陆优化：`https://yeschoy.com`
- 全球加速：`https://api.yeschoy.com`
- `https://yeschoy.pro` 不再作为客户端可选线路、新凭据来源、诊断目标或自动回退目标。
- 两个稳定线路 ID 保持 `mainland_optimized` 与 `global_accelerated`，用户不需要重新理解第三个线路选项。

## 恢复验证

`api.yeschoy.com` 当前 DNS 指向 Cloudflare，TLS 校验成功。`/api/status`、`/api/pricing` 和 `/api/desktop/v2/bootstrap` 返回成功，未授权访问 `/v1/models` 返回 401。桌面 v2 bootstrap 声明六项桌面能力均可用，其规范化内容与 `yeschoy.com`、`yeschoy.pro` 的响应哈希一致；公开状态公告也将该域名标为 CF 全球加速线路。

以上证明域名、TLS 和匿名桌面契约已经恢复，但不代替真实账号的登录、刷新、模型读取、令牌创建与模型回复验收。

## 客户端改动

- 原生线路目录、账户 origin 解析、配置接入和诊断统一使用两条编译线路。
- 新写入的工具凭据只接受 `yeschoy.com` 与 `api.yeschoy.com`；不会把完整密钥送入前端。
- 七种工具继续使用同一套已选线路、字段归属、回读、失败恢复与目标真实回复验证。
- 切换线路不会清除有效登录；刷新仍由签发会话的线路完成，不会静默跨域重试写操作。
- 客户端版本统一提升为 0.4.2；自动更新和遥测仍保持关闭。

## 已执行检查

- TypeScript 类型检查通过。
- 前端全量单元与组件测试 303 项通过，0 失败。
- Rust 原生测试 62 项通过，0 失败。
- 0.4.2 线路恢复验收 3 项通过，0 失败。
- 前端生产构建通过。
- 候选验收的 Cargo、Vitest 与 Vite 输出均写入独立临时目录，不依赖旧构建缓存，也不改写受测源码。

## 安装包状态

- macOS 通用版已生成：`release/ru030-local/野菜API-0.4.2-macOS-universal.dmg`。
- DMG 大小为 9,627,697 bytes，SHA-256 为 `9706693cf7c29378b3da93e61433bdfbc24b55dd55a5270ca04acef09ce98c84`。
- 应用同时包含 `x86_64` 与 `arm64`，使用 `Developer ID Application: xiangguo Zheng (BRG82P5ZB7)` 签名。
- Apple 公证已通过，submission ID 为 `e167ce03-eeab-412b-ad02-0e433bf81d49`；票据已钉入 DMG，Gatekeeper 复核结果为 `Notarized Developer ID`。
- Windows 0.4.2 构建已从提交 `c6c4d794` 触发，但 GitHub Actions 在分配 runner 前因账户付款失败或消费上限不足而拒绝启动。失败运行号为 `33858702962`，未生成 0.4.2 Windows 安装包，也没有复用旧包。

## 发布前剩余实机项

- 使用真实野菜API账户在 macOS 与 Windows 各完成一次登录、切到全球加速、刷新余额与模型、选择计费分组、一键接入和真实回复。
- 处理 GitHub 账户的 Actions 付款或消费上限问题后，重新触发 Windows x64 未签名内测 EXE 构建并核对产物；不得继续分发旧 0.4.1 安装包作为当前版本。

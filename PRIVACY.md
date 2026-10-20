# 隐私说明 / Privacy

> 适用于开源仓库中的 **野菜API 桌面助手**（YesChoy AI Desktop）。托管中转站 [yeschoy.com](https://yeschoy.com) 另有站点侧条款；本文只描述**本机桌面应用**。

## 我们收集什么（桌面端）

桌面助手默认在本机运行。下列数据主要留在你的电脑上：

| 类别 | 存在哪里 | 说明 |
| --- | --- | --- |
| 账户会话 / API 令牌 | 系统钥匙串或凭据管理器；必要时应用数据目录 | 用于已确认接入的站点；登出或「恢复原设置」可清除相关项 |
| 原应用配置备份 | 本机加密存储（+ 仅本人可读的应急副本） | 写入前备份；「恢复原设置」只撤回助手写过的项 |
| 已信任站点列表 | 应用数据目录 `trusted-stations.json` | 你确认过的 HTTPS 中转站 |
| 界面偏好、语言等 | 本机应用设置 | 不出站 |
| 诊断日志 | 本机 | 可能含站点域名与错误信息；**设计上避免**把完整密钥写入日志 |

助手**不会**为了「产品分析」在后台静默上传聊天内容或本地项目源码。接入校验不主动发收费的模型请求（见 README）。

## 何时会联网

仅在你使用对应功能时：

1. **连接中转站** — 按站点协议交换设备授权 / 令牌、拉取账户与模型列表（你确认接入的站点）。
2. **检查更新** — 社区构建默认请求 GitHub Releases 上的更新清单；官方安装包走官方更新源（见 [`docs/OPEN_SOURCE.md`](docs/OPEN_SOURCE.md)）。
3. **打开下载 / 文档链接** — 由你点击后用系统浏览器打开。
4. **已接入应用发推理请求** — 由该应用经本机转发或站点地址发出；流量归属你连接的站点与对应模型服务，不经过「野菜桌面助手云」中转聊天内容（助手本身不是云端对话产品）。

## 第三方

- **你选择的中转站**（默认可为野菜API，也可为你添加并确认的其它 HTTPS 站点）按其自身隐私政策处理账户与用量。
- **GitHub**（社区更新、Issue、Security Advisories）遵循 GitHub 政策。
- **QQ 群等社区入口**为可选；加群即适用腾讯侧规则。群号见 [SUPPORT.md](./SUPPORT.md)。

本开源树**不附带**第三方中转商品牌 Banner/Logo 资源包（见 `docs/OPEN_SOURCE.md`「合作方商标图」）。

## 你能做什么

- 在应用内登出、断开接入、使用「恢复原设置」。
- 删除本机应用数据目录与钥匙串中与本应用相关的条目（具体路径因系统而异）。
- 卸载应用；卸载不会自动改回第三方 AI 应用的配置，请先在助手内恢复。

## 儿童

本应用面向开发者与成年用户配置 AI 工具，不面向儿童提供在线服务。

## 变更

隐私实践若有实质变更，会更新本文件并在发布说明中提示。安全问题请按 [SECURITY.md](./SECURITY.md) 报告。

---

## English (summary)

YesChoy AI Desktop runs locally. Credentials stay in the OS keychain / credential store; config backups stay encrypted on disk. Network use is limited to station connect, update checks, links you open, and traffic from apps you connected. It does not silently upload your chat transcripts or project source for analytics. Third-party stations and GitHub have their own policies. See the Chinese sections above for the authoritative detail.

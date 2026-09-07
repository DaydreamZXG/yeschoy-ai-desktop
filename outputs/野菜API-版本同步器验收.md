# 野菜API：官方桌面版同步器验收

记录日期：2026-09-06。本次按 govern-product-build 的 RU-045 独立范围实施；只验收服务器同步与私有暂存，不代表客户端自动升级或公开分发已经完成。

## 已部署

- 独立 `yeschoy-vendor-sync.timer` 已启用，状态 active。
- 每六小时检查一次：北京时间 02:00、08:00、14:00、20:00；部署验收时下一次为 2026-09-06 14:00。
- 代码位于 `/opt/yeschoy-vendor-sync`，暂存包位于 `/var/lib/private/yeschoy-vendor-sync`。服务使用 systemd 动态用户、0700 私有目录和只读系统保护，不能访问公开下载目录。
- 初始缓存预算 17,122,514,944 字节，来自部署时可用空间的一半；空间不足暂停新增下载，不自动删除历史包或其他服务数据。
- 没有改动已有 Web 服务、客户端、账号、NewAPI、数据库、SSH 登录方式，也没有打包或发布新客户端。

## 实测结果

| 官方来源 | 私有暂存 | 本轮真实版本 | 原生签名/安装状态 |
| --- | --- | --- | --- |
| Codex macOS Apple Silicon | 成功 | 26.901.51231（Mac 上读包确认） | codesign 与 Gatekeeper 通过；未安装 |
| Codex Windows x64 | 成功 | 26.901.5280.0 | 只核对 MSIX 身份、架构、版本；原生签名及安装待验 |
| Codex Windows ARM64 | 成功 | 26.901.5280.0 | 同上 |
| Claude Desktop macOS 通用 | 未成功 | 未知，不猜测 | 官方入口 HTTP 403 验证页；未绕过 |
| Claude Desktop Windows x64 | 成功 | 1.46388.4.0 | 只核对 MSIX 身份、架构、版本；原生签名及安装待验 |
| Claude Desktop Windows ARM64 | 成功 | 1.46388.4.0 | 同上 |

首轮在北京时间 13:25:35 结束，暂存五份原包，共 2,754,728,531 字节。13:28:04 复跑结束，五份均为 `unchanged`，没有重复下载；Claude macOS 仍记录 `source_challenge`。

因为有一个官方来源失败，服务退出码为 2，保留本轮部分失败状态；定时器仍为 active，将按计划再检查。这不是所有来源成功，也不是客户端安装成功。当前只有服务器日志和状态记录，未配置微信、邮件等外部告警接收人。

Codex macOS 原包 SHA256：

`b6ffed73d581047862e85de5b4d322ba431004949a5338736e7059182dff6082`

在 Mac 上检查了原包完整性、只读挂载内的代码签名、OpenAI Team ID `2DC432GLL2`、bundle ID `com.openai.codex` 和 Gatekeeper。验签前后哈希与服务器暂存包一致；没有打开或安装应用。临时验签副本已删除，服务器原包及机器可读验签结果保留，可重新取用。

## 回归与边界

本地五组 pytest 测试通过，覆盖六来源清单、HTTPS/域名/DNS 限制、错误网页识别、续传及完整重下、缓存重放、目录收据中断、版本倒退、损坏包、并发锁、空间限制、符号链接/路径和 MSIX 身份。正式只读隔离验收以 RU-045 最新执行报告为准；保留先前未通过的验收记录，不改写历史结果。

源站检查仍为：健康接口 200、stable 更新接口 204、暂存包公开 URL 404。共享 Caddy 配置与健康文件的 SHA256 没有变化。

**尚未完成：** Claude macOS 自动下载、Windows 原生签名验证、两系统实际安装/升级、第三方公开分发许可核对、客户端“下载安装”入口、野菜自身签名自动升级，以及无代理大陆网络实测。Windows 离线许可证、框架依赖和 Cowork 组件也不能因主体安装包下载完成而算已解决。

## 操作与回退

查状态：

```sh
systemctl list-timers yeschoy-vendor-sync.timer
python3 /opt/yeschoy-vendor-sync/sync.py status --state-dir /var/lib/private/yeschoy-vendor-sync
journalctl -u yeschoy-vendor-sync.service -n 80 --no-pager
```

停用只影响本同步器，保留缓存与其他服务：

```sh
systemctl disable --now yeschoy-vendor-sync.timer
systemctl stop yeschoy-vendor-sync.service
```

详细说明见 `deploy/vendor-sync/README.md`，机器记录见 `deployment-receipt.json` 和 `native-codex-macos-verification.json`。仓库与公开目录均没有服务器口令、用户 API 密钥或更新签名私钥。

# 0.4.16 本地构建与双通道发布记录

2026-09-10：主版、朋友版 Windows 和 macOS 已发布到 ergou.qzz.io。没有修改官网页面；下列链接可直接挂到各自官网。

| 版本 | Windows x64 中文安装器 | Mac Intel / Apple 芯片通用安装器 |
| --- | --- | --- |
| 主版，登录确认页 yeschoy.com | [下载 EXE](https://ergou.qzz.io/updates/releases/official/0.4.16/yeschoy-0.4.16-official-windows-x86_64-installer.exe) | [下载 DMG](https://ergou.qzz.io/updates/releases/official/0.4.16/yeschoy-0.4.16-official-macos-universal-installer.dmg) |
| 朋友版，登录确认页 ai.yeschoy.io | [下载 EXE](https://ergou.qzz.io/updates/releases/partner/0.4.16/yeschoy-0.4.16-partner-windows-x86_64-installer.exe) | [下载 DMG](https://ergou.qzz.io/updates/releases/partner/0.4.16/yeschoy-0.4.16-partner-macos-universal-installer.dmg) |

## 更新方式

旧用户先手动安装对应版本一次，之后助手自动检查本版更新，并由用户点击下载、安装和重启。0.4.16 客户端检查本次同版本清单会显示当前最新，不会重复安装自己。

- 主版更新：`https://ergou.qzz.io/updates/official/stable.json`。
- 朋友版更新：`https://ergou.qzz.io/updates/partner/stable.json`。
- 官网机器可读下载清单：`https://ergou.qzz.io/releases/yeschoy-official.json` 和 `https://ergou.qzz.io/releases/yeschoy-partner.json`。
- 旧版共用 stable / beta 更新地址永久 HTTP 204，不能将其改为某一个版别的清单。
- 两版采用不同公钥，客户端固定版别、域名与构件路径。两版仍是同一应用标识/安装目录，不是可同时安装的两套独立软件。

## 已完成的检查

本地 Rust 1.94.0 / cargo-xwin / 中文 NSIS 构建 Windows，Xcode / Tauri 构建两种架构 Mac；没有使用 GitHub Actions。Windows 资源按 0.4.16 当次编译，没有复用旧版资源。

- TypeScript 和 renderer 生产构建通过；前端 42 个文件、534 项测试通过。
- 原生更新器 3 项、通道隔离 2 项测试通过。
- 发布器 9 项密码学、错版、损坏签名、重放、并发、回滚、停用和旧 CLI 隔离回归通过。缓存规则修改后补跑路由用例，并验证真实 Caddy 路由。
- 两版 Mac App 与 DMG 均通过 Developer ID 签名、Apple 公证、staple 和 Gatekeeper；DMG 只读挂载核对，更新 tar.gz 解压后再次验证 App 签名、票据及二进制一致性。
- 本地与服务器独立 minisign 验证六个最终构件；公网完整字节、哈希和签名验证通过。完整公网下载校验使用现有本地代理；另行无代理验证 TLS、两版清单、健康状态及全部六个文件的一字节 Range。直连完整下载的主版 Windows 已完成，Mac 慢速传输在独立完整验签后主动中止；不将它记作完整直连下载通过，也不作全国网络速度承诺。
- 两版公开清单哈希与本地试提升收据完全一致。原第三方安装目录的 catalog SHA-256 前后均为 `f54b58e958240935c3647d6091ea346cda28b45fb99159141fa252490fef3236`。
- 只重启指定的下载源容器 `yeschoy-download-origin`，共享入口和其他服务未重启。配置备份位于服务器 `/opt/yeschoy-download/backups/channels-0.4.16-20260910-3IZMXp*`。

详细发布收据和全部构件 SHA-256 见 `published-0.4.16.json`。本地构件、编译输入哈希、公证收据位于 `release/internal/0.4.16-channels-20260910.3IZMXp`。生产输入未在编译后变化；唯一后续源码测试变更是补充安装器只读初始化和 /UPDATE 合约断言，不进入安装包。

本轮“推送更新”执行的是下载/更新服务器发布，未另行执行 Git 提交或 Git 推送；上述源码、发布脚本和记录仍保留在当前工作树。

## 边界与后续维护

Windows 安装器沿用此前方式，**没有 Authenticode 微软发布者证书**，可能出现未知发布者提示；这不同于本次已验证的 Tauri 更新签名。

没有在用户本机安装、启动、退出或重新配置 Codex、Claude、野菜；没有执行本轮 Windows/macOS 真实设备端到端自动升级。`govern-product-build` 要求保留的历史治理失败报告保持原样，未伪造冻结或全产品验收通过。按用户明确的本地继续发布要求执行限定范围验证，不能把本记录当作全产品零缺陷声明。

更新私钥仅保存在本机用户配置目录 `yeschoy-release/updater-v2`，另有 `updater-v2-backup` 同机备份（目录 0700、私钥 0600）。未上传服务器、未提交仓库。它们没有口令加密，同机备份也不能抵御整机丢失；需另做安全离线备份。后续 0.4.17+ 沿用各自密钥，不要重新生成。

原 vendor-sync 定时器仅同步到私有暂存目录，不受本次公开清单变动影响。其旧人工发布器的健康协议和共享锁需要在下一次第三方包提升前迁移；本次没有调用它或覆盖第三方清单。回滚与单通道撤回命令见 `deploy/self-update/README.md`，必须带当前清单哈希，不能覆盖后续发布。

# 官方桌面版同步器（私有暂存）

这套服务每 6 小时检查 Codex 和 Claude Desktop 的官方完整安装包，只下载变化的文件；不依赖 NewAPI、不使用用户账户、不运行安装包。同步服务没有公开发布权限；本目录另有必须由操作员单独调用的 `publish.py`，详见下方。

## 当前范围

- Codex：macOS Apple Silicon DMG、Windows x64 / ARM64 MSIX。
- Claude Desktop：macOS 通用 ZIP、Windows x64 / ARM64 MSIX。Mac 使用 [官方 RELEASES.json](https://downloads.claude.ai/releases/darwin/universal/RELEASES.json) 的 `currentRelease` 和该版本唯一 `updateTo.url`，不再依赖会要求浏览器确认的 DMG redirect。没有把 Claude Code CLI、Codex CLI 或 Claude Science 当成桌面安装包。
- 来源见 `sources.json`，证据为 [OpenAI 桌面文档](https://learn.chatgpt.com/docs/app)、[Windows 部署文档](https://learn.chatgpt.com/docs/enterprise/windows-deployment)、[Claude 下载页](https://claude.com/download)和 [Claude Windows 部署文档](https://support.claude.com/en/articles/12622703-deploy-claude-desktop-for-windows)。
- Codex 的 macOS 官方文件仍命名为 `Codex.dmg`；Windows 文件叫 `ChatGPT-*.msix`，观察到的包身份是 `OpenAI.Codex`。名称不能用来替代包身份校验。
- MSIX 是应用主体安装包，不是“完全离线安装已验证”的承诺。离线许可证（需要时）、系统框架依赖、用户权限及 Cowork 组件还需在后续真实安装流程中逐项核对。

## 行为与边界

1. 逐个检查固定官方入口。Claude Windows 跳转入口使用 GET（HEAD 实测返回 405）；Mac feed 使用有 1 MiB 上限的 JSON GET，拒绝重复键、歧义版本、非官方 URL 和版本路径不匹配。安装包本身使用 HEAD 检查大小和强 ETag。版本已知时，在下载旧版之前拒绝倒退。
2. 每一跳均限制 HTTPS、精确域名和路径；DNS 必须全部为公网地址，实际 TLS 连接绑定已检查的 IP，并继续校验证书和原始主机名。没有代理环境、Cookie、Authorization、关闭 TLS 校验或绕过验证码的逻辑。
3. 要求明确长度、允许的二进制类型和强 ETag。用 `If-Match` / `If-Range` 绑定断点续传；发生变化重新发现版本。若服务器忽略 Range，只接受同一 ETag、正确完整长度的 200，并重写本次暂存文件。
4. 下载完成后计算 SHA256，检查 DMG 尾部格式、MSIX 内部清单或 Claude ZIP 的全体成员及 Info.plist，不解包执行。单安装包及 ZIP 总展开大小上限均为 4 GiB。**自己算出的 SHA256 只用于存储完整性；存在签名文件不等于签名有效。** 一律记为 `downloaded_unverified` / `pending_native_verification`，真实安装兼容性为 `not_tested`。
5. 每个完整文件有独立收据；目录状态原子替换。保存目录状态前崩溃，下次可从文件收据恢复，不必重复下载。重复任务由排他锁串行化；读取状态不需要获得写锁。
6. 网络中断最多立即重试 3 次，单次连接/读取超时 30 秒，单文件一轮下载上限 30 分钟，服务总运行时间上限 3 小时。这是资源保护，不是下载 SLA。403 验证、身份错误、文件变化、空间不足不快速重试，留待后续检查或管理员处理。
7. 新来源失败不会删除已有候选版本。已保存包再次使用前重新核对大小和 SHA256；发现损坏明确报错。Windows 版本倒退不替换较新候选；macOS 版本未知时不猜测版本或宣称已验证。
8. 初版不自动清理历史包。预算包括旧包、断点文件及收据；达到上限就暂停新增下载。部署预算取现场约 34.2 GB 可用空间的一半（约 17.1 GB），不改磁盘配额，不删除服务器其他数据，也不对公网带宽费用作保证。

## 服务器位置与运行

- 代码及配置：`/opt/yeschoy-vendor-sync`，由 root 管理、服务只读。
- 私有状态：systemd `StateDirectory=yeschoy-vendor-sync`，主机实际位置 `/var/lib/private/yeschoy-vendor-sync`；`/var/lib/yeschoy-vendor-sync` 为 systemd 管理入口。
- 动态非登录用户，目录 0700，无 capabilities，禁止写系统目录和访问公开下载目录；未改已有 Web 服务、SSH、数据库或应用设置。
- 定时：UTC 00:00 / 06:00 / 12:00 / 18:00，即北京时间 08:00 / 14:00 / 20:00 / 次日 02:00。重启后补一次漏掉的检查。

查看任务：

```sh
systemctl list-timers yeschoy-vendor-sync.timer
systemctl status yeschoy-vendor-sync.service
journalctl -u yeschoy-vendor-sync.service -n 80 --no-pager
python3 /opt/yeschoy-vendor-sync/sync.py status --state-dir /var/lib/private/yeschoy-vendor-sync
```

手动复查：

```sh
systemctl start --no-block yeschoy-vendor-sync.service
```

退出码：0 表示这一轮各来源完成检查；2 表示部分来源失败（其他成功文件保留）；3 表示本地配置/锁/状态错误。即使退出码为 0，也不代表原生签名、实机安装或公开分发已经验证。错误只有来源 ID 和短代码，不保存 HTTP 验证页、Cookie 或用户内容。

定时器不会因为上一轮返回 2 而停用。`last-run.json` 和 journal 提供本地告警线索；当前没有配置邮件/微信等外部通知接收人，不能声称已向管理员推送告警。

## 独立的原生签名核验

把指定暂存包取到对应测试系统，保留与服务器收据相同的 SHA256，然后运行：

```sh
python3 verify_native.py --source codex-macos-arm64 --file /absolute/path/Codex.dmg
python3 verify_native.py --source claude-macos-universal --file /absolute/path/Claude.zip
```

macOS 使用系统 `hdiutil verify`、只读挂载、`codesign --verify --deep --strict` 和 Gatekeeper，再核对已观察到的厂商 Team ID 与 bundle ID。不会打开或安装 app。Windows 需要 Python 和 Windows SDK 的 `signtool.exe` 在 PATH 中，传入**独立确认的**精确 Appx Publisher：

Claude ZIP 不挂载磁盘映像：在 Python ZIP 库读取中央目录前，先验证 classic 单磁盘 EOCD、成员数量、中央目录字节窗口和逐项固定头；不接受 ZIP64。目录预算为 `65534 × (1024 + 46)` 字节，成员和含隐式父目录的命名空间最多 65,534 项，总展开最多 4 GiB；成员路径和链接目标的 UTF-8 长度必须小于 Darwin PATH_MAX 的 1024 字节，Info.plist 最多 1 MiB。命名空间包括隐式目录，使用 NFD/casefold/NFD 检查大小写与 Unicode 别名、类型冲突。链接最多经过 Darwin MAXSYMLINKS 的 32 跳；除直接链接环外，也拒绝链接指向祖先以及兄弟目录之间构成的遍历环。

全清单通过后，才解到本次创建的 0700 临时目录；只允许 `Claude.app` 根，拒绝路径穿越、特殊文件、setuid/setgid/sticky 模式、加密成员、父路径符号链接和越界/循环/悬空链接。这些是解析资源保护上限，不是厂商包大小保证。合法框架 `Versions/Current -> A` 等相对链接在普通文件写完后创建。随后使用相同的 codesign、精确 Team/bundle 和 Gatekeeper 核验；不运行程序。核验结束再次计算原 ZIP 的 SHA256，结果仍是原 ZIP 字节的收据。

```powershell
python verify_native.py --source codex-windows-x64 --file C:\staging\ChatGPT-x64.msix --expected-publisher "<独立确认的发布者>"
```

使用 [Microsoft SignTool](https://learn.microsoft.com/en-us/windows/win32/seccrypto/signtool) 的 `verify /pa /all`，不生成证书、不导入信任、不关闭策略。工具缺失、发布者未确认或验证失败均不能输出 `native_verified`。核验结果包含文件 SHA256，仍然写明 `installation: not_tested`、`published: false`；它不会自动更改服务器状态或下载入口。

## 部署与回退

先确认新目录/服务名称无冲突，将本目录明确列出的文件复制到专用目录并比对 SHA256。对两个 unit 执行 `systemd-analyze verify`；安装到 `/etc/systemd/system` 后仅 `daemon-reload`、启动本服务并启用本定时器，不重启共享 Web 容器。

停用时只运行：

```sh
systemctl disable --now yeschoy-vendor-sync.timer
systemctl stop yeschoy-vendor-sync.service
```

中断留下的本服务断点文件可在下一次检查续传；保留目录和收据，不自动删除。后续升级脚本前先等本轮结束或停用本服务，核对部署前后文件哈希。

## 还没有做的事

没有强制或静默升级第三方应用，也没有发布野菜自己的自动更新包。公网可下载、跨平台编译和包结构检查都不等于 Windows / Apple Silicon 干净机器安装成功；签名、系统依赖、安装和真实接入仍分别验收。第三方分发政策由运营方独立管理，不作为客户端弹窗或普通用户操作门槛，技术工具也不声称替运营方作出法律判断。

## 独立操作员发布器与停用开关

`publish.py` 只读取明确指定的私有同步状态，并写入明确指定的、操作员拥有且不可被其他用户写入的公开根。该根必须已有标准 `health.json`；不能是 `/`、`/srv`、`/var`、`/tmp` 或 home。输入、目标和父目录拒绝符号链接（systemd 的固定私有状态入口除外，会先解析到真实私有目录）。脚本是本地操作员工具，不提供 HTTP 写入接口，也不会加入同步 timer。

发布不再要求许可或批准 JSON。一次显式命令只能发布对应槽位当前已完整同步的官方原包，不能传入任意 URL 或文件。可选 `--verification` 是 `verify_native.py` 产生、与来源和 SHA256 完全绑定的原生签名收据：有有效收据时目录写 `native_verified`；没有收据时只写 `client_native_required`。后者明确表示服务器没有声称完成原生验签，客户端安装前仍必须通过同一套系统签名和厂商身份校验。固定名称且无法安全读出身份/版本的 DMG 仍需原生收据。

发布一个来源：

```sh
python3 /opt/yeschoy-vendor-sync/publish.py publish \
  --state-dir /var/lib/private/yeschoy-vendor-sync \
  --public-root /srv/yeschoy-download/public \
  --source claude-macos-universal \
  --verification /absolute/private/operator/claude-native.json
```

没有原生收据的平台可以省略最后一行，公开状态会是 `client_native_required`。紧急停用全部镜像：

```sh
python3 /opt/yeschoy-vendor-sync/publish.py disable \
  --public-root /srv/yeschoy-download/public
```

发布流程：

1. 用 `.publish.lock` 拒绝并发；读取并完整验证现有严格清单及其原有公开对象。
2. 新对象要求当前同步槽为 `downloaded_unverified` 且元数据与候选一致；重新检查原始包结构、身份、架构、发布者、大小和摘要。已失败、变化、损坏或无法确认身份的来源不能推进。相同原生收据可以在私有扫描后来失败时重放已有不可变对象并修复 health。
3. 流式复制原始包字节到隐藏临时文件，再以 create-exclusive 硬链接发布 `/apps/<sourceId>/<sha256>.<format>`，文件 0644，目录 0755。不会重新打包、重新签名或替换同名对象。重复对象逐字节摘要核对后复用。
4. 再检查私有候选未变化，原子替换 `/apps/catalog.json`。严格遵守 `vendor-download-catalog@v2`：最多六个唯一槽，公开 URL 固定为 `https://ergou.qzz.io`，其他已发布槽和全部旧对象保留，自动版本倒退被拒绝。公开目录不包含批准依据、操作员身份、私有路径、账户或密钥。
5. 根据成功提交的非空清单设置标准 health 的 `thirdPartyInstallersPublished`，`updatesPublished` 继续 false，不改 updater 路由。
6. `disable` 先原子发布一个合法的空目录，再把 health 设为 false；旧对象保留但不再具有目录权威。客户端无需升级即可看到空目录并自动回退厂商官网。重新执行逐来源 `publish` 即可恢复。

退出 0 输出来源、摘要、校验状态及是否复用；退出 2 输出脱敏错误及 `publication: incomplete, replayRequired: true`，不笼统声称没有发生过提交。复制/清单替换失败保留旧清单；已完成的新不可变对象可能保留供重放。清单是客户端权威，health 若在进程中断后短暂低报，可重放发布或停用命令修复。没有自动清理旧对象、自动降级或删除命令。

隔离测试（不会访问服务器、真实安装应用或发布真实包）：

```sh
python3 -B -m pytest \
  tests/work_packages/ru052/test_vendor_pipeline.py \
  tests/work_packages/ru045/test_acceptance.py \
  -q -p no:cacheprovider --import-mode=importlib
```

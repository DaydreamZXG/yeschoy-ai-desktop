# 野菜桌面端本地构建与双通道发布（0.4.16 起）

当前使用用户确认的本地打包流程，不依赖 GitHub Actions 额度或 Secret。主版与朋友版第一次必须手动安装各自的 0.4.16 或更新版本；0.4.15 及更早版本的共用更新地址无法区分版别，永久返回 HTTP 204。

| 版别 | 登录确认页 | 更新清单 | 官网下载清单 |
| --- | --- | --- | --- |
| official 主版 | `https://yeschoy.com` | `https://ergou.qzz.io/updates/official/stable.json` | `https://ergou.qzz.io/releases/yeschoy-official.json` |
| partner 朋友版 | `https://ai.yeschoy.io` | `https://ergou.qzz.io/updates/partner/stable.json` | `https://ergou.qzz.io/releases/yeschoy-partner.json` |

版别由构建时登录确认页配置决定，不读取用户会话、IPC 或网址参数。账户/API/线路配置没有因为分通道而更换。两版各有独立的 Tauri 公钥；注册表见 `src-tauri/update-channels.json`。这是更新通道隔离，不是同时安装两个独立应用：两版仍使用同一应用标识和安装目录。

## 当前本地流程

1. 同步 `package.json`、Cargo 与 Tauri 版本，生成一次 renderer 生产构建。使用 Rust 1.94.0、已缓存的 cargo-xwin/NSIS 和 Xcode；Windows 资源由 Zig RC 当次编译，不能复用上一版资源。
2. `node deploy/self-update/local-signing.mjs init official`（或 `partner`）只在首次生成密钥。密钥位于用户配置目录的 `yeschoy-release/updater-v2`，另有同机 `updater-v2-backup`。目录 0700、文件 0600；**这是无口令私钥的本地权限保护，不是加密备份，也不是异地备份**。不要删除、重新生成、提交或上传私钥。
3. `node deploy/self-update/build-local.mjs windows official /绝对路径/release/internal/本次目录`；另一版换 `partner`。两版顺序构建，避免共享缓存混入不同登录域名。Windows 沿用中文自定义 NSIS；本次没有 Authenticode 证书，不能声称通过微软代码签名。Tauri 更新签名仍必须通过。
4. Mac 使用同脚本的 `macos` 阶段，之后 `notarize-local.mjs /本次目录/macos official submit-app`。分阶段执行 `status-app`、`app-to-dmg`、`status-dmg`、`verify-dmg`；只有 Apple 返回 Accepted 并验证 staple/Gatekeeper 后才能执行 `build-local.mjs macos-finalize ...`。另一版同理。生成双架构 DMG 与 updater `.app.tar.gz`，两者分别签名。
5. 将每版三个精确构件（EXE、DMG、App tar.gz）和各自 `.sig` 汇总到私有 candidate 目录。先用 `publish_variant.py` 对本地临时 public 根目录进行一次真实验签提升；再上传到服务器私有 incoming 目录。不要上传整个工作目录或密钥目录。
6. 部署 Caddy 双通道路由前备份原配置并验证；只重启 `yeschoy-download-origin`，不更改共享入口、第三方下载或旧版本构件。旧 stable/beta 路由即使残留文件也固定 204。
7. 服务器使用独立 minisign 重新验签。发布器内部持有 `/opt/yeschoy-download/publish.lock`，每版分开调用：

```sh
python3 publish_variant.py publish --root /srv/yeschoy-download/public \
  --variant official --lock /opt/yeschoy-download/publish.lock \
  --candidate /opt/yeschoy-download/incoming/本次目录/candidate \
  --version 0.4.16 --notes /opt/yeschoy-download/incoming/本次目录/release-notes.txt \
  --channel-config /opt/yeschoy-download/incoming/本次目录/update-channels.json
```

构件固定存放于 `/updates/releases/{variant}/{version}/`，同一路径不可换字节。清单依次原子替换，健康状态最后更新；两份清单不是跨文件数据库事务。私有发布水位阻止重复发布/降级，包括撤回后重放。

8. `probe_variant_release.py --version 0.4.16 --channel-config src-tauri/update-channels.json --candidate /本次候选目录 --minisign /minisign路径` 验证两版公网清单、完整构件 SHA-256、真实签名、Range、健康状态和旧通道 204。探针没有安装或启动用户应用，不能代替真实设备端到端更新验收。

## 回滚和单通道停用

```sh
python3 publish_variant.py rollback --root /srv/yeschoy-download/public \
  --variant official --lock /opt/yeschoy-download/publish.lock --expected-sha256 本次发布收据的manifestSha256
python3 publish_variant.py disable --root /srv/yeschoy-download/public \
  --variant partner --lock /opt/yeschoy-download/publish.lock --expected-sha256 当前朋友版清单SHA256
```

回滚要求当前更新和官网清单都仍等于本次发布收据，拒绝覆盖后来发布。停用只撤下指定更新清单，不影响该版手动下载。两种操作都保留另一版、第三方目录、不可变安装包和版本水位。

测试：`python3 -m pytest deploy/self-update/test_variant_release.py -q`。测试使用隔离临时目录、临时密钥和真实密码学验证；不是完整安装器 UI 冒烟。

边界：现有 `vendor-sync` 定时器仅把第三方构件同步到私有暂存区，现有公开第三方目录保留。本次不调用旧的第三方人工发布器；其健康协议仍要求 `updatesPublished: false`，后续再次提升第三方构件前需要迁移到共享锁和保留双通道字段的协议，不能用旧发布器覆盖当前健康状态。

---

# 历史方案存档（0.4.15 单通道 GitHub 工作流，不再用于本次发布）

以下是旧流程记录；共用清单已经退役，不能用它发布 0.4.16+ 双通道。旧 `publish.py` 的文件工具函数由新发布器复用，但旧 CLI 不是当前发布入口。

这条链路只发布野菜自身，不读取或修改模型、账户、线路、第三方安装目录或用户配置。候选构建与公网发布是两个独立动作：候选任务成功不会自动更新任何用户。

## 信任边界

- 客户端只内置 Tauri updater 公钥；更新私钥仅保存在 GitHub Actions Secret。
- Windows 公开安装包必须具有有效且未过期的 Authenticode 签名；macOS App 与 DMG 必须通过 Developer ID、公证和 stapler 校验。
- updater 的 macOS、Windows 构件还必须具有客户端内置公钥可验证的 Tauri/minisign 签名。
- `ergou.qzz.io` 只保存公开安装包、更新构件、签名文本和清单，不保存任何签名私钥。

## 第一步：生成私有候选构件

在 GitHub Actions 手动运行 `Yeschoy Signed Update Candidate`，输入与 `package.json`、`src-tauri/tauri.conf.json`、`src-tauri/Cargo.toml` 完全一致的稳定版本号。任务会完成依赖审计、前后端测试、Windows 原生签名以及 macOS 签名/公证，并生成保留 14 天的私有 artifact。

所需 Secrets：

- `TAURI_SIGNING_PRIVATE_KEY`、`TAURI_SIGNING_PRIVATE_KEY_PASSWORD`
- `WINDOWS_CERTIFICATE`（PFX 的 base64）、`WINDOWS_CERTIFICATE_PASSWORD`、`WINDOWS_TIMESTAMP_URL`
- `APPLE_CERTIFICATE`、`APPLE_CERTIFICATE_PASSWORD`、`KEYCHAIN_PASSWORD`
- `APPLE_SIGNING_IDENTITY`、`APPLE_ID`、`APPLE_PASSWORD`、`APPLE_TEAM_ID`

任何 Secret 缺失、原生签名无效、公证失败或测试失败都会使候选任务失败，且不会发布。

## 第二步：人工提升为稳定版

候选安装包完成真实 Windows/macOS 验收后，手动运行 `Promote Yeschoy Stable Release`：

1. 输入同一版本号和候选 workflow run ID；
2. 填写 1–600 字公开更新说明；
3. 输入 `PUBLISH <版本号>`；
4. 通过 `yeschoy-production` Environment 的审批（建议启用 required reviewers）。

提升任务只接受来自同一仓库、同一提交、成功完成的候选任务；它重新检查精确文件集合、SHA-256 和两个 updater 签名。随后通过固定 SSH host key 把文件送入服务器独立暂存目录，在排他锁内复制版本化构件，并依次原子替换：

- `/releases/yeschoy.json`：官网 Windows/macOS 下载清单，含 URL、大小和 SHA-256；
- `/updates/stable.json`：Tauri 稳定更新清单，含三个目标和 updater 签名；
- `/health.json`：只在前两者成功后标记更新已发布。

所需 Environment Secrets：

- `DOWNLOAD_ORIGIN_USER`：仅允许安全用户名字符；
- `DOWNLOAD_ORIGIN_SSH_KEY`：发布专用 SSH 私钥；
- `DOWNLOAD_ORIGIN_HOST_KEY`：预先核验的 `43.134.77.210` known_hosts 行。

不得在工作流中使用账户密码，也不得用 `ssh-keyscan` 临时信任未知主机。

## 原子发布器

服务器端等价命令如下；生产流程由工作流生成，不应手工拼接未校验文件：

```sh
python3 publish.py publish \
  --root /srv/yeschoy-download/public \
  --version 0.4.15 \
  --notes /opt/yeschoy-download/incoming/RUN/release-notes.txt \
  --platform darwin-aarch64=/opt/yeschoy-download/incoming/RUN/yeschoy-0.4.15-macos-universal.app.tar.gz,/opt/yeschoy-download/incoming/RUN/yeschoy-0.4.15-macos-universal.app.tar.gz.sig \
  --platform darwin-x86_64=/opt/yeschoy-download/incoming/RUN/yeschoy-0.4.15-macos-universal.app.tar.gz,/opt/yeschoy-download/incoming/RUN/yeschoy-0.4.15-macos-universal.app.tar.gz.sig \
  --platform windows-x86_64=/opt/yeschoy-download/incoming/RUN/yeschoy-0.4.15-windows-x86_64.nsis.zip,/opt/yeschoy-download/incoming/RUN/yeschoy-0.4.15-windows-x86_64.nsis.zip.sig \
  --installer macos-universal=/opt/yeschoy-download/incoming/RUN/yeschoy-0.4.15-macos-universal-installer.dmg \
  --installer windows-x86_64=/opt/yeschoy-download/incoming/RUN/yeschoy-0.4.15-windows-x86_64-installer.exe
```

同版本重放、降级、路径穿越、符号链接、空文件、不完整目标、重复目标和已有异字节对象全部失败关闭。版本化构件不可覆盖。

## 自动回滚与紧急停用

提升后的公网探针若失败，工作流会用发布收据中的当前/上一版清单哈希执行 compare-and-swap 回滚；如果期间已有另一项发布改动清单，回滚会拒绝覆盖它。历史清单保存在各自 `history/` 目录，版本化构件不会删除。

推广使用 `probe_public_release.py` 同时验证健康状态、更新清单、官网安装清单和清单引用的每个公网构件。构件必须通过 HTTPS 一字节 Range 请求并返回可信的 `Content-Range`；官网安装包的总大小还必须与清单完全一致。清单存在但文件 404、路由错误或大小不符都会使推广失败并触发上述精确回滚。

需要紧急关闭自动更新频道时可运行：

```sh
python3 publish.py disable --root /srv/yeschoy-download/public
```

该命令只撤下 `/updates/stable.json` 并更新健康状态，不撤下官网安装包清单，也不影响第三方应用目录。没有稳定清单时 Caddy 返回空 HTTP 204，旧客户端仍可继续使用。

0.4.13 及更早版本没有更新器，因此仍需手工安装第一个支持自动更新的版本；服务器清单无法跨过这一次性引导边界。

# 野菜桌面端候选构建与稳定发布

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

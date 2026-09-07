# 野菜桌面端自动更新发布

这条发布链只服务野菜自身更新，不复用 `deploy/vendor-sync` 的 Codex、Claude 等第三方应用目录，也不访问模型、账户、线路或用户配置。

## 信任和发布顺序

- Tauri 私钥只保存在发布机或 GitHub Actions Secret；客户端只内置公钥，`ergou.qzz.io` 只保存公开安装包、签名文本和清单。
- macOS 和 Windows 更新构件先由 Tauri 生成并签名；发布器要求三个目标都齐全：`darwin-aarch64`、`darwin-x86_64`、`windows-x86_64`。通用 macOS 包可供两个 Darwin 目标共用。
- `publish.py` 先以排他方式写入 `/updates/releases/<version>/` 的不可变构件并复核 SHA-256，最后原子替换 `stable.json`，成功后才把 `health.json` 的 `updatesPublished` 改为 `true`。
- 同版本重放、降级、符号链接、路径穿越、不完整目标、空签名和已有异字节文件都会失败关闭。

发布示例（路径必须是明确的普通文件）：

```sh
python3 publish.py publish \
  --root /srv/yeschoy-download/public \
  --version 0.4.14 \
  --notes /opt/yeschoy-download/release-notes-0.4.14.txt \
  --platform darwin-aarch64=/opt/yeschoy-download/artifacts/yeschoy-0.4.14-macos-universal.app.tar.gz,/opt/yeschoy-download/artifacts/yeschoy-0.4.14-macos-universal.app.tar.gz.sig \
  --platform darwin-x86_64=/opt/yeschoy-download/artifacts/yeschoy-0.4.14-macos-universal.app.tar.gz,/opt/yeschoy-download/artifacts/yeschoy-0.4.14-macos-universal.app.tar.gz.sig \
  --platform windows-x86_64=/opt/yeschoy-download/artifacts/yeschoy-0.4.14-windows-x86_64.nsis.zip,/opt/yeschoy-download/artifacts/yeschoy-0.4.14-windows-x86_64.nsis.zip.sig
```

## 回退

```sh
python3 publish.py disable --root /srv/yeschoy-download/public
```

该操作先把频道清单移到不公开的 `updates/disabled/` 审计目录，再原子更新健康状态。版本化构件不会删除，第三方应用目录和健康字段不会改变。没有 `stable.json` 时 Caddy 返回空的 HTTP 204，现有客户端继续正常使用，只是暂时检查不到更新。

0.4.13 及更早版本不包含更新器，因此第一次仍需人工安装首个更新器版本；这是一次性的引导边界，不能由服务器清单绕过。

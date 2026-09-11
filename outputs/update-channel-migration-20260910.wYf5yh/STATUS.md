# 主版 / 朋友版自动更新：迁移决定与发布阻塞

记录时间：2026-09-10 20:19（北京时间）。源码基线：`399cf0c8c9bc549fefc70a3067377ca743030c10`。

后续状态：用户随后明确要求沿用本地打包并继续执行，已记录 DEC-123。0.4.16 双版 Win/Mac 现已构建、验签和发布；详见本目录 `RELEASE-0.4.16.md` 与 `published-0.4.16.json`。以下内容保留为 20:19 的历史检查快照，不代表最终交付状态；原始治理失败报告未改写。

## 已确认的用户决定

用户明确接受“首次手动升级”：旧用户先下载安装对应的主版或朋友版，后续才进入各自的自动更新通道。已登记 `DEC-122`、`REQ-097`。这次确认不等于批准跳过验签、改写历史验收记录或操作用户本机的 Codex、Claude、野菜应用。

## 已核实的现状

- 两版当前均为 0.4.15，继承相同的 updater 公钥和 `https://ergou.qzz.io/updates/stable.json`。`app_update.rs` 的构造调用没有追加版别标识。不能依靠旧请求安全区分两版。
- 现有发布器、撤回、回滚、健康状态和公网检查都是单一 stable 通道语义；直接给安装包改名字不能实现隔离。
- Windows 工作流设置 `createUpdaterArtifacts: true`，却寻找 `*.nsis.zip`。按当前 Tauri v2 配置，应使用 `.exe` 和 `.exe.sig`；ZIP 是 `v1Compatible` 的兼容形式。macOS 自动更新使用 `.app.tar.gz` 和签名，官网下载 DMG 不能直接替代。[Tauri 官方说明](https://v2.tauri.app/plugin/updater/#building)
- `publish.py` 只验证签名文件的基本格式，真正的 minisign 验签在 `sync-r2.yml`。若今后采用本地交叉编译加手动上传，必须保留独立加密验签，不能绕过它。Tauri 的更新签名验证本身不可关闭。[签名说明](https://v2.tauri.app/plugin/updater/#signing-updates)
- Caddy 目前只在旧清单缺失时返回 204；文件一旦出现仍会提供更新，因此“永久退役共用通道”尚未实现。

代码定位：`src-tauri/tauri.conf.json:33`；`src-tauri/src/app_update.rs:185`；`deploy/self-update/publish.py:70,253,358,405`；`deploy/download-origin/Caddyfile.origin:38`；`.github/workflows/release.yml:155,195`；`.github/workflows/sync-r2.yml:145`。

## 本轮实时检查

仅执行了读取，没有修改远端：

| 检查 | 结果 |
| --- | --- |
| 共用 `/updates/stable.json` | HTTP 204，未下发更新 |
| `/health.json` | HTTP 200，`updatesPublished: false`，第三方下载仍为已发布 |
| GitHub 仓库 Secret 名称 | 仅列出 `TAURI_SIGNING_PRIVATE_KEY` 与其密码项；未读取值 |
| 最近检查的 Windows 构建 run `34195302521` | 未执行任何 step；GitHub 注明付款失败或需提高额度 |
| SSH 只读、密钥认证尝试 | 认证失败；未更改服务器认证方式 |

[Windows 构建记录](https://github.com/DaydreamZXG/yeschoy-ai-desktop/actions/runs/34195302521)。这是该次运行的失败原因，不保证将来的 runner 状态。Windows 正式发布工作流还要求 Authenticode 证书；它与必须保留的 Tauri 更新签名是两回事。现有手工 Windows 包没有 Authenticode，不能因此声称正式签名门槛已经通过。

## 为什么暂停实现和上线

依 `govern-product-build`，先运行当前待审实现单元的门槛检查：

```sh
python3 /Users/zxg/.codex/skills/govern-product-build/scripts/validate_governance.py \
  --project-root /Users/zxg/.codex/worktrees/c485/xiaobai-ai-desktop \
  --mode review --release-unit RU-076 \
  --report /Users/zxg/.codex/worktrees/c485/xiaobai-ai-desktop/outputs/update-channel-migration-20260910.wYf5yh/governance-review.current.json
```

报告为 **失败，169 条历史验收后文件状态变化**。全部来自 RU-075 的旧验收与当前树不一致：109 条在 `outputs/`，60 条在源码/工作流。**这不代表发现了 169 个运行时 bug**，也不代表当前修复应该回退。

检查是在本决定和本说明写入前进行；后续说明文件增加不改变失败的性质。完整原始报告保留在同目录。没有修改旧快照、验收哈希、技能校验器或冻结标记来制造通过。

治理校验器在 successor 的 review 阶段仍要求旧 authority 的当前输出等于历史已测状态，只有 frozen successor 才停止该比较，因此当前不能直接封存新的双版更新实现单元。此前连接状态修复的 `DEC-121` 是针对另一源码基线和修复范围的有限确认，不能自动扩展成这次上线的豁免。

## 待解决后才能继续的事项

1. 审核并恢复适用于当前已提交源码的实现/验收基线，保持历史记录不变；不要改写“已通过”来解锁。
2. 明确可执行的签名发布方式：恢复现有 GitHub 签名流程，或审定本地签名与密钥安全保存方式。不得把私钥提交仓库、上传到公开下载目录或要求用户贴在聊天中。
3. 按双版边界修复发布器、客户端配置、Windows 构件格式、旧通道退役、健康检查和回滚；形成独立的隔离、损坏签名、重放、并发及恢复测试。
4. 构建高于 0.4.15 的手动迁移版本，分别验证两版 Win/Mac 安装器和更新构件，再发布并检查公网。具体版本尚未更改。

本轮只登记决定、收集证据并保存失败报告。**未修改业务源码、未重新打包、未上传、未发布新更新，也未测试或关闭用户正在使用的应用。**

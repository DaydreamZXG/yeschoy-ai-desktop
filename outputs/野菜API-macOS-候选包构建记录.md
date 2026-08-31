# 野菜API macOS 本机候选构建记录

状态快照：2026-08-31 16:52（Asia/Shanghai）。这不是正式发布批准。

## 当前结论

Intel macOS 本机候选 app 和 DMG 已构建，Developer ID 签名、安全时间戳、app hardened runtime、包内容及磁盘映像完整性均已实际核验。源配置与生成 app 中继承的 `ccswitch://` 注册已移除，没有启用替代协议或登录回调。

Apple 公证已上传，当前状态为 **In Progress**，尚未取得 Accepted 与票据验证证据。该文件仅限本机候选，不对测试者或公众分发；没有安装到 Applications，也没有运行图形界面测试。

## 本次产物与签名证据

- DMG：`release/ru009-local-Jrjf0n/yeschoy-0.1.0-x64-signed-unnotarized.dmg`。
- 提交公证前大小：`3,115,625` 字节（约 3.1 MB）。
- 提交公证前 SHA-256：`133bbb1992091c356cd3bc219ccb64a403c740f73468a0a6186cd3de3ec14c2b`。票据附加会改变文件哈希，届时需另记最终值。
- app CDHash：`7f1df59cf9f488c2c861cee18b67fe108372070b`；DMG CDHash：`6e0c89c8fcaea1bd023c121fd52c475312871941`。
- 签名身份：Developer ID Application，Team ID `BRG82P5ZB7`，本机身份指纹 `952554E26C708644686E7687E3FE7E2BFEF08D67`。
- app 安全时间戳：2026-08-31 16:42:38 +08:00；DMG 安全时间戳：2026-08-31 16:44:19 +08:00。
- `codesign --verify --deep --strict` 校验 app 通过；`codesign --verify --strict` 校验 DMG 通过。`codesign --display --verbose=4` 确認 app `flags=0x10000(runtime)`、正确 TeamIdentifier、Developer ID 证书链及 Timestamp；DMG 不是可执行文件，不要求 runtime 标志。
- `hdiutil verify` 通过。DMG 以只读、不自动打开方式挂载核验后已正常卸载；映像内只有 `野菜API.app` 与指向 `/Applications` 的快捷链接，没有往 Applications 写文件。
- 挂载后的 app 签名再次通过，3 个文件均与原始签名 app 逐字节相同：

| app 内文件 | SHA-256 |
|---|---|
| `Contents/Info.plist` | `4f8426a0d2c9099480f5118b1bd0275601059109607baca15dade11dce3803e8` |
| `Contents/MacOS/yeschoy-desktop` | `f8600e7aeeb6449d101e0c3dd81a72f484b363b21766a9064dac7bdfecf8952a` |
| `Contents/_CodeSignature/CodeResources` | `6686de10a28a2fe11b36cbb86dcbacc827cfc4ea116b4dabf1845e5aee629e9b` |

## Apple 公证

- 本机凭据配置：`yeschoy-notary`，用户在本机终端完成验证与保存；未读取或导出密码。
- 提交 ID：`1486a7f7-58fb-4b0d-bc22-06c95237c861`。
- 命令：`xcrun notarytool submit <上述 DMG> --keychain-profile yeschoy-notary --wait`；服务已确认上传，处理中。
- 提交前 Gatekeeper 分别检查 app 与 DMG，均返回 `rejected / source=Unnotarized Developer ID`；app 的 `stapler validate` 返回没有票据。这些结果不被写成通过，也没有绕过 Gatekeeper、清除隔离属性或关闭安全校验。
- 待补：Accepted 状态、Apple 公证日志、staple、stapler validate、最终 Gatekeeper 结果与票据附加后的 SHA-256。公证成功也不等于安装/界面/后端功能已验证。

## 源码与构建环境

- 构建源码提交：`cfaf70902683b0842078c1da8601e486ee3b0968`，分支 `product/yeschoy-v1`；开始构建时 Git 工作区干净。后续只更新验收关联与产物记录，不代表重新编译源代码。未推送 GitHub。
- 系统：macOS 15.7.9（24G830），Intel x86_64；不代表 Apple Silicon、Universal 或 Windows 验证。
- Node：24.16.0；pnpm：10.12.3；Tauri CLI：2.8.0。
- Rust/Cargo：stable 1.97.1；使用 `RUSTUP_TOOLCHAIN=stable` 与 `--locked`。
- `pnpm-lock.yaml` SHA-256：`40016cef2ca79be59e608ea6b7cc8b2bd36bd5db71d8273c6d8314bf19aaf91c`。
- `src-tauri/Cargo.lock` SHA-256：`a169e30dcb54526a13bd067c1a9db1fd530ce75bfb624306af2ef109cf02fb33`。
- 独立构建目录：`release/ru009-local-Jrjf0n/build`。保留此前 `src-tauri/target/release/bundle` 和 RU-008 的旧文件，没有把旧安装包当作本次产物。
- 构建配置：`src-tauri/tauri.candidate.conf.json`，其中明确启用 `bundle.macOS.hardenedRuntime: true`，使用 `--bundles app`；随后通过 `hdiutil create -format UDZO -fs HFS+ -nospotlight` 制作 DMG 并单独签名。自动更新产物保持关闭。
- 构建进程显式取消公证认证、导入证书与更新私钥相关环境变量，只使用已经安装在钥匙串中的签名身份；没有导出私钥。

## 已完成验证

- `pnpm typecheck` 通过。
- `pnpm test:candidate`：7 个文件、30 项测试通过。
- 当前原生 Rust 测试：13 项通过，无忽略、过滤或空测试集。
- RU-005 / RU-006 / RU-008 / RU-009 pytest 合计 23 项通过。
- Rust fmt 与 Clippy（all-targets、warnings as errors）通过。
- RU-008 与 RU-009 的历史正式验收报告保留。RU-009 跟进派发因重叠的旧包“当前输出”依赖校验失败，因此 DEC-052 / RU-010 将收尾改为独立的当前状态验收，重跑原有 6 项运行时语义场景和 2 项打包场景；不修改全局验收器、隔离机制或旧报告。最新完成状态以 `.product-governance/execution/work-package-runs.json` 为准。语义测试不是实际产物或在线后端的替代证据。
- Tauri renderer 构建与 Rust release 编译通过，独立 release 编译耗时 4 分 02 秒；app 和 DMG 签名均退出成功。
- 编译产物架构确认为 Mach-O x86_64；生成的 plist 中应用名为“野菜API”、标识为 `com.yeschoy.desktop`、版本为 `0.1.0`。

- `otool -L` 列出的动态依赖均为系统库/系统框架，没有依赖本机构建目录的第三方 dylib。
- app 仅含上述 3 个文件且没有内部符号链接；renderer 的 3 个文件及 app 均未发现扫描规则识别的 JWT、API key、GitHub token 或完整私钥。扫描只输出路径/类别/计数，不输出可能的秘密；这属于启发式检查，不是完整安全审计。

构建仍有前端依赖数据过期及大 chunk 警告；本次没有升级依赖或将警告视为兼容性验证通过。plist 中的 `LSMinimumSystemVersion=10.13` 只是构建默认声明，不代表本次在旧系统运行通过。

## 历史失败与剩余限制

首次 RU-008 构建在用户完成钥匙串授权后以 `timestamps differ by 1673 seconds` 退出，且该旧包仍含继承协议声明。本次在源 plist 修复后从干净源码重新构建并重新签名，成功；没有改系统时钟、降低时间戳要求或只修补已签名 app。

尚未进行安装、首次打开、原生图形界面、重启/卸载、Apple Silicon、Windows 或最低系统版本真机验证；签名、镜像挂载和单元测试不能填补这些证据。登录、余额、凭据管理及配置应用等仍受客户端与后端的既定缺项限制。

## 范围边界

不访问或修改服务器、NewAPI、Nginx、业务账户、API 密钥、用户工具配置、付款、遥测、自动更新、GitHub CI 或公开发布。登录、凭据管理及配置应用仍未启用。即使后续本机签名和公证完成，也不能据此宣布完整商业 V1 已就绪。

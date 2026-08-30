# 野菜API V1 工具版本支持与验证矩阵

状态：产品策略已确认；验证计划草案，未执行，不授权实现  
日期：2026-08-29；产品策略确认于 2026-08-30  
范围：Claude Code、Codex、OpenCode、Pi、DSH `web` profile；仅客户端规划，不修改 NewAPI、服务器或线上配置

## 1. 已确认结论

产品负责人于 2026-08-30 确认：V1 不使用“最低版本以上全部支持”这样的宽泛规则，正式采用：

- 客户端只对通过完整验证的**精确工具版本**开放一键配置。
- 首轮验证时，以当时最新三个正式版本覆盖 Claude Code、Codex、OpenCode 和 Pi。
- DSH 当前官方标签仍带预发布后缀，单独按一个精确标签验证，不能继承“最新三个正式版本”规则；通过完整矩阵后按正常 V1 支持展示，不加 Developer Preview 产品标签。
- DSH 尚未通过矩阵不阻止整个 V1 发布；发布时保持只读发现，后续只能通过签名客户端更新增加正常支持。
- 新版本、未知版本、解析失败、多安装冲突或未通过全部平台单元的版本只读发现，不创建 Key、不写配置、不装插件。
- 兼容清单随签名客户端发布；以后若改为独立下发，也必须是签名、版本化、可回退且失效时关闭写入的可信清单。

这样牺牲少量“新版本刚发布就立即可配置”的速度，换取小白用户不会因为上游悄悄改字段、认证优先级或插件接口而被写坏配置。

## 2. 当前发布快照

以下是 2026-08-29 的官方仓库快照，只是**验证起点**，不是已经支持的版本：

| 工具 | 当前最新正式发布或标签 | 当前首轮候选集合 | 已确认的结构事实 | 不能据此声称 |
|---|---|---|---|---|
| Claude Code | `2.1.251` | `2.1.248`、`2.1.250`、`2.1.251` | 当前变更记录包含 `apiKeyHelper` 单独作为凭据时的网关修复和第三方 Base URL 修复 | 三个版本都已通过野菜线路、双平台和秘密扫描 |
| Codex | `0.151.0` | `0.150.0`、`0.150.1`、`0.151.0` | `0.146.0` 至 `0.151.0` 的固定源码均存在 provider `auth.command` schema 和 bearer-token 测试 | 任一版本已在野菜线路通过 Responses、流式、工具调用和官方登录共存验证 |
| OpenCode | `1.18.25` | `1.18.23`、`1.18.24`、`1.18.25` | `1.18.25` 的插件 API 存在 provider 认证 `loader` | 野菜认证插件已经存在，或 loader 在候选三版本中行为一致 |
| Pi | `0.84.4` | `0.84.2`、`0.84.3`、`0.84.4` | `0.84.4` 文档明确支持 `apiKey: "!command"` 请求时解析 | Windows 路径转义、优先级冲突和三个候选版本都已验收 |
| DSH | `0.1.2-alpha.1` 标签 | 仅 `0.1.2-alpha.1` | 存在 credential-provider 扩展 seam；官方明确没有随附 Keychain/helper provider | 该标签已通过野菜矩阵、野菜 provider 已实现，或上游已把该标签改称稳定版 |

官方锚点：

- [Claude Code 2.1.251](https://github.com/anthropics/claude-code/releases/tag/v2.1.251)
- [Codex 0.151.0](https://github.com/openai/codex/releases/tag/rust-v0.151.0)
- [OpenCode 1.18.25](https://github.com/anomalyco/opencode/releases/tag/v1.18.25)
- [Pi 0.84.4](https://github.com/earendil-works/pi/releases/tag/v0.84.4)
- [DSH tags](https://github.com/deepseek-ai/deepseek-harness/tags)
- [OpenAI Codex 配置参考](https://developers.openai.com/codex/config-reference/)

真正开始验证时必须重新抓取一次官方正式版本列表，并把本节作为新的带时间戳基线；不能拿今天的列表永久发布。

## 3. 版本状态与小白文案

| 内部状态 | 是否允许写入 | 小白界面 |
|---|---:|---|
| `verified_supported` | 是 | 已支持，可一键配置 |
| `installed_unverified` | 否 | 已检测到新版本，正在适配；暂不修改你的配置 |
| `installed_too_old` | 否 | 版本较旧，请升级后再配置 |
| `installed_prerelease` | 默认否；精确通过矩阵的 DSH `web` 例外 | 当前版本尚未通过适配验证，暂不自动配置 |
| `installed_failed_matrix` | 否 | 当前版本存在已知兼容问题，暂不配置 |
| `version_unparseable` | 否 | 无法确认工具版本，请查看诊断 |
| `multiple_installations` | 否 | 发现多个安装，需要先确认平常使用的是哪个 |
| `discovery_only_profile` | 否 | 已发现该使用方式，第一版暂不支持自动配置 |

“更高版本”不自动等于“更好且兼容”。客户端可以提示用户升级第三方工具，但 V1 不代替用户静默升级、降级或卸载第三方工具。

## 4. 首轮平台规模

已确认的官方客户端平台基线为：

1. Windows 10 22H2 x64
2. Windows 11 x64
3. macOS 13 Intel
4. macOS 13 或更高版本 Apple Silicon

若四款稳定工具各验证三个精确版本，就是 `4 × 3 × 4 = 48` 个基础版本平台单元；DSH 一个精确标签增加 `1 × 1 × 4 = 4` 个单元，合计 52 个。这个数字只是基础单元数，不能把一个单元内的多种配置状态折算成“测过一次”。

Windows 11 ARM64 的 x64 模拟仍是 best-effort，不进入正式阻断矩阵；WSL 在另行确认前只读发现。

## 5. 每个单元必须验证什么

每一个“工具版本 × 操作系统 × CPU 身份”都必须完成同一组门槛：

### 5.1 身份与发现

- 找到 PATH 实际命中的可执行文件，记录规范化路径、原始版本字符串和解析版本。
- 覆盖不存在、多安装、损坏 shim、App Execution Alias、无执行权限和版本输出超时。
- 配置目录或 profile 不唯一时停止写入，不能替用户猜。

### 5.2 配置事务

- 空配置、已有野菜节点、已有第三方 provider、未知字段、注释、损坏文件和只读文件分别建样本。
- 预览后发生外部修改时返回冲突；写入采用临时加密快照、写前复核、原子替换和读回校验。
- 只拥有野菜精确字段和收据记录的元素；不覆盖其他账号、provider、插件和默认设置。

### 5.3 凭据边界

- Claude Code 只通过 `apiKeyHelper`，Codex 只通过 provider `auth.command`，OpenCode 只通过认证 loader，Pi 只通过 `!command`，DSH 只通过系统凭据 provider。
- helper、插件、provider、系统凭据库或版本门槛任一失败时立即停止，不生成字面 Key、环境文件或旁路秘密文件。
- 验证外部环境变量、原生登录缓存、项目配置和 managed settings 的真实优先级；发生覆盖时显示 `external_override`，不能显示“配置成功”。

### 5.4 工具级请求

- 使用空临时项目和固定短提示，确认真实命中的 provider、精确模型 ID、线路及协议。
- 覆盖最小文本响应、流式、工具调用，以及该工具承诺支持的推理参数。
- 区分认证失败、余额不足、模型不存在、协议不兼容、TLS、DNS、超时和上游错误。
- 线上线路测试只读使用既有接口；本阶段不修改 NewAPI、Nginx、DNS 或服务器配置。

### 5.5 中断、恢复与移除

- 在凭据创建后、文件替换前、文件替换后、工具请求中和收据保存前分别模拟退出。
- 重启后只能完成可证明安全的恢复，不能靠猜测覆盖当前文件。
- 移除只清理仍与收据匹配的野菜字段和组件；V1 不承诺恢复被覆盖的第三方默认值，也不保留历史备份。

### 5.6 秘密扫描

- 完整 Key 不得出现在普通配置、工具认证文件、野菜 SQLite、日志、遥测、崩溃记录、诊断包、临时文件或进程参数。
- 测试必须使用可唯一识别的合成秘密，并扫描整个测试用户目录和捕获输出。
- 发现一次完整秘密即判定该平台版本单元失败，不能以“功能能用”覆盖安全失败。

## 6. 各工具额外门槛

| 工具 | 额外门槛 |
|---|---|
| Claude Code | `ANTHROPIC_AUTH_TOKEN`、`ANTHROPIC_API_KEY` 和官方登录的优先级；第三方 Base URL 的模型发现与流式工具调用；helper 失败和缓存刷新 |
| Codex | Responses wire API；provider 保留 ID；官方登录不被破坏；`auth.command` 初次失败、401 后刷新和命令非零退出 |
| OpenCode | 插件固定路径、签名与加载顺序；项目和 managed 配置覆盖；已有 `auth.json` 同名凭据；插件故障无普通凭据回退 |
| Pi | Windows/macOS 命令转义；`--api-key`、`auth.json` 和 provider 字段优先级；`settings.json` 默认模型冲突；不修改 Pi 登录 |
| DSH | 仅 `web` profile；完整 credential service 契约；非野菜 reference/record 共存；插件固定版本与完整性；其他 profile 零写入 |

## 7. 兼容清单的数据形状

每条通过记录至少包含：

- `tool_id`
- `exact_tool_version`
- `upstream_tag_or_package`
- `adapter_contract_version`
- `platform_id` 与 CPU 身份
- `credential_bridge_version`
- `supported_profiles`
- `verification_run_id` 与证据摘要哈希
- `catalog_version`、`verified_at`、`expires_or_review_after`
- `known_conflicts` 与 `blocked_capabilities`

客户端根据“精确版本 + 平台 + profile + 适配器契约”匹配，不根据版本号大小推测。清单签名不可验证、已过审查期限或缺少当前平台单元时，保持只读。

## 8. 当前冻结阻断项

1. 52 个基础单元尚未执行；当前只有官方源码和发布信息的静态结构证据。
2. OpenCode 认证插件和 DSH 系统凭据 provider 尚未实现，相关单元当前必然不能通过。
3. DSH 已确认为正常 V1 支持目标且不阻止整体发布；在它通过矩阵前，产品页、发布说明和客户端不得宣称已支持 DSH 自动配置。
4. 首批模型清单、工具级最小请求和两条线路的协议实测仍未完成。

因此，本文不能把任何版本标记为“已支持”。当前版本号只是规划测试资源和建立可重复基线的候选输入。

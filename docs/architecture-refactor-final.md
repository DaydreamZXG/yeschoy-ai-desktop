# 架构重构最终报告（YesChoy AI Desktop）

> 代码位置：工作区 `main` 分支（基线 `fb7e8ea5`）。未提交的用户改动全部保留，未执行任何提交。
> 验证命令与结果见第 5 节；所有数字均为最后一次实跑结果。

---

## 1. 原有架构的主要问题

1. **一个命令承担全部接入语义**：`src-tauri/src/tool_activation.rs` 约 5.1k 行，同时包含 IPC DTO、准入/取消、令牌协议、模型探测、工具分发、生命周期启停、回滚补偿与连接恢复。任何一类改动都要改同一个文件，事故半径大。
2. **跨层反向依赖**：`connection_recovery`（基础设施）反向引用 `tool_adapters/common` 与 DSH 专用恢复函数；工具适配器与站点协议没有边界。
3. **凭据校验与站点信任不一致（安全缺陷）**：`tool_credentials` 只接受 `yeschoy.com`/`api.yeschoy.com`，而 `station_trust` 允许任意已确认 HTTPS 站点 → 第三方站点接入后凭据被判无效、恢复也读不出；同时信任输入经 `url.origin()` 规范化时可能吞掉 userinfo、显式端口等证据。
4. **重复实现**：7 个适配器各写一份 `config_error`；`server_success`/`data` 在两个模块各一份；激活锁既有全局 `static` 又被外部模块直接引用。
5. **日志泄露面**：令牌被拒时把中转返回的自由文本 `message` 写入日志，第三方站点可在其中夹带密钥或账户数据，违反“日志不记密钥/账号”的产品不变量。
6. **前端单页过载**：`ConfigurationPreviewView.tsx` 约 2.9k 行，混合模型选择、工具安装检测、接入状态、线路测速、账户状态与庆祝反馈，且直接调用 IPC。

## 2. 实际完成的架构重构

### 2.1 分层与边界（模块化单体 + Ports & Adapters）

```text
app（Tauri 命令门面）       tool_activation.rs：仅 3 个 #[tauri::command] 包装与兼容导出
application
  activation/  request · tokens · capability · lifecycle · service · session · result · error
               issuance_recovery · envelope · 对应 *_tests.rs
  connections/  连接检查、恢复、退出恢复（显式 ConnectionContext）
  ports/        ConfigFiles / ConfigCommit / ConfigurationPlan / SharedCommit /
                SharedRollback / StationProvider
adapters/stations/  NewApiStationProvider（同源 /api/ 路径、origin 绑定）
domain/         model（绑定校验）· error（闭合错误）· configuration（文件变更意图）·
                station（规范站点身份）
infrastructure/config/   安全文件 IO + 唯一字节事务
infrastructure/secrets/  owner-only 的签发清单（只存名字 + 序号，不存密钥）
tool_adapters/  各工具格式实现，仅实现契约，互不依赖
```

依赖方向：`命令 → application → domain / ports`，`adapters`、`infrastructure` 实现端口，`domain` 只依赖 std + serde（可通过独立 `rustc --test` 编译）。

### 2.2 一键接入拆分（最高优先级项）

- 编排体整体迁入 `application/activation/service.rs`，函数签名 `configure_desktop_tool(&ActivationContext, request)`，依赖（AppHandle、各状态、各运行时）显式传入，不再读取门面的全局状态。
- 令牌铸造/复用、模型能力与 Codex 传输判定、工具生命周期与补偿顺序、单事务准入与取消、结果投影分别独立；失败补偿顺序保持“文件与凭据 → 运行时重启 → 登录态归还”。
- 保持：只重复用精确分组/模型/本机作用域的密钥；新令牌提交成功后才退役旧密钥；取消/并发/停机语义不变；命令名、请求/响应字段、原因码、`schemaVersion` 全部不变。

### 2.3 统一配置事务与崩溃补偿

- 所有适配器统一实现 `SharedCommit`（事务提交 → 读回 → 适配器校验）与 `SharedRollback`（按新到旧撤销，被他人修改的文件只报告不覆盖）；WorkBuddy 的权限收窄、DSH 的 profile 锁留在各自实现内，属工具特有副作用。
- 唯一字节事务支持通过 `ConfigFiles` 端口注入故障（写失败/读回不一致/外部修改），用于故障注入测试。
- **签发清单**：创建远端密钥前，先把精确名字、工具、分组与单调序号写入 `~/.yeschoy/activation-issuance.json`（原子写、仅本人可读）；拿到密钥后删除；无法写入则拒绝铸造（`issuance_record_unavailable`）。重启后，仅对“超过 30 分钟 + 精确名字仍在远端列表 + 携带本机作用域 + 不会被正常复用路径收养”的记录删除，其余一律不动。

### 2.4 站点边界与安全修复

- `NewApiStationProvider` 只接受 `StationOrigin` 规范身份下的同源 `/api/` 相对路径，拒绝绝对地址、跨源、`..`/编码 `..`、fragment 与非 `/api/` 的付费推理端点。
- 信任输入在规范化前拒绝 userinfo、显式端口、路径/查询/片段；凭据有效性不再依赖可能丢失的信任文件，历史 `api.yeschoy.com` 凭据保持可读。相关修复均有安全测试。
- 令牌拒绝日志只记录操作类别与状态码；`success:false` 的 200 响应由独立的、已测试的纯函数判定。

### 2.5 前端

- 预览页 2890 → 1871 行：`features/activation/`（模型菜单、线路选择、进度、庆祝、选中/目标/测速 hook、纯规则与文案），`shared/api/`（严格解码的只读连通性 IPC）。
- 页面职责收敛为展示与交互；状态组合在 hooks；IPC 经统一解码层；UI 结构、操作流程与文案未重新设计。

## 3. 修改和新增的核心文件

**新增**
- `src-tauri/src/application/{activation,connections,ports}/`（含 `service.rs`、`tokens.rs`、`lifecycle.rs`、`issuance_recovery.rs`、`result.rs`、`session.rs`、`envelope.rs`、`error.rs` 及 `*_tests.rs`）
- `src-tauri/src/domain/{model,error,configuration,station}.rs`
- `src-tauri/src/adapters/stations/{mod,new_api}.rs`
- `src-tauri/src/infrastructure/config/{files,transaction}.rs`、`infrastructure/secrets/issuance.rs`
- `src/features/activation/*`、`src/shared/api/*`
- `docs/architecture-refactor.md`（本文即最终版内容）

**主要修改**
- `src-tauri/src/tool_activation.rs`（5154 → 101 行）、`tool_adapters/{pi,opencode,claude_code,claude_desktop,codex_desktop,workbuddy,dsh_web,common,mod,terminal_launch}.rs`
- `connection_recovery.rs`、`tool_credentials.rs`、`station_trust.rs`、`station_connect.rs`、`open_connection.rs`、`app_installation/mod.rs`、`lib.rs`
- `src/configuration/ConfigurationPreviewView.tsx`、`src/i18n/locales/{zh,en}.json`（新增 `issuanceRecordFailed` 文案）、`vitest.config.ts`（限定根仓库测试范围）

## 4. 新模块职责与依赖关系

| 模块 | 职责 | 依赖 |
|---|---|---|
| `application/activation/service` | 一键接入编排与失败出口 | activation 子模块、ports、domain |
| `application/activation/tokens` | 令牌枚举/复用/铸造/退役、签发清单接线 | ports/station、infrastructure/secrets |
| `application/activation/issuance_recovery` | 判定与释放可证明遗留的密钥 | infrastructure/secrets、tokens |
| `application/activation/lifecycle` | 工具差异表、启停、桌面重载、补偿顺序 | tool_adapters、ports |
| `application/activation/session` | 单事务准入、取消、进度事件 | Tauri（仅此处） |
| `application/connections` | 连接检查/恢复/退出恢复 | recovery、credentials、lifecycle |
| `ports` | 配置事务、站点 Provider 的最小契约 | domain |
| `adapters/stations` | New API 账户协议（仅 `/api/`） | ports、domain、account_v2 客户端 |
| `infrastructure/config` | 安全 IO 与唯一字节事务 | ports |
| `infrastructure/secrets` | 签发清单（owner-only） | 无 |
| `domain` | 纯规则 | std + serde |

工具适配器之间、站点 Provider 与工具适配器之间均无直接依赖。

## 5. 已完成的测试及结果（最后一次实跑）

| 检查 | 结果 |
|---|---|
| `cargo fmt --check` | 通过 |
| `cargo check --locked` | 通过 |
| `cargo clippy --locked --all-targets -- -D warnings` | 通过 |
| `python3 scripts/ci/run-rust-tests.py` | **852 通过，0 失败，3 个已声明真机测试未运行**（基线 799；含本轮 Codex 开关、端到端编排、JSONC 最小编辑新增用例） |
| `pnpm typecheck` | 通过 |
| `pnpm test:unit` | **78 文件 / 751 项全部通过** |
| `pnpm build:renderer` | 通过 |

回归覆盖（均使用模拟凭据与受控临时目录，不触碰真实配置/钥匙串/付费接口）：

- 接入成功路径：令牌复用、变更作用域轮换、旧密钥提交后退役、伪造回显拒绝；
- 文件修改失败 / 读回不一致 / 外部修改：事务回滚与冲突保留；
- 凭据写入失败（内存密钥库）与恢复；
- 令牌创建失败、读密钥失败只删除新建那把；
- 用户取消、并发第二个请求被拒、停机期间取消；
- 用户在接入期间修改配置（ExternalChange）；
- 崩溃后遗留密钥：仅精确名字 + 本机作用域 + 超时才释放，本机未记录的密钥保留；
- 不同中转站凭据隔离（origin 统一校验、历史 `api.yeschoy.com` 兼容）；
- 配置格式兼容：各适配器的“写入即读回一致”、注释/无关字段保留的既有用例全部保留通过。

源码守卫测试仅按代码真实新位置更新（`terminal_launch` 的接入/打开一致性检查改读生命周期模块；`exit_restore` 的源码一致性检查仍校验真实门面），没有删除或忽略任何用例，也没有新增 `#[ignore]`。

## 6. 尚未完成的工作与风险

1. ~~**端到端编排夹具**~~ **部分完成（2026-10-30）**：`application/activation/end_to_end_tests.rs` 用 `tauri::test::mock_app()` 的真实 `AppHandle` 跑同一份 `configure_desktop_tool`，覆盖“非法请求不进管线”“第二个请求被拒”“停在第一个真实关口并给出原因码”“进度事件逐字段正确”。为此把 `ActivationContext` 与 `emit_activation_progress` 对运行时泛型化（生产代码不写类型参数），并开启 `tauri` 的空 `test` feature。**仍缺** `native_session_access` 之后的半段（铸密钥、写配置、启动）：那需要可替换的网络发送端，为它开测试专用旁路会破坏网络边界，故不做。两点实测结论：CLI 发现走 PATH 不受 `HOME` 影响，夹具须对环境自适应；**不要在测试里改 `HOME`**——macOS 会按 `HOME` 缓存钥匙串默认项，污染同进程后续的钥匙串访问。
2. **崩溃清理的保守取舍**：仍可被正常复用路径收养的令牌不删除（删除会让在用连接失去唯一密钥）；因此极端场景下仍可能留下一条记录，直到它超时且不再可复用。
3. ~~**Codex 探测**：接入时仍会发最小真实生成请求~~ **已解决（2026-10-29）**：探测改为**用户显式开启**的选项（设置 → 通用 → Codex 协议探测，默认关闭）。关闭时接入不发任何推理请求，所有模型停在直连 Responses ——也就是探测拿不到结论时的同一终点；开启后行为与原来一致（Responses 被明确拒绝且 Chat 可用时才切到桥上）。判断收敛在以 `probe_requested` 为名的单点函数，由 `the_probe_only_runs_when_the_user_turned_it_on_for_codex` 与前端 `codexProbe.test.ts`、`activation.test.ts` 共同守卫。
4. ~~**JSONC/JSON5 写入**：被修改的文档会被重排，注释不保证保留~~ **已解决（2026-10-30）**：新增 `tool_adapters/jsonc.rs`，在原文上做**最小编辑**——只替换我们拥有的那几处值，其余字节（注释、缩进、尾随逗号、单引号/无引号键）原样保留。已接入 `pi`（`models.json` / `settings.json`）与 `opencode`（`opencode.jsonc`）。任何定位失败（内联容器、结构超预期）都**回退整份重排**，语义始终正确；两处读回校验改为 JSONC 感知（此前用严格 JSON 解析自己刚写出的、含注释的文件会误判失败）。原始字节在回滚与“恢复原设置”时仍可精确还原。
5. **前端未做 UI 改造**：仅结构调整，视觉与交互保持原样。本轮把预览页剩余的账户/连接判读（`savedConnection`、`connectionStateUnavailable`、`signedIn`、余额提醒）收进 `features/activation/{connectionReading.ts,useAccountConnectionReading.ts}` 并补了纯函数测试；仍可继续下沉的是安装流程与庆祝反馈的状态组合。
6. **未跟踪的嵌套副本** `yeschoy-ai-desktop/`：仅通过 Vitest 配置排除，未删除，仍占用工作区。
7. **Trellis**：本会话按你的选择未新建 Trellis 任务。
8. 全程未对本机真实工具配置或钥匙串执行接入/恢复，也未调用真实付费接口。

## 7. 后续新增 Agent 或 Station Provider 的标准步骤

**新增一个 AI 工具（Agent）**
1. 新建 `src-tauri/src/tool_adapters/<id>.rs`：实现发现、`prepare_*` 生成 `FileChange` 计划、格式读回校验、回滚。
2. 在 `tool_adapters/mod.rs` 的 `TARGETS` 增加条目（工具清单唯一来源）。
3. 若该工具需要本地桥/运行时，实现 `SharedCommit`/`SharedRollback` 内包装，并加入 `HelperRuntimes` 与生命周期分发表。
4. 前端：`appCatalog`/`ACTIVATION_TOOL_IDS` 增加 id 与展示信息；如需新文案，`zh.json`/`en.json` 同步。
5. 运行全量检查；新增适配器测试与（如涉及）源码守卫更新。

**新增一个中转站（Station Provider）**
1. 在 `src-tauri/src/adapters/stations/` 实现 `StationProvider`（仅账户 JSON，路径限定同源 `/api/`）。
2. 站点身份统一用 `StationOrigin`；首次信任仍走既有确认流程。
3. 在接入/连接处按 origin 选择 Provider，工具适配器与配置格式无需改动。
4. 新增 Provider 测试（路径白名单、状态分类、不泄露服务器文本），并跑全量检查。

---

### 过程说明（如实记录）

- 重构分阶段推进，每阶段都以编译/测试作为闸门；中途多次因自动化脚本切分失误导致结构被破坏，均通过保留的备份与全量测试恢复，未遗留半成品代码。
- 验证过程中出现过若干次测试失败，均按根因修复（例如站点传输只接受同源相对路径、测试夹具命名形状与生产不一致、共享测试清单造成断言过宽），没有通过放宽断言或删除用例来“变绿”。

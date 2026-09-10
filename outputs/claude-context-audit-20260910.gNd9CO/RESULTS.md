# Claude 逐模型长上下文修复

2026-09-10。源码基线 `1c77a96b0ed5cc48ee053a66c8f4bd41c409f867` 加已有未提交修复。此次只修改四个业务源码/数据文件及本目录记录，未提交、打包、发布，也未操作本机已安装的 Claude、Codex 或真实账户配置。

## 结论

原来不是统一写了 `contextWindow:200000`，而是 Claude Desktop 的所有自定义模型都写了 `supports1m:false`，同时共享能力表漏填了 Claude 窗口。已改为按真实模型元数据判断：窗口至少 1,000,000 tokens 才启用 1M 选项，不拿用于思考强度的 Claude 映射角色推断容量。

依据 [Anthropic 上下文说明](https://platform.claude.com/docs/en/build-with-claude/context-windows)，补齐现存十个 Claude 型号的 1M 元数据及来源：Sonnet 4.6/5、Opus 4.6/4.7/4.8/5、Fable 5/5.1、Mythos 5/5.1。GPT、DeepSeek 已有的窗口保持不变。没有元数据的型号不猜测，不因名字相似自动宣称 1M。这里只是模型参考能力，不是账号的可用模型清单或上游通道保障。

## 实现与兼容

- `src/model-profiles/catalog.json` 仍为单一能力来源；`tool_model_profile.rs` 统一派生 1M 标记，明确支持已知 Claude 的 `anthropic/` 命名空间，不改变真实模型 ID。
- Claude Desktop 单模型、多模型和模型发现端点使用相同标记；新配置写入后的读取校验会核对标记。只读检查兼容旧版本的关闭标记，不会仅因旧标记把已接入状态判为失败。
- 助手原有启动迁移会更新自有 profile：要求匹配本地地址、令牌和 gateway 类型。对完整有效的列表，保留当前模型顺序、默认选择与额外选项，只更新自有模型名称/标签/能力字段；旧别名仍可迁移。不完整或重复的列表沿用既有的标准列表修复逻辑。
- 复用继承自 CC Switch 的尾部 `[1m]` 解析规则，支持大小写与空白；只用来查找已登记的本地别名。未知请求原样保留，不进行角色猜测。转发时还原真实模型 ID，其余请求内容、思考强度、输出预算与流式响应保持不变。
- Codex/Pi/DSH/OpenClaw 继续读取同一能力表，不新增各自的窗口常量；Pi/DSH/OpenClaw 已有用户显式窗口/输出预算不被提高。
- Claude Code 只读审计未发现这条 `supports1m:false` 设置。按 [官方模型配置](https://code.claude.com/docs/en/model-config) 与 [设置参考](https://code.claude.com/docs/en/settings-reference#modelpicker)，没有凭空增加逐行 `contextWindow` 字段，也没有给混合模型列表强加全局 1M 环境变量；原有思考强度配置保持。

## 回归证据

所有 native 命令使用 `RUSTUP_TOOLCHAIN=1.94.0 cargo test --manifest-path src-tauri/Cargo.toml --locked --offline --lib <filter> -- --nocapture --test-threads=1`。只调用纯函数、临时目录和随机端口的本机假上游；假上游使用合成凭据，禁用代理，不访问真实 API。

| 验证 | 结果 | 记录 |
| --- | --- | --- |
| 先复现全部关闭标记、1M 别名未还原 | 2 项按预期失败 | `native-before.log` |
| 复现迁移覆盖模型顺序/额外选项 | 1 项按预期失败 | `migration-before.log` |
| 新增长上下文回归 | 11 通过 | `native-after.log` |
| 共享能力模块 | 9 通过 | `model-profile-regression.log` |
| Claude Desktop 配置与迁移 | 11 通过 | `desktop-regression.log` |
| Claude 本地别名转发 | 6 通过 | `bridge-regression.log` |
| 其他原生消费者 `ru043_` | 8 通过 | `native-consumer-regression.log` |
| 思考强度 `ru056_` | 4 通过 | `effort-regression.log` |
| 原状态读取问题 `inspection_regression_` | 5 通过 | `status-regression.log` |
| 模型选择/计费卡片/配置预览前端 | 3 文件、14 测试通过 | `frontend.log` |
| TypeScript 类型检查 | 通过 | `typecheck.log` |
| 修改的 Rust 文件与能力 JSON 格式 | 通过 | `rustfmt.log`、`catalog-format.log` |

上表 native 筛选集有重叠，不能把各行相加当作独立用例数。HTTP 回归包含新旧别名、Claude/GPT 模型切换、1M 标记剥离、版本/beta 头保留、请求其余字段不变、SSE 回传。它不是百万 token 推理测试，也没有启动真实客户端 GUI。

## 边界与交付状态

- Windows 与 macOS 使用这段共享逻辑，但本次没有运行 Windows VM 或真实客户端长对话验收，不能据此宣称所有上游的 1M 都已实测可用。
- 更新后的配置提供 1M 能力/选项，不强制改动 Claude 已保存的 200K 会话。实际更新到含修复的助手、启动迁移后，Claude 可能需要重新读取设置或重新选择 1M 选项；不会由此次开发测试代替用户操作现有应用。
- 本轮未打包，之前交付的 `0.4.15 statefix2` 安装包不包含本轮上下文修复。主版本和朋友版共用修复源码，后续需要分别重新构建。
- 按 `govern-product-build` 保留范围与隔离回归证据，复用现有能力所有者，不增加另一套运行时。其严格冻结校验仍被 67 项历史当前树/旧验收快照差异阻断，详见 `SCOPE.md` 和 `governance-review.json`；保留失败记录，不改写历史证明，不宣称正式发布就绪。
- 前端测试仅出现已存在的浏览器兼容数据库过期提示，没有为此变更依赖。

## 四个修复文件 SHA-256

```text
882bc2b22556fa26a1419450f77a44358eecfc1f9709fbcde75da2504fc6eeaf  src/model-profiles/catalog.json
42136c34da125c3c852de4d940e2579188bd2a2f82e76952bb30906c041772a0  src-tauri/src/tool_model_profile.rs
955d31ecd4c42e2d9f3e3001dcccb6cf8e1b70ad600ce430b49030045f967890  src-tauri/src/tool_adapters/claude_desktop.rs
a6397d875abfa410896292d42c936bd003cbbed25efcf7ed23493076ffb1616b  src-tauri/src/claude_bridge.rs
```

已核对此前状态读取修复的 `codex_desktop.rs`、`tool_activation.rs`、`connections.tsx`、`connections.test.tsx` 字节哈希与本轮开始前相同，未覆盖之前的修复。

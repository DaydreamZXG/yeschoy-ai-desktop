# 野菜API 0.4.8：跨应用接入修复

本轮修复客户端，不改 NewAPI、服务器、用户真实配置或密钥，不发付费测试请求，不推送 GitHub、不发布自动更新。保留 0.4.7 的模型说明隐藏与 Claude 合法空响应修复。

## 两张截图对应的问题

1. **线路诊断误报。** 原生成功枚举实际序列化为 `tcp443_reachable`，界面却检查 `tcp_443_reachable`。任意成功线路都可能使整份结果被拒绝，显示“两条都尚未检查”。已显式固定原生值；同一份原生序列化样本同时进入界面测试。现在逐条保留有效结果，调用失败、格式异常和真正的 DNS/TCP 失败分别显示；上次结果会标旧，不充当本次结果。
2. **Windows Codex 启动失败。** 商店/MSIX 应用不能都按普通 exe 启动。现在从 Windows 已安装包获取实际 AUMID，核对清单对应的 GUI，再通过系统应用激活接口启动；普通安装仍走已验证的 GUI 路径。忽略图标程序、更新器参数和未经验证的注册表命令，支持常见版本子目录。初始化与日常打开共用同一启动器，保留安全的阶段及系统错误码。依据：[GetPackageApplicationIds](https://learn.microsoft.com/en-us/windows/win32/api/appmodel/nf-appmodel-getpackageapplicationids)、[ActivateApplication](https://learn.microsoft.com/en-us/windows/win32/api/shobjidl_core/nf-shobjidl_core-iapplicationactivationmanager-activateapplication)。

截图证明失败停在打开应用阶段，不证明模型被拒绝。以上修复有可执行回归与 Windows 编译证据，但尚未在截图对应的 Windows 电脑上复测，不能把该机器的具体原因宣称已最终确认。

## 其他应用一起修复

| 应用 | 本轮处理 | 验证边界 |
| --- | --- | --- |
| Claude Desktop | 共用原生桌面启动器，保留本地桥接与恢复机制 | 合成网络/配置测试，不宣称真实 GUI 对话通过 |
| Codex Desktop | 区分商店和普通安装；去掉没有实际使用的“必须自带 CLI”检查 | 配置读回、所选模型连接测试、系统启动请求；不冒充 GUI 回复验证 |
| Claude Code | 只读校验已接入配置，增加“打开终端使用”；保留真实 JSON 回复校验 | 临时可执行程序正反例，不操作真实配置 |
| Pi | 拒绝启动横幅、帮助、回显及非零退出；增加终端打开 | 单次模式的最终回复校验 |
| DSH web | 拒绝提示词回显；保留已验证的 web 进程复用 | headless 最终回复及本地进程生命周期测试 |
| Hermes | 拒绝 stdout 非空即成功；增加只读校验与终端打开 | `-z` 单次最终回复校验 |
| OpenClaw | 校验完整密钥助手调用；增加终端打开 | JSON 最终回复和只读配置测试 |

单次模式依据：[Hermes CLI](https://hermes-agent.nousresearch.com/docs/reference/cli-commands)、[Pi print mode](https://raw.githubusercontent.com/earendil-works/pi/main/packages/coding-agent/src/modes/print-mode.ts)、[DSH headless](https://raw.githubusercontent.com/deepseek-ai/deepseek-harness/master/packages/bundle/headless/README.md)。不会因为未知版本号直接禁止接入，也不会用帮助文字当模型成功回复。

日常“打开使用”不登录、不重新生成密钥、不改应用配置、不发送付费探针。macOS 终端通过系统打开私有临时 `.command` 文档，无 AppleScript 控制权限；文件仅含已解析路径的启动与自身清理，权限 0700、无密钥。失败精确清理，成功交接后由脚本启动时清理；若系统接受请求但从未执行，可能留下不含密钥的私有临时文件，不扫描删除其他临时目录。Windows 使用新的原生终端窗口，无提权、无执行策略绕过。两者只说明已请求打开，不把它当成模型验证。

## 错误和恢复

- 分开显示应用启动失败、测试超时、未读取到有效回复、配置恢复失败和原密钥恢复失败。
- 恢复未完成时保留待恢复记录，不再显示“没有改动”或“原设置已保留”。已有逐项恢复能力保持；外部改动不会被静默覆盖。
- 同一次操作重复点击只启动一次，旧操作结果不会显示到新应用/新配置上。
- 进程等待和输出读取共用截止时间、输出上限；取消时终止自己的子进程，不留下独立输出读取任务。

## 验收记录

- 原生库测试：387 通过，0 失败、0 忽略；含五个 CLI 验证器的真实临时可执行程序、超时/取消、七工具恢复、Windows 包身份与分发测试。
- 当前客户端 `src` 界面回归：404 通过，0 失败、0 跳过；其中诊断 26 项、日常使用 34 项。
- TypeScript 类型检查、实际 Vite 生产构建、macOS 与 Windows `clippy --all-targets -- -D warnings` 通过。Windows 的 Clang 编译参数提示属于交叉工具链提示，不是 Rust 警告。
- 全仓附加测试：1,282 通过，7 失败，失败均在 `tests/integration/App.test.tsx`。此结果单列，不宣称全仓全绿；冻结验收不删除或放宽这些历史测试。
- 版本清单统一为 0.4.8。固定验收入口：`tests/work_packages/ru041/test_acceptance.py`；最终机器记录位于 `.product-governance/execution/reports/`，安装包签名、公证、哈希和实机边界另见 `release/internal/0.4.8/BUILD-RECEIPT.json`。

## 尚需真实设备确认

新包需要在受影响 Windows 电脑验证：线路检查显示真实结果；Codex/Claude 可分别打开；商店版与普通安装均能继续对话。Pi/Claude Code/Hermes/OpenClaw 的新终端窗口也需要目标系统实测。交叉编译不等于这些真实应用均已运行成功。

Windows 包仍无代码签名证书；macOS 继续使用已有 Developer ID 签名和公证。没有启用自动更新，也没有远程更改用户正在使用的应用。

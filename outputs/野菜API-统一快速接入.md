# 野菜 API：统一快速接入修复验收

日期：2026-09-06
范围：Claude Code、Claude Desktop、Codex Desktop、Pi、DSH web、Hermes、OpenClaw；macOS 与 Windows。
本轮未执行：打包、签名、上传、发布、版本号修改、更新器修改、服务器或数据库修改。

## 结论

旧实现把“写好本机配置”和“上游模型成功回复”放在同一笔接入事务中。Codex 的旧探针允许等待 600 秒，Claude Desktop 等待应用请求 90 秒，CLI/DSH 探针允许等待 120–150 秒；任何上游拥堵、配额或应用启动问题都会让“一键接入”长时间旋转，并可能把已经正确的本机配置回滚。应用列表还会启动最多五类 CLI 获取版本，单个候选允许等待 8 秒，进一步拖慢初次检测。

现在接入成功只表示：工具密钥已安全发布、配置已原子写入并读回、需要的本地路由已启动、应用打开动作已发出。接入过程不再发送付费测试消息，也不再等待模型回复。用户第一次正常请求的真实结果继续进入“最近连接结果”；上游错误不会撤销已经有效的本机配置。

应用接入列表现在只检查受控路径清单，不启动第三方 CLI。版本诊断仍由独立诊断入口完成，不再阻塞小白用户选择应用。

## 正在使用的应用如何处理

- Claude Desktop 与 Codex Desktop：先按“精确安装身份 + 精确路径”检查是否正在运行。首次点击只返回“请保存工作”，不会读取账户、创建密钥或修改配置。用户明确确认后，macOS 请求应用正常退出；Windows 向精确可执行文件所属的顶层窗口发送 `WM_CLOSE`。最多等待 10 秒，拒绝或超时即停止，不强杀、不继续写设置。
- 如果应用已经正常退出，但后续设置步骤失败，回滚完成后由保护器重新打开同一个已发现安装；不会让用户莫名其妙丢掉正在用的应用。
- Claude Code、Pi、Hermes、OpenClaw：不退出、不终止当前终端或编辑器会话。设置完成后提示新开会话，当前任务不被打断。
- DSH web：只启动或复用野菜自己拥有的本地运行时，不管理用户的其他进程，也不发送模型探针。

## 与 CC Switch 的对照

本轮参考了 CC Switch 的公开实现和使用说明。其普通切换链路以校验、补全、写配置和同步为主，并没有把强制结束进程当作通用切换步骤；文档分别说明 Codex、Claude Desktop 需要重启，Claude Code 首次接管通常从新会话生效。野菜在此基础上增加了面向小白用户的显式保存确认、精确进程身份、10 秒正常退出上限和失败后重开保护。

- CC Switch provider switching source: https://github.com/farion1231/cc-switch/blob/38cfafdc199604b132be5eafe3f5384d85124a81/src-tauri/src/services/provider/mod.rs
- CC Switch switching guide: https://github.com/farion1231/cc-switch/blob/38cfafdc199604b132be5eafe3f5384d85124a81/docs/user-manual/en/2-providers/2.2-switch.md
- CC Switch Claude Desktop guide: https://github.com/farion1231/cc-switch/blob/38cfafdc199604b132be5eafe3f5384d85124a81/docs/user-manual/en/2-providers/2.6-claude-desktop.md

## 验收结果

- Rust 原生单元测试：477 项通过，0 失败。
- 前端完整单元测试：453 项通过，0 失败；其中覆盖桌面应用保存确认、确认后重试、四类 CLI 保留当前会话和旧 v3 返回兼容。
- 严格 Rust Clippy：通过，`-D warnings`。
- TypeScript 类型检查：通过。
- 前端生产构建：通过。
- Apple Silicon (`aarch64-apple-darwin`) 编译检查：通过。
- Windows x64 (`x86_64-pc-windows-msvc`) 编译检查：通过，包括 Windows 进程路径枚举和 `WM_CLOSE` 分支。
- 统一 RU-056 验收：原生、渲染器和双平台构建三组场景均已在本机预验收；正式治理验证记录由验证器生成。

## 仍需实机确认的边界

编译和合成测试不能冒充真实用户环境。打包前仍应在一台 macOS 与一台 Windows 机器上分别做一次：应用有未保存内容时取消退出、保存后确认退出并重开、上游故障时本机配置仍保留、CLI 当前会话不中断且新会话生效。由于用户要求暂不打包，本轮没有生成新的安装文件。

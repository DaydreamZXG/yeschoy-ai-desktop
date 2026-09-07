# 野菜 API · 安装并接入

本次实现面向未安装客户端的新用户：选应用 → 自动选择安装包 → 完成安装 → 连接已选模型。没有打包、升级版本、推送代码或修改下载服务器；版本仍为 0.4.8。

## 已实现

- 空白电脑直接显示 Codex Desktop、Claude Desktop 的「安装并接入」。已安装的应用继续使用原有接入、打开和恢复原设置流程。
- macOS 按芯片识别官方安装包；检查原包、应用签名、发布者、应用身份和系统安全许可，再安装到当前用户的 Applications。已有应用不覆盖。
- Windows 自动识别 x64 / ARM64，下载官方 MSIX，交由 Windows 应用安装程序核验和确认。用户在系统中点击「安装」，再回野菜点击「检查安装并继续」。野菜不静默部署，也不更改证书或系统安全策略。
- 下载进度、暂停与续传、重试、失败原因、官方帮助、跨页面进度入口。Windows 可以结束野菜的引导，继续安装其他应用；这不会取消 Windows 安装。
- 下载只接受内置官方 HTTPS 来源。续传绑定服务器的强校验标识，限制内容类型、长度、跳转、时间与包大小；重新下载不会改写 Windows 正在使用的包。
- 安装完成后重新查找应用，只对本次安装的唯一目标继续接入。变更账户、应用、模型、分组、默认模型、线路或离开接入页后，需要重新确认；本机也独立校验一次性授权，不能靠旧界面状态跳过。
- 安装完成不等于接入成功。只有原有真实连接测试通过，才显示接入成功；连接失败保留已安装的应用，并使用原有设置恢复机制。测试消息按所选分组计费。
- Claude Code、Pi、DSH、Hermes、OpenClaw 暂时提供官方安装说明，未伪装成已经具备自动安装能力。

## 验证及边界

开发验证已运行：451 项 Rust 测试全部通过，包含本地 HTTP 断流续传、卡住下载时退出、原文件/硬链接保护、Windows 精确目标映射、不可变安装包、取消授权和异常工作进程清理；新安装界面 11 项测试通过，相关原有界面加新测试共 104 项通过。后续最终验收使用同一完整回归入口，不以这些开发结果代替最终记录。

最终原生、Windows / Apple Silicon 编译、全部界面测试、类型检查和前端构建，以 [RU-050 自动验收记录](../.product-governance/execution/reports/188b24bb6c58d318.b870f3886de6409c94e2.json) 为准。该记录生成前不表示已通过最终验收。失败或未完成的历史工作单元及其证据均保留。

界面检查使用实际安装组件和明确标注的示例数据，检查了浅色、深色和 900 像素小窗口，未通过预览执行真实安装。

仍需完成的发布验收：

1. 干净 Windows x64 / ARM64、苹果芯片 Mac 上的真实安装、系统确认、首次启动、接入及恢复设置。当前电脑已有应用，没有卸载或替换它们来制造测试环境；交叉编译不等于上述实机验收。
2. 国内下载源启用及访问验证。`ergou.qzz.io` 的原安装包仍属于私有准备阶段，本次客户端只使用官方原始下载链接，不能承诺无代理网络一定能下载成功。Claude 下载源遇到浏览器验证时不绕过验证。
3. 其他客户端的依赖安装适配、Claude Cowork 的附加服务/权限等，不在本次自动安装范围。Windows 若显示「重新安装 / 替换」，应取消并使用已有应用。
4. 官方 Codex 新安装包的 Mac 支持范围为苹果芯片；Intel Mac 会给出替代和官方支持说明，已有 Codex 的检测与接入不因此被禁用。

## 主要代码

- `src-tauri/src/app_installation/`：来源、下载、系统安装与一次性接入授权。
- `src/installation/`：进度管理、安装界面与测试。
- `src/configuration/ConfigurationPreviewView.tsx`：选项变更保护与安装后的接入。
- `src/workbench/AppLibraryView.tsx`：未安装应用入口。
- `tests/work_packages/ru046/`：完整验收入口和不执行真实安装的组件预览。

实现沿用项目产品治理与前端设计规范：以可验证的结果区分安装和连接，使用现有绿色品牌、中文系统字体和明确的恢复操作。没有引入可以任意执行电脑命令的 Agent。

## 官方依据

- [Codex 桌面版支持范围](https://learn.chatgpt.com/docs/app)
- [Codex Windows 原包部署说明](https://learn.chatgpt.com/docs/enterprise/windows-deployment)
- [Claude Desktop 安装](https://support.claude.com/en/articles/10065433-install-claude-desktop)
- [Claude Windows 部署与附加组件](https://support.claude.com/en/articles/12622703-deploy-claude-desktop-for-windows)
- [Windows 安装程序的 Install / Reinstall 界面](https://learn.microsoft.com/en-us/windows/msix/app-installer/app-installer-ui-dialog)

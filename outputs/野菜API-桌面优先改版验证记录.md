# 野菜API 桌面优先改版验证记录

- 验证日期：2026-08-31
- 变更单元：RU-011 / WP-RU011-DESKTOP-FIRST-EXPERIENCE
- 验证范围：客户端界面、桌面应用只读识别、品牌图标、四语言文案与既有安全边界

## 结论

本轮把候选版从命令行工具优先改为桌面应用优先。首页和接入页以 Claude Desktop 与 Codex 两个图形桌面应用为主入口；Claude Code、Codex CLI、OpenCode、Pi 与 DSH 保留在“高级工具”。本轮没有修改服务器、NewAPI、Nginx、DNS、线上配置、用户密钥或第三方应用配置。

当前版本仍是安全预览候选版：可以识别本机应用并读取公开模型目录，但“检测到应用”不等于已登录、已接入、已配置或已兼容。账户余额、用量、充值、密钥领取与真正写入配置继续显示为未接入或禁用状态。

## 视觉与交互检查

通过本地生产同源渲染页面进行了桌面窗口检查：

- 1120 × 760：深色常驻侧栏、品牌图标、双应用卡片、账户摘要和四步接入路径均正常显示；首页主流程没有要求用户理解 Base URL 等技术配置。
- 1120 × 760 接入页：应用、线路、模型与确认四个阶段可辨认；高级技术信息默认折叠；未满足条件时“应用”按钮保持禁用。
- 860 × 640：导航、应用卡片、余额摘要和重新检查入口仍可操作，无横向内容溢出。
- 浏览器控制台无 error 或 warning。由于普通浏览器没有 Tauri 原生桥，页面按设计展示可恢复的“暂时无法读取桌面应用状态”，没有用旧结果或假数据替代。
- 新图标以野菜叶片、线路和节点为视觉元素，已生成并接入 macOS ICNS、Windows ICO 和各尺寸 PNG 资源。

## 自动化验证

以下命令在当前工作区通过：

```text
python3 -m pytest -q tests/work_packages/ru011/test_acceptance.py
6 passed

pnpm run typecheck
passed

pnpm exec vitest run <candidate suites> --exclude 'release/**'
7 files passed, 30 tests passed

cargo test --manifest-path src-tauri/Cargo.toml --lib --no-fail-fast
18 passed

pnpm run build:renderer
passed
```

Rust 验证使用本机已经安装完成的 stable 工具链直接执行。默认 rustup 包装器会尝试恢复一个未完整安装的未来工具链，并因 `cargo-fmt` 文件冲突失败；这属于本机工具链状态，不是客户端代码失败。

## 识别与安全边界

- 原生识别只接受受限 `requestId`，只返回 Claude Desktop 与 Codex 的固定身份、受控状态和经过清洗的版本号。
- macOS 只检查 `/Applications` 与当前用户 `Applications` 下的固定应用名，并核对公开 bundle identifier；不会返回完整路径。
- Windows 只组合预先编译的常用安装位置；不会扫描整盘、执行应用或运行命令行版本。
- 不读取 API 密钥，不读取或写入目标应用配置，不保存扫描历史，不上传遥测。
- 公开模型读取和线路诊断仍需用户主动触发；模型 ID 继续显示真实公开值，不生成看似真实的占位数据。
- 自动更新、签名公钥轮换、支付与服务器接口不在本轮实现范围内。

## 旧冻结测试说明

RU-001、RU-002、RU-003、RU-005 与 RU-008 中存在面向历史界面结构的源码字符串或旧运行夹具断言，例如要求客户端默认打开账户页、要求命令行工具继续作为首页主流程，或只允许最早的单一 Tauri 命令。这些断言与已冻结并明确取代旧入口优先级的 RU-011 目标冲突，因此不作为本轮验收门槛。对应的当前能力通过 RU-011 场景、现有 Vitest 候选套件和 Rust 全库测试重新覆盖；RU-006 与 RU-009 的独立验收仍通过。

# 已接入后换模型，再次接入无法点击：修复记录

日期：2026-09-10。范围：共享确认弹窗及 Codex Desktop / Claude Desktop 重复接入的前端流程。

## 结论

已在隔离浏览器中复现并修复一处确定的页面假死原因：重启确认框缺失实际生效的定位样式，落在视口下方，但 Radix 已经锁住弹窗外的输入。

这是本工具前端的缺陷，不能据此认定网关又挂了。当前没有反馈设备的原始日志，不能保证每一种“卡死”均由此引起。本记录也不是 Windows 安装包、原生进程退出或真实模型请求的整体验收。

## 证据链

1. `ConfigurationPreviewView.tsx` 收到 `application_running` 后，打开现有 `ConfirmDialog`，提示保存内容后允许重新打开目标应用。
2. `ConfirmDialog` 使用共享 Radix `DialogContent`。修改前只有 `fixed`、`left-1/2` 等 Tailwind 类，但当前 `main.tsx` 加载的是手写 CSS，没有 Tailwind utilities 生成入口。
3. 使用真实 `ConfirmDialog` 和工作台 CSS 的隔离页面，确认前正常；打开弹窗后 `body.pointer-events` 变为 `none`。修改前弹窗 `position: static`，顶部等于视口高度，全部位于可视区外。保存了修改前的组件文件 `dialog.before.tsx`。
4. 修复后直接检查浏览器计算样式、实际点击、生产构建 CSS，而非只检查 React 节点是否存在。

| 测量场景 | 定位 | 弹窗范围 | 页面输入 |
| --- | --- | --- | --- |
| 修改前，隔离开发页面 | `static` | 顶部 826、底部 935.78125；视口高 826 | `pointer-events: none`，没有可见弹窗 |
| 修改后，生产构建，1280 × 720 | `fixed` | 左 430、右 850；顶部 238.71875、底部 481.28125 | 弹窗可见，按钮可点击 |
| 生产构建，Escape 取消 | 已卸载 | 无弹窗 | `pointer-events: auto`，设置探针计数从 0 增至 1 |
| 生产构建，确认后保持模拟配置未完成 | 已卸载 | 无弹窗 | `pointer-events: auto`，设置探针从 1 增至 2；后台请求仍 pending |
| 生产构建，深色，380 × 640 | `fixed` | 左 16、右 364；顶部 173.5234375、底部 466.4765625 | 两个按钮均在视口内，取消可用 |

开发构建另在同一页面连续完成四轮确认 / 等待 / 点击其他操作 / 完成模拟配置；接入计数从 1 到 5，结束时无残留弹窗、`pointer-events: auto`。深色和窄窗口已视觉检查；临时视口覆盖已恢复。

## 修改

- `src/components/ui/dialog.tsx` 显式导入组件自己的 `dialog.css`，增加稳定的具名类和层级 / 全屏属性。
- `src/components/ui/dialog.css` 定义视口定位、遮罩层级、有限高度与内部滚动，使用现有工作台的主题变量；确认按钮、取消按钮和长标题在窄窗口仍可操作。不重新启用另一套全局 reset。
- `src/components/ConfirmDialog.tsx` 为现有结构增加样式类，不改退出授权和回调行为。
- `src/components/ConfirmDialog.test.tsx` 新增 7 项真实 Radix 回归，包含定位、取消 / Escape / 确认后输入恢复、重复打开、层级和全屏。
- `src/configuration/DailyUse.test.tsx` 新增 2 项实际接入页面回归，分别覆盖 Codex 和 Claude：首次成功后不卸载页面，连续选择两个不同模型；提示保存，取消，再试，明确同意后才传递 `restartRunningApp: true`；模拟请求未完成时遮罩已卸载，其他操作仍可点击。

没有改动原生退出逻辑、网关、凭据、账户、计费分组或模型上下文参数。原生桥、两种桌面适配器、接入事务、模型 profile / catalog、连接状态读取代码及测试等 8 个文件的 SHA-256 与本轮前基线一致。`DailyUse.test.tsx` 原有的连接状态测试修复仍保留。

## 验证

- 相关前端回归：13 个测试文件、213 项通过。机器可读结果：`regression-final.json`。
- `pnpm typecheck`：通过。
- 5 个本轮源码 / 测试文件 Prettier 检查：通过。
- `git diff --check`：通过。
- 正式 renderer 的生产构建：通过，产物保留在 `renderer-dist/`，没有覆盖原有 `dist/` 或安装包。
- 隔离页面生产构建：通过，产物在 `browser-dist/`；通过内置浏览器在本地回环地址操作验证。
- `verify-production-css.mjs` 对以上两套真实压缩产物做只读解析，检查定位、层级、滚动、全屏等 9 个必要声明：均通过，结果见 `production-css-check.json`。正式 CSS SHA-256：`acd37c814f1f14b6abec5054181c268aa7f5f7156f8bb15faad9ae106be08a15`。

构建仍提示既有的 Browserslist / baseline 数据较旧及 JS chunk 超过 500 kB；没有把警告当作失败，也没有为本次局部修复升级依赖。

最初的测试扫描整个仓库，采样显示大量目录扫描，未进入断言；只停止了本轮启动的两个 Vitest 进程，再用 `--dir src` 限定发现范围。初版用例中的 CSS raw import 被 Vitest 清空、Vite 对 `new URL` 的资源转换、原生预检调用次数及 Radix ARIA 属性假设随后校正；失败报告 `focused-regression.json` 和 `focused-regression-2.json` 保留。最终断言验证真实行为，没有 mock 弹窗来绕过问题。

重跑命令（仓库根目录）：

```sh
pnpm exec vitest run --dir src src/components/ConfirmDialog.test.tsx src/configuration src/workbench src/settings/QuitAssistant.test.tsx --reporter=default --reporter=json --outputFile=outputs/reconfigure-freeze-audit-20260910.oLWWVE/regression-final.json --maxWorkers=2 --minWorkers=1
pnpm typecheck
pnpm exec vite build --outDir /Users/zxg/.codex/worktrees/c485/xiaobai-ai-desktop/outputs/reconfigure-freeze-audit-20260910.oLWWVE/renderer-dist
pnpm exec vite build --config outputs/reconfigure-freeze-audit-20260910.oLWWVE/browser/vite.config.mjs
node outputs/reconfigure-freeze-audit-20260910.oLWWVE/verify-production-css.mjs
```

## 边界与交付状态

遵照 `govern-product-build` 保留范围、修改前证据、失败记录和回归结果，复用原组件所有者。既有 RU-075 快照与当前工作树不一致，RU-076 严格治理 review 仍有 73 项错误，见 `governance-review.json` / `SCOPE.md`；没有改写旧冻结证明或宣称发布门禁通过。

本次未启动、退出、重新配置用户安装的 Codex、Claude 或野菜；没有真实账户或模型请求。浏览器测试只运行合成状态，实际接入页面测试只运行模拟原生接口。未做 Windows 真机或 macOS 安装包回归。

本次只修源码并构建前端检查产物，未创建安装包、未提交 / 推送、未上传或修改更新源。主版与朋友版共用此组件，后续均需重新构建安装包才会包含修复；官网此前上传的安装包仍未包含本次修改。

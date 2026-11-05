# 闪切 · FLASHCUT — visual system v3

> 上一版「青芒」仍是置中柔雾卡片 + 礼貌标题，截图像同一套 SaaS。这一版按日漫混剪（AMV）重做：硬切、冲击帧、光刃、色差、撑满屏幕的中文标题。
> 侧栏信息架构不变：我的应用 / 接入 / 账单 / 公告 / 设置。标准主题不受影响。

## 方向

**闪切**把开机当成一段 6–8 秒可跳过的混剪，而不是一张对话卡片。工作台用墨色侧栏和硬边色块，让截图也看得出换皮。

| 不要 | 改成 |
|------|------|
| 置中柔雾卡片 + 小标题 | 全屏电影遮幅，标题按幕次撑满画面 |
| 青芒六瓣纹章 / 橙色准星 HUD | 斜切光刃 + 集中线 + 品红/青色谱线 |
| 「青芒初启 / 把光折进来 / 启明」 | 「第一刀 / 把世界切开 / 切入 / 闪光落下」 |
| 奶油毛玻璃侧栏 | 近黑墨轨 + 电柠斜切当前项 |
| 壁纸当右侧淡水印 | 壁纸参与右侧构图，左侧表单仍可读 |

开机底板是墨黑 `#07070a`，冲击帧走白闪。工作台内容区仍是暖纸色，表单、密钥、诊断保持高不透明。二次元皮继续锁暖白（关掉深浅切换），墨色只给侧栏。

## 开机四幕

全程硬切（冲击白帧，不淡入滑出）。可 **跳过 / Esc**。`prefers-reduced-motion: reduce` 时去掉冲击、光束与色差。

| 幕 | 中文 | 英文 | 交互 |
|----|------|------|------|
| 01 | 第一刀 | THE FIRST CUT | 自动，约 1.2s |
| 02 | 把世界切开 | SLICE THE FRAME | 「切入」或 Enter / Space；约 2.8s 无操作自动 |
| 03 | 闪光落下 | IMPACT FLARE | 光束拉满，自动约 1.2s |
| 04 | 登录一次，接入你的 AI 应用 | Sign in once · Connect your AI apps | 产品白话后硬切进工作台 |

仅 `data-skin="anime"` 会播；标准皮不播。默认每天一次。整段约 7 秒（含切入闪光），可随时跳过。

## SVG 零件

| 组件 | 路径 | 作用 |
|------|------|------|
| `BootSigilSvg` | `src/workbench/bootSvg/BootSigilSvg.tsx` | 色差光刃纹章 |
| `BootVeilSvg` | `src/workbench/bootSvg/BootVeilSvg.tsx` | 全屏集中线 + 光束 |
| `BootMotesSvg` | `src/workbench/bootSvg/BootMotesSvg.tsx` | 飞溅光痕 |
| `BootRefractSvg` | `src/workbench/bootSvg/BootRefractSvg.tsx` | 「切入」时的闪光星爆 |
| 工作台光晕 | `src/assets/skins/ambient-glow.svg` | 右侧色差光刃，不是橙环 |

`data-visual="flashcut"`。旧青芒柑橘纹章与橙色 HUD 已删除。

## 工作台铬层

`src/theme/anime-skin.css` 在二次元皮下：

- 侧栏：近黑墨轨 + 右侧电柠竖刃 + 竖排 FLASHCUT
- 当前导航：电柠斜切色块（指示器藏掉）
- 卡片：白底、2px 墨线、硬投影；账单首卡铺满电柠
- 页标题：更大字号 + 斜切色条
- 壁纸：右侧构图，对角线遮罩，不盖住左侧表单
- 诊断 / 关于 / 高级连接：约 96% 不透明，无模糊、无硬投影

## 怎么预览

```bash
pnpm install
pnpm exec tsc --noEmit
pnpm exec vitest run src/workbench/skin.test.ts src/workbench/bootPrefs.test.ts src/workbench/BootRitual.test.tsx src/designSystem.test.ts
pnpm exec vite
```

浏览器打开本地 Vite 地址。强制重看开机：清 `localStorage` 的 `yeschoy-boot`（以及需要时的 `yeschoy-skin`），或把开机频率设成「总是」。

二次元皮会锁暖白；切到「标准」可对比原工作台。角色壁纸仍是占位 SVG（`src/assets/skins/*.svg`），正式立绘待授权。

桌面壳请在本机：`pnpm tauri dev`（浏览器里没有 Tauri 桥，「待确认」是预期降级）。

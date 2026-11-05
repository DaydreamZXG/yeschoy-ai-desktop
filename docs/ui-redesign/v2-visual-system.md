# 月华 · TSUKIHANA — visual system v4

> 闪切（墨黑遮幅 + 电柠斜切 + 色差大字）被判定失败，整套废弃。青芒奶油卡片和橙色准星 HUD 也已废弃。
> 这一版走另一条路：角色站在右侧舞台，界面是澄镜玻璃（Liquid Glass × Linear × 日系产品感），开机是启幕而不是混剪硬切。
> 侧栏信息架构不变：我的应用 / 接入 / 账单 / 公告 / 设置。标准主题不受影响。

## 方向

**月华**把工作台当成一座夜色剧场：角色是主体，UI 漂在前面。开机和进入后共用同一套丁香天空、虹彩和玻璃，而不是播完一段 AMV 再掉进原来的应用列表。

| 不要                             | 改成                                  |
| -------------------------------- | ------------------------------------- |
| 奶油置中卡片 + 橙色准星          | 浮起的澄镜侧栏与卡片，虹紫 / 樱粉强调 |
| 青芒柔雾柑橘                     | 丁香天空 + 月华光晕，不要青柠         |
| 墨黑电影遮幅、斜切大字、色差闪光 | 左侧文案缓缓升起，右侧角色渐渐显影    |
| 壁纸当淡水印 / 电柠硬边海报卡    | 角色占满右侧舞台，卡片为半透明玻璃    |
| 「第一刀 / 切入 / FLASHCUT」     | 「月出 / 启幕 / 花开 / 月華」         |

二次元皮继续锁定浅色澄镜（关掉深浅切换），避免深色主题把玻璃舞台冲掉。表单、密钥、诊断保持高不透明。

## 开机四幕

全程柔和升起（模糊淡入，不硬切）。可 **跳过 / Esc**。`prefers-reduced-motion: reduce` 时去掉旋转、花瓣漂浮和绽放。

| 幕  | 中文                       | 英文                                | 交互                                         |
| --- | -------------------------- | ----------------------------------- | -------------------------------------------- |
| 01  | 月出                       | THE MOON RISES                      | 自动，约 1.8s                                |
| 02  | 启幕                       | THE CURTAIN LIFTS                   | 「启幕」或 Enter / Space；约 4.5s 无操作自动 |
| 03  | 花开                       | BLOOM                               | 月华绽放，自动约 1.6s                        |
| 04  | 登录一次，接入你的 AI 应用 | Sign in once · Connect your AI apps | 产品白话后淡入工作台                         |

仅 `data-skin="anime"` 会播；标准皮不播。默认每天一次。整段约 10 秒（含启幕绽放），可随时跳过。

## SVG 零件

| 组件             | 路径                                       | 作用                           |
| ---------------- | ------------------------------------------ | ------------------------------ |
| `BootSigilSvg`   | `src/workbench/bootSvg/BootSigilSvg.tsx`   | 三环月华、新月、六瓣           |
| `BootVeilSvg`    | `src/workbench/bootSvg/BootVeilSvg.tsx`    | 极光纱带 + 舞台光环            |
| `BootMotesSvg`   | `src/workbench/bootSvg/BootMotesSvg.tsx`   | 漂浮花瓣与光点                 |
| `BootRefractSvg` | `src/workbench/bootSvg/BootRefractSvg.tsx` | 「启幕」时的绽放环             |
| 工作台光晕       | `src/assets/skins/ambient-glow.svg`        | 右侧虹紫 / 樱粉光球 + 舞台地面 |

`data-visual="tsukihana"`。闪切光刃、青芒纹章与橙色 HUD 已删除。

## 工作台铬层

`src/theme/anime-skin.css` 在二次元皮下：

- 侧栏：内缩的浮起澄镜栏（圆角、模糊、内高光），不是满高墨轨
- 当前导航：玻璃胶囊 + 虹紫柔光指示器
- 卡片：半透明玻璃、细白描边、柔和彩影；账单首卡铺虹彩薄雾
- 页标题：更轻的字重 + 「月華」眉题
- 壁纸：右侧大面积舞台，左侧给表单留出阅读区
- 诊断 / 关于 / 高级连接：约 92% 不透明，无模糊

## 怎么预览

```bash
pnpm install
pnpm exec tsc --noEmit
pnpm exec vitest run src/workbench/skin.test.ts src/workbench/bootPrefs.test.ts src/workbench/BootRitual.test.tsx src/designSystem.test.ts
pnpm exec vite
```

浏览器打开本地 Vite 地址。强制重看开机：清 `localStorage` 的 `yeschoy-boot`（以及需要时的 `yeschoy-skin`），或把开机频率设成「总是」。

二次元皮会锁浅色澄镜；切到「标准」可对比原工作台。角色壁纸仍是占位 SVG（`src/assets/skins/*.svg`），正式立绘待授权。

桌面壳请在本机：`pnpm tauri dev`（浏览器里没有 Tauri 桥，「待确认」是预期降级）。

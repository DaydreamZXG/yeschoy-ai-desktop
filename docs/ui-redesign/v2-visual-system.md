# 青芒 · Qingmang — visual system v2

> 上一版开机是「暖白卡片 + 橙色准星 HUD」。看起来像把游戏准星贴在 SaaS 上，所以整套视觉（含动画）按 **青芒** 重做。
> 侧栏信息架构不变：我的应用 / 接入 / 账单 / 公告 / 设置。标准主题不受影响。

## 方向

**青芒**是野菜已有的电柠 `#c9fa10` 被当成一道光，而不是一层霓虹贴花。

| 不要 | 改成 |
|------|------|
| 橙色准星 / 扫描弧 / HUD 括号 | 六瓣柑橘纹章（切开的青柠） |
| 「神经链路」「击穿边界 · BREACH」 | 短中文：青芒初启 / 把光折进来 / 路径已亮；按钮「启明」 |
| 军事大写英文 ticker | 静默署名「野菜 · 青芒」 |
| 小玻璃对话框中央置中 | 全屏澄光纱 + 标题式字叠，径向柔雾保证可读 |
| 工作台还是普通暖白卡 | 同一套奶油底、叶绿、电柠轨与毛玻璃 |

底板仍是暖瓷白 `#f3efe4`，不是纯黑赛博。工作台表单、密钥、诊断保持高不透明。

## 开机四幕

| 幕 | 中文 | 英文 | 交互 |
|----|------|------|------|
| 01 | 青芒初启 | The first blade of light | 自动，纹章展开 |
| 02 | 把光折进来 | Refract the light in | 「启明」或 Enter / Space；约 5 秒无操作自动 |
| 03 | 路径已亮 | The path is lit | 光谱条，自动 |
| 04 | 登录一次，接入你的 AI 应用 | Sign in once · Connect your AI apps | 落地产品白话后淡出 |

全程可 **跳过 / Esc**。`prefers-reduced-motion: reduce` 时缩短时序、关掉漂浮与折光爆发。

仅 `data-skin="anime"` 会播；标准皮不播。默认每天一次。

## SVG 零件

| 组件 | 路径 | 作用 |
|------|------|------|
| `BootSigilSvg` | `src/workbench/bootSvg/BootSigilSvg.tsx` | 六瓣柑橘纹章 + 种子 + 植物轨道 |
| `BootVeilSvg` | `src/workbench/bootSvg/BootVeilSvg.tsx` | 全屏光带与树冠焦散 |
| `BootMotesSvg` | `src/workbench/bootSvg/BootMotesSvg.tsx` | 花粉 / 种子光点 |
| `BootRefractSvg` | `src/workbench/bootSvg/BootRefractSvg.tsx` | 「启明」时六瓣折光爆发 |
| 工作台光晕 | `src/assets/skins/ambient-glow.svg` | 右侧极淡柑光，不是橙环 |

旧的 `BootReticleSvg` / `BootHudFrameSvg` / `BootParticlesSvg` / `BootEnergyPulseSvg` 已删除。

## 工作台铬层

`src/theme/anime-skin.css` 在二次元皮下：

- 侧栏：澄光毛玻璃 + 左侧一道垂直青芒光轨
- 当前导航：电柠雾胶囊（沿用已有滑动指示器，改成青芒辉光）
- 卡片：白玻璃 + 极淡柠色描边；账单首卡略提亮
- 壁纸：右侧水印，并向左渐隐，不盖住表单
- 诊断 / 关于 / 高级连接：约 94% 不透明，无模糊

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

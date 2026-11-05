# BootRitual SVG 炫酷层

> 在暖白毛玻璃开机仪式上叠一层**纯 SVG / CSS** 动效。角色立绘仍是占位壁纸；准星、扫描弧、粒子、HUD 框用矢量自己画，可随时调参。

## 校准原则

| 更炫（开机壳） | 更克制（工作台） |
|----------------|------------------|
| 多环准星 + 扫描弧旋转 | 壁纸右侧淡水印不动 |
| BREACH 色差闪 + 能量环爆发 | 仅一层极淡 ambient-glow.svg |
| 粒子火花随幕次略增亮 | 密钥 / 诊断卡仍高不透明 |
| SKIP / Esc / reduced-motion 可关 | 侧栏 IA 不变 |

橙强调仍是 `#ff7500`。不要暗黑底板、不要满屏霓虹网格。

## SVG 零件清单

| 组件 / 资源 | 路径 | 作用 | 调参入口 |
|-------------|------|------|----------|
| `BootReticleSvg` | `src/workbench/bootSvg/BootReticleSvg.tsx` | 三环准星、对向扫描弧、八向 glyph ticks、中心核 | 环半径 / `strokeDasharray`；转速见 CSS `--` 无，直接改 `boot-svg-spin` 时长 |
| `BootHudFrameSvg` | `src/workbench/bootSvg/BootHudFrameSvg.tsx` | 毛玻璃卡片四角括号 + 顶底软轨 + 刻度 | `inset` 在 `.boot-svg-hud-frame`；括号路径坐标 |
| `BootParticlesSvg` | `src/workbench/bootSvg/BootParticlesSvg.tsx` | 10 个软火花点 | `SPARKS` 数组（位置 / 半径 / delay） |
| `BootEnergyPulseSvg` | `src/workbench/bootSvg/BootEnergyPulseSvg.tsx` | BREACH 时三层能量环爆发 | `data-active`；时长对齐 `BREACH_FLASH_MS`（320ms） |
| `ambient-glow.svg` | `src/assets/skins/ambient-glow.svg` | 工作台壁纸后极淡橙/青晕 | `anime-skin.css` 里 `body::after` 的 `opacity` / `background-position` |
| 动画 | `src/workbench/bootRitual.css` | spin / pulse / spark-drift / energy-burst | `@keyframes boot-svg-*`；`prefers-reduced-motion` 全关 |

挂载点：`BootRitual.tsx`（粒子 + 能量环在 HUD 外全窗；准星与 HUD 框在卡片内）。

## 时序（仍约 6–8s，可跳过）

- 幕 1 ≈ 1.7s → 幕 2 等 BREACH（默认 5s 超时自动）→ 色差 + 能量环 320ms → 幕 3 ≈ 1.6s → 幕 4 ≈ 1.5s → 淡出。
- `prefers-reduced-motion: reduce`：去掉旋转 / 粒子漂浮 / 能量爆发，只留静态 SVG 轮廓。

## 怎么再炫一点（仍别砸工作台）

1. 给 `BootParticlesSvg` 加 4–6 个点，或幕 2 时用 CSS 提高 `.boot-svg-particles` opacity。
2. 准星外环 `strokeDasharray` 改成 `2 5` 更「雷达」。
3. BREACH 时短暂把 `.boot-svg-hud-frame` opacity 拉到 1，并给 brackets 加 `stroke-width: 2`。
4. **不要**把能量环 / 扫描弧搬进侧栏或应用列表。

## 自测

```bash
pnpm exec vitest run src/workbench/BootRitual.test.tsx
```

清 `localStorage` 的 `yeschoy-boot*` 可强制重播开机。

> **已替换：** 橙色准星、青芒纹章、闪切光刃均已拆除。现行零件是月华环 / 花瓣 / 极光纱带，详见 [`v2-visual-system.md`](./v2-visual-system.md)。

# BootRitual SVG 月华层

> 在澄镜开机仪式上叠一层**纯 SVG / CSS** 动效。角色立绘仍是占位舞台；月华、纱带、花瓣、绽放环用矢量自己画，可随时调参。

## 校准原则

| 更炫（开机壳）                   | 更克制（工作台）             |
| -------------------------------- | ---------------------------- |
| 三环月华 + 花瓣缓转              | 壁纸作为右侧舞台，不进卡片   |
| 启幕绽放环 + 柔光                | 仅一层 ambient-glow.svg 光球 |
| 花瓣光点随幕次略增亮             | 密钥 / 诊断卡仍高不透明      |
| SKIP / Esc / reduced-motion 可关 | 侧栏 IA 不变                 |

强调色是虹紫 `#6d5cff` 与樱粉 `#ff7eb3`。不要墨黑底板、不要电柠斜切、不要橙色 HUD。

## SVG 零件清单

| 组件 / 资源        | 路径                                       | 作用                     | 调参入口                                               |
| ------------------ | ------------------------------------------ | ------------------------ | ------------------------------------------------------ |
| `BootSigilSvg`     | `src/workbench/bootSvg/BootSigilSvg.tsx`   | 三环、新月、六瓣、中心核 | 环半径；转速见 `boot-svg-spin`                         |
| `BootVeilSvg`      | `src/workbench/bootSvg/BootVeilSvg.tsx`    | 极光纱带 + 舞台光环      | `STAGE` 坐标、`RINGS`                                  |
| `BootMotesSvg`     | `src/workbench/bootSvg/BootMotesSvg.tsx`   | 花瓣与光点               | `MOTES` 数组                                           |
| `BootRefractSvg`   | `src/workbench/bootSvg/BootRefractSvg.tsx` | 启幕时三层绽放环         | `data-active`；时长对齐 `UNVEIL_MS`（520ms）           |
| `ambient-glow.svg` | `src/assets/skins/ambient-glow.svg`        | 工作台右侧虹彩光球       | `anime-skin.css` 里 `body::after`                      |
| 动画               | `src/workbench/bootRitual.css`             | spin / drift / bloom     | `@keyframes boot-svg-*`；`prefers-reduced-motion` 全关 |

挂载点：`BootRitual.tsx`（花瓣与绽放在全窗；月华纹章叠在角色一侧）。

## 时序（约 10s，可跳过）

- 幕 1 ≈ 1.8s → 幕 2 等启幕（默认 4.5s 超时自动）→ 绽放 520ms → 幕 3 ≈ 1.6s → 幕 4 ≈ 1.8s → 淡出。
- `prefers-reduced-motion: reduce`：去掉旋转 / 花瓣漂浮 / 绽放，只留静态 SVG 轮廓。

## 怎么再炫一点（仍别砸工作台）

1. 给 `BootMotesSvg` 加几片花瓣，或幕 2 时提高 `.boot-svg-motes` opacity。
2. 外环 `strokeDasharray` 改密一点，更像月轨。
3. 启幕时短暂把 `.boot-svg-sigil` 的 drop-shadow 拉亮。
4. **不要**把绽放环 / 纱带搬进侧栏或应用列表。

## 自测

```bash
pnpm exec vitest run src/workbench/BootRitual.test.tsx
```

清 `localStorage` 的 `yeschoy-boot*` 可强制重播开机。

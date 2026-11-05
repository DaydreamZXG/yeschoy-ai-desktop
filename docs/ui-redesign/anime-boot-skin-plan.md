> **v2：** 视觉已改为「青芒 · Qingmang」。本文保留产品约束与设置键，外观与开机文案以 [`v2-visual-system.md`](./v2-visual-system.md) 为准。

# 二次元皮肤 + 开机仪式（定案落地）

> 分支目标：在**不改信息架构**的前提下，给客户端盖一层可开关的二次元皮肤，并在冷启动叠四幕暖白赛博开机仪式。默认角色：**大肥鱼**。

## 约束（产品定案）

1. 侧栏仍是：我的应用 / 应用接入 / 用量账单 / 公告 / 设置。密钥、接入、诊断保持可读可信，不被壁纸糊掉。
2. 角色是**壁纸层**，不是业务卡片，不替换应用列表。
3. 不要暗黑赛博底板；暖白 / 浅色毛玻璃 + 开机 HUD 橙强调 `#ff7500`。
4. 正式商业立绘待授权；本 PR 用抽象剪影 / 渐变占位（`src/assets/skins/*.svg`），标注 PLACEHOLDER。

## 分层

```
┌─────────────────────────────────────────┐
│ BootRitual 全窗覆盖（播完 / SKIP 卸载）   │  z 最高
├─────────────────────────────────────────┤
│ 工作台 UI（侧栏 + 各 View，IA 不动）      │
├─────────────────────────────────────────┤
│ 毛玻璃卡片 / 更高圆角（data-skin=anime） │
├─────────────────────────────────────────┤
│ 角色壁纸（右侧淡水印；开机时全幅更亮）    │
└─────────────────────────────────────────┘
```

- `document.documentElement.dataset.skin = "anime" | "standard"`
- `document.documentElement.dataset.skinCharacter = "dafeiyu" | "claude" | "gpt"`
- CSS 变量在 `src/theme/anime-skin.css` 覆盖 tokens（毛玻璃、圆角、壁纸 background-image）。

## 开机四幕

| 幕 | 中文 | 英文 | 交互 |
|----|------|------|------|
| 01 | 神经链路点火 | NEURAL LINK IGNITION | 自动推进 |
| 02 | 穿越奇点 | CROSSING THE SINGULARITY | 按钮「击穿边界 · BREACH」或 Enter / Space |
| 03 | 频率已锁定 | FREQUENCY LOCKED | 轻色散闪一下，自动 |
| 04 | 你已站在 AGI 这一侧 | WELCOME TO THE OTHER SIDE | 淡入工作台后卸载 |

全程 **SKIP / Esc** 可跳过。`prefers-reduced-motion: reduce` 时缩短时序、去掉色散。

## 设置项

- 界面风格：标准 / 二次元（默认二次元）
- 壁纸皮肤：大肥鱼（默认）/ Claude大小姐 / GPT小吉（仅二次元时可选）
- 启用开机动画：开关
- 播放频率：每次会话 / 每天一次 / 总是

偏好键（localStorage）：

- `yeschoy-skin` → `{ mode, character }`
- `yeschoy-boot` → `{ enabled, frequency, lastPlayedDay }`
- 会话内已播：`sessionStorage` 键 `yeschoy-boot-session-played`

## 文件清单

| 路径 | 作用 |
|------|------|
| `docs/ui-redesign/anime-boot-skin-plan.md` | 本方案 |
| `src/workbench/skin.ts` | 皮肤偏好读写 + `data-skin` 挂载 |
| `src/workbench/bootPrefs.ts` | 开机偏好与「该不该播」判定 |
| `src/workbench/BootRitual.tsx` | 四幕覆盖层组件 |
| `src/workbench/bootRitual.css` | 开机 HUD 样式 |
| `src/workbench/bootSvg/*` | SVG 准星 / 粒子 / HUD 框 / BREACH 能量环 |
| `src/assets/skins/ambient-glow.svg` | 工作台极淡光晕 |
| `docs/ui-redesign/boot-svg-fx.md` | SVG 零件与调参说明 |
| `src/theme/anime-skin.css` | 二次元皮 tokens / 壁纸 |
| `src/assets/skins/*` | 壁纸占位 SVG + README |
| `src/App.tsx` | 挂载仪式 + 应用皮肤 |
| `src/settings/SettingsView.tsx` | 设置区块 |
| `src/i18n/locales/zh.json` / `en.json` | 文案 |
| `src/workbench/skin.test.ts` | 偏好 / data 属性 |
| `src/workbench/bootPrefs.test.ts` | 频率判定 |
| `src/workbench/BootRitual.test.tsx` | skip / BREACH |

## 本地预览

```bash
pnpm install
pnpm exec vitest run src/workbench/skin.test.ts src/workbench/bootPrefs.test.ts src/workbench/BootRitual.test.tsx
pnpm tauri dev   # 或 pnpm exec vite
```

清除 `localStorage` 的 `yeschoy-boot*` / `yeschoy-skin` 可强制再看开机。

## 已知缺口

- 立绘授权未落地：占位 SVG，正式发版前替换。
- 壁纸未做真正动态（Wallpaper Engine 级）；本版静态 + CSS 微动 + SVG HUD 即可。详见 `boot-svg-fx.md`。
- 不 merge / 不 deploy 生产；由 PR 审阅。

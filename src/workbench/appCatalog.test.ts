import { describe, expect, it } from "vitest";
import { COMING_SOON_APPS, WORKBENCH_APPS } from "./appCatalog";
import { ACTIVATION_TOOL_IDS } from "../configuration/activation";
import { TOOL_CATALOG } from "../tool-discovery/contract";
import en from "../i18n/locales/en.json";
import zh from "../i18n/locales/zh.json";

/**
 * #9 工具清单单源强校验（PRD §3.2 定案，2026-09-13）。
 *
 * 事实源分工：
 * - `appCatalog.ts` 是 UI 展示清单的唯一事实源（V1 可接入 + 即将支持）；
 * - `activation.ts ACTIVATION_TOOL_IDS` 是 Rust 扫描协议契约（含遗留工具）；
 * - `tool-discovery/contract.ts TOOL_CATALOG` 是只读发现协议契约。
 *
 * 任何一处增删工具都会被这里的断言拦下，防止三处清单再次漂移
 * （批次 0 复盘：Hermes/OpenClaw 超 PRD 适配器混入三处清单）。
 */
describe("tool catalog single source of truth (#9)", () => {
  const LEGACY_TOOL_IDS = ["hermes", "openclaw"] as const;

  it("lists every V1 app with a unique, activation-compatible id", () => {
    const ids = WORKBENCH_APPS.map((app) => app.id);
    expect(new Set(ids).size).toBe(ids.length);
    for (const id of ids) expect(ACTIVATION_TOOL_IDS).toContain(id);
  });

  it("keeps V1 scope aligned with PRD 3.2 (no hermes/openclaw)", () => {
    const ids = WORKBENCH_APPS.map((app) => app.id);
    for (const legacy of LEGACY_TOOL_IDS) expect(ids).not.toContain(legacy);
  });

  it("exposes coming-soon apps without an activation adapter", () => {
    expect(COMING_SOON_APPS.length).toBeGreaterThan(0);
    for (const app of COMING_SOON_APPS)
      expect(ACTIVATION_TOOL_IDS).not.toContain(app.id);
  });

  it("keeps the activation protocol aligned with the V1 display list", () => {
    // 2026-09-14：Rust 侧适配器已清理遗留工具，协议层与展示层必须完全
    // 一致（6 个）。新增工具必须先进 appCatalog（展示源），再同步协议
    // 与 Rust；任何一处漂移都会被此断言拦截。
    const outsideV1 = ACTIVATION_TOOL_IDS.filter(
      (id) => !WORKBENCH_APPS.some((app) => app.id === id),
    );
    expect(outsideV1).toEqual([]);
  });

  // 应用说明从 `appCatalog.ts` 搬进了语言文件（同一段文字要中英两份，
  // 留在数据文件里就只能有一份）。这个文件本来就是防清单漂移的，
  // 说明与清单的漂移也归它管：新增应用忘了补说明，这里当场红。
  it("gives every listed app a description in both languages", () => {
    for (const app of [...WORKBENCH_APPS, ...COMING_SOON_APPS]) {
      expect(zh.yeschoyCatalog.appDescription).toHaveProperty(app.id);
      expect(en.yeschoyCatalog.appDescription).toHaveProperty(app.id);
    }
  });

  it("keeps read-only discovery able to see coming-soon apps", () => {
    const discoveryIds: string[] = TOOL_CATALOG.map((tool) => tool.id);
    for (const app of COMING_SOON_APPS) expect(discoveryIds).toContain(app.id);
    // 2026-09-14：Rust 侧只读发现已清理遗留工具，协议层不再含它们。
    const legacyInDiscovery = LEGACY_TOOL_IDS.filter((id) =>
      discoveryIds.includes(id),
    );
    expect(legacyInDiscovery).toEqual([]);
  });
});

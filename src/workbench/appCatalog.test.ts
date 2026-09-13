import { describe, expect, it } from "vitest";
import { COMING_SOON_APPS, WORKBENCH_APPS } from "./appCatalog";
import { ACTIVATION_TOOL_IDS } from "../configuration/activation";
import { TOOL_CATALOG } from "../tool-discovery/contract";

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
    for (const id of ids)
      expect(ACTIVATION_TOOL_IDS).toContain(id);
  });

  it("keeps V1 scope aligned with PRD 3.2 (no hermes/openclaw)", () => {
    const ids = WORKBENCH_APPS.map((app) => app.id);
    for (const legacy of LEGACY_TOOL_IDS)
      expect(ids).not.toContain(legacy);
  });

  it("exposes coming-soon apps without an activation adapter", () => {
    expect(COMING_SOON_APPS.length).toBeGreaterThan(0);
    for (const app of COMING_SOON_APPS)
      expect(ACTIVATION_TOOL_IDS).not.toContain(app.id);
  });

  it("reserves activation protocol ids outside V1 for legacy tools only", () => {
    // 协议层比展示层多出的 id 只能是遗留工具。新增工具必须先进
    // appCatalog（展示源），再同步协议；反之则会被此断言拦截。
    const outsideV1 = ACTIVATION_TOOL_IDS.filter(
      (id) => !WORKBENCH_APPS.some((app) => app.id === id),
    );
    expect([...outsideV1].sort()).toEqual([...LEGACY_TOOL_IDS]);
  });

  it("keeps read-only discovery able to see coming-soon apps", () => {
    const discoveryIds = TOOL_CATALOG.map((tool) => tool.id);
    for (const app of COMING_SOON_APPS)
      expect(discoveryIds).toContain(app.id);
    // 展示过滤（App.tsx V1_DISCOVERY_TOOLS）摘除的只是遗留工具。
    const legacyInDiscovery = LEGACY_TOOL_IDS.filter((id) =>
      discoveryIds.includes(id),
    );
    expect(legacyInDiscovery).toEqual([...LEGACY_TOOL_IDS]);
  });
});

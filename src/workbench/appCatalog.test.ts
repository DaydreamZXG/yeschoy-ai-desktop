import { describe, expect, it } from "vitest";
import { COMING_SOON_APPS, WORKBENCH_APPS, appsForHome } from "./appCatalog";
import { ACTIVATION_TOOL_IDS } from "../configuration/activation";

/**
 * #9 工具清单单源强校验（PRD §3.2 定案，2026-09-13）。
 *
 * 事实源分工：
 * - `appCatalog.ts` 是 UI 展示清单的唯一事实源（V1 可接入 + 即将支持）；
 * - `activation.ts ACTIVATION_TOOL_IDS` 是 Rust 扫描协议契约（含遗留工具）。
 *
 * 任何一处增删工具都会被这里的断言拦下，防止两处清单再次漂移
 * （批次 0 复盘：Hermes/OpenClaw 超 PRD 适配器混入三处清单）。
 *
 * 2026-11-22：Hermes Agent 和 OpenClaw 正式加入（有适配器、有本地转发、有
 * 改回原样），不再算遗留。
 */
describe("tool catalog single source of truth (#9)", () => {
  it("lists every V1 app with a unique, activation-compatible id", () => {
    const ids = WORKBENCH_APPS.map((app) => app.id);
    expect(new Set(ids).size).toBe(ids.length);
    for (const id of ids) expect(ACTIVATION_TOOL_IDS).toContain(id);
  });

  it("lists exactly the apps the native side can connect", () => {
    const ids = WORKBENCH_APPS.map((app) => app.id);
    expect([...ids].sort()).toEqual([...ACTIVATION_TOOL_IDS].sort());
  });

  it("exposes coming-soon apps without an activation adapter", () => {
    // OpenCode 已正式接入；名单可以为空，但里面的应用一律不能有适配器。
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

  it("常用的露在外面，不常用的收着", () => {
    expect(
      WORKBENCH_APPS.filter((app) => app.common).map((app) => app.id),
    ).toEqual([
      "claude_desktop",
      "codex_desktop",
      "opencode",
      "claude_code",
      "workbuddy",
      "pi",
    ]);
  });

  it("首页那一排：接过的排前面，然后是常用的", () => {
    expect(appsForHome([], 5).map((app) => app.id)).toEqual([
      "claude_desktop",
      "codex_desktop",
      "opencode",
      "claude_code",
      "workbuddy",
    ]);
    expect(
      appsForHome(["dsh_web", "claude_code"], 5).map((app) => app.id),
    ).toEqual([
      "claude_code",
      "dsh_web",
      "claude_desktop",
      "codex_desktop",
      "opencode",
    ]);
  });
});

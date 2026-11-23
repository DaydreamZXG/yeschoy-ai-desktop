import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const invoke = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/core", () => ({ invoke }));

import {
  handoffEnabled,
  readHandoffScreen,
  setHandoffEnabled,
  withHandoff,
} from "./handoff";

const agent = (value: string) =>
  vi.spyOn(navigator, "userAgent", "get").mockReturnValue(value);
const WINDOWS = "Mozilla/5.0 (Windows NT 10.0; Win64; x64)";

describe("打开应用之前先登记由谁登场", () => {
  beforeEach(() => {
    invoke.mockReset().mockResolvedValue({ status: "armed" });
    localStorage.clear();
  });
  afterEach(() => vi.restoreAllMocks());

  it("Windows 上：先登记，做完再清掉，结果原样交回", async () => {
    agent(WINDOWS);
    const order: string[] = [];
    invoke.mockImplementation(async (command: string) => {
      order.push(command);
    });
    const result = await withHandoff("codex_desktop", "ds", async () => {
      order.push("run");
      return "done";
    });
    expect(result).toBe("done");
    expect(order).toEqual([
      "app_handoff_arm_v1",
      "run",
      "app_handoff_disarm_v1",
    ]);
    expect(invoke).toHaveBeenCalledWith("app_handoff_arm_v1", {
      request: { toolId: "codex_desktop", who: "ds" },
    });
  });

  it("那次操作失败了也清掉登记，错误照样抛出去", async () => {
    agent(WINDOWS);
    await expect(
      withHandoff("codex_desktop", "ds", async () => {
        throw new Error("boom");
      }),
    ).rejects.toThrow("boom");
    expect(invoke).toHaveBeenLastCalledWith("app_handoff_disarm_v1");
  });

  it("登记没成也照常做", async () => {
    agent(WINDOWS);
    invoke.mockRejectedValue(new Error("no backend"));
    expect(await withHandoff("codex_desktop", "ds", async () => 1)).toBe(1);
  });

  it("别的系统、用户关了、系统要求少动效：都不登记", async () => {
    agent("Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7)");
    await withHandoff("codex_desktop", "ds", async () => 1);

    agent(WINDOWS);
    setHandoffEnabled(false);
    expect(handoffEnabled()).toBe(false);
    await withHandoff("codex_desktop", "ds", async () => 1);

    setHandoffEnabled(true);
    expect(handoffEnabled()).toBe(true);
    vi.stubGlobal("matchMedia", () => ({ matches: true }));
    await withHandoff("codex_desktop", "ds", async () => 1);
    vi.unstubAllGlobals();

    expect(invoke).not.toHaveBeenCalled();
  });
});

describe("后端发来的屏幕位置", () => {
  const good = {
    from: [10, 20, 300, 200],
    to: [0, 0, 1200, 800],
    fadeMs: 260,
    holdMs: 650,
    growMs: 720,
  };

  it("形状对的原样收下", () => {
    expect(readHandoffScreen(good)).toEqual(good);
  });

  it("形状不对的当没收到", () => {
    for (const bad of [
      null,
      "x",
      {},
      { ...good, from: [1, 2, 3] },
      { ...good, to: [1, 2, 3, Number.NaN] },
      { ...good, to: [1, 2, 3, "4"] },
      { ...good, growMs: -1 },
      { ...good, growMs: 60_000 },
      { ...good, fadeMs: undefined },
    ]) {
      expect(readHandoffScreen(bad)).toBeNull();
    }
  });
});

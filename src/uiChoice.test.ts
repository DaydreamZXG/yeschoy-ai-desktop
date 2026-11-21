import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const load = () => import("./uiChoice");

describe("新旧两套界面之间怎么换", () => {
  const reload = vi.fn();

  beforeEach(() => {
    sessionStorage.clear();
    vi.resetModules();
    reload.mockClear();
    vi.stubGlobal("location", { ...window.location, reload });
  });
  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it("打开客户端看到的是新界面，开机动画照放", async () => {
    const ui = await load();
    expect(ui.readUiChoice()).toBe("kaifan");
    expect(ui.RETURNING).toBe(false);
    expect(ui.CLASSIC_START).toEqual({ view: null, app: null });
  });

  it("去旧界面的某一页：记下去哪页、哪个应用，然后整页重载", async () => {
    (await load()).openClassic("setup", "codex_desktop");
    expect(reload).toHaveBeenCalledTimes(1);

    vi.resetModules();
    const ui = await load();
    expect(ui.readUiChoice()).toBe("classic");
    expect(ui.CLASSIC_START).toEqual({ view: "setup", app: "codex_desktop" });
  });

  it("去哪页只用一次：旧界面里再刷新，不会又被拽回那一页", async () => {
    (await load()).openClassic("account");
    vi.resetModules();
    await load();
    vi.resetModules();
    expect((await load()).CLASSIC_START).toEqual({ view: null, app: null });
  });

  it("被乱写的页名当没写", async () => {
    sessionStorage.setItem("kaifan.ui", "classic");
    sessionStorage.setItem("kaifan.classicView", "nowhere");
    expect((await load()).CLASSIC_START.view).toBeNull();
  });

  it("从旧界面回来：回到新界面，而且不再放一遍开机动画", async () => {
    (await load()).openClassic("settings");
    vi.resetModules();
    (await load()).openKaifan();
    expect(reload).toHaveBeenCalledTimes(2);

    vi.resetModules();
    const ui = await load();
    expect(ui.readUiChoice()).toBe("kaifan");
    expect(ui.RETURNING).toBe(true);

    vi.resetModules();
    expect((await load()).RETURNING).toBe(false);
  });
});

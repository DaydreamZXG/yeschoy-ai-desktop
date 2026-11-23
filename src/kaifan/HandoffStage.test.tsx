import { act, fireEvent, render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

const invoke = vi.hoisted(() => vi.fn());
const heard = vi.hoisted(
  () => new Map<string, (message: { payload: unknown }) => void>(),
);
vi.mock("@tauri-apps/api/core", () => ({ invoke }));
vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(
    async (event: string, handler: (message: { payload: unknown }) => void) => {
      heard.set(event, handler);
      return () => heard.delete(event);
    },
  ),
}));

import "../i18n";
import { HandoffStage, emptyScreen, handoffCast } from "./HandoffStage";

const say = (event: string, payload: unknown = null) =>
  act(() => heard.get(event)?.({ payload }));
const box = () => {
  const { left, top, width, height } =
    screen.getByTestId("handoff-screen").style;
  return [left, top, width, height];
};
const SCREEN = {
  from: [600, 250, 400, 250],
  to: [100, 80, 1200, 750],
  fadeMs: 260,
  holdMs: 650,
  growMs: 720,
};

describe("地址里说由谁登场", () => {
  it("没带 handoff 的是平常那个窗口", () => {
    expect(handoffCast("")).toBeNull();
    expect(handoffCast("?s=signed_in")).toBeNull();
  });

  it("认得的角色原样用，认不出的按转校生", () => {
    expect(handoffCast("?handoff=codex_desktop&who=gpt")).toEqual({
      toolId: "codex_desktop",
      who: "gpt",
    });
    for (const odd of ["&who=nobody", "&who=__proto__", ""]) {
      expect(handoffCast(`?handoff=codex_desktop${odd}`)?.who).toBe("new");
    }
  });
});

describe("空屏幕摆在哪", () => {
  it("偏右居中、16:10，最宽四成、最高一半", () => {
    expect(emptyScreen(1920, 1080)).toEqual([844.8, 300, 768, 480]);
    // 又窄又高的屏幕：按宽度收。竖着放不下的：按高度收。
    const [, , wide, tall] = emptyScreen(3440, 600);
    expect([wide, tall]).toEqual([480, 300]);
  });
});

describe("暗场那一页", () => {
  beforeEach(() => {
    invoke.mockReset().mockResolvedValue(null);
    heard.clear();
    vi.stubGlobal("requestAnimationFrame", (run: () => void) => {
      run();
      return 0;
    });
  });

  it("听好了、画出来了，才告诉后端可以叫应用了", async () => {
    render(<HandoffStage toolId="codex_desktop" who="ds" />);
    expect(invoke).not.toHaveBeenCalled();
    await act(async () => {});
    expect(heard.size).toBe(3);
    expect(invoke).toHaveBeenCalledWith("app_handoff_ready_v1");
    expect(screen.getByRole("status").textContent).toContain("Codex");
  });

  it("屏幕框先到画面出现的位置，放大时走到应用的位置，最后淡掉", async () => {
    render(<HandoffStage toolId="codex_desktop" who="ds" />);
    await act(async () => {});
    const stage = screen.getByTestId("handoff");
    expect(stage.className).toContain("ho-wait");

    say("handoff://screen", SCREEN);
    expect(stage.className).toContain("ho-show");
    expect(box()).toEqual(["600px", "250px", "400px", "250px"]);

    say("handoff://land");
    expect(stage.className).toContain("ho-land");
    expect(box()).toEqual(["100px", "80px", "1200px", "750px"]);

    say("handoff://leave");
    expect(stage.className).toContain("ho-leave");
    // 淡出的时候框不往回跳。
    expect(box()).toEqual(["100px", "80px", "1200px", "750px"]);
  });

  it("后端发来的位置形状不对：不动，照常等撤场", async () => {
    render(<HandoffStage toolId="codex_desktop" who="ds" />);
    await act(async () => {});
    const before = box();
    say("handoff://screen", { from: [1, 2], to: "x" });
    expect(screen.getByTestId("handoff").className).toContain("ho-wait");
    expect(box()).toEqual(before);
  });

  it("点一下或按任意键就跳过", async () => {
    render(<HandoffStage toolId="codex_desktop" who="ds" />);
    await act(async () => {});
    fireEvent.keyDown(window, { key: "a" });
    expect(invoke).toHaveBeenCalledWith("app_handoff_end_v1");
    expect(screen.getByTestId("handoff").className).toContain("ho-leave");

    invoke.mockClear();
    fireEvent.pointerDown(window);
    expect(invoke).toHaveBeenCalledWith("app_handoff_end_v1");
  });
});

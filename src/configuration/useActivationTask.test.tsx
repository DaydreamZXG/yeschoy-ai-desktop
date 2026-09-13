import { act, renderHook } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import {
  activateDesktopTool,
  cancelDesktopToolActivation,
  type ToolActivationProjection,
} from "./activation";
import { useActivationTask, type ActivationContext } from "./useActivationTask";

vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(async () => () => {}),
}));
vi.mock("./activation", async (original) => ({
  ...(await original<typeof import("./activation")>()),
  activateDesktopTool: vi.fn(),
  cancelDesktopToolActivation: vi.fn(),
}));
const activate = vi.mocked(activateDesktopTool);
const cancel = vi.mocked(cancelDesktopToolActivation);
const input = {
  toolId: "codex_desktop" as const,
  modelId: "model-a",
  billingGroup: "group-a",
  lineId: "mainland_optimized" as const,
  installationId: "fixture-install",
};
const context: ActivationContext = {
  key: "first",
  account: "fixture",
  app: "Codex Desktop",
  model: "model-a",
  group: "group-a",
  line: "mainland_optimized",
};
const ready: ToolActivationProjection = {
  requestId: "fixture-1",
  schemaVersion: 3,
  toolId: "codex_desktop",
  modelId: "model-a",
  billingGroup: "group-a",
  status: "ready",
  reasonCode: "configuration_ready",
  observedAtEpochMs: 1,
};

beforeEach(() => {
  activate.mockReset();
  cancel.mockReset();
});

describe("activation task ownership", () => {
  it("keeps the active request through reset/navigation and rejects immediate duplicate clicks", async () => {
    let finish!: (r: ToolActivationProjection) => void;
    activate.mockImplementation((i) => {
      i.onRequestId?.("fixture-1");
      return new Promise((resolve) => {
        finish = resolve;
      });
    });
    cancel.mockResolvedValue("cancel_requested");
    const completed = vi.fn();
    const { result, rerender } = renderHook(() => useActivationTask(completed));
    let first!: ReturnType<typeof result.current.run>;
    await act(async () => {
      first = result.current.run(input, context);
      expect(
        await result.current.run(input, { ...context, key: "duplicate" }),
      ).toBeUndefined();
    });
    act(() => result.current.reset());
    rerender();
    expect(result.current.phase).toBe("applying");
    expect(result.current.context).toEqual(context);
    await act(async () => {
      await result.current.cancel();
    });
    expect(cancel).toHaveBeenCalledWith("fixture-1");
    expect(activate).toHaveBeenCalledTimes(1);
    await act(async () => {
      finish(ready);
      await first;
    });
    expect(result.current.result).toEqual(ready);
    expect(result.current.phase).toBe("finished");
    expect(completed).toHaveBeenCalledTimes(1);
  });

  it("shows a retryable cancel failure without clearing the active task", async () => {
    let finish!: (r: ToolActivationProjection) => void;
    activate.mockImplementation((i) => {
      i.onRequestId?.("fixture-1");
      return new Promise((resolve) => {
        finish = resolve;
      });
    });
    cancel
      .mockRejectedValueOnce(new Error("private native detail"))
      .mockResolvedValue("cancel_requested");
    const { result } = renderHook(() => useActivationTask(vi.fn()));
    let run!: ReturnType<typeof result.current.run>;
    act(() => {
      run = result.current.run(input, context);
    });
    await act(async () => {
      await result.current.cancel();
    });
    expect(result.current.cancelFailed).toBe(true);
    expect(result.current.cancelRequested).toBe(false);
    expect(result.current.phase).toBe("applying");
    await act(async () => {
      await result.current.cancel();
    });
    expect(result.current.cancelFailed).toBe(false);
    expect(result.current.cancelRequested).toBe(true);
    await act(async () => {
      finish(ready);
      await run;
    });
  });

  it("does not let an old cancellation reply change a newer task", async () => {
    const finishes: Array<(r: ToolActivationProjection) => void> = [];
    activate.mockImplementation((i) => {
      i.onRequestId?.(`fixture-${finishes.length + 1}`);
      return new Promise((resolve) => finishes.push(resolve));
    });
    let finishCancel!: (r: "not_found") => void;
    cancel.mockImplementation(
      () =>
        new Promise((resolve) => {
          finishCancel = resolve;
        }),
    );
    const { result } = renderHook(() => useActivationTask(vi.fn()));
    let first!: ReturnType<typeof result.current.run>;
    act(() => {
      first = result.current.run(input, context);
    });
    let cancelling!: ReturnType<typeof result.current.cancel>;
    act(() => {
      cancelling = result.current.cancel();
    });
    await act(async () => {
      finishes[0](ready);
      await first;
    });
    let second!: ReturnType<typeof result.current.run>;
    act(() => {
      second = result.current.run(input, { ...context, key: "second" });
    });
    await act(async () => {
      finishCancel("not_found");
      await cancelling;
    });
    expect(result.current.cancelFailed).toBe(false);
    expect(result.current.context?.key).toBe("second");
    expect(result.current.phase).toBe("applying");
    await act(async () => {
      finishes[1](ready);
      await second;
    });
  });

  it("projects a safe error and allows a new attempt without leaking raw details", async () => {
    activate
      .mockRejectedValueOnce(new Error("secret-token https://private-host"))
      .mockResolvedValue(ready);
    const { result } = renderHook(() => useActivationTask(vi.fn()));
    await act(async () => {
      await result.current.run(input, context);
    });
    expect(result.current.result?.reasonCode).toBe("invalid_response");
    expect(JSON.stringify(result.current.result)).not.toContain("secret-token");
    act(() => result.current.reset());
    expect(result.current.phase).toBe("idle");
    await act(async () => {
      await result.current.run(input, context);
    });
    expect(result.current.result?.status).toBe("ready");
  });

  it("does not publish completion to an unmounted owner", async () => {
    let finish!: (r: ToolActivationProjection) => void;
    activate.mockImplementation(
      () =>
        new Promise((resolve) => {
          finish = resolve;
        }),
    );
    const completed = vi.fn();
    const { result, unmount } = renderHook(() => useActivationTask(completed));
    let run!: ReturnType<typeof result.current.run>;
    act(() => {
      run = result.current.run(input, context);
    });
    unmount();
    await act(async () => {
      finish(ready);
      await run;
    });
    expect(completed).not.toHaveBeenCalled();
  });
});

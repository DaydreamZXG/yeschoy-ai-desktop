import {
  act,
  cleanup,
  fireEvent,
  render,
  renderHook,
  screen,
  within,
} from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import {
  ConnectionProvider,
  decodeConnections,
  useToolConnections,
  type ToolConnection,
} from "./connections";
import { connectionsFixture } from "./connection-test-fixtures";
import { RestoreConnection } from "./RestoreConnection";
import { OPEN_REQUEST_DEADLINE_MS } from "./launchApi";
vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
const native = vi.mocked(invoke);
beforeEach(() => {
  native.mockReset();
  native.mockImplementation(async (_cmd, args) =>
    connectionsFixture(
      (args as { request: { requestId: string } }).request.requestId,
    ),
  );
  HTMLDialogElement.prototype.showModal = function () {
    this.setAttribute("open", "");
  };
  HTMLDialogElement.prototype.close = function () {
    this.removeAttribute("open");
  };
});
afterEach(cleanup);
describe("reversible local connections", () => {
  it("ru042 decodes only secret-free latest request and model bindings", () => {
    const base = connectionsFixture("one");
    const value = {
      ...base,
      schemaVersion: 2,
      connections: base.connections.map((c) => ({ ...c, models: [] })),
    };
    const observation = {
      modelId: "gpt-6-astra",
      billingGroup: "special",
      lineId: "global_accelerated",
      outcome: "timeout",
      httpStatus: 504,
      observedAtEpochMs: 1000,
    };
    Object.assign(value.connections[0], {
      models: [{ modelId: "gpt-6-astra", billingGroup: "special" }],
      lastRequest: observation,
    });
    expect(decodeConnections(value, "one")).not.toBeNull();
    for (const lastRequest of [
      { ...observation, apiKey: "synthetic-secret" },
      { ...observation, outcome: "raw provider message" },
      { ...observation, httpStatus: 999 },
      { ...observation, lineId: "untrusted" },
    ]) {
      Object.assign(value.connections[0], { lastRequest });
      expect(decodeConnections(value, "one")).toBeNull();
    }
  });
  it("rejects secret fields, duplicate tools and mismatched replies", () => {
    const valid = connectionsFixture("one");
    expect(decodeConnections(valid, "one")).not.toBeNull();
    expect(
      decodeConnections({ ...valid, apiKey: "must-not-render" }, "one"),
    ).toBeNull();
    expect(decodeConnections(valid, "other")).toBeNull();
    const duplicate = connectionsFixture("one");
    duplicate.connections[1] = duplicate.connections[0];
    expect(decodeConnections(duplicate, "one")).toBeNull();
    const invalid = connectionsFixture("one");
    invalid.connections[0].modelId = "injected\nline";
    expect(decodeConnections(invalid, "one")).toBeNull();
  });
  it("loads local state without requiring login and restores only the requested tool", async () => {
    const { result } = renderHook(() => useToolConnections());
    await act(async () => {});
    expect(result.current.connections).toHaveLength(7);
    expect(result.current.error).toBe(false);
    await act(async () => {
      await result.current.restore("pi");
    });
    expect(native).toHaveBeenLastCalledWith("manage_tool_connections_v1", {
      request: {
        requestId: expect.any(String),
        operation: "restore",
        toolId: "pi",
      },
    });
    expect(
      native.mock.calls.every(
        (call) => call[0] === "manage_tool_connections_v1",
      ),
    ).toBe(true);
  });
  it("retains the last local state when reading it fails", async () => {
    const { result } = renderHook(() => useToolConnections());
    await act(async () => {});
    native.mockRejectedValueOnce(Error("unavailable"));
    await act(async () => {
      await result.current.refresh();
    });
    expect(result.current.connections).toHaveLength(7);
    expect(result.current.error).toBe(true);
    expect(result.current.loading).toBe(false);
  });
  it("keeps the last usable projection interactive during a background refresh", async () => {
    const { result } = renderHook(() => useToolConnections());
    await act(async () => {});
    let finish!: (value: unknown) => void;
    native.mockImplementationOnce(
      async () =>
        await new Promise((resolve) => {
          finish = resolve;
        }),
    );
    let refresh!: Promise<void>;
    act(() => {
      refresh = result.current.refresh();
    });
    expect(result.current.connections).toHaveLength(7);
    expect(result.current.loading).toBe(false);
    await act(async () => {
      finish(connectionsFixture("wrong-stale-id"));
      await refresh;
    });
    expect(result.current.loading).toBe(false);
    expect(result.current.connections).toHaveLength(7);
  });
  it("releases the launch busy state when a native open reply is lost", async () => {
    const { result } = renderHook(() => useToolConnections());
    await act(async () => {});
    native.mockImplementationOnce(async () => await new Promise(() => {}));
    vi.useFakeTimers();
    try {
      let opening!: Promise<unknown>;
      act(() => {
        opening = result.current.open("codex_desktop").catch((cause) => cause);
      });
      expect(result.current.opening).toBe("codex_desktop");
      let outcome: unknown;
      await act(async () => {
        await vi.advanceTimersByTimeAsync(OPEN_REQUEST_DEADLINE_MS);
        outcome = await opening;
      });
      expect(outcome).toEqual(Error("open_request_timed_out"));
      expect(result.current.opening).toBeNull();
      expect(result.current.loading).toBe(false);
    } finally {
      vi.useRealTimers();
    }
  });
  it.each([false, true])(
    "settles loading after restore (failure: %s) supersedes a pending refresh",
    async (failRestore) => {
      const { result } = renderHook(() => useToolConnections());
      await act(async () => {});
      let finishRead!: (value: unknown) => void;
      let finishRestore!: (value: unknown) => void;
      let rejectRestore!: (cause: Error) => void;
      let readId = "";
      let restoreId = "";
      native.mockClear();
      native.mockImplementation(async (_command, args) => {
        const request = (
          args as {
            request: { requestId: string; operation: string };
          }
        ).request;
        if (request.operation === "inspect") {
          readId = request.requestId;
          return await new Promise((resolve) => {
            finishRead = resolve;
          });
        }
        restoreId = request.requestId;
        return await new Promise((resolve, reject) => {
          finishRestore = resolve;
          rejectRestore = reject;
        });
      });
      let read!: Promise<void>;
      let restoring!: Promise<unknown>;
      act(() => {
        read = result.current.refresh();
      });
      expect(result.current.loading).toBe(false);
      act(() => {
        restoring = result.current.restore("pi").catch(() => undefined);
      });
      await act(async () => {
        window.dispatchEvent(new Event("focus"));
      });
      expect(native).toHaveBeenCalledTimes(2);
      await act(async () => {
        if (failRestore) rejectRestore(Error("restore response unavailable"));
        else
          finishRestore({
            ...connectionsFixture(restoreId),
            status: "restored",
          });
        await restoring;
      });
      expect(result.current.loading).toBe(false);
      expect(result.current.restoring).toBeNull();
      expect(result.current.error).toBe(failRestore);
      const stale = connectionsFixture(readId);
      stale.connections.find((c) => c.toolId === "pi")!.modelId =
        "stale-before-restore";
      await act(async () => {
        finishRead(stale);
        await read;
      });
      expect(result.current.loading).toBe(false);
      expect(
        result.current.connections.find((c) => c.toolId === "pi")?.modelId,
      ).toBe("");
      expect(result.current.error).toBe(failRestore);
    },
  );
  it.each(["original", "remove_yeschoy"] as const)(
    "confirms %s recovery, preserves the cancel path and describes its limit",
    async (mode) => {
      const connection: ToolConnection = {
        ...connectionsFixture("one").connections[0],
        state: mode === "original" ? "connected" : "legacy",
        restoreMode: mode,
        modelId: "glm-5.3",
      };
      const restore = vi.fn().mockResolvedValue({
        ...connectionsFixture("one"),
        status: "restored_with_changes",
      });
      render(
        <ConnectionProvider
          value={{
            connections: [connection],
            loading: false,
            error: false,
            restoring: null,
            opening: null,
            open: vi.fn(),
            refresh: vi.fn(),
            restore,
          }}
        >
          <RestoreConnection connection={connection} name="Claude Code" />
        </ConnectionProvider>,
      );
      const label = mode === "original" ? "恢复原设置" : "撤销野菜设置";
      fireEvent.click(screen.getByRole("button", { name: label }));
      const dialog = screen.getByRole("dialog");
      expect(
        within(dialog).getByRole("button", { name: "先不恢复" }),
      ).toHaveFocus();
      expect(restore).not.toHaveBeenCalled();
      if (mode === "remove_yeschoy")
        expect(dialog).toHaveTextContent("无法找回原来的值");
      fireEvent.click(within(dialog).getByRole("button", { name: "先不恢复" }));
      expect(restore).not.toHaveBeenCalled();
      fireEvent.click(screen.getByRole("button", { name: label }));
      await act(async () => {
        fireEvent.click(
          within(screen.getByRole("dialog")).getByRole("button", {
            name: label,
          }),
        );
      });
      expect(restore).toHaveBeenCalledWith("claude_code");
      expect(screen.getByRole("status")).toHaveTextContent(
        "你之后修改的内容已保留",
      );
    },
  );
});

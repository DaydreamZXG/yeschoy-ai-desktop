import {
  act,
  cleanup,
  fireEvent,
  render,
  renderHook,
  screen,
  within,
  waitFor,
} from "@testing-library/react";
import { StrictMode } from "react";
import { onlineManager } from "@tanstack/react-query";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import {
  CONNECTION_INSPECTION_DEADLINE_MS,
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
afterEach(() => {
  vi.useRealTimers();
  cleanup();
});
describe("reversible local connections", () => {
  it("ru076 deduplicates repeated focus and manual reads including effect replay", async () => {
    let finish!: (value: unknown) => void;
    let requestId = "";
    native.mockImplementationOnce(async (_command, args) => {
      requestId = (args as { request: { requestId: string } }).request
        .requestId;
      return new Promise((resolve) => {
        finish = resolve;
      });
    });
    const { result } = renderHook(() => useToolConnections(), {
      wrapper: StrictMode,
    });
    let first!: Promise<void>;
    let second!: Promise<void>;
    act(() => {
      window.dispatchEvent(new Event("focus"));
      window.dispatchEvent(new Event("focus"));
      first = result.current.refresh();
      second = result.current.refresh();
    });
    expect(native).toHaveBeenCalledTimes(1);
    await act(async () => {
      finish(connectionsFixture(requestId));
      await Promise.all([first, second]);
    });
    await waitFor(() => expect(result.current.connections).toHaveLength(5));
    expect(result.current.loading).toBe(false);
  });
  it.each([
    ["connection_operation_busy", "connection_operation_busy"],
    [Error("assistant_shutting_down"), "assistant_shutting_down"],
    [
      Error("/Users/private/secret-key-do-not-display"),
      "connection_call_failed",
    ],
  ])(
    "ru076 exposes only safe error codes and our request ID: %s",
    async (cause, expected) => {
      native.mockRejectedValueOnce(cause);
      const { result } = renderHook(() => useToolConnections());
      await waitFor(() => expect(result.current.error).toBe(true));
      expect(result.current.errorInfo?.code).toBe(expected);
      expect(result.current.errorInfo?.requestId).toMatch(
        /^connections-[a-z0-9]+-\d+$/,
      );
      expect(result.current.errorInfo?.message).not.toMatch(
        /private|secret-key/,
      );
      expect(result.current.connections).toEqual([]);
      expect(result.current.loading).toBe(false);
      await act(async () => {
        await result.current.refresh();
      });
      await waitFor(() => expect(result.current.connections).toHaveLength(5));
      expect(result.current.error).toBe(false);
      expect(result.current.errorInfo).toBeUndefined();
    },
  );
  it("ru076 still inspects local settings when the browser reports offline", async () => {
    onlineManager.setOnline(false);
    try {
      const { result } = renderHook(() => useToolConnections());
      await waitFor(() => expect(result.current.connections).toHaveLength(5));
      expect(native).toHaveBeenCalledTimes(1);
    } finally {
      onlineManager.setOnline(true);
    }
  });
  it("ru076 shares an in-flight background refresh without dropping the successful snapshot", async () => {
    const { result } = renderHook(() => useToolConnections());
    await waitFor(() => expect(result.current.connections).toHaveLength(5));
    let finish!: (value: unknown) => void;
    let requestId = "";
    native.mockClear();
    native.mockImplementationOnce(async (_command, args) => {
      requestId = (args as { request: { requestId: string } }).request
        .requestId;
      return new Promise((resolve) => {
        finish = resolve;
      });
    });
    let refresh!: Promise<void>;
    act(() => {
      refresh = result.current.refresh();
      window.dispatchEvent(new Event("focus"));
      void result.current.refresh();
    });
    expect(native).toHaveBeenCalledTimes(1);
    expect(result.current.connections).toHaveLength(5);
    expect(result.current.loading).toBe(false);
    await act(async () => {
      finish(connectionsFixture(requestId));
      await refresh;
    });
    await waitFor(() => expect(result.current.refreshing).toBe(false));
  });
  it("ru076 treats an invalid response as unknown, not an empty successful snapshot", async () => {
    native.mockResolvedValueOnce({ requestId: "incorrect", connections: [] });
    const { result } = renderHook(() => useToolConnections());
    await waitFor(() =>
      expect(result.current.errorInfo?.code).toBe(
        "invalid_connection_response",
      ),
    );
    expect(result.current.connections).toEqual([]);
    expect(result.current.loading).toBe(false);
  });
  it.each(["secure_storage_unavailable", "connection_inspection_failed"])(
    "ru076 correlates partial failures while retaining the other four results: %s",
    async (reason) => {
      native.mockImplementationOnce(async (_command, args) => {
        const response = connectionsFixture(
          (args as { request: { requestId: string } }).request.requestId,
        );
        response.connections[0].state = "unavailable";
        response.connections[0].reasonCode = reason;
        return response;
      });
      const { result } = renderHook(() => useToolConnections());
      await waitFor(() => expect(result.current.connections).toHaveLength(5));
      expect(result.current.error).toBe(false);
      expect(result.current.errorInfo?.code).toBe(
        "connection_partial_unavailable",
      );
      expect(result.current.errorInfo?.requestId).toMatch(/^connections-/);
      expect(
        result.current.connections.filter((c) => c.state === "not_connected"),
      ).toHaveLength(4);
    },
  );
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
    await waitFor(() => expect(result.current.connections).toHaveLength(5));
    expect(result.current.connections).toHaveLength(5);
    expect(result.current.error).toBe(false);
    await act(async () => {
      await result.current.restore("pi");
    });
    expect(native).toHaveBeenLastCalledWith("manage_tool_connections_v1", {
      request: {
        requestId: expect.any(String),
        operation: "restore",
        toolId: "pi",
        revokeTokens: true,
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
    await waitFor(() => expect(result.current.connections).toHaveLength(5));
    native.mockRejectedValueOnce(Error("unavailable"));
    await act(async () => {
      await result.current.refresh();
    });
    await waitFor(() => expect(result.current.error).toBe(true));
    expect(result.current.connections).toHaveLength(5);
    expect(result.current.error).toBe(true);
    expect(result.current.loading).toBe(false);
  });
  it("releases initial loading when the native inspection reply is lost", async () => {
    vi.useFakeTimers();
    native.mockImplementationOnce(async () => await new Promise(() => {}));
    const { result } = renderHook(() => useToolConnections());
    expect(result.current.loading).toBe(true);
    await act(async () => {
      await vi.advanceTimersByTimeAsync(CONNECTION_INSPECTION_DEADLINE_MS);
    });
    expect(result.current.loading).toBe(false);
    expect(result.current.error).toBe(true);
  });
  it("keeps the last usable projection interactive during a background refresh", async () => {
    const { result } = renderHook(() => useToolConnections());
    await waitFor(() => expect(result.current.connections).toHaveLength(5));
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
    expect(result.current.connections).toHaveLength(5);
    expect(result.current.loading).toBe(false);
    await act(async () => {
      finish(connectionsFixture("wrong-stale-id"));
      await refresh;
    });
    expect(result.current.loading).toBe(false);
    expect(result.current.connections).toHaveLength(5);
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
      await waitFor(() => expect(result.current.connections).toHaveLength(5));
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
      expect(restore).toHaveBeenCalledWith("claude_code", true);
      expect(screen.getByRole("status")).toHaveTextContent(
        "你之后修改的内容已保留",
      );
    },
  );
  it("asks about key revocation separately and keeps the key when unchecked", async () => {
    const connection: ToolConnection = {
      ...connectionsFixture("one").connections[0],
      state: "connected",
      restoreMode: "original",
      modelId: "glm-5.3",
    };
    const restore = vi.fn().mockResolvedValue({
      ...connectionsFixture("one"),
      status: "restored",
      reasonCode: "local_settings_restored_token_kept",
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
    fireEvent.click(screen.getByRole("button", { name: "恢复原设置" }));
    const dialog = screen.getByRole("dialog");
    const revoke = within(dialog).getByRole("checkbox", {
      name: /同时撤销此应用的专用 Key/,
    });
    expect(revoke).toBeChecked();
    expect(dialog).toHaveTextContent("立即失效，不再产生任何计费");
    fireEvent.click(revoke);
    expect(revoke).not.toBeChecked();
    expect(dialog).toHaveTextContent("仍然有效且可能继续计费");
    await act(async () => {
      fireEvent.click(
        within(dialog).getByRole("button", { name: "恢复原设置" }),
      );
    });
    expect(restore).toHaveBeenCalledWith("claude_code", false);
    expect(screen.getByRole("status")).toHaveTextContent(
      "已按你的选择保留",
    );
  });
  it("offers the restore action as the retry entry when revocation fails", async () => {
    const connection: ToolConnection = {
      ...connectionsFixture("one").connections[0],
      state: "connected",
      restoreMode: "original",
      modelId: "glm-5.3",
    };
    const restore = vi.fn().mockResolvedValue({
      ...connectionsFixture("one"),
      status: "restored",
      reasonCode: "local_settings_restored_token_cleanup_pending",
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
    fireEvent.click(screen.getByRole("button", { name: "恢复原设置" }));
    await act(async () => {
      fireEvent.click(
        within(screen.getByRole("dialog")).getByRole("button", {
          name: "恢复原设置",
        }),
      );
    });
    expect(restore).toHaveBeenCalledWith("claude_code", true);
    expect(screen.getByRole("status")).toHaveTextContent(
      "再次点击「恢复原设置」即可重试撤销",
    );
  });
});

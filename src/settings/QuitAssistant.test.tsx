import { StrictMode } from "react";
import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
  within,
} from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { useConnections } from "../configuration/connections";
import { QuitAssistant, ShutdownHost, ShutdownProvider } from "./QuitAssistant";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn() }));
vi.mock("../configuration/connections", () => ({
  useConnections: vi.fn(() => null),
}));

const native = vi.mocked(invoke);
const listeners = new Map<string, Set<(event: { payload: unknown }) => void>>();
let closeRequested = false;
let shutdown: { status: string } | null = null;

const flush = async () => {
  await act(async () => {});
};
function emit(event: string, payload: unknown = null) {
  act(() => {
    listeners.get(event)?.forEach((callback) => callback({ payload }));
  });
}
async function openSettingsChoice() {
  render(
    <ShutdownProvider>
      <QuitAssistant />
    </ShutdownProvider>,
  );
  await flush();
  fireEvent.click(screen.getByRole("button", { name: "退出野菜助手" }));
  await flush();
}
function quitCalls() {
  return native.mock.calls.filter(
    ([command]) => command === "quit_desktop_assistant",
  );
}

beforeEach(() => {
  vi.useFakeTimers();
  closeRequested = false;
  shutdown = null;
  listeners.clear();
  vi.mocked(useConnections).mockReturnValue(null);
  native.mockReset();
  native.mockImplementation(async (command) => {
    if (command === "read_desktop_exit_state")
      return { closeRequested, shutdown };
    if (command === "quit_desktop_assistant") {
      shutdown = { status: "exiting" };
      return shutdown;
    }
    return undefined;
  });
  vi.mocked(listen).mockImplementation(async (event, callback) => {
    const callbacks = listeners.get(event) ?? new Set();
    callbacks.add(callback as (event: { payload: unknown }) => void);
    listeners.set(event, callbacks);
    return () => {
      callbacks.delete(callback as (event: { payload: unknown }) => void);
    };
  });
  HTMLDialogElement.prototype.showModal = function () {
    this.setAttribute("open", "");
  };
  HTMLDialogElement.prototype.close = function () {
    this.removeAttribute("open");
  };
});

afterEach(() => {
  cleanup();
  vi.useRealTimers();
});

describe("global desktop close choice and cooperative exit", () => {
  it("defaults to restoring and preserving remote keys through the native owner", async () => {
    await openSettingsChoice();
    expect(screen.getByRole("dialog")).toHaveTextContent("请先保存任务");
    fireEvent.click(screen.getByRole("button", { name: "恢复原设置并退出" }));
    await flush();
    expect(native).toHaveBeenCalledWith("quit_desktop_assistant", {
      restoreSettings: true,
    });
    expect(
      native.mock.calls.some(
        ([command]) => command === "manage_tool_connections_v1",
      ),
    ).toBe(false);
  });
  it("explicit preserve exit never requests restoration", async () => {
    await openSettingsChoice();
    fireEvent.click(screen.getByRole("button", { name: "保留接入并退出" }));
    await flush();
    expect(native).toHaveBeenCalledWith("quit_desktop_assistant", {
      restoreSettings: false,
    });
  });
  it("names failed tools and offers retry or preserve exit without hiding successes", async () => {
    await openSettingsChoice();
    fireEvent.click(screen.getByRole("button", { name: "恢复原设置并退出" }));
    await flush();
    emit("yeschoy://exit-progress", {
      status: "restore_failed",
      failedTools: ["codex_desktop"],
    });
    expect(screen.getByRole("alert")).toHaveTextContent("Codex Desktop");
    expect(
      screen.getByRole("button", { name: "重试恢复并退出" }),
    ).toBeEnabled();
    fireEvent.click(screen.getByRole("button", { name: "重试恢复并退出" }));
    await flush();
    expect(quitCalls().at(-1)?.[1]).toEqual({ restoreSettings: true });
    emit("yeschoy://exit-progress", {
      status: "restore_failed",
      failedTools: ["codex_desktop"],
    });
    fireEvent.click(screen.getByRole("button", { name: "保留剩余设置并退出" }));
    await flush();
    expect(quitCalls().at(-1)?.[1]).toEqual({ restoreSettings: false });
  });
  it("accepts and names WorkBuddy restoration failures", async () => {
    await openSettingsChoice();
    emit("yeschoy://exit-progress", {
      status: "restore_failed",
      failedTools: ["workbuddy"],
    });
    expect(screen.getByRole("alert")).toHaveTextContent("WorkBuddy");
  });
  it("reads back a lost restoration result and keeps unsafe tool names off screen", async () => {
    await openSettingsChoice();
    fireEvent.click(screen.getByRole("button", { name: "恢复原设置并退出" }));
    await flush();
    emit("yeschoy://exit-progress", { status: "restoring_settings" });
    emit("yeschoy://exit-progress", {
      status: "restore_failed",
      failedTools: ["private/path"],
    });
    expect(document.body).not.toHaveTextContent("private/path");
    shutdown = {
      status: "restore_failed",
      failedTools: ["pi"],
    } as typeof shutdown;
    await act(async () => {
      await vi.advanceTimersByTimeAsync(20_000);
    });
    expect(screen.getByRole("alert")).toHaveTextContent("Pi");
    expect(
      screen.getByRole("button", { name: "保留剩余设置并退出" }),
    ).toBeEnabled();
  });
  it("opens from native X without visiting Settings and does not request shutdown", async () => {
    render(<ShutdownHost />);
    await flush();
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    emit("yeschoy://close-choice");
    expect(screen.getByRole("dialog")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "后台运行" })).toBeEnabled();
    expect(
      screen.getByRole("button", { name: "恢复原设置并退出" }),
    ).toBeEnabled();
    expect(quitCalls()).toHaveLength(0);
  });

  it("recovers a native close intent issued before the host listener mounted", async () => {
    closeRequested = true;
    render(<ShutdownHost />);
    await flush();
    expect(screen.getByRole("dialog")).toBeInTheDocument();
    expect(quitCalls()).toHaveLength(0);
  });

  it("settings and repeated X share one dialog", async () => {
    await openSettingsChoice();
    emit("yeschoy://close-choice");
    emit("yeschoy://close-choice");
    expect(screen.getAllByRole("dialog")).toHaveLength(1);
    expect(quitCalls()).toHaveLength(0);
  });

  it("background minimizes through its native action without invoking quit", async () => {
    await openSettingsChoice();
    fireEvent.click(screen.getByRole("button", { name: "后台运行" }));
    await flush();
    expect(native).toHaveBeenCalledWith("background_desktop_assistant");
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    expect(quitCalls()).toHaveLength(0);
  });

  it("cancel dismisses the choice without quitting or minimizing", async () => {
    await openSettingsChoice();
    fireEvent.click(screen.getByRole("button", { name: "取消" }));
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    expect(native).toHaveBeenCalledWith("dismiss_desktop_exit_prompt");
    expect(native).not.toHaveBeenCalledWith("background_desktop_assistant");
    expect(quitCalls()).toHaveLength(0);
  });

  it("names only background-dependent connections including pending recovery", async () => {
    vi.mocked(useConnections).mockReturnValue({
      loading: false,
      error: false,
      connections: [
        {
          toolId: "codex_desktop",
          state: "connected",
          requiresBackground: true,
        },
        {
          toolId: "dsh_web",
          state: "recovery_pending",
          requiresBackground: true,
        },
        {
          toolId: "claude_code",
          state: "connected",
          requiresBackground: false,
        },
      ],
    } as ReturnType<typeof useConnections>);
    await openSettingsChoice();
    const list = within(screen.getByRole("dialog")).getByRole("list");
    expect(list).toHaveTextContent("Codex Desktop");
    expect(list).toHaveTextContent("DSH");
    expect(list).not.toHaveTextContent("Claude Code");
  });

  it("a pending IPC becomes recoverable after 20 seconds without cancelling its promise", async () => {
    let finish: (value: unknown) => void = () => {};
    native.mockImplementation(async (command) => {
      if (command === "read_desktop_exit_state")
        return { closeRequested: false, shutdown: null };
      if (command === "quit_desktop_assistant")
        return new Promise((resolve) => {
          finish = resolve;
        });
    });
    await openSettingsChoice();
    fireEvent.click(screen.getByRole("button", { name: "恢复原设置并退出" }));
    await act(async () => {
      await vi.advanceTimersByTimeAsync(20_000);
    });
    expect(screen.getByRole("status")).toHaveTextContent(
      "安全收尾后会自动退出",
    );
    expect(screen.getByRole("button", { name: "收起提示" })).toBeEnabled();
    expect(screen.getByRole("button", { name: "在后台等待" })).toBeEnabled();
    expect(screen.getByRole("button", { name: "重试退出" })).toBeEnabled();
    expect(quitCalls()).toHaveLength(1);
    await act(async () => {
      finish({ status: "finishing_operation" });
    });
    expect(screen.getByRole("status")).toHaveTextContent("当前接入或账号操作");
  });

  it("accepts native finishing progress and allows idempotent repeat requests", async () => {
    await openSettingsChoice();
    fireEvent.click(screen.getByRole("button", { name: "恢复原设置并退出" }));
    await flush();
    emit("yeschoy://exit-progress", { status: "finishing_operation" });
    fireEvent.click(screen.getByRole("button", { name: "重试退出" }));
    await flush();
    expect(quitCalls()).toHaveLength(2);
    expect(native).not.toHaveBeenCalledWith("background_desktop_assistant");
  });

  it("can dismiss progress with Escape without undoing the confirmed exit", async () => {
    await openSettingsChoice();
    fireEvent.click(screen.getByRole("button", { name: "恢复原设置并退出" }));
    await flush();
    fireEvent(
      screen.getByRole("dialog"),
      new Event("cancel", { cancelable: true }),
    );
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    emit("yeschoy://exit-progress", { status: "finishing_operation" });
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    expect(quitCalls()).toHaveLength(1);
    emit("yeschoy://close-choice");
    await flush();
    expect(screen.getByRole("dialog")).toBeInTheDocument();
  });

  it("presents IPC failures as a retry without exposing raw details", async () => {
    await openSettingsChoice();
    native.mockRejectedValueOnce(new Error("secret-fixture-path-and-key"));
    fireEvent.click(screen.getByRole("button", { name: "恢复原设置并退出" }));
    await flush();
    expect(screen.getByRole("alert")).toHaveTextContent(
      "暂时无法确认退出状态",
    );
    expect(screen.getByRole("button", { name: "重试退出" })).toBeEnabled();
    expect(document.body).not.toHaveTextContent("secret-fixture-path-and-key");
  });

  it("rejects invalid success payloads instead of claiming the host exited", async () => {
    await openSettingsChoice();
    native.mockResolvedValueOnce({ status: "finished", raw: "private-error" });
    fireEvent.click(screen.getByRole("button", { name: "恢复原设置并退出" }));
    await flush();
    expect(screen.getByRole("alert")).toBeInTheDocument();
    expect(document.body).not.toHaveTextContent("private-error");
  });

  it("keeps the choice available when minimizing fails", async () => {
    await openSettingsChoice();
    native.mockRejectedValueOnce(new Error("synthetic-window-failure"));
    fireEvent.click(screen.getByRole("button", { name: "后台运行" }));
    await flush();
    expect(screen.getByRole("dialog")).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "恢复原设置并退出" }),
    ).toBeEnabled();
    expect(quitCalls()).toHaveLength(0);
  });

  it("does not leak duplicate native listeners across StrictMode remounts", async () => {
    const view = render(
      <StrictMode>
        <ShutdownHost />
      </StrictMode>,
    );
    await flush();
    expect(listeners.get("yeschoy://close-choice")?.size).toBe(1);
    expect(listeners.get("yeschoy://exit-progress")?.size).toBe(1);
    view.unmount();
    expect(listeners.get("yeschoy://close-choice")?.size).toBe(0);
    expect(listeners.get("yeschoy://exit-progress")?.size).toBe(0);
  });
});

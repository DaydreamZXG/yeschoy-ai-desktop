import { act, cleanup, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { useAccountSession } from "./useAccountSession";
import {
  readAccountSessionSnapshot,
  runAccountCommand,
  type AccountProjection,
} from "./session";
import type { ConfigurationLineId } from "../configuration/preview";
vi.mock("./session", async (load) => ({
  ...(await load<typeof import("./session")>()),
  runAccountCommand: vi.fn(),
  readAccountSessionSnapshot: vi.fn(),
}));
const command = vi.mocked(runAccountCommand);
const sessionSnapshot = vi.mocked(readAccountSessionSnapshot);
function projection(status: AccountProjection["status"]): AccountProjection {
  return {
    requestId: "fixture",
    schemaVersion: 3,
    status,
    userCode: "ABCD-EFGH",
    pollAfterSeconds: 1,
    expiresAtEpochMs: Date.now() + 60000,
    observedAtEpochMs: Date.now(),
    account: {
      available: status === "signed_in",
      displayName: "示例用户",
      username: "example",
      balanceQuota: "123",
      usedQuota: "12",
      requestCount: "4",
      quotaPerUnit: "500000",
    },
    usage: {
      available: false,
      consumedQuota: "",
      requestRate: "",
      tokenCount: "",
    },
    models: [],
    comparisonFx: "1",
    reasonCode: status,
  };
}
beforeEach(() => {
  vi.useFakeTimers();
  command.mockReset();
  sessionSnapshot.mockReset();
  sessionSnapshot.mockResolvedValue({
    hasStoredSession: true,
    storedLineId: "mainland_optimized",
  });
  command.mockResolvedValue(projection("signed_out"));
});
afterEach(() => {
  cleanup();
  vi.useRealTimers();
  void import("./accountProjectionCache").then((m) =>
    m.clearCachedSignedInProjection(),
  );
});
async function flush() {
  await act(async () => {});
}
describe("resilient account authorization", () => {
  it("skips account_inspect on cold start when the keyring has no session", async () => {
    sessionSnapshot.mockResolvedValue({
      hasStoredSession: false,
      storedLineId: null,
    });
    const { result } = renderHook(() =>
      useAccountSession("mainland_optimized"),
    );
    await flush();
    expect(command).not.toHaveBeenCalled();
    expect(result.current.loading).toBe(false);
    expect(result.current.projection?.status).toBe("signed_out");
    expect(result.current.lastError).toBeNull();
  });

  it("keeps loading false during silent startup inspect when the keyring has a session", async () => {
    let complete!: (value: AccountProjection) => void;
    command.mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          complete = resolve;
        }),
    );
    const { result } = renderHook(() =>
      useAccountSession("mainland_optimized"),
    );
    await flush();
    expect(command).toHaveBeenCalledTimes(1);
    expect(result.current.loading).toBe(false);
    await act(async () => {
      complete(projection("signed_in"));
    });
    await flush();
    expect(result.current.projection?.status).toBe("signed_in");
  });

  it("paints the cached signed-in projection before the station replies", async () => {
    const { writeCachedSignedInProjection, clearCachedSignedInProjection } =
      await import("./accountProjectionCache");
    clearCachedSignedInProjection();
    writeCachedSignedInProjection(projection("signed_in"));
    let complete!: (value: AccountProjection) => void;
    command.mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          complete = resolve;
        }),
    );
    const { result } = renderHook(() =>
      useAccountSession("mainland_optimized"),
    );
    await flush();
    expect(result.current.projection?.status).toBe("signed_in");
    expect(result.current.projection?.account.displayName).toBe("示例用户");
    expect(result.current.stationCheck).toBe("checking");
    expect(result.current.loading).toBe(false);
    await act(async () => {
      complete(projection("network_error"));
    });
    await flush();
    // 短暂失败时保留上次登录，不要闪成未登录。
    expect(result.current.projection?.status).toBe("signed_in");
    expect(result.current.stationCheck).toBe("unreachable");
    clearCachedSignedInProjection();
  });

  it("does not retry transient failures on startup inspect", async () => {
    command.mockResolvedValue(projection("network_error"));
    const { result } = renderHook(() =>
      useAccountSession("mainland_optimized"),
    );
    await flush();
    expect(command).toHaveBeenCalledTimes(1);
    expect(result.current.loading).toBe(false);
    expect(result.current.projection?.status).toBe("network_error");
    expect(result.current.lastError).toBe("network_error");
  });

  it("retries a transient inspection on manual refresh", async () => {
    command.mockResolvedValueOnce(projection("network_error"));
    const { result } = renderHook(() =>
      useAccountSession("mainland_optimized"),
    );
    await flush();
    command
      .mockResolvedValueOnce(projection("network_error"))
      .mockResolvedValueOnce(projection("signed_out"));
    await act(async () => {
      const pending = result.current.refresh();
      await vi.advanceTimersByTimeAsync(250);
      await pending;
    });
    expect(command).toHaveBeenCalledTimes(3);
    expect(result.current.projection?.status).toBe("signed_out");
    expect(result.current.loading).toBe(false);
    expect(result.current.lastError).toBeNull();
  });

  it("shows an error only after bounded retries are exhausted on refresh", async () => {
    command.mockResolvedValueOnce(projection("network_error"));
    const { result } = renderHook(() =>
      useAccountSession("mainland_optimized"),
    );
    await flush();
    command.mockResolvedValue(projection("network_error"));
    await act(async () => {
      const pending = result.current.refresh();
      await vi.advanceTimersByTimeAsync(2_500);
      await pending;
    });
    expect(command).toHaveBeenCalledTimes(5);
    expect(result.current.projection?.status).toBe("network_error");
    expect(result.current.loading).toBe(false);
    expect(result.current.lastError).toBe("network_error");
  });

  it("does not retry cold-start transport exceptions on startup inspect", async () => {
    command.mockRejectedValueOnce(new Error("offline"));
    const { result } = renderHook(() =>
      useAccountSession("mainland_optimized"),
    );
    await flush();
    expect(command).toHaveBeenCalledTimes(1);
    expect(result.current.loading).toBe(false);
    expect(result.current.lastError).toBe("network_error");
  });

  it("keeps polling after a temporary response error and then signs in", async () => {
    const { result } = renderHook(() =>
      useAccountSession("mainland_optimized"),
    );
    await flush();
    command.mockResolvedValueOnce(projection("authorization_pending"));
    await act(async () => {
      await result.current.beginAuthorization();
    });
    command
      .mockResolvedValueOnce(projection("network_error"))
      .mockResolvedValueOnce(projection("signed_in"));
    await act(async () => {
      await vi.advanceTimersByTimeAsync(1000);
    });
    expect(result.current.projection?.status).toBe("authorization_pending");
    expect(result.current.lastError).toBe("network_error");
    await act(async () => {
      await vi.advanceTimersByTimeAsync(1000);
    });
    expect(result.current.projection?.status).toBe("signed_in");
    expect(result.current.lastError).toBeNull();
  });
  it("surfaces the real failure once a pending authorization stops recovering", async () => {
    const { result } = renderHook(() =>
      useAccountSession("mainland_optimized"),
    );
    await flush();
    command.mockResolvedValueOnce(projection("authorization_pending"));
    await act(async () => {
      await result.current.beginAuthorization();
    });
    // A failure run long enough to outlast a route hiccup must stop rendering
    // as "waiting for authorization": the user already approved the page, and
    // masking this until the device code expires is what makes a finished
    // sign-in look broken with no reason shown.
    for (let attempt = 0; attempt < 5; attempt += 1) {
      command.mockResolvedValueOnce(projection("invalid_response"));
      await act(async () => {
        await vi.advanceTimersByTimeAsync(1000);
      });
    }
    expect(result.current.projection?.status).toBe("invalid_response");
    expect(result.current.lastError).toBe("invalid_response");
  });
  it("retries IPC exceptions instead of silently stopping authorization", async () => {
    const { result } = renderHook(() =>
      useAccountSession("mainland_optimized"),
    );
    await flush();
    command.mockResolvedValueOnce(projection("authorization_pending"));
    await act(async () => {
      await result.current.beginAuthorization();
    });
    command
      .mockRejectedValueOnce(new Error("offline"))
      .mockResolvedValueOnce(projection("signed_in"));
    await act(async () => {
      await vi.advanceTimersByTimeAsync(1000);
    });
    expect(result.current.projection?.status).toBe("authorization_pending");
    await act(async () => {
      await vi.advanceTimersByTimeAsync(1000);
    });
    expect(result.current.projection?.status).toBe("signed_in");
  });
  it("ignores a late successful poll after cancellation", async () => {
    const { result } = renderHook(() =>
      useAccountSession("mainland_optimized"),
    );
    await flush();
    command.mockResolvedValueOnce(projection("authorization_pending"));
    await act(async () => {
      await result.current.beginAuthorization();
    });
    let complete!: (value: AccountProjection) => void;
    command.mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          complete = resolve;
        }),
    );
    await act(async () => {
      await vi.advanceTimersByTimeAsync(1000);
    });
    command.mockResolvedValueOnce(projection("cancelled"));
    await act(async () => {
      await result.current.cancelAuthorization();
    });
    await act(async () => {
      complete(projection("signed_in"));
    });
    expect(result.current.projection?.status).toBe("cancelled");
    const count = command.mock.calls.length;
    await act(async () => {
      await vi.advanceTimersByTimeAsync(10000);
    });
    expect(command).toHaveBeenCalledTimes(count);
  });
  it("keeps authorization on its issuing route while the user changes network route", async () => {
    const { result, rerender } = renderHook(
      ({ line }: { line: ConfigurationLineId }) => useAccountSession(line),
      { initialProps: { line: "mainland_optimized" } },
    );
    await flush();
    command.mockResolvedValueOnce(projection("authorization_pending"));
    await act(async () => {
      await result.current.beginAuthorization();
    });
    command.mockResolvedValueOnce(projection("authorization_pending"));
    rerender({ line: "global_accelerated" });
    await flush();
    expect(command).toHaveBeenLastCalledWith(
      "account_poll_authorization_v2",
      "mainland_optimized",
    );
    expect(result.current.projection?.status).toBe("authorization_pending");
  });
  it("reopens the same pending authorization on its issuing route", async () => {
    const { result } = renderHook(() =>
      useAccountSession("mainland_optimized"),
    );
    await flush();
    command.mockResolvedValueOnce(projection("authorization_pending"));
    await act(async () => {
      await result.current.beginAuthorization();
    });
    command.mockResolvedValueOnce(projection("authorization_pending"));
    await act(async () => {
      await result.current.openAuthorization();
    });
    expect(command).toHaveBeenLastCalledWith(
      "account_open_authorization_v2",
      "mainland_optimized",
    );
    expect(result.current.projection?.userCode).toBe("ABCD-EFGH");
  });
  it("does not erase signed-in account data after a refresh exception", async () => {
    command.mockResolvedValueOnce(projection("signed_in"));
    const { result } = renderHook(() =>
      useAccountSession("mainland_optimized"),
    );
    await flush();
    command.mockRejectedValueOnce(new Error("offline"));
    await act(async () => {
      await result.current.refresh();
    });
    expect(result.current.projection?.account.balanceQuota).toBe("123");
    expect(result.current.projection?.status).toBe("signed_in");
    expect(result.current.lastError).toBe("network_error");
  });
});

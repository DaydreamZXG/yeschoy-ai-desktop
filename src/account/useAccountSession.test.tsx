import { act, cleanup, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { useAccountSession } from "./useAccountSession";
import { runAccountCommand, type AccountProjection } from "./session";
import type { ConfigurationLineId } from "../configuration/preview";
vi.mock("./session", async (load) => ({
  ...(await load<typeof import("./session")>()),
  runAccountCommand: vi.fn(),
}));
const command = vi.mocked(runAccountCommand);
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
  command.mockResolvedValue(projection("signed_out"));
});
afterEach(() => {
  cleanup();
  vi.useRealTimers();
});
async function flush() {
  await act(async () => {});
}
describe("resilient account authorization", () => {
  it("retries a transient cold-start inspection before treating it as unavailable", async () => {
    command
      .mockResolvedValueOnce(projection("network_error"))
      .mockResolvedValueOnce(projection("signed_out"));
    const { result } = renderHook(() =>
      useAccountSession("mainland_optimized"),
    );
    await flush();
    expect(command).toHaveBeenCalledTimes(1);
    expect(result.current.loading).toBe(true);
    expect(result.current.lastError).toBeNull();
    await act(async () => {
      await vi.advanceTimersByTimeAsync(250);
    });
    expect(command).toHaveBeenCalledTimes(2);
    expect(result.current.projection?.status).toBe("signed_out");
    expect(result.current.loading).toBe(false);
    expect(result.current.lastError).toBeNull();
  });

  it("shows a cold-start error only after the bounded retries are exhausted", async () => {
    command.mockResolvedValue(projection("network_error"));
    const { result } = renderHook(() =>
      useAccountSession("mainland_optimized"),
    );
    await flush();
    await act(async () => {
      await vi.advanceTimersByTimeAsync(2_500);
    });
    expect(command).toHaveBeenCalledTimes(4);
    expect(result.current.projection?.status).toBe("network_error");
    expect(result.current.loading).toBe(false);
    expect(result.current.lastError).toBe("network_error");
  });

  it("also retries cold-start transport exceptions", async () => {
    command
      .mockRejectedValueOnce(new Error("offline"))
      .mockResolvedValueOnce(projection("signed_in"));
    const { result } = renderHook(() =>
      useAccountSession("mainland_optimized"),
    );
    await flush();
    await act(async () => {
      await vi.advanceTimersByTimeAsync(250);
    });
    expect(command).toHaveBeenCalledTimes(2);
    expect(result.current.projection?.status).toBe("signed_in");
    expect(result.current.lastError).toBeNull();
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

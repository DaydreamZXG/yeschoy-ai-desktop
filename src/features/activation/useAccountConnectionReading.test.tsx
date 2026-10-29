import { renderHook } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import {
  useAccountConnectionReadingWith,
  useBalanceIssue,
} from "./useAccountConnectionReading";
import type { ToolConnection } from "../../configuration/connections";

function connection(state: ToolConnection["state"]): ToolConnection {
  return {
    toolId: "pi",
    state,
    modelId: "glm-5.3",
    lineId: "mainland_optimized",
    billingGroup: "default",
    updatedAtEpochMs: 1,
    restoreMode: "none",
    requiresBackground: false,
    reasonCode: state,
  };
}

describe("useAccountConnectionReadingWith", () => {
  it("combines the row, the sign-in state and readability", () => {
    const { result } = renderHook(() =>
      useAccountConnectionReadingWith(
        {
          loading: false,
          error: false,
          connections: [connection("connected")],
        },
        { toolId: "pi", sessionStatus: "signed_in" },
      ),
    );
    expect(result.current.savedConnection?.state).toBe("connected");
    expect(result.current.signedIn).toBe(true);
    expect(result.current.connectionStateUnavailable).toBe(false);
  });

  it("keeps the same object while nothing changed", () => {
    // 页面把这个结果直接喂给别的 memo；每次渲染都换新对象会让那些 memo 失效。
    const connections = { loading: false, error: false, connections: [] };
    const input = { toolId: "pi" as const, sessionStatus: "signed_out" };
    const { result, rerender } = renderHook(
      () => useAccountConnectionReadingWith(connections, input),
      { initialProps: {} },
    );
    const first = result.current;
    rerender({});
    expect(result.current).toBe(first);
    expect(first.connectionStateUnavailable).toBe(false);
    expect(first.signedIn).toBe(false);
  });

  it("treats a missing controller as unavailable", () => {
    const { result } = renderHook(() =>
      useAccountConnectionReadingWith(undefined, {
        toolId: "pi",
        sessionStatus: "signed_in",
      }),
    );
    expect(result.current.connectionStateUnavailable).toBe(true);
  });
});

describe("useBalanceIssue", () => {
  it("says nothing before sign-in even if the projection still has money", () => {
    const { result } = renderHook(() =>
      useBalanceIssue(false, {
        currency: "CNY",
        balanceAmount: "0",
        consumedAmount: "0",
        displayRate: "1",
      }),
    );
    expect(result.current).toBeNull();
  });

  it("reports a depleted balance once signed in", () => {
    const { result } = renderHook(() =>
      useBalanceIssue(true, {
        currency: "CNY",
        balanceAmount: "0",
        consumedAmount: "0",
        displayRate: "1",
      }),
    );
    expect(result.current?.level).toBe("depleted");
  });
});

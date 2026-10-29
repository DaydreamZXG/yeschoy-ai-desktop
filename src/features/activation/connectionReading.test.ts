import { describe, expect, it } from "vitest";
import type { ToolConnection } from "../../configuration/connections";
import type { ActivationToolId } from "../../configuration/activation";
import {
  connectionStateUnavailable,
  isSignedIn,
  savedConnectionFor,
} from "./connectionReading";

function connection(
  toolId: ActivationToolId,
  state: ToolConnection["state"],
): ToolConnection {
  return {
    toolId,
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

describe("savedConnectionFor", () => {
  it("picks only this tool's row", () => {
    const rows = [
      connection("pi", "connected"),
      connection("opencode", "connected"),
    ];
    expect(savedConnectionFor({ connections: rows }, "opencode")?.toolId).toBe(
      "opencode",
    );
    expect(savedConnectionFor({ connections: rows }, "claude_code")).toBe(
      undefined,
    );
  });

  it("treats a missing controller as no row", () => {
    expect(savedConnectionFor(undefined, "pi")).toBeUndefined();
    expect(savedConnectionFor(null, "pi")).toBeUndefined();
    expect(savedConnectionFor({}, "pi")).toBeUndefined();
  });
});

describe("connectionStateUnavailable", () => {
  const saved = connection("pi", "connected");

  it("is unavailable while the first read is in flight", () => {
    expect(
      connectionStateUnavailable({
        loading: true,
        error: false,
        connections: [],
        saved: undefined,
      }),
    ).toBe(true);
  });

  it("is unavailable when the read failed and nothing was cached", () => {
    expect(
      connectionStateUnavailable({
        loading: false,
        error: true,
        connections: [],
        saved: undefined,
      }),
    ).toBe(true);
  });

  it("stays readable when the read failed but a cached row exists", () => {
    // 有缓存时页面照旧显示上一次的状态：一句「读不到」比一个陈旧但真实的
    // 「已接入」更没用。
    expect(
      connectionStateUnavailable({
        loading: false,
        error: true,
        connections: [saved],
        saved,
      }),
    ).toBe(false);
  });

  it("is unavailable when this row itself is unavailable", () => {
    const degraded = connection("pi", "unavailable");
    expect(
      connectionStateUnavailable({
        loading: false,
        error: true,
        connections: [degraded],
        saved: degraded,
      }),
    ).toBe(true);
  });

  it("is readable for a healthy read", () => {
    expect(
      connectionStateUnavailable({
        loading: false,
        error: false,
        connections: [saved],
        saved,
      }),
    ).toBe(false);
  });
});

describe("isSignedIn", () => {
  it("only accepts an explicit signed_in projection", () => {
    expect(isSignedIn("signed_in")).toBe(true);
    expect(isSignedIn("signed_out")).toBe(false);
    expect(isSignedIn("expired")).toBe(false);
    expect(isSignedIn(undefined)).toBe(false);
    expect(isSignedIn(null)).toBe(false);
  });
});

import { describe, expect, it } from "vitest";
import {
  CONNECTIVITY_LINES,
  isCompleteConnectivityProjection,
} from "./contract";
import type { ConnectivityResponse } from "./contract";

function response(requestId = "diag-1"): ConnectivityResponse {
  return {
    requestId,
    startedAtEpochMs: 100,
    completedAtEpochMs: 120,
    lines: CONNECTIVITY_LINES.map((line) => ({
      ...line,
      status: "reachable" as const,
      latencyMs: 10,
      reasonCode: "tcp_443_reachable" as const,
    })),
  };
}

describe("line connectivity projection", () => {
  it("accepts exactly the two approved line results", () => {
    expect(isCompleteConnectivityProjection(response(), "diag-1")).toBe(true);
  });

  it("rejects stale, duplicated, and mismatched projections", () => {
    expect(isCompleteConnectivityProjection(response("old"), "diag-1")).toBe(
      false,
    );

    const duplicated = response();
    duplicated.lines = [duplicated.lines[0], duplicated.lines[0]];
    expect(isCompleteConnectivityProjection(duplicated, "diag-1")).toBe(false);

    const mismatched = response();
    mismatched.lines[0] = {
      ...mismatched.lines[0],
      reasonCode: "tcp_connection_failed",
    };
    expect(isCompleteConnectivityProjection(mismatched, "diag-1")).toBe(false);
  });
});

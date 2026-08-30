import { describe, expect, it } from "vitest";
import { createCandidateReadiness } from "./readiness";

describe("client candidate readiness", () => {
  it("exposes only the three bounded client-side capabilities", () => {
    const readiness = createCandidateReadiness();
    expect(
      readiness.capabilities
        .filter((capability) => capability.status === "available_client_side")
        .map((capability) => capability.id),
    ).toEqual(["tool_discovery", "configuration_preview", "line_connectivity"]);
    expect(readiness.productionReady).toBe(false);
  });

  it("keeps backup history, telemetry, and automatic updates disabled", () => {
    const readiness = createCandidateReadiness();
    expect(readiness.retainedBackupHistory).toBe(false);
    expect(readiness.telemetryUploadEnabled).toBe(false);
    expect(readiness.automaticUpdateEnabled).toBe(false);
  });
});

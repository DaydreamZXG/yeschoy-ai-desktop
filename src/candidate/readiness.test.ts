import { describe, expect, it } from "vitest";
import { version } from "../../package.json";
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
    expect(readiness.supportedToolCount).toBe(7);
  });

  it("keeps local backup history and telemetry closed while exposing signed updates", () => {
    const readiness = createCandidateReadiness();
    expect(readiness.retainedBackupHistory).toBe(false);
    expect(readiness.telemetryUploadEnabled).toBe(false);
    expect(readiness.automaticUpdateEnabled).toBe(true);
    expect(readiness.productionReady).toBe(false);
  });

  it("marks the v2 account and price surfaces as integration candidates", () => {
    const readiness = createCandidateReadiness();
    expect(readiness.version).toBe(version);
    expect(
      readiness.capabilities
        .filter((capability) => capability.status === "integration_candidate")
        .map((capability) => capability.id),
    ).toEqual(["account_and_billing", "model_and_pricing", "automatic_update"]);
    expect(
      readiness.capabilities.find(
        (capability) => capability.id === "secure_tool_credentials",
      )?.status,
    ).toBe("security_evidence_required");
  });
});

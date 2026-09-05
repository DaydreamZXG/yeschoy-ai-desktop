import { ACTIVATION_TOOL_IDS } from "./activation";
import type { ConnectionResponse } from "./connections";

export function connectionsFixture(requestId: string): ConnectionResponse {
  return {
    requestId,
    schemaVersion: 1,
    status: "ok",
    reasonCode: "local_state",
    connections: ACTIVATION_TOOL_IDS.map((toolId) => ({
      toolId,
      state: "not_connected",
      modelId: "",
      lineId: "",
      billingGroup: "",
      updatedAtEpochMs: 0,
      restoreMode: "none",
      requiresBackground: false,
      reasonCode: "not_connected",
    })),
  };
}

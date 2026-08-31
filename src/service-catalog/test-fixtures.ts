// Synthetic fixtures for tests only. Runtime modules must never import this file.
import type { ConfigurationLineId } from "../configuration/preview";
import type { ServiceCatalog } from "./contract";

export function catalogFixture(
  requestId = "test-read",
  lineId: ConfigurationLineId = "mainland_optimized",
): ServiceCatalog {
  return {
    requestId,
    lineId,
    observedAtEpochMs: 1700000000000,
    catalogStatus: "available",
    catalogError: "none",
    serviceVersion: "test-version",
    backendDisplayExchangeRate: "1",
    groups: [
      {
        id: "test-group",
        description: "仅供测试，不作为流式限制的机器契约",
        multiplier: "0.25",
      },
    ],
    models: [
      {
        id: "test/model",
        groups: ["test-group"],
        endpoints: ["anthropic", "openai-response", "openai"],
        billingMode: "tiered_expr",
      },
    ],
    desktopBackend: {
      status: "not_deployed",
      error: "none",
      declaredCapabilities: [],
    },
    userSpecific: false,
    secretsAccessed: false,
  };
}

import { describe, expect, it } from "vitest";
import {
  CONFIGURATION_LINES,
  CONFIGURATION_TOOLS,
  createConfigurationPreview,
} from "./preview";

describe("guided configuration preview", () => {
  it("keeps the approved catalogs stable", () => {
    expect(CONFIGURATION_TOOLS.map((tool) => tool.id)).toEqual([
      "claude",
      "codex",
      "opencode",
      "pi",
      "dsh",
    ]);
    expect(CONFIGURATION_LINES).toMatchObject([
      { id: "mainland_optimized", rootUrl: "https://yeschoy.com" },
      { id: "global_accelerated", rootUrl: "https://api.yeschoy.com" },
    ]);
  });

  it.each([
    ["claude", "https://yeschoy.com"],
    ["codex", "https://yeschoy.com/v1"],
    ["opencode", "https://yeschoy.com/v1"],
    ["pi", "https://yeschoy.com/v1"],
  ] as const)("projects the documented %s endpoint", (toolId, endpoint) => {
    const projection = createConfigurationPreview({
      requestId: `preview-${toolId}`,
      toolId,
      lineId: "mainland_optimized",
    });

    expect(projection.protocolEndpoint).toBe(endpoint);
    expect(projection.apply.status).toBe("blocked");
    expect(projection.networkAttempted).toBe(false);
    expect(projection.configurationRead).toBe(false);
    expect(projection.configurationWritten).toBe(false);
    expect(projection.credentialAccessed).toBe(false);
  });

  it("withholds the unverified DSH adapter details", () => {
    const projection = createConfigurationPreview({
      requestId: "preview-dsh",
      toolId: "dsh",
      lineId: "global_accelerated",
    });

    expect(projection.rootUrl).toBe("https://api.yeschoy.com");
    expect(projection.protocolEndpoint).toBe("");
    expect(projection.targetFile).toBe("");
    expect(projection.ownedFields).toEqual([]);
    expect(projection.apply.blockers).toContain("dsh_web_adapter_required");
  });

  it("never invents a model", () => {
    const projection = createConfigurationPreview({
      requestId: "preview-model",
      toolId: "codex",
      lineId: "global_accelerated",
    });

    expect(projection.model).toEqual({
      status: "server_catalog_required",
      modelId: "",
    });
  });
});

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
      "workbuddy",
      "hermes",
      "openclaw",
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
    ["dsh", "https://yeschoy.com/v1"],
    ["workbuddy", "https://yeschoy.com/v1/chat/completions"],
    ["hermes", "https://yeschoy.com/v1"],
    ["openclaw", "https://yeschoy.com/v1"],
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

  it("projects the documented DSH adapter details", () => {
    const projection = createConfigurationPreview({
      requestId: "preview-dsh",
      toolId: "dsh",
      lineId: "global_accelerated",
    });

    expect(projection.rootUrl).toBe("https://api.yeschoy.com");
    expect(projection.protocolEndpoint).toBe("https://api.yeschoy.com/v1");
    expect(projection.targetFile).toBe("~/.dsh/settings.yaml");
    expect(projection.ownedFields).toContain("llm-pi-ai.providers.yeschoy");
    expect(projection.endpointStatus).toBe("documented_preview");
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

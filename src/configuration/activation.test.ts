import { invoke } from "@tauri-apps/api/core";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { activateDesktopTool, decodeToolActivation } from "./activation";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
const native = vi.mocked(invoke);

function projection(requestId = "activate-test") {
  return {
    requestId,
    schemaVersion: 1,
    status: "configured",
    toolId: "codex_desktop",
    modelId: "glm-5.3",
    observedAtEpochMs: 1_788_195_600_000,
    reasonCode: "configured",
  };
}

beforeEach(() => native.mockReset());

describe("desktop tool activation boundary", () => {
  it("accepts only the exact secret-free native projection", () => {
    expect(decodeToolActivation(projection(), "activate-test")).toEqual(
      projection(),
    );
    for (const field of ["apiKey", "token", "configPath", "backupPath"]) {
      expect(
        decodeToolActivation(
          { ...projection(), [field]: "must-not-cross-renderer" },
          "activate-test",
        ),
      ).toBeNull();
    }
  });

  it("rejects mismatched requests and unsafe model identifiers", () => {
    expect(decodeToolActivation(projection(), "another-request")).toBeNull();
    expect(
      decodeToolActivation(
        { ...projection(), modelId: "glm-5.3\nforged" },
        "activate-test",
      ),
    ).toBeNull();
  });

  it("invokes the native writer with the selected route, app and model", async () => {
    vi.spyOn(Date, "now").mockReturnValue(1234);
    native.mockResolvedValue(projection("activate-ya-1"));
    const result = await activateDesktopTool({
      lineId: "global_accelerated",
      toolId: "codex_desktop",
      modelId: "glm-5.3",
    });
    expect(result.status).toBe("configured");
    expect(native).toHaveBeenCalledWith("configure_desktop_tool_v1", {
      request: {
        requestId: expect.stringMatching(/^activate-/),
        lineId: "global_accelerated",
        toolId: "codex_desktop",
        modelId: "glm-5.3",
      },
    });
  });
});

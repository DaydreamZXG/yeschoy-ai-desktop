import { invoke } from "@tauri-apps/api/core";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  activateDesktopTool,
  decodeActivationTargetScan,
  decodeToolActivation,
  scanActivationTargets,
} from "./activation";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
const native = vi.mocked(invoke);

function projection(requestId = "activate-test") {
  return {
    requestId,
    schemaVersion: 2,
    status: "ready",
    toolId: "codex_desktop",
    modelId: "glm-5.3",
    observedAtEpochMs: 1_788_195_600_000,
    reasonCode: "tool_request_verified",
  };
}

function targetScan(requestId = "target-scan-test") {
  const target = (
    toolId:
      | "claude_code"
      | "claude_desktop"
      | "codex_desktop"
      | "pi"
      | "dsh_web",
    displayName: string,
  ) => ({
    toolId,
    displayName,
    surface: "本机应用",
    status: "available",
    installations: [
      {
        installationId: "i0123456789abcdef",
        label: "安装 1 · 系统应用",
        version: "1.2.3",
        supported: true,
        recommended: true,
      },
    ],
  });
  return {
    requestId,
    schemaVersion: 1,
    platform: "macos",
    targets: [
      target("claude_code", "Claude Code"),
      target("claude_desktop", "Claude Desktop"),
      target("codex_desktop", "Codex Desktop"),
      target("pi", "Pi"),
      target("dsh_web", "DSH web"),
    ],
  };
}

beforeEach(() => native.mockReset());
afterEach(() => vi.restoreAllMocks());

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

  it("accepts only a complete five-target installation scan", () => {
    expect(
      decodeActivationTargetScan(targetScan(), "target-scan-test"),
    ).toEqual(targetScan());
    expect(
      decodeActivationTargetScan(
        { ...targetScan(), targets: targetScan().targets.slice(0, 4) },
        "target-scan-test",
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
      installationId: "i0123456789abcdef",
    });
    expect(result.status).toBe("ready");
    expect(native).toHaveBeenCalledWith("configure_desktop_tool_v2", {
      request: {
        requestId: expect.stringMatching(/^activate-/),
        lineId: "global_accelerated",
        toolId: "codex_desktop",
        modelId: "glm-5.3",
        installationId: "i0123456789abcdef",
      },
    });
  });

  it("scans exact local targets through the read-only native command", async () => {
    vi.spyOn(Date, "now").mockReturnValue(2222);
    native.mockResolvedValue(targetScan("target-scan-1pq-1"));
    const result = await scanActivationTargets();
    expect(result.targets.map((target) => target.toolId)).toEqual([
      "claude_code",
      "claude_desktop",
      "codex_desktop",
      "pi",
      "dsh_web",
    ]);
    expect(native).toHaveBeenCalledWith("scan_activation_targets_v1", {
      request: { requestId: expect.stringMatching(/^target-scan-/) },
    });
  });
});

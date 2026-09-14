import { invoke } from "@tauri-apps/api/core";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  ACTIVATION_REQUEST_DEADLINE_MS,
  ACTIVATION_TARGET_SCAN_DEADLINE_MS,
  activateDesktopTool,
  cancelDesktopToolActivation,
  decodeActivationProgress,
  decodeActivationTargetScan,
  decodeToolActivation,
  scanActivationTargets,
} from "./activation";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
const native = vi.mocked(invoke);

function projection(requestId = "activate-test") {
  return {
    requestId,
    schemaVersion: 3,
    status: "ready",
    toolId: "codex_desktop",
    modelId: "glm-5.3",
    billingGroup: "国模特价分组",
    observedAtEpochMs: 1_788_195_600_000,
    reasonCode: "configuration_ready",
  };
}

function targetScan(requestId = "target-scan-test") {
  const target = (
    toolId: "claude_code" | "claude_desktop" | "codex_desktop" | "pi" | "dsh_web",
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
afterEach(() => {
  vi.useRealTimers();
  vi.restoreAllMocks();
});

describe("desktop tool activation boundary", () => {
  it("ru042 validates model-set responses and rejects silent extra enrollment", async () => {
    const bindings = [
      { modelId: "glm-5.3", billingGroup: "国模特价分组" },
      { modelId: "gpt-6-astra", billingGroup: "default" },
    ];
    const response = { ...projection(), schemaVersion: 4, models: bindings };
    expect(decodeToolActivation(response, "activate-test")).not.toBeNull();
    for (const models of [
      [],
      [...bindings, bindings[0]],
      [{ ...bindings[0], apiKey: "synthetic-secret" }],
    ]) {
      expect(
        decodeToolActivation({ ...response, models }, "activate-test"),
      ).toBeNull();
    }
    native.mockImplementation(async (_, args) => ({
      ...response,
      requestId: (args as { request: { requestId: string } }).request.requestId,
      models: [
        ...bindings,
        { modelId: "not-selected", billingGroup: "default" },
      ],
    }));
    await expect(
      activateDesktopTool({
        lineId: "mainland_optimized",
        toolId: "codex_desktop",
        modelId: "glm-5.3",
        billingGroup: "国模特价分组",
        installationId: "i0123456789abcdef",
        models: bindings,
      }),
    ).rejects.toThrow("invalid_tool_activation_projection");
  });
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
    native.mockImplementation(async (_, args) =>
      projection(
        (args as { request: { requestId: string } }).request.requestId,
      ),
    );
    const result = await activateDesktopTool({
      lineId: "global_accelerated",
      toolId: "codex_desktop",
      modelId: "glm-5.3",
      installationId: "i0123456789abcdef",
      billingGroup: "国模特价分组",
    });
    expect(result.status).toBe("ready");
    expect(native).toHaveBeenCalledWith("configure_desktop_tool_v2", {
      request: {
        requestId: expect.stringMatching(/^activate-/),
        lineId: "global_accelerated",
        toolId: "codex_desktop",
        modelId: "glm-5.3",
        installationId: "i0123456789abcdef",
        billingGroup: "国模特价分组",
      },
    });
  });

  it("accepts the bounded running-app handoff and sends restart consent only after confirmation", async () => {
    vi.spyOn(Date, "now").mockReturnValue(2234);
    native.mockImplementation(async (_, args) => ({
      ...projection(
        (args as { request: { requestId: string } }).request.requestId,
      ),
      status: "application_running",
      reasonCode: "save_work_before_restart",
    }));
    const result = await activateDesktopTool({
      lineId: "mainland_optimized",
      toolId: "codex_desktop",
      modelId: "glm-5.3",
      installationId: "i0123456789abcdef",
      billingGroup: "国模特价分组",
      restartRunningApp: true,
    });
    expect(result.status).toBe("application_running");
    expect(native).toHaveBeenCalledWith("configure_desktop_tool_v2", {
      request: expect.objectContaining({ restartRunningApp: true }),
    });
  });

  it("recovers the renderer and requests native cancellation after a hard deadline", async () => {
    vi.useFakeTimers();
    native.mockImplementation((command, args) => {
      if (command === "cancel_tool_activation_v1") {
        return Promise.resolve({
          requestId: (args as { request: { requestId: string } }).request
            .requestId,
          status: "cancel_requested",
        });
      }
      return new Promise(() => undefined);
    });
    const activation = activateDesktopTool({
      lineId: "mainland_optimized",
      toolId: "codex_desktop",
      modelId: "glm-5.3",
      installationId: "i0123456789abcdef",
      billingGroup: "国模特价分组",
    });
    const assertion = expect(activation).rejects.toThrow(
      "activation_request_timed_out",
    );
    await vi.advanceTimersByTimeAsync(ACTIVATION_REQUEST_DEADLINE_MS);
    await assertion;
    expect(native).toHaveBeenCalledWith("cancel_tool_activation_v1", {
      request: { requestId: expect.stringMatching(/^activate-/) },
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

  it("releases a target scan when the native reply is lost", async () => {
    vi.useFakeTimers();
    native.mockImplementation(async () => await new Promise(() => {}));
    const scan = scanActivationTargets();
    const assertion = expect(scan).rejects.toThrow(
      "activation_target_scan_timed_out",
    );
    await vi.advanceTimersByTimeAsync(ACTIVATION_TARGET_SCAN_DEADLINE_MS);
    await assertion;
  });

  it("accepts future and unread versions as metadata, without a version whitelist", () => {
    for (const version of ["2027.999.999.1", "", "future-beta"]) {
      const scan = targetScan();
      scan.targets.forEach((target) => {
        target.installations[0].version = version;
      });
      expect(decodeActivationTargetScan(scan, scan.requestId)).toEqual(scan);
    }
  });

  it("does not accept success for a different billing group", async () => {
    native.mockImplementation(async (_command, args) => ({
      ...projection(
        (args as { request: { requestId: string } }).request.requestId,
      ),
      billingGroup: "default",
    }));
    await expect(
      activateDesktopTool({
        lineId: "mainland_optimized",
        toolId: "codex_desktop",
        modelId: "glm-5.3",
        installationId: "i0123456789abcdef",
        billingGroup: "国模特价分组",
      }),
    ).rejects.toThrow("invalid_tool_activation_projection");
  });

  it("accepts only bounded progress for the matching seven-adapter contract", () => {
    const progress = {
      requestId: "activate-safe",
      toolId: "pi",
      stage: "securing_access",
      completedSteps: 4,
      totalSteps: 7,
    };
    expect(decodeActivationProgress(progress)).toEqual(progress);
    for (const toolId of ["codex_desktop", "claude_desktop"]) {
      const startup = {
        ...progress,
        toolId,
        stage: "checking_application_started",
        completedSteps: 7,
      };
      expect(decodeActivationProgress(startup)).toEqual(startup);
    }
    expect(
      decodeActivationProgress({ ...progress, completedSteps: 8 }),
    ).toBeNull();
    expect(
      decodeActivationProgress({ ...progress, stage: "reading_secrets" }),
    ).toBeNull();
    expect(
      decodeActivationProgress({ ...progress, accessToken: "secret" }),
    ).toBeNull();
  });

  it("cancels only an exact active request and validates the reply", async () => {
    native.mockResolvedValue({
      requestId: "activate-safe",
      status: "cancel_requested",
    });
    await expect(cancelDesktopToolActivation("activate-safe")).resolves.toBe(
      "cancel_requested",
    );
    expect(native).toHaveBeenCalledWith("cancel_tool_activation_v1", {
      request: { requestId: "activate-safe" },
    });
    await expect(cancelDesktopToolActivation("bad\nrequest")).rejects.toThrow(
      "invalid_activation_cancel_request",
    );
  });
});

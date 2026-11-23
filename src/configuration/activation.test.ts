import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
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
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn() }));
const native = vi.mocked(invoke);
const events = vi.mocked(listen);
/** 假装后端报了一次进度：发给这次接入正在听的那个回调。 */
const report = (stage: string) => {
  const hear = events.mock.calls.at(-1)![1] as (event: {
    payload: unknown;
  }) => void;
  const { requestId } = (
    native.mock.calls.find(
      ([command]) => command === "configure_desktop_tool_v2",
    )![1] as { request: { requestId: string } }
  ).request;
  hear({
    payload: {
      requestId,
      toolId: "opencode",
      stage,
      completedSteps: 7,
      totalSteps: 7,
    },
  });
};

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
    toolId:
      | "claude_code"
      | "claude_desktop"
      | "codex_desktop"
      | "pi"
      | "opencode"
      | "dsh_web"
      | "workbuddy"
      | "dsh_desktop"
      | "cline"
      | "hermes"
      | "openclaw",
    displayName: string,
  ) => ({
    toolId,
    displayName,
    surface: "本机应用",
    manualLocation: "none",
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
      target("codex_desktop", "Codex"),
      target("pi", "Pi"),
      target("opencode", "OpenCode"),
      target("dsh_web", "DSH web"),
      target("workbuddy", "WorkBuddy"),
      target("dsh_desktop", "DeepSeek Harness"),
      target("cline", "Cline"),
      target("hermes", "Hermes Agent"),
      target("openclaw", "OpenClaw"),
    ],
  };
}

beforeEach(() => {
  native.mockReset();
  events.mockReset();
  events.mockResolvedValue(() => undefined);
});
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
    // v4 和 v5 都要拦。这条检查以前写死 `schemaVersion === 4`，升到 5 之后
    // 就整个被跳过了 —— 校验器变成摆设，而不会有任何测试红。
    for (const schemaVersion of [4, 5]) {
      native.mockImplementation(async (_, args) => ({
        ...response,
        schemaVersion,
        ...(schemaVersion >= 5 ? { skipped: [] } : {}),
        requestId: (args as { request: { requestId: string } }).request
          .requestId,
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
    }
  });
  it("carries the bindings a v5 native skipped, and rejects a malformed one", () => {
    // 原生从 schema 5 起会说清「哪个模型的分组这次配不出来」。没有这一段，
    // 十七个不同的失败原因都只能显示同一句「稍后重试」，而重试多少次都是
    // 同一个分组失败 —— 用户就卡在那儿了。
    const bindings = [
      { modelId: "glm-5.3-flash", billingGroup: "【特价】glm5.3 flash" },
      { modelId: "kimi-k3", billingGroup: "default" },
    ];
    const v5 = {
      ...projection(),
      schemaVersion: 5,
      modelId: "glm-5.3-flash",
      billingGroup: "【特价】glm5.3 flash",
      models: bindings,
      skipped: [{ ...bindings[0], reasonCode: "server_unavailable" }],
    };
    expect(decodeToolActivation(v5, "activate-test")).toEqual(v5);
    // 成功但没跳过谁，也要能过 —— 那是最常见的一条。
    expect(
      decodeToolActivation({ ...v5, skipped: [] }, "activate-test"),
    ).not.toBeNull();
    for (const skipped of [
      undefined,
      "not-an-array",
      [{ modelId: "glm-5.3-flash", billingGroup: "x" }], // 少了 reasonCode
      [{ ...bindings[0], reasonCode: "x", apiKey: "synthetic-secret" }],
    ]) {
      expect(
        decodeToolActivation({ ...v5, skipped }, "activate-test"),
      ).toBeNull();
    }
    // 旧原生（v4，没有 skipped）仍然要能过：升级期间两边不一定同时更新。
    const { skipped: _skipped, ...v4 } = { ...v5, schemaVersion: 4 };
    expect(decodeToolActivation(v4, "activate-test")).not.toBeNull();
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

  it("accepts only a complete installation scan", () => {
    expect(
      decodeActivationTargetScan(targetScan(), "target-scan-test"),
    ).toEqual(targetScan());
    expect(
      decodeActivationTargetScan(
        { ...targetScan(), targets: targetScan().targets.slice(0, -1) },
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

  it("does not time the step that waits for the user to pick a folder, and resumes the clock after it", async () => {
    vi.useFakeTimers();
    let finish: (value: unknown) => void = () => undefined;
    native.mockImplementation((command, args) => {
      if (command === "cancel_tool_activation_v1")
        return Promise.resolve({
          requestId: (args as { request: { requestId: string } }).request
            .requestId,
          status: "cancel_requested",
        });
      return new Promise((resolve) => {
        finish = resolve;
      });
    });
    const activation = activateDesktopTool({
      lineId: "mainland_optimized",
      toolId: "opencode",
      modelId: "glm-5.3",
      installationId: "i0123456789abcdef",
      billingGroup: "国模特价分组",
      opensInTerminal: true,
    });
    const settled = vi.fn();
    activation.then(settled, settled);

    await vi.advanceTimersByTimeAsync(ACTIVATION_REQUEST_DEADLINE_MS - 1000);
    report("opening_application");
    await vi.advanceTimersByTimeAsync(ACTIVATION_REQUEST_DEADLINE_MS * 5);
    expect(settled).not.toHaveBeenCalled();
    expect(native).not.toHaveBeenCalledWith(
      "cancel_tool_activation_v1",
      expect.anything(),
    );

    const { requestId } = (
      native.mock.calls[0]![1] as { request: { requestId: string } }
    ).request;
    expect(native.mock.calls[0]![1]).not.toHaveProperty(
      "request.opensInTerminal",
    );
    finish({ ...projection(requestId), toolId: "opencode" });
    await expect(activation).resolves.toMatchObject({ status: "ready" });
  });

  it("still gives up when a later step hangs: only the time left before the untimed step remains", async () => {
    vi.useFakeTimers();
    native.mockImplementation((command, args) => {
      if (command === "cancel_tool_activation_v1")
        return Promise.resolve({
          requestId: (args as { request: { requestId: string } }).request
            .requestId,
          status: "cancel_requested",
        });
      return new Promise(() => undefined);
    });
    const activation = activateDesktopTool({
      lineId: "mainland_optimized",
      toolId: "opencode",
      modelId: "glm-5.3",
      installationId: "i0123456789abcdef",
      billingGroup: "国模特价分组",
      opensInTerminal: true,
    });
    const assertion = expect(activation).rejects.toThrow(
      "activation_request_timed_out",
    );
    await vi.advanceTimersByTimeAsync(ACTIVATION_REQUEST_DEADLINE_MS - 1000);
    report("opening_application");
    await vi.advanceTimersByTimeAsync(ACTIVATION_REQUEST_DEADLINE_MS);
    report("checking_application_started");
    await vi.advanceTimersByTimeAsync(1000);
    await assertion;
  });

  it("keeps the deadline while a desktop app is being opened: nobody is picking a folder there", async () => {
    vi.useFakeTimers();
    native.mockImplementation((command, args) => {
      if (command === "cancel_tool_activation_v1")
        return Promise.resolve({
          requestId: (args as { request: { requestId: string } }).request
            .requestId,
          status: "cancel_requested",
        });
      return new Promise(() => undefined);
    });
    const activation = activateDesktopTool({
      lineId: "mainland_optimized",
      toolId: "workbuddy",
      modelId: "glm-5.3",
      installationId: "i0123456789abcdef",
      billingGroup: "国模特价分组",
    });
    const assertion = expect(activation).rejects.toThrow(
      "activation_request_timed_out",
    );
    await vi.advanceTimersByTimeAsync(1000);
    report("opening_application");
    await vi.advanceTimersByTimeAsync(ACTIVATION_REQUEST_DEADLINE_MS - 1000);
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
      "opencode",
      "dsh_web",
      "workbuddy",
      "dsh_desktop",
      "cline",
      "hermes",
      "openclaw",
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

  it("an installation may say it opens in a terminal, and nothing else may ride along", () => {
    const mark = (terminal: unknown) => {
      const scan = targetScan();
      Object.assign(scan.targets[0].installations[0], { terminal });
      return decodeActivationTargetScan(scan, scan.requestId);
    };
    expect(mark(true)?.targets[0].installations[0].terminal).toBe(true);
    expect(mark(false)).toBeNull();
    expect(mark("yes")).toBeNull();
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

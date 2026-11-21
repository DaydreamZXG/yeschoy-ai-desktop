import { beforeEach, describe, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import en from "../i18n/locales/en.json";
import zh from "../i18n/locales/zh.json";
import {
  ACTIVATION_TOOL_IDS,
  decodeActivationTargetScan,
  type ActivationTargetScan,
} from "../configuration/activation";
import { decodeInstallation, type InstallationProgress } from "./api";
import {
  MANUAL_LOCATION_OUTCOMES,
  pickManualLocation,
} from "./manualLocationApi";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
const native = vi.mocked(invoke);

function progress(
  patch: Partial<InstallationProgress> = {},
): InstallationProgress {
  return {
    schemaVersion: 2,
    requestId: "fixture",
    toolId: "codex_desktop",
    jobId: "",
    mode: "automatic",
    phase: "idle",
    downloadedBytes: 0,
    totalBytes: 0,
    canCancel: false,
    reasonCode: "none",
    source: "none",
    platform: "macos",
    architecture: "arm64",
    disposition: "none",
    installationId: "",
    ...patch,
  };
}
function targets(): ActivationTargetScan {
  return {
    schemaVersion: 1,
    requestId: "test",
    platform: "macos",
    targets: ACTIVATION_TOOL_IDS.map((toolId) => ({
      toolId,
      displayName: toolId,
      surface: "桌面应用",
      manualLocation: "none",
      status: "not_found",
      installations: [],
    })),
  };
}

beforeEach(() => {
  native.mockReset();
});

describe("后端报回来的安装进度", () => {
  it("只认内置的下载来源", () => {
    expect(
      decodeInstallation(progress({ source: "mirror" }), "fixture")?.source,
    ).toBe("mirror");
    expect(
      decodeInstallation(
        { ...progress(), source: "unknown-provider" },
        "fixture",
      ),
    ).toBeNull();
  });

  it("多出来的字段、对不上的数、不像样的位置代号都不认", () => {
    expect(decodeInstallation(progress(), "fixture")).not.toBeNull();
    for (const patch of [
      { token: "secret" },
      { downloadedBytes: 200, totalBytes: 100 },
      { phase: "installed", canCancel: true },
      { installationId: "../../path" },
    ])
      expect(
        decodeInstallation({ ...progress(), ...patch }, "fixture"),
      ).toBeNull();
  });
});

describe("用户自己指应用在哪", () => {
  it("只把应用的代号发给后端，路径不经过界面", async () => {
    native.mockImplementation(async (_command, args) => ({
      requestId: (args as { request: { requestId: string } }).request.requestId,
      outcome: "saved",
    }));
    expect(await pickManualLocation("dsh_desktop")).toBe("saved");
    const [command, args] = native.mock.calls[0];
    expect(command).toBe("manual_location_pick");
    expect(Object.keys((args as { request: object }).request).sort()).toEqual([
      "requestId",
      "toolId",
    ]);
  });

  it("回的不是这一次的请求、或者是不认识的结果：算没成", async () => {
    native.mockResolvedValue({ requestId: "someone-else", outcome: "saved" });
    expect(await pickManualLocation("dsh_desktop")).toBe("unavailable");
    native.mockImplementation(async (_command, args) => ({
      requestId: (args as { request: { requestId: string } }).request.requestId,
      outcome: "C:\\Users\\x",
    }));
    expect(await pickManualLocation("dsh_desktop")).toBe("unavailable");
    native.mockRejectedValue(new Error("no bridge"));
    expect(await pickManualLocation("dsh_desktop")).toBe("unavailable");
  });

  it("每种结果在接入屏上两种语言都有一句话", () => {
    for (const locale of [zh, en])
      for (const outcome of MANUAL_LOCATION_OUTCOMES)
        expect(locale.kaifan.join.spot).toHaveProperty(outcome);
  });

  it("扫描结果里手动位置的状态不认识：整份不认", () => {
    const valid = targets();
    expect(decodeActivationTargetScan(valid, "test")).not.toBeNull();
    expect(
      decodeActivationTargetScan(
        {
          ...valid,
          targets: valid.targets.map((target, index) =>
            index === 0
              ? { ...target, manualLocation: "C:\\Users\\x" }
              : target,
          ),
        },
        "test",
      ),
    ).toBeNull();
  });
});

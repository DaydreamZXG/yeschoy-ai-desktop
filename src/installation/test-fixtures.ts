import {
  ACTIVATION_TOOL_IDS,
  type ActivationToolId,
} from "../configuration/activation";
import type { InstallationProgress } from "./api";

/** Passive-navigation fixtures permit only exact read-only inspection. */
export function installationInspectionFixture(
  args: unknown,
): InstallationProgress {
  const request = (args as { request?: Record<string, unknown> })?.request;
  if (
    !request ||
    request.action !== "inspect" ||
    request.jobId !== "" ||
    Object.keys(request).sort().join(",") !== "action,jobId,requestId,toolId" ||
    typeof request.requestId !== "string" ||
    !ACTIVATION_TOOL_IDS.includes(request.toolId as ActivationToolId)
  ) {
    throw new Error(
      "Passive navigation must not install, open a guide, or confirm a connection",
    );
  }
  return {
    schemaVersion: 2,
    requestId: request.requestId,
    toolId: request.toolId as ActivationToolId,
    jobId: "",
    phase: "idle",
    mode: ["claude_desktop", "codex_desktop"].includes(String(request.toolId))
      ? "automatic"
      : "guided",
    downloadedBytes: 0,
    totalBytes: 0,
    canCancel: false,
    source: "none",
    platform: "macos",
    architecture: "arm64",
    reasonCode: "none",
    disposition: "none",
    installationId: "",
  };
}

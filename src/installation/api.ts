import { invoke } from "@tauri-apps/api/core";
import {
  ACTIVATION_TOOL_IDS,
  decodeModelBindings,
  type ActivationToolId,
  type ModelBinding,
} from "../configuration/activation";
import type { ConfigurationLineId } from "../configuration/preview";

export interface InstallationIntent {
  lineId: ConfigurationLineId;
  modelId: string;
  billingGroup: string;
  models: ModelBinding[];
}
export const INSTALL_PHASES = [
  "idle",
  "checking",
  "downloading",
  "verifying",
  "installing",
  "checking_install",
  "awaiting_system_confirmation",
  "installed",
  "cancelled",
  "failed",
] as const;
export interface InstallationProgress {
  schemaVersion: 2;
  requestId: string;
  toolId: ActivationToolId;
  jobId: string;
  mode: "automatic" | "system_assisted" | "guided" | "unsupported";
  phase: (typeof INSTALL_PHASES)[number];
  downloadedBytes: number;
  totalBytes: number;
  canCancel: boolean;
  reasonCode: string;
  source: "official" | "mirror" | "none";
  platform: "macos" | "windows" | "linux" | "unknown";
  architecture: "arm64" | "x64" | "unknown";
  disposition: "none" | "created" | "existing" | "confirmed" | "unconfirmed";
  installationId: string;
}
export type InstallationAction =
  | "inspect"
  | "start"
  | "cancel"
  | "help"
  | "confirm"
  | "reopen";
const keys = [
  "schemaVersion",
  "requestId",
  "toolId",
  "jobId",
  "mode",
  "phase",
  "downloadedBytes",
  "totalBytes",
  "canCancel",
  "reasonCode",
  "source",
  "platform",
  "architecture",
  "disposition",
  "installationId",
];
const oneOf = (v: unknown, choices: readonly string[]) =>
  typeof v === "string" && choices.includes(v);
export function decodeInstallation(
  value: unknown,
  id: string,
): InstallationProgress | null {
  if (!value || typeof value !== "object" || Array.isArray(value)) return null;
  const v = value as Record<string, unknown>;
  if (
    Object.keys(v).length !== keys.length ||
    keys.some((k) => !(k in v)) ||
    v.schemaVersion !== 2 ||
    v.requestId !== id ||
    !oneOf(v.toolId, ACTIVATION_TOOL_IDS) ||
    !oneOf(v.phase, INSTALL_PHASES) ||
    !oneOf(v.mode, ["automatic", "system_assisted", "guided", "unsupported"]) ||
    !oneOf(v.source, ["official", "mirror", "none"]) ||
    !oneOf(v.platform, ["macos", "windows", "linux", "unknown"]) ||
    !oneOf(v.architecture, ["arm64", "x64", "unknown"]) ||
    !oneOf(v.disposition, [
      "none",
      "created",
      "existing",
      "confirmed",
      "unconfirmed",
    ]) ||
    typeof v.jobId !== "string" ||
    !/^[A-Za-z0-9_-]{0,64}$/.test(v.jobId) ||
    typeof v.installationId !== "string" ||
    !/^[A-Za-z0-9_-]{0,128}$/.test(v.installationId) ||
    typeof v.reasonCode !== "string" ||
    !/^[a-z0-9_]{1,80}$/.test(v.reasonCode) ||
    typeof v.canCancel !== "boolean" ||
    ![v.downloadedBytes, v.totalBytes].every(
      (n) =>
        typeof n === "number" &&
        Number.isSafeInteger(n) &&
        n >= 0 &&
        n <= 4 * 1024 ** 3,
    ) ||
    (v.downloadedBytes as number) > (v.totalBytes as number)
  )
    return null;
  if (v.canCancel && !["checking", "downloading"].includes(v.phase as string))
    return null;
  if (
    v.installationId &&
    (v.phase !== "installed" ||
      !["created", "confirmed"].includes(v.disposition as string))
  )
    return null;
  return value as InstallationProgress;
}
let sequence = 0;
export async function installationAction(
  toolId: ActivationToolId,
  action: InstallationAction,
  jobId = "",
  intent?: InstallationIntent,
) {
  if (
    intent &&
    (!decodeModelBindings(intent.models) ||
      !intent.models.some(
        (m) =>
          m.modelId === intent.modelId &&
          m.billingGroup === intent.billingGroup,
      ))
  )
    throw new Error("invalid_installation_intent");
  const requestId = `install-${Date.now().toString(36)}-${++sequence}`;
  const value = await invoke<unknown>("manage_app_installation_v2", {
    request: {
      requestId,
      toolId,
      action,
      jobId,
      ...(intent ? { intent } : {}),
    },
  });
  const progress = decodeInstallation(value, requestId);
  if (!progress) throw new Error("invalid_installation_progress");
  return progress;
}
export function installationActive(value: InstallationProgress | null) {
  return (
    !!value &&
    !["idle", "installed", "cancelled", "failed"].includes(value.phase)
  );
}

/** Value equality alone misses A → B → A. Each observed change retires consent. */
export class SelectionRevision {
  private key = "";
  private revision = 0;
  observe(key: string) {
    if (key !== this.key) {
      this.key = key;
      this.revision++;
    }
    return this.revision;
  }
}

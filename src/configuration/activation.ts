import { invoke } from "@tauri-apps/api/core";
import type { ConfigurationLineId } from "./preview";

// PRD §3.2 定案：与 Rust 侧 tool_adapters::TARGETS / CONNECTION_TOOLS 保持
// 一致（6 个已支持工具）。遗留 hermes/openclaw 已无适配器，禁止回加；
// 增删工具须先改 appCatalog 展示源并通过强校验，再同步此处与 Rust。
export const ACTIVATION_TOOL_IDS = [
  "claude_code",
  "claude_desktop",
  "codex_desktop",
  "pi",
  "dsh_web",
  "workbuddy",
] as const;

export type ActivationToolId = (typeof ACTIVATION_TOOL_IDS)[number];

export interface ModelBinding {
  modelId: string;
  billingGroup: string;
}
export function decodeModelBindings(value: unknown): ModelBinding[] | null {
  if (!Array.isArray(value) || value.length > 200) return null;
  if (
    value.some(
      (v) =>
        !object(v) ||
        !exactKeys(v, ["modelId", "billingGroup"]) ||
        !safeText(v.modelId, 200, false) ||
        !safeText(v.billingGroup, 128, false) ||
        v.billingGroup === "auto",
    )
  )
    return null;
  if (new Set(value.map((v) => v.modelId)).size !== value.length) return null;
  return value as ModelBinding[];
}
export function sameModelBindings(a: ModelBinding[], b: ModelBinding[]) {
  return (
    a.length === b.length &&
    a.every((m) =>
      b.some(
        (n) => n.modelId === m.modelId && n.billingGroup === m.billingGroup,
      ),
    )
  );
}

export const ACTIVATION_TARGET_STATUSES = [
  "not_found",
  "available",
  "selection_required",
  "missing_runtime",
] as const;

export type ActivationTargetStatus =
  (typeof ACTIVATION_TARGET_STATUSES)[number];

export interface ActivationInstallation {
  installationId: string;
  label: string;
  version: string;
  supported: boolean;
  recommended: boolean;
}

export interface ActivationTarget {
  toolId: ActivationToolId;
  displayName: string;
  surface: string;
  status: ActivationTargetStatus;
  installations: ActivationInstallation[];
}

export interface ActivationTargetScan {
  requestId: string;
  schemaVersion: 1;
  platform: "macos" | "windows" | "linux" | "unknown";
  targets: ActivationTarget[];
}

export const TOOL_ACTIVATION_STATUSES = [
  "ready",
  "application_running",
  "signed_out",
  "tool_not_found",
  "multiple_installations",
  "missing_runtime",
  "unsupported_profile",
  "unsupported_model",
  "external_override",
  "secure_storage_unavailable",
  "configuration_failed",
  "launch_failed",
  "verification_failed",
  "server_unavailable",
  "invalid_request",
] as const;

export type ToolActivationStatus = (typeof TOOL_ACTIVATION_STATUSES)[number];

export interface ToolActivationProjection {
  requestId: string;
  schemaVersion: 3 | 4;
  status: ToolActivationStatus;
  toolId: ActivationToolId;
  modelId: string;
  billingGroup: string;
  observedAtEpochMs: number;
  reasonCode: string;
  models?: ModelBinding[];
}

export const ACTIVATION_PROGRESS_EVENT = "yeschoy://activation-progress";
// Discovery is read-only, but Windows package metadata and trust checks can be
// delayed by a damaged install or security software. Never leave the setup
// action in a permanent "checking" state when the native reply is lost.
export const ACTIVATION_TARGET_SCAN_DEADLINE_MS = 15_000;
// Native activation includes bounded account calls and a desktop restart. The
// renderer must still recover if the OS, keychain, installer, or network stack
// never returns. Native cancellation then performs transaction cleanup.
export const ACTIVATION_REQUEST_DEADLINE_MS = 90_000;
export const ACTIVATION_PROGRESS_STAGES = [
  "queued",
  "checking_application",
  "authenticating",
  "checking_models",
  "securing_access",
  "preparing_settings",
  "applying_settings",
  "restoring_settings",
  "opening_application",
  "checking_application_started",
  "complete",
] as const;

export interface ActivationProgress {
  requestId: string;
  toolId: ActivationToolId;
  stage: (typeof ACTIVATION_PROGRESS_STAGES)[number];
  completedSteps: number;
  totalSteps: number;
}

export function decodeActivationProgress(
  value: unknown,
): ActivationProgress | null {
  if (!object(value)) return null;
  if (
    !exactKeys(value, [
      "requestId",
      "toolId",
      "stage",
      "completedSteps",
      "totalSteps",
    ]) ||
    !safeText(value.requestId, 120, false) ||
    !ACTIVATION_TOOL_IDS.includes(value.toolId as ActivationToolId) ||
    !ACTIVATION_PROGRESS_STAGES.includes(
      value.stage as ActivationProgress["stage"],
    ) ||
    !Number.isSafeInteger(value.completedSteps) ||
    !Number.isSafeInteger(value.totalSteps) ||
    (value.completedSteps as number) < 0 ||
    (value.totalSteps as number) < 1 ||
    (value.completedSteps as number) > (value.totalSteps as number)
  )
    return null;
  return value as unknown as ActivationProgress;
}

function object(value: unknown): value is Record<string, unknown> {
  return !!value && typeof value === "object" && !Array.isArray(value);
}

function exactKeys(value: Record<string, unknown>, keys: readonly string[]) {
  return (
    Object.keys(value).length === keys.length &&
    keys.every((key) => Object.prototype.hasOwnProperty.call(value, key))
  );
}

function safeText(
  value: unknown,
  maximum: number,
  empty = true,
): value is string {
  return (
    typeof value === "string" &&
    (empty || value.length > 0) &&
    Array.from(value).length <= maximum &&
    !/[\u0000-\u001f\u007f-\u009f\u202a-\u202e\u2066-\u2069]/u.test(value)
  );
}

function decodeInstallation(value: unknown): ActivationInstallation | null {
  if (!object(value)) return null;
  if (
    !exactKeys(value, [
      "installationId",
      "label",
      "version",
      "supported",
      "recommended",
    ]) ||
    !/^i[0-9a-f]{16}$/.test(String(value.installationId)) ||
    !safeText(value.label, 100, false) ||
    !safeText(value.version, 80) ||
    typeof value.supported !== "boolean" ||
    typeof value.recommended !== "boolean"
  )
    return null;
  return value as unknown as ActivationInstallation;
}

function decodeTarget(value: unknown): ActivationTarget | null {
  if (!object(value)) return null;
  if (
    !exactKeys(value, [
      "toolId",
      "displayName",
      "surface",
      "status",
      "installations",
    ]) ||
    !ACTIVATION_TOOL_IDS.includes(value.toolId as ActivationToolId) ||
    !safeText(value.displayName, 80, false) ||
    !safeText(value.surface, 120, false) ||
    !ACTIVATION_TARGET_STATUSES.includes(
      value.status as ActivationTargetStatus,
    ) ||
    !Array.isArray(value.installations) ||
    value.installations.length > 8
  )
    return null;
  const installations = value.installations.map(decodeInstallation);
  if (installations.some((item) => item === null)) return null;
  return {
    ...(value as unknown as Omit<ActivationTarget, "installations">),
    installations: installations as ActivationInstallation[],
  };
}

export function decodeActivationTargetScan(
  value: unknown,
  requestId: string,
): ActivationTargetScan | null {
  if (!object(value)) return null;
  if (
    !exactKeys(value, ["requestId", "schemaVersion", "platform", "targets"]) ||
    value.requestId !== requestId ||
    value.schemaVersion !== 1 ||
    !["macos", "windows", "linux", "unknown"].includes(
      String(value.platform),
    ) ||
    !Array.isArray(value.targets) ||
    value.targets.length !== ACTIVATION_TOOL_IDS.length
  )
    return null;
  const targets = value.targets.map(decodeTarget);
  if (targets.some((target) => target === null)) return null;
  const toolIds = targets.map((target) => target!.toolId);
  if (
    new Set(toolIds).size !== ACTIVATION_TOOL_IDS.length ||
    !ACTIVATION_TOOL_IDS.every((toolId) => toolIds.includes(toolId))
  )
    return null;
  return {
    ...(value as unknown as Omit<ActivationTargetScan, "targets">),
    targets: targets as ActivationTarget[],
  };
}

export function decodeToolActivation(
  value: unknown,
  requestId: string,
): ToolActivationProjection | null {
  if (!object(value)) return null;
  if (
    !exactKeys(value, [
      "requestId",
      "schemaVersion",
      "status",
      "toolId",
      "modelId",
      "billingGroup",
      "observedAtEpochMs",
      "reasonCode",
      ...(value.schemaVersion === 4 ? ["models"] : []),
    ]) ||
    value.requestId !== requestId ||
    ![3, 4].includes(Number(value.schemaVersion)) ||
    (value.schemaVersion === 4 &&
      (!decodeModelBindings(value.models) ||
        !(value.models as ModelBinding[]).some(
          (m) =>
            m.modelId === value.modelId &&
            m.billingGroup === value.billingGroup,
        ))) ||
    !TOOL_ACTIVATION_STATUSES.includes(value.status as ToolActivationStatus) ||
    !ACTIVATION_TOOL_IDS.includes(value.toolId as ActivationToolId) ||
    !safeText(value.modelId, 200, false) ||
    !safeText(value.billingGroup, 128, false) ||
    !Number.isSafeInteger(value.observedAtEpochMs) ||
    (value.observedAtEpochMs as number) < 0 ||
    !safeText(value.reasonCode, 80, false)
  )
    return null;
  return value as unknown as ToolActivationProjection;
}

let scanSequence = 0;
let activationSequence = 0;

export async function scanActivationTargets(): Promise<ActivationTargetScan> {
  scanSequence += 1;
  const requestId = `target-scan-${Date.now().toString(36)}-${scanSequence.toString(36)}`;
  let deadline: ReturnType<typeof setTimeout> | undefined;
  const raw = await Promise.race([
    invoke<unknown>("scan_activation_targets_v1", {
      request: { requestId },
    }),
    new Promise<never>((_resolve, reject) => {
      deadline = setTimeout(
        () => reject(new Error("activation_target_scan_timed_out")),
        ACTIVATION_TARGET_SCAN_DEADLINE_MS,
      );
    }),
  ]).finally(() => {
    if (deadline !== undefined) clearTimeout(deadline);
  });
  const result = decodeActivationTargetScan(raw, requestId);
  if (!result) throw new Error("invalid_activation_target_scan");
  return result;
}

export async function activateDesktopTool(input: {
  lineId: ConfigurationLineId;
  toolId: ActivationToolId;
  modelId: string;
  installationId: string;
  billingGroup: string;
  models?: ModelBinding[];
  installationJobId?: string;
  restartRunningApp?: boolean;
  onRequestId?: (requestId: string) => void;
}): Promise<ToolActivationProjection> {
  if (
    input.models &&
    (!decodeModelBindings(input.models) ||
      !input.models.some(
        (m) =>
          m.modelId === input.modelId && m.billingGroup === input.billingGroup,
      ))
  )
    throw new Error("invalid_model_set");
  activationSequence += 1;
  const requestId = `activate-${Date.now().toString(36)}-${activationSequence.toString(36)}`;
  input.onRequestId?.(requestId);
  let deadline: ReturnType<typeof setTimeout> | undefined;
  const nativeRequest = invoke<unknown>("configure_desktop_tool_v2", {
    request: {
      requestId,
      lineId: input.lineId,
      toolId: input.toolId,
      modelId: input.modelId,
      installationId: input.installationId,
      billingGroup: input.billingGroup,
      ...(input.models ? { models: input.models } : {}),
      ...(input.installationJobId
        ? { installationJobId: input.installationJobId }
        : {}),
      ...(input.restartRunningApp ? { restartRunningApp: true } : {}),
    },
  });
  const timedOut = new Promise<never>((_, reject) => {
    deadline = setTimeout(() => {
      void cancelDesktopToolActivation(requestId).catch(() => undefined);
      reject(new Error("activation_request_timed_out"));
    }, ACTIVATION_REQUEST_DEADLINE_MS);
  });
  let raw: unknown;
  try {
    raw = await Promise.race([nativeRequest, timedOut]);
  } finally {
    if (deadline !== undefined) clearTimeout(deadline);
  }
  const result = decodeToolActivation(raw, requestId);
  if (
    !result ||
    result.toolId !== input.toolId ||
    result.modelId !== input.modelId ||
    result.billingGroup !== input.billingGroup ||
    (result.schemaVersion === 4 &&
      !sameModelBindings(
        result.models!,
        input.models ?? [
          { modelId: input.modelId, billingGroup: input.billingGroup },
        ],
      )) ||
    (input.models && input.models.length > 1 && result.schemaVersion !== 4)
  )
    throw new Error("invalid_tool_activation_projection");
  return result;
}

export async function cancelDesktopToolActivation(
  requestId: string,
): Promise<"cancel_requested" | "not_found"> {
  if (!safeText(requestId, 120, false))
    throw new Error("invalid_activation_cancel_request");
  const raw = await invoke<unknown>("cancel_tool_activation_v1", {
    request: { requestId },
  });
  if (
    !object(raw) ||
    !exactKeys(raw, ["requestId", "status"]) ||
    raw.requestId !== requestId ||
    !["cancel_requested", "not_found"].includes(String(raw.status))
  )
    throw new Error("invalid_activation_cancel_response");
  return raw.status as "cancel_requested" | "not_found";
}

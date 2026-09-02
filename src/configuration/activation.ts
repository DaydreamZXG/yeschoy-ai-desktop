import { invoke } from "@tauri-apps/api/core";
import type { DesktopAppId } from "../desktop-apps/contract";
import type { ConfigurationLineId } from "./preview";

export const TOOL_ACTIVATION_STATUSES = [
  "configured",
  "signed_out",
  "unsupported_model",
  "server_unavailable",
  "configuration_failed",
  "invalid_request",
] as const;

export type ToolActivationStatus = (typeof TOOL_ACTIVATION_STATUSES)[number];

export interface ToolActivationProjection {
  requestId: string;
  schemaVersion: 1;
  status: ToolActivationStatus;
  toolId: DesktopAppId;
  modelId: string;
  observedAtEpochMs: number;
  reasonCode: string;
}

function object(value: unknown): value is Record<string, unknown> {
  return !!value && typeof value === "object" && !Array.isArray(value);
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

export function decodeToolActivation(
  value: unknown,
  requestId: string,
): ToolActivationProjection | null {
  if (!object(value)) return null;
  const keys = [
    "requestId",
    "schemaVersion",
    "status",
    "toolId",
    "modelId",
    "observedAtEpochMs",
    "reasonCode",
  ];
  if (
    Object.keys(value).length !== keys.length ||
    !keys.every((key) => Object.prototype.hasOwnProperty.call(value, key)) ||
    value.requestId !== requestId ||
    value.schemaVersion !== 1 ||
    !TOOL_ACTIVATION_STATUSES.includes(value.status as ToolActivationStatus) ||
    !["claude_desktop", "codex_desktop"].includes(value.toolId as string) ||
    !safeText(value.modelId, 200, false) ||
    !Number.isSafeInteger(value.observedAtEpochMs) ||
    (value.observedAtEpochMs as number) < 0 ||
    !safeText(value.reasonCode, 80, false)
  )
    return null;
  return value as unknown as ToolActivationProjection;
}

let sequence = 0;

export async function activateDesktopTool(input: {
  lineId: ConfigurationLineId;
  toolId: DesktopAppId;
  modelId: string;
}): Promise<ToolActivationProjection> {
  sequence += 1;
  const requestId = `activate-${Date.now().toString(36)}-${sequence.toString(36)}`;
  const raw = await invoke<unknown>("configure_desktop_tool_v1", {
    request: { requestId, ...input },
  });
  const result = decodeToolActivation(raw, requestId);
  if (!result) throw new Error("invalid_tool_activation_projection");
  return result;
}

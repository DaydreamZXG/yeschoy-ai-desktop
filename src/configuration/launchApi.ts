import { invoke } from "@tauri-apps/api/core";
import { ACTIVATION_TOOL_IDS, type ActivationToolId } from "./activation";

export const OPENABLE_TOOLS = ACTIVATION_TOOL_IDS;
export const TERMINAL_TOOLS = [
  "claude_code",
  "pi",
  "hermes",
  "openclaw",
] as const;
export const OPEN_STATUSES = [
  "opened",
  "not_connected",
  "settings_changed",
  "recovery_pending",
  "secure_storage_unavailable",
  "tool_not_found",
  "launch_failed",
  "busy",
] as const;
export type OpenStatus = (typeof OPEN_STATUSES)[number];
let sequence = 0;
export async function openConnection(
  toolId: ActivationToolId,
): Promise<OpenStatus> {
  if (!(OPENABLE_TOOLS as readonly string[]).includes(toolId))
    throw Error("invalid_open_target");
  const requestId = `open-${Date.now().toString(36)}-${++sequence}`;
  const raw = await invoke<unknown>("open_tool_connection_v1", {
    request: { requestId, toolId },
  });
  if (!raw || typeof raw !== "object" || Array.isArray(raw))
    throw Error("invalid_open_reply");
  const value = raw as Record<string, unknown>;
  if (
    Object.keys(value).length !== 4 ||
    value.requestId !== requestId ||
    value.toolId !== toolId ||
    value.schemaVersion !== 1 ||
    !OPEN_STATUSES.includes(value.status as OpenStatus)
  )
    throw Error("invalid_open_reply");
  return value.status as OpenStatus;
}

import { invoke } from "@tauri-apps/api/core";
import { ACTIVATION_TOOL_IDS, type ActivationToolId } from "./activation";

export const OPENABLE_TOOLS = ACTIVATION_TOOL_IDS;
export const TERMINAL_TOOLS = ["claude_code", "pi"] as const;
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

/**
 * `reasonCode` is why `status` came out that way. For `launch_failed` it is the
 * adapter's own code — the native side used to collapse every launch failure
 * into the bare status, so a missing workspace and a terminal that would not
 * spawn reached the user as the same sentence.
 */
export interface OpenResult {
  status: OpenStatus;
  reasonCode: string;
}
// Native macOS launch already has a 15-second cap. Five additional seconds
// cover bridge startup and IPC delivery while still guaranteeing that a lost
// native reply cannot leave the renderer in a permanent busy state.
export const OPEN_REQUEST_DEADLINE_MS = 20_000;
let sequence = 0;
export async function openConnection(
  toolId: ActivationToolId,
): Promise<OpenResult> {
  if (!(OPENABLE_TOOLS as readonly string[]).includes(toolId))
    throw Error("invalid_open_target");
  const requestId = `open-${Date.now().toString(36)}-${++sequence}`;
  let deadline: ReturnType<typeof setTimeout> | undefined;
  const raw = await Promise.race([
    invoke<unknown>("open_tool_connection_v1", {
      request: { requestId, toolId },
    }),
    new Promise<never>((_resolve, reject) => {
      deadline = setTimeout(
        () => reject(Error("open_request_timed_out")),
        OPEN_REQUEST_DEADLINE_MS,
      );
    }),
  ]).finally(() => {
    if (deadline !== undefined) clearTimeout(deadline);
  });
  if (!raw || typeof raw !== "object" || Array.isArray(raw))
    throw Error("invalid_open_reply");
  const value = raw as Record<string, unknown>;
  if (
    Object.keys(value).length !== 5 ||
    value.requestId !== requestId ||
    value.toolId !== toolId ||
    value.schemaVersion !== 2 ||
    !OPEN_STATUSES.includes(value.status as OpenStatus) ||
    typeof value.reasonCode !== "string" ||
    !/^[a-z0-9_]{1,80}$/.test(value.reasonCode)
  )
    throw Error("invalid_open_reply");
  return {
    status: value.status as OpenStatus,
    reasonCode: value.reasonCode,
  };
}

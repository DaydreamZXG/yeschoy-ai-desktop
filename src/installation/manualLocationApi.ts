import { invoke } from "@tauri-apps/api/core";
import {
  ACTIVATION_TOOL_IDS,
  type ActivationToolId,
} from "../configuration/activation";

/**
 * 手动指定安装位置。
 *
 * 路径从不经过界面：原生侧弹系统的文件选择框，按自动检测同一套规则
 * （文件名、签名 / bundle id）验过才记住。界面只拿到一个结果码。
 */
export const MANUAL_LOCATION_OUTCOMES = [
  "saved",
  "cleared",
  "cancelled",
  "unavailable",
  "wrong_file",
  "not_genuine",
  "bundled_copy",
  "unsupported_platform",
] as const;

export type ManualLocationOutcome = (typeof MANUAL_LOCATION_OUTCOMES)[number];

let sequence = 0;

async function call(
  command: "manual_location_pick" | "manual_location_clear",
  toolId: ActivationToolId,
): Promise<ManualLocationOutcome> {
  if (!ACTIVATION_TOOL_IDS.includes(toolId)) return "unavailable";
  sequence += 1;
  const requestId = `manual-${Date.now().toString(36)}-${sequence.toString(36)}`;
  try {
    const raw = await invoke<unknown>(command, {
      request: { requestId, toolId },
    });
    if (
      typeof raw !== "object" ||
      raw === null ||
      Object.keys(raw).length !== 2 ||
      (raw as { requestId?: unknown }).requestId !== requestId
    )
      return "unavailable";
    const outcome = (raw as { outcome?: unknown }).outcome;
    return MANUAL_LOCATION_OUTCOMES.includes(outcome as ManualLocationOutcome)
      ? (outcome as ManualLocationOutcome)
      : "unavailable";
  } catch {
    return "unavailable";
  }
}

export const pickManualLocation = (toolId: ActivationToolId) =>
  call("manual_location_pick", toolId);

export const clearManualLocation = (toolId: ActivationToolId) =>
  call("manual_location_clear", toolId);

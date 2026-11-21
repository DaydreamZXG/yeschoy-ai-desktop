/**
 * 「改回原样」做完之后该说哪一句。设置里和接入里说的是同一件事，用同一套话。
 */
import type {
  ConnectionResponse,
  ToolConnection,
} from "../configuration/connections";

export type UndoKey =
  | "restored"
  | "restoredKept"
  | "restoredKeyPending"
  | "removed"
  | "restoreFailed"
  | "restoreError";

export function undoKey(
  result: Pick<ConnectionResponse, "status" | "reasonCode">,
  restoreMode: ToolConnection["restoreMode"],
): UndoKey {
  if (result.status === "recovery_failed") return "restoreFailed";
  if (result.reasonCode === "local_settings_restored_token_cleanup_pending")
    return "restoredKeyPending";
  if (result.status === "restored_with_changes") return "restoredKept";
  return restoreMode === "original" ? "restored" : "removed";
}

/** 没改成的那两种：角色要着急，按钮给「再试一次」。 */
export const undoFailed = (key: UndoKey) =>
  key === "restoreFailed" || key === "restoreError";

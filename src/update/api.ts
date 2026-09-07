import { invoke } from "@tauri-apps/api/core";
import { decodeUpdateProjection, type UpdateProjection } from "./contract";

type UpdateAction = "check" | "install";

export async function runNativeUpdate(
  requestId: string,
  action: UpdateAction,
  expectedVersion = "",
): Promise<UpdateProjection> {
  const raw = await invoke<unknown>("manage_desktop_update_v1", {
    request: { requestId, action, expectedVersion },
  });
  const projection = decodeUpdateProjection(raw, requestId);
  if (!projection) throw new Error("invalid_update_projection");
  return projection;
}

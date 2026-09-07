import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useRef,
  useState,
  type ReactNode,
} from "react";
import { invoke } from "@tauri-apps/api/core";
import {
  ACTIVATION_TOOL_IDS,
  decodeModelBindings,
  type ModelBinding,
  type ActivationToolId,
} from "./activation";
import type { ConfigurationLineId } from "./preview";
import { openConnection, type OpenStatus } from "./launchApi";

export interface ToolConnection {
  toolId: ActivationToolId;
  state:
    | "not_connected"
    | "connected"
    | "changed"
    | "legacy"
    | "recovery_pending"
    | "unavailable";
  modelId: string;
  lineId: ConfigurationLineId | "";
  billingGroup: string;
  updatedAtEpochMs: number;
  restoreMode: "original" | "remove_yeschoy" | "none";
  requiresBackground: boolean;
  reasonCode: string;
  models?: ModelBinding[];
  lastRequest?: RequestObservation;
}
export interface RequestObservation {
  modelId: string;
  billingGroup: string;
  lineId: ConfigurationLineId;
  outcome:
    | "ok"
    | "timeout"
    | "network_error"
    | "upstream_error"
    | "invalid_response"
    | "stream_interrupted"
    | "unknown_model";
  httpStatus: number;
  observedAtEpochMs: number;
}
export interface ConnectionResponse {
  requestId: string;
  schemaVersion: 1 | 2;
  status: "ok" | "restored" | "restored_with_changes" | "recovery_failed";
  connections: ToolConnection[];
  reasonCode: string;
}
const object = (v: unknown): v is Record<string, unknown> =>
  !!v && typeof v === "object" && !Array.isArray(v);
const keys = (v: Record<string, unknown>, names: string[]) =>
  Object.keys(v).length === names.length &&
  names.every((k) => Object.prototype.hasOwnProperty.call(v, k));
const text = (v: unknown, max: number) =>
  typeof v === "string" &&
  v.length <= max &&
  !/[\u0000-\u001f\u007f-\u009f\u202a-\u202e\u2066-\u2069]/u.test(v);
export function decodeConnections(
  value: unknown,
  requestId: string,
): ConnectionResponse | null {
  if (
    !object(value) ||
    !keys(value, [
      "requestId",
      "schemaVersion",
      "status",
      "connections",
      "reasonCode",
    ]) ||
    value.requestId !== requestId ||
    ![1, 2].includes(Number(value.schemaVersion)) ||
    !["ok", "restored", "restored_with_changes", "recovery_failed"].includes(
      String(value.status),
    ) ||
    !text(value.reasonCode, 80) ||
    !Array.isArray(value.connections) ||
    value.connections.length !== 7
  )
    return null;
  for (const c of value.connections) {
    if (
      !object(c) ||
      !keys(c, [
        "toolId",
        "state",
        "modelId",
        "lineId",
        "billingGroup",
        "updatedAtEpochMs",
        "restoreMode",
        "requiresBackground",
        "reasonCode",
        ...(value.schemaVersion === 2 ? ["models"] : []),
        ...(value.schemaVersion === 2 && "lastRequest" in c
          ? ["lastRequest"]
          : []),
      ]) ||
      !ACTIVATION_TOOL_IDS.includes(c.toolId as ActivationToolId) ||
      ![
        "not_connected",
        "connected",
        "changed",
        "legacy",
        "recovery_pending",
        "unavailable",
      ].includes(String(c.state)) ||
      !["", "mainland_optimized", "global_accelerated"].includes(
        String(c.lineId),
      ) ||
      !["original", "remove_yeschoy", "none"].includes(String(c.restoreMode)) ||
      !text(c.modelId, 200) ||
      !text(c.billingGroup, 128) ||
      !text(c.reasonCode, 80) ||
      !Number.isSafeInteger(c.updatedAtEpochMs) ||
      Number(c.updatedAtEpochMs) < 0 ||
      typeof c.requiresBackground !== "boolean" ||
      (value.schemaVersion === 2 && !decodeModelBindings(c.models)) ||
      ("lastRequest" in c && !validObservation(c.lastRequest))
    )
      return null;
  }
  if (new Set(value.connections.map((c) => c.toolId)).size !== 7) return null;
  return value as unknown as ConnectionResponse;
}
function validObservation(v: unknown): v is RequestObservation {
  return (
    object(v) &&
    keys(v, [
      "modelId",
      "billingGroup",
      "lineId",
      "outcome",
      "httpStatus",
      "observedAtEpochMs",
    ]) &&
    text(v.modelId, 200) &&
    text(v.billingGroup, 128) &&
    ["mainland_optimized", "global_accelerated"].includes(String(v.lineId)) &&
    [
      "ok",
      "timeout",
      "network_error",
      "upstream_error",
      "invalid_response",
      "stream_interrupted",
      "unknown_model",
    ].includes(String(v.outcome)) &&
    Number.isInteger(v.httpStatus) &&
    Number(v.httpStatus) >= 0 &&
    Number(v.httpStatus) <= 599 &&
    Number.isSafeInteger(v.observedAtEpochMs) &&
    Number(v.observedAtEpochMs) >= 0
  );
}
let sequence = 0;
async function manage(
  operation: "inspect" | "restore",
  toolId: ActivationToolId | "" = "",
) {
  const requestId = `connections-${Date.now().toString(36)}-${++sequence}`;
  const result = decodeConnections(
    await invoke("manage_tool_connections_v1", {
      request: { requestId, operation, toolId },
    }),
    requestId,
  );
  if (!result) throw new Error("invalid_connection_response");
  return result;
}
export function useToolConnections() {
  const [connections, setConnections] = useState<ToolConnection[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState(false);
  const [restoring, setRestoring] = useState<ActivationToolId | null>(null);
  const [opening, setOpening] = useState<ActivationToolId | null>(null);
  const operation = useRef(false);
  const openingOperation = useRef(false);
  const initialInspectionSettled = useRef(false);
  const revision = useRef(0);
  const mounted = useRef(true);
  const refresh = useCallback(async () => {
    // A focus event must not supersede the result of an active restore. Once an
    // initial projection exists, keep it interactive while a later focus refresh
    // runs instead of turning the whole connection screen back into loading.
    if (operation.current) return;
    const current = ++revision.current;
    if (!initialInspectionSettled.current) setLoading(true);
    try {
      const result = await manage("inspect");
      if (mounted.current && current === revision.current) {
        setConnections(result.connections);
        setError(false);
      }
    } catch {
      if (mounted.current && current === revision.current) setError(true);
    } finally {
      if (mounted.current && current === revision.current) {
        initialInspectionSettled.current = true;
        setLoading(false);
      }
    }
  }, []);
  const restore = useCallback(async (tool: ActivationToolId) => {
    if (operation.current || openingOperation.current)
      throw Error("connection_operation_busy");
    operation.current = true;
    const current = ++revision.current;
    setRestoring(tool);
    try {
      const result = await manage("restore", tool);
      if (mounted.current && current === revision.current) {
        setConnections(result.connections);
        setError(false);
      }
      return result;
    } catch (cause) {
      if (mounted.current && current === revision.current) setError(true);
      throw cause;
    } finally {
      operation.current = false;
      if (mounted.current) setRestoring(null);
      if (mounted.current && current === revision.current) {
        initialInspectionSettled.current = true;
        setLoading(false);
      }
    }
  }, []);
  const open = useCallback(
    async (tool: ActivationToolId): Promise<OpenStatus> => {
      if (operation.current || openingOperation.current) return "busy";
      openingOperation.current = true;
      setOpening(tool);
      try {
        return await openConnection(tool);
      } finally {
        openingOperation.current = false;
        if (mounted.current) setOpening(null);
      }
    },
    [],
  );
  useEffect(() => {
    mounted.current = true;
    void refresh();
    window.addEventListener("focus", refresh);
    return () => {
      mounted.current = false;
      ++revision.current;
      window.removeEventListener("focus", refresh);
    };
  }, [refresh]);
  return {
    connections,
    loading,
    error,
    restoring,
    opening,
    refresh,
    restore,
    open,
  };
}
type Controller = ReturnType<typeof useToolConnections>;
const Context = createContext<Controller | null>(null);
export function ConnectionProvider({
  value,
  children,
}: {
  value: Controller;
  children: ReactNode;
}) {
  return <Context.Provider value={value}>{children}</Context.Provider>;
}
export const useConnections = () => useContext(Context);
export const connectionLabel = (state?: ToolConnection["state"]) =>
  ({
    connected: "已接入",
    changed: "设置已变更",
    legacy: "已有野菜接入",
    recovery_pending: "需要恢复",
    unavailable: "状态待确认",
    not_connected: "未接入",
  })[state ?? "not_connected"];

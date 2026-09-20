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
import i18n from "i18next";
import { useQuery } from "@tanstack/react-query";
import { queryClient } from "../lib/query/queryClient";
import {
  ACTIVATION_TOOL_IDS,
  decodeModelBindings,
  type ModelBinding,
  type ActivationToolId,
} from "./activation";
import type { ConfigurationLineId } from "./preview";
import { openConnection, type OpenResult } from "./launchApi";

export const CONNECTION_INSPECTION_DEADLINE_MS = 15_000;
export const CONNECTION_RESTORE_DEADLINE_MS = 60_000;
const CONNECTION_QUERY_KEY = ["local-tool-connections", 2] as const;

/**
 * 原生侧会送回来的读取失败原因码。文案在语言文件的 `connectionError` 里 ——
 * 这里只留清单，因为 `ConnectionIssue["code"]` 这个联合类型别处在用，
 * 而清单和文案的漂移由 `activationFailureCopy.test.ts` 的守卫盯着。
 */
export const CONNECTION_ERROR_CODES = [
  "connection_partial_unavailable",
  "connection_inspect_timed_out",
  "connection_restore_timed_out",
  "connection_operation_busy",
  "assistant_shutting_down",
  "invalid_connection_response",
  "connection_inspection_failed",
  "connection_call_failed",
] as const;
export interface ConnectionIssue {
  code: (typeof CONNECTION_ERROR_CODES)[number];
  message: string;
  requestId: string;
}
class ConnectionReadError extends Error implements ConnectionIssue {
  readonly code: ConnectionIssue["code"];
  constructor(
    cause: unknown,
    readonly requestId: string,
  ) {
    const rawCode = cause instanceof Error ? cause.message : cause;
    const code = (CONNECTION_ERROR_CODES as readonly string[]).includes(
      String(rawCode),
    )
      ? (rawCode as ConnectionIssue["code"])
      : "connection_call_failed";
    // `message` 是给日志和 `Error` 语义用的快照。界面由 `ConnectionStatusNotice`
    // 按 `code` 在渲染时翻译，这样切语言之后显示的也跟着变。
    super(i18n.t(`connectionError.${code}`));
    this.code = code;
  }
}

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
    | "unknown_model"
    | "payload_too_large"
    | "local_busy";
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
    value.connections.length !== ACTIVATION_TOOL_IDS.length
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
  if (
    new Set(value.connections.map((c) => c.toolId)).size !==
    ACTIVATION_TOOL_IDS.length
  )
    return null;
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
      "payload_too_large",
      "local_busy",
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
  revokeTokens?: boolean,
) {
  const requestId = `connections-${Date.now().toString(36)}-${++sequence}`;
  let deadline: ReturnType<typeof setTimeout> | undefined;
  const deadlineMs =
    operation === "inspect"
      ? CONNECTION_INSPECTION_DEADLINE_MS
      : CONNECTION_RESTORE_DEADLINE_MS;
  try {
    const result = decodeConnections(
      await Promise.race([
        invoke("manage_tool_connections_v1", {
          request: {
            requestId,
            operation,
            toolId,
            ...(revokeTokens === undefined ? {} : { revokeTokens }),
          },
        }),
        new Promise<never>((_resolve, reject) => {
          deadline = setTimeout(
            () => reject(new Error(`connection_${operation}_timed_out`)),
            deadlineMs,
          );
        }),
      ]),
      requestId,
    );
    if (!result) throw new Error("invalid_connection_response");
    return result;
  } catch (cause) {
    // Only fixed codes and our own correlation ID may cross into the UI.
    throw new ConnectionReadError(cause, requestId);
  } finally {
    if (deadline !== undefined) clearTimeout(deadline);
  }
}
export function useToolConnections() {
  // Reuse CC Switch's QueryClient: one in-flight read and one last-successful
  // snapshot. This is local state, so browser offline status must not block it.
  const query = useQuery(
    {
      queryKey: CONNECTION_QUERY_KEY,
      queryFn: () => manage("inspect"),
      enabled: false,
      retry: false,
      networkMode: "always",
    },
    queryClient,
  );
  const [restoreError, setRestoreError] = useState<ConnectionReadError | null>(
    null,
  );
  const [restoring, setRestoring] = useState<ActivationToolId | null>(null);
  const [opening, setOpening] = useState<ActivationToolId | null>(null);
  const operation = useRef(false);
  const openingOperation = useRef(false);
  const mounted = useRef(true);
  const refetch = query.refetch;
  const refresh = useCallback(async () => {
    // A focus event must not supersede the result of an active restore. Once an
    // initial projection exists, keep it interactive while a later focus refresh
    // runs instead of turning the whole connection screen back into loading.
    if (operation.current) return;
    const result = await refetch({ cancelRefetch: false });
    if (mounted.current && !operation.current && result.isSuccess)
      setRestoreError(null);
  }, [refetch]);
  const restore = useCallback(
    async (tool: ActivationToolId, revokeTokens = true) => {
      if (operation.current || openingOperation.current)
        throw Error("connection_operation_busy");
      operation.current = true;
      setRestoring(tool);
      try {
        // Cancel the query's ownership of its result, not the native write. A
        // late read can no longer overwrite the post-restore projection.
        await queryClient.cancelQueries({ queryKey: CONNECTION_QUERY_KEY });
        const result = await manage("restore", tool, revokeTokens);
        queryClient.setQueryData(CONNECTION_QUERY_KEY, result);
        if (mounted.current) setRestoreError(null);
        return result;
      } catch (cause) {
        if (mounted.current) setRestoreError(cause as ConnectionReadError);
        throw cause;
      } finally {
        operation.current = false;
        if (mounted.current) setRestoring(null);
      }
    },
    [],
  );
  const open = useCallback(
    async (tool: ActivationToolId): Promise<OpenResult> => {
      if (operation.current || openingOperation.current)
        return { status: "busy", reasonCode: "busy" };
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
      window.removeEventListener("focus", refresh);
    };
  }, [refresh]);
  const issue = restoreError ?? query.error;
  const errorInfo =
    issue instanceof ConnectionReadError
      ? issue
      : query.data?.connections.some((c) => c.state === "unavailable")
        ? new ConnectionReadError(
            "connection_partial_unavailable",
            query.data.requestId,
          )
        : undefined;
  return {
    connections: query.data?.connections ?? [],
    // #11 高级模式：暴露最近一次成功读取的编号，供诊断复制使用。
    requestId: query.data?.requestId,
    loading: !query.data && !issue && query.isPending,
    refreshing: query.isFetching,
    error: !!issue,
    errorInfo,
    restoring,
    opening,
    refresh,
    restore,
    open,
  };
}
type Controller = Omit<
  ReturnType<typeof useToolConnections>,
  "refreshing" | "errorInfo" | "requestId"
> & {
  refreshing?: boolean;
  errorInfo?: ConnectionIssue;
  // #11 高级模式：测试替身与旧注入点可不提供读取编号。
  requestId?: string;
};
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
/**
 * 状态标签在语言文件里（`yeschoyConfiguration.connectionState`），三个视图共用。
 *
 * 这里走 i18next 的单例 `t` 而不是 `useTranslation()`：调用方之一
 * （`AdvancedConnectionDetails` 的 `diagnosticsText`）是模块级函数，不是组件，
 * 拿不到 hook。单例的代价是它自己不随语言变化重渲染——但真正渲染它的两个
 * 组件都调了 `useTranslation()`，语言一变它们重渲染、这个函数被重新调用。
 */
export const connectionLabel = (state?: ToolConnection["state"]) =>
  i18n.t(`yeschoyConfiguration.connectionState.${state ?? "not_connected"}`);

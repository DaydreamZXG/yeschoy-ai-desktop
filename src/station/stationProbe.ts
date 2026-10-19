import { invoke } from "@tauri-apps/api/core";

export type ProbeStatus =
  | "ok"
  | "invalid_origin"
  | "unreachable"
  | "timeout"
  | "not_relay";
export type ProbeKind = "new_api" | "one_api" | "openai_compatible" | "unknown";
export type TokenStatus =
  | "not_provided"
  | "valid"
  | "invalid"
  | "no_quota"
  | "error";

export interface StationProbeResult {
  origin: string;
  status: ProbeStatus;
  kind: ProbeKind;
  systemName: string;
  version: string;
  desktopConnect: boolean;
  tokenStatus: TokenStatus;
  models: string[];
}

const STATUSES = ["ok", "invalid_origin", "unreachable", "timeout", "not_relay"];
const KINDS = ["new_api", "one_api", "openai_compatible", "unknown"];
const TOKENS = ["not_provided", "valid", "invalid", "no_quota", "error"];

export function decodeProbe(value: unknown): StationProbeResult | null {
  if (!value || typeof value !== "object" || Array.isArray(value)) return null;
  const v = value as Record<string, unknown>;
  if (
    typeof v.origin !== "string" ||
    !STATUSES.includes(v.status as string) ||
    !KINDS.includes(v.kind as string) ||
    !TOKENS.includes(v.tokenStatus as string) ||
    typeof v.desktopConnect !== "boolean" ||
    !Array.isArray(v.models) ||
    v.models.some((m) => typeof m !== "string")
  )
    return null;
  return {
    origin: v.origin,
    status: v.status as ProbeStatus,
    kind: v.kind as ProbeKind,
    systemName: typeof v.systemName === "string" ? v.systemName : "",
    version: typeof v.version === "string" ? v.version : "",
    desktopConnect: v.desktopConnect,
    tokenStatus: v.tokenStatus as TokenStatus,
    models: (v.models as string[]).slice(0, 500),
  };
}

/** 令牌只随这一次调用传给原生侧，前端不保存。 */
export async function probeStation(
  origin: string,
  token?: string,
): Promise<StationProbeResult | null> {
  try {
    const raw = await invoke<unknown>("station_probe_v1", {
      request: token ? { origin, token } : { origin },
    });
    return decodeProbe(raw);
  } catch {
    return null;
  }
}

/** 内置参考站点：保持旧版「一点就开浏览器」的行为，不额外检测。 */
export const BUILTIN_STATION_ORIGINS = [
  "https://yeschoy.com",
  "https://ai.yeschoy.io",
];

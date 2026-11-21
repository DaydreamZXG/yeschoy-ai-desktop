import { canonicalStationHttpsOrigin } from "../../connect/stationConnectPage";
import { probeStation, type StationProbeResult } from "../station/stationProbe";

/** 用户常常只贴域名、或者带着路径贴过来；只留下站点本身。 */
export function normalizeStationInput(raw: string): string | null {
  const text = raw.trim();
  if (!text) return null;
  const withScheme = /^[a-z]+:\/\//i.test(text) ? text : `https://${text}`;
  try {
    return canonicalStationHttpsOrigin(new URL(withScheme).origin);
  } catch {
    return null;
  }
}

export function hostOf(origin: string): string {
  try {
    return new URL(origin).host;
  } catch {
    return origin;
  }
}

/** 线路 id 里带着站点地址（station_connect:https://… / station_key:https://…）。 */
export function hostOfLine(lineId: string): string | null {
  const at = lineId.indexOf(":");
  if (at < 0) return null;
  const origin = lineId.slice(at + 1);
  return /^https:\/\//.test(origin) ? hostOf(origin) : null;
}

const PROBE_DEADLINE_MS = 4_500;

export type ProbeOutcome =
  | { ok: true; probe: StationProbeResult }
  | { ok: false; reason: string };

async function once(origin: string, token?: string): Promise<ProbeOutcome> {
  let timer: ReturnType<typeof setTimeout> | undefined;
  try {
    const result = await Promise.race([
      probeStation(origin, token),
      new Promise<"timeout">((resolve) => {
        timer = setTimeout(() => resolve("timeout"), PROBE_DEADLINE_MS);
      }),
    ]);
    if (result === "timeout") return { ok: false, reason: "timeout" };
    if (!result) return { ok: false, reason: "unreachable" };
    if (result.status !== "ok") return { ok: false, reason: result.status };
    return { ok: true, probe: result };
  } catch {
    return { ok: false, reason: "unreachable" };
  } finally {
    if (timer) clearTimeout(timer);
  }
}

/**
 * 看看这个站点。超时或一时连不上，先自己再试一次，再不行才告诉用户：
 * 探测不花钱，重试没有代价。
 */
export async function lookAtStation(
  origin: string,
  token?: string,
): Promise<ProbeOutcome> {
  const first = await once(origin, token);
  if (first.ok || !["timeout", "unreachable"].includes(first.reason))
    return first;
  return once(origin, token);
}

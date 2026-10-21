import { invoke } from "@tauri-apps/api/core";
import {
  canonicalStationHttpsOrigin,
  lineIdForStationConnectOrigin,
  stationConnectPageUrl,
} from "../../connect/stationConnectPage";
import type { AccountLineId } from "../configuration/preview";
import { isOfficialDistribution } from "../distribution/identity";
import { REFERENCE_STATION_ORIGIN } from "../distribution/softTraffic";

export const STATION_ORIGIN_DRAFT_KEY = "yeschoy.stationConnectOriginDraft";

/**
 * Initial home-field value: saved draft first.
 * Official builds may prefill the YesChoy reference origin; OSS / community
 * builds leave the field empty so a third-party station is never the silent default.
 */
export function readStationOriginDraftInitial(): string {
  try {
    const saved = localStorage.getItem(STATION_ORIGIN_DRAFT_KEY);
    if (saved) return saved;
  } catch {
    /* ignore */
  }
  return isOfficialDistribution() ? REFERENCE_STATION_ORIGIN : "";
}

export type OpenStationConnectPageResult =
  | { ok: true; origin: string; lineId: AccountLineId; pageUrl: string }
  | { ok: false; reason: "invalid_origin" | "browser_open_failed" };

export async function openStationConnectPage(
  input: string,
): Promise<OpenStationConnectPageResult> {
  const origin = canonicalStationHttpsOrigin(input);
  if (!origin) {
    return { ok: false, reason: "invalid_origin" };
  }

  const lineId = lineIdForStationConnectOrigin(origin);
  const pageUrl = stationConnectPageUrl(origin);

  try {
    // 用户已经在表单里亲手输入并确认了这个站点，先记为信任，
    // 否则原生侧会拒绝为未确认的来源打开授权页。
    await invoke("station_trust_origin_v1", { origin });
    await invoke("station_connect_open_connect_page_v1", { origin });
    return { ok: true, origin, lineId, pageUrl };
  } catch {
    return { ok: false, reason: "browser_open_failed" };
  }
}

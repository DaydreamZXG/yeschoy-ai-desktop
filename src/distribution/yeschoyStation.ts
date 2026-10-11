import type { AccountProjection } from "../account/session";
import {
  STATION_CONNECT_LINE_PREFIX,
  type AccountLineId,
} from "../configuration/preview";
import { qqCommunityConfigAvailable } from "./qqCommunity";
import { isCanonicalStationOrigin } from "./softTraffic";

/** HTTPS origins operated by YesChoy (catalog lines + deep-link connect). */
export const YESCHOY_OPERATOR_STATION_ORIGINS = [
  "https://yeschoy.com",
  "https://api.yeschoy.com",
  "https://ai.yeschoy.io",
] as const;

export function accountLineUsesYeschoyStation(lineId: string): boolean {
  if (lineId === "mainland_optimized" || lineId === "global_accelerated") {
    return true;
  }
  if (!lineId.startsWith(STATION_CONNECT_LINE_PREFIX)) {
    return false;
  }
  const origin = lineId.slice(STATION_CONNECT_LINE_PREFIX.length);
  if ((YESCHOY_OPERATOR_STATION_ORIGINS as readonly string[]).includes(origin)) {
    return true;
  }
  return isCanonicalStationOrigin(origin);
}

export type YeschoyCommunitySession = {
  accountProjection: AccountProjection | null;
  lineId: AccountLineId | string;
};

/** QQ / community entry: only while signed in to a YesChoy-operated station. */
export function showYeschoyCommunityUi(session: YeschoyCommunitySession): boolean {
  if (!qqCommunityConfigAvailable()) {
    return false;
  }
  if (session.accountProjection?.status !== "signed_in") {
    return false;
  }
  return accountLineUsesYeschoyStation(session.lineId);
}

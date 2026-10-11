import type { AccountProjection } from "../account/session";
import { isOfficialDistribution } from "./identity";

/** Canonical Yeschoy station origins (compile-time device-code allowlist today). */
export const CANONICAL_STATION_ORIGINS = [
  "https://yeschoy.com",
  "https://ai.yeschoy.io",
] as const;

export const REFERENCE_STATION_ORIGIN = "https://yeschoy.com";

export function isCanonicalStationOrigin(origin: string): boolean {
  return (CANONICAL_STATION_ORIGINS as readonly string[]).includes(origin);
}

export type SoftTrafficSession = {
  accountProjection: AccountProjection | null;
  /**
   * When station deep-link connect lands, pass the active third-party origin here
   * so OSS soft traffic stays hidden for non-canonical sessions.
   */
  connectedStationOrigin?: string | null;
};

/**
 * Quiet OSS attribution / optional CTA — never while the user is on another station.
 */
export function showOssSoftTraffic(session: SoftTrafficSession): boolean {
  if (isOfficialDistribution()) return false;
  if (session.connectedStationOrigin) {
    return false;
  }
  if (session.accountProjection?.status === "signed_in") {
    // Connected to a station session (today: canonical lines only).
    return false;
  }
  return true;
}

export function showOfficialCommunityUi(): boolean {
  return isOfficialDistribution();
}

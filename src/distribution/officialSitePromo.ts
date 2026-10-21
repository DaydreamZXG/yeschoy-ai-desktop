import { isOfficialDistribution } from "./identity";

/** Product site (landing + download). Not a relay API origin. */
export const OFFICIAL_SITE_ORIGIN = "https://yeschoy.app";

export const OFFICIAL_SITE_PROMO_DISMISS_KEY =
  "yeschoy.officialSitePromo.dismissed.v1";

export function readOfficialSitePromoDismissed(): boolean {
  try {
    return localStorage.getItem(OFFICIAL_SITE_PROMO_DISMISS_KEY) === "1";
  } catch {
    return false;
  }
}

export function dismissOfficialSitePromo(): void {
  try {
    localStorage.setItem(OFFICIAL_SITE_PROMO_DISMISS_KEY, "1");
  } catch {
    /* Storage is optional; never block the UI. */
  }
}

/**
 * Lightweight official-build attribution to yeschoy.app.
 * Never shown on OSS builds; hidden permanently after dismiss.
 */
export function showOfficialSitePromo(): boolean {
  if (!isOfficialDistribution()) return false;
  return !readOfficialSitePromoDismissed();
}

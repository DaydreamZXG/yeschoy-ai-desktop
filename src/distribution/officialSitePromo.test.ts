import { afterEach, describe, expect, it, vi } from "vitest";
import {
  OFFICIAL_SITE_PROMO_DISMISS_KEY,
  dismissOfficialSitePromo,
  readOfficialSitePromoDismissed,
  showOfficialSitePromo,
} from "./officialSitePromo";

afterEach(() => {
  localStorage.clear();
  vi.unstubAllEnvs();
});

describe("officialSitePromo", () => {
  it("hides on OSS builds even when not dismissed", () => {
    vi.stubEnv("VITE_YESCHOY_DISTRIBUTION", "oss");
    expect(showOfficialSitePromo()).toBe(false);
  });

  it("shows on official builds until dismissed", () => {
    vi.stubEnv("VITE_YESCHOY_DISTRIBUTION", "official");
    expect(showOfficialSitePromo()).toBe(true);
    dismissOfficialSitePromo();
    expect(readOfficialSitePromoDismissed()).toBe(true);
    expect(localStorage.getItem(OFFICIAL_SITE_PROMO_DISMISS_KEY)).toBe("1");
    expect(showOfficialSitePromo()).toBe(false);
  });
});

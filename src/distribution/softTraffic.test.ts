import { describe, expect, it, vi } from "vitest";
import type { AccountProjection } from "../account/session";
import { isCanonicalStationOrigin, showOssSoftTraffic } from "./softTraffic";

function projection(status: AccountProjection["status"]): AccountProjection {
  return {
    requestId: "req",
    schemaVersion: 3,
    status,
    userCode: "",
    pollAfterSeconds: 0,
    expiresAtEpochMs: 0,
    observedAtEpochMs: 0,
    account: {
      available: false,
      displayName: "",
      username: "",
      balanceQuota: "",
      usedQuota: "",
      requestCount: "",
      quotaPerUnit: "",
    },
    usage: {
      available: false,
      consumedQuota: "",
      requestRate: "",
      tokenCount: "",
    },
    models: [],
    comparisonFx: "",
    reasonCode: status,
  };
}

describe("softTraffic", () => {
  it("treats yeschoy origins as canonical", () => {
    expect(isCanonicalStationOrigin("https://yeschoy.com")).toBe(true);
    expect(isCanonicalStationOrigin("https://ai.yeschoy.io")).toBe(true);
    expect(isCanonicalStationOrigin("https://example.com")).toBe(false);
  });

  it("hides OSS soft traffic when signed in", () => {
    vi.stubEnv("VITE_YESCHOY_DISTRIBUTION", "oss");
    expect(
      showOssSoftTraffic({
        accountProjection: projection("signed_in"),
      }),
    ).toBe(false);
    vi.unstubAllEnvs();
  });

  it("hides OSS soft traffic when a non-canonical station is connected", () => {
    vi.stubEnv("VITE_YESCHOY_DISTRIBUTION", "oss");
    expect(
      showOssSoftTraffic({
        accountProjection: projection("signed_out"),
        connectedStationOrigin: "https://partner.example.com",
      }),
    ).toBe(false);
    vi.unstubAllEnvs();
  });

  it("shows OSS soft traffic for signed-out users without a third-party station", () => {
    vi.stubEnv("VITE_YESCHOY_DISTRIBUTION", "oss");
    expect(
      showOssSoftTraffic({
        accountProjection: projection("signed_out"),
      }),
    ).toBe(true);
    vi.unstubAllEnvs();
  });

});

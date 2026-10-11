import { describe, expect, it, vi } from "vitest";
import type { AccountProjection } from "../account/session";
import {
  accountLineUsesYeschoyStation,
  showYeschoyCommunityUi,
} from "./yeschoyStation";

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

describe("accountLineUsesYeschoyStation", () => {
  it("accepts catalog relay lines", () => {
    expect(accountLineUsesYeschoyStation("mainland_optimized")).toBe(true);
    expect(accountLineUsesYeschoyStation("global_accelerated")).toBe(true);
  });

  it("accepts station_connect lines for YesChoy origins", () => {
    expect(
      accountLineUsesYeschoyStation("station_connect:https://yeschoy.com"),
    ).toBe(true);
    expect(
      accountLineUsesYeschoyStation("station_connect:https://api.yeschoy.com"),
    ).toBe(true);
  });

  it("rejects third-party station connect lines", () => {
    expect(
      accountLineUsesYeschoyStation(
        "station_connect:https://partner.example.com",
      ),
    ).toBe(false);
  });
});

describe("showYeschoyCommunityUi", () => {
  it("shows QQ entry when signed in to YesChoy on OSS builds", () => {
    vi.stubEnv("VITE_YESCHOY_DISTRIBUTION", "oss");
    expect(
      showYeschoyCommunityUi({
        lineId: "mainland_optimized",
        accountProjection: projection("signed_in"),
      }),
    ).toBe(true);
    vi.unstubAllEnvs();
  });

  it("hides QQ when signed out", () => {
    expect(
      showYeschoyCommunityUi({
        lineId: "mainland_optimized",
        accountProjection: projection("signed_out"),
      }),
    ).toBe(false);
  });

  it("hides QQ when signed in to a third-party station", () => {
    expect(
      showYeschoyCommunityUi({
        lineId: "station_connect:https://partner.example.com",
        accountProjection: projection("signed_in"),
      }),
    ).toBe(false);
  });

  it("hides QQ when config disables the group", () => {
    vi.stubEnv("VITE_YESCHOY_QQ_GROUP", "0");
    expect(
      showYeschoyCommunityUi({
        lineId: "mainland_optimized",
        accountProjection: projection("signed_in"),
      }),
    ).toBe(false);
    vi.unstubAllEnvs();
  });
});

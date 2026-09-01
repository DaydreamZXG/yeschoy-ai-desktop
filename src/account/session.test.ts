import { describe, expect, it } from "vitest";
import { decodeAccountProjection, quotaToUsd } from "./session";

function signedIn(requestId = "account-test-1") {
  return {
    requestId,
    schemaVersion: 2,
    status: "signed_in",
    userCode: "",
    pollAfterSeconds: 0,
    expiresAtEpochMs: 0,
    observedAtEpochMs: 1_788_195_600_000,
    account: {
      available: true,
      displayName: "测试用户",
      username: "member@example.com",
      balanceQuota: "350000",
      usedQuota: "120000",
      requestCount: "42",
      quotaPerUnit: "500000",
    },
    usage: {
      available: true,
      consumedQuota: "120000",
      requestRate: "42",
      tokenCount: "980000",
    },
    models: [
      {
        id: "glm-5.3",
        description: "",
        billingMode: "ratio",
        pricingAvailable: true,
        officialInputCnyPerMillion: "13.5",
        officialOutputCnyPerMillion: "54",
        actualInputCnyPerMillion: "6.75",
        actualOutputCnyPerMillion: "27",
      },
    ],
    comparisonFx: "6.75",
    reasonCode: "none",
  };
}

describe("account v2 renderer boundary", () => {
  it("accepts a complete sanitized signed-in projection", () => {
    expect(decodeAccountProjection(signedIn(), "account-test-1")).toEqual(
      signedIn(),
    );
  });

  it.each(["accessToken", "refreshToken", "deviceCode", "authorizationUrl"])(
    "rejects an unexpected secret-capable field: %s",
    (field) => {
      const payload = { ...signedIn(), [field]: "must-not-cross-renderer" };
      expect(decodeAccountProjection(payload, "account-test-1")).toBeNull();
    },
  );

  it("rejects mismatched requests and malformed pending codes", () => {
    expect(decodeAccountProjection(signedIn(), "another-request")).toBeNull();
    const pending = {
      ...signedIn("account-pending"),
      status: "authorization_pending",
      userCode: "unsafe-code",
      pollAfterSeconds: 3,
      expiresAtEpochMs: 1_788_195_900_000,
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
      reasonCode: "pending",
    };
    expect(decodeAccountProjection(pending, "account-pending")).toBeNull();
  });

  it("converts NewAPI quota units only when the inputs are valid", () => {
    expect(quotaToUsd("350000", "500000")).toBe(0.7);
    expect(quotaToUsd("-1", "500000")).toBeNull();
    expect(quotaToUsd("1", "0")).toBeNull();
  });
});

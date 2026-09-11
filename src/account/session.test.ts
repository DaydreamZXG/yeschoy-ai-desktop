import { describe, expect, it } from "vitest";
import { decodeAccountProjection, quotaToUsd } from "./session";

function signedIn(requestId = "account-test-1") {
  return {
    requestId,
    schemaVersion: 3,
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
        officialInputCnyPerMillion: "14",
        officialOutputCnyPerMillion: "56",
        actualInputCnyPerMillion: "7",
        actualOutputCnyPerMillion: "28",
      },
    ],
    comparisonFx: "7",
    reasonCode: "none",
  };
}

describe("account v2 renderer boundary", () => {
  const withBilling = () => {
    const payload = signedIn();
    return {
      ...payload,
      models: payload.models.map((model) => ({
        ...model,
        supportedEndpointTypes: ["openai", "anthropic"],
        billing: {
          groups: [
            { id: "国模特价分组", description: "账户分组", ratio: 0.35 },
          ],
          baseInputUsd: 2,
          baseOutputUsd: 8,
          requestUsd: null,
          expression: "",
        },
      })),
    };
  };

  it("accepts account-specific groups, including zero and unavailable ratios", () => {
    for (const ratio of [0, 0.35, null]) {
      const payload = withBilling();
      const billing = payload.models[0].billing;
      const raw = {
        ...payload,
        models: [
          {
            ...payload.models[0],
            billing: {
              ...billing,
              groups: [{ ...billing.groups[0], ratio }],
            },
          },
        ],
      };
      expect(decodeAccountProjection(raw, "account-test-1")).toEqual(raw);
    }
  });

  it("rejects ambiguous groups, invalid ratios and unexpected nested credential fields", () => {
    const payload = withBilling();
    const billing = payload.models[0].billing;
    const group = billing.groups[0];
    for (const groups of [
      [group, group],
      [{ ...group, ratio: -1 }],
      [{ ...group, ratio: Infinity }],
      [{ ...group, ratio: "0.35" }],
      [{ ...group, id: "auto" }],
      [{ ...group, id: "bad\nname" }],
      [{ ...group, apiKey: "must-not-cross-renderer" }],
    ]) {
      expect(
        decodeAccountProjection(
          {
            ...payload,
            models: [{ ...payload.models[0], billing: { ...billing, groups } }],
          },
          "account-test-1",
        ),
      ).toBeNull();
    }
  });

  it("accepts a complete sanitized signed-in projection", () => {
    expect(decodeAccountProjection(signedIn(), "account-test-1")).toEqual(
      signedIn(),
    );
  });

  it("normalizes closed schema4 optional prices without turning missing values into zero", () => {
    const previous = withBilling();
    const billing = {
      groups: [{ id: "default", description: "" }],
      baseInputUsd: 0,
      cacheReadUsd: 0,
      expression: "",
    };
    const raw = {
      ...previous,
      schemaVersion: 4,
      models: [{ ...previous.models[0], billing }],
    };
    const parsed = decodeAccountProjection(raw, raw.requestId)!;
    expect(parsed.models[0].billing).toEqual({
      ...billing,
      groups: [{ id: "default", description: "", ratio: null }],
      baseOutputUsd: null,
      cacheWriteUsd: null,
      requestUsd: null,
    });
    for (const invalid of [
      null,
      { ...billing, requestUsd: null },
      { ...billing, apiKey: "not-allowed" },
      { ...billing, groups: [{ id: "default", description: "", ratio: null }] },
    ]) {
      expect(
        decodeAccountProjection(
          { ...raw, models: [{ ...raw.models[0], billing: invalid }] },
          raw.requestId,
        ),
      ).toBeNull();
    }
    const { billing: _omitted, ...withoutBilling } = raw.models[0];
    expect(
      decodeAccountProjection(
        { ...raw, models: [withoutBilling] },
        raw.requestId,
      )?.models[0].billing,
    ).toBeNull();
  });

  it("accepts a negative balance without accepting negative usage counters", () => {
    const payload = signedIn();
    payload.account.balanceQuota = "-125000";
    expect(decodeAccountProjection(payload, "account-test-1")).toEqual(payload);

    for (const field of ["usedQuota", "requestCount"] as const) {
      const invalid = signedIn();
      invalid.account[field] = "-1";
      expect(decodeAccountProjection(invalid, "account-test-1")).toBeNull();
    }
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
    expect(quotaToUsd("-125000", "500000")).toBe(-0.25);
    expect(quotaToUsd("1", "0")).toBeNull();
  });
});

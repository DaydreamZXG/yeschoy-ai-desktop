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
        // 已定价行会携带该字段；未定价行可能合法省略，表示上游没有声明。
        supportedEndpointTypes: [],
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

function usageLogPayload() {
  return {
    status: "available",
    reasonCode: "none",
    recordCount: 2,
    scannedCount: 3,
    windowDays: 30,
    truncated: false,
    oldestAtEpochMs: 1_788_195_000_000,
    newestAtEpochMs: 1_788_195_600_000,
    records: [
      {
        toolId: "codex_desktop",
        modelId: "gpt-6-astra",
        observedAtEpochMs: 1_788_195_600_000,
        promptTokens: 1200,
        completionTokens: 300,
        cacheTokens: 400,
        amount: "7",
      },
      {
        toolId: "",
        modelId: "claude-sonnet-4-6",
        observedAtEpochMs: 1_788_195_000_000,
        promptTokens: 100,
        completionTokens: 50,
        cacheTokens: 0,
        amount: "3.5",
      },
    ],
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

  it("decodes a v6 projection with the usage log report attached", () => {
    const usageLog = usageLogPayload();
    const account = signedIn();
    const { supportedEndpointTypes: _unknown, ...unpriced } = account.models[0];
    const raw = {
      ...account,
      schemaVersion: 5,
      usageLog,
      models: [
        account.models[0],
        {
          ...unpriced,
          id: "deepseek-v4.1-flash",
          billingMode: "unknown",
          pricingAvailable: false,
          officialInputCnyPerMillion: "",
          officialOutputCnyPerMillion: "",
          actualInputCnyPerMillion: "",
          actualOutputCnyPerMillion: "",
        },
      ],
    };
    // 升级到 v6：原生侧新增 usageLog；v5 的 money/savings 语义不变。
    const payload = {
      ...raw,
      money: {
        currency: "CNY",
        balanceAmount: "0.7",
        consumedAmount: "1.68",
        displayRate: "7",
      },
      savings: {
        status: "unavailable",
        // Rust 侧 unavailable 的 reasonCode 只会是这三个值（默认 logs_unavailable）。
        reasonCode: "logs_unavailable",
        officialAmount: "",
        siteAmount: "",
        savedAmount: "",
        referenceRate: "",
        priceRate: "",
        recordLimit: 100,
        scannedCount: 0,
        includedCount: 0,
        excludedCount: 0,
        oldestAtEpochMs: 0,
        newestAtEpochMs: 0,
      },
    };
    const parsed = decodeAccountProjection(
      { ...payload, schemaVersion: 6 },
      payload.requestId,
    )!;
    expect(parsed.usageLog).toEqual(usageLog);
    expect(parsed.money?.currency).toBe("CNY");
    expect(parsed.models[0].id).toBe("glm-5.3");
    expect(parsed.models[1]).toMatchObject({
      id: "deepseek-v4.1-flash",
      billing: null,
    });
    expect(parsed.models[1].supportedEndpointTypes).toBeUndefined();
  });

  it("rejects v6 usage logs that leak into non-signed-in states or miscount", () => {
    const usageLog = usageLogPayload();
    const signedOut = {
      ...signedIn(),
      status: "signed_out" as const,
      schemaVersion: 6,
      // 非登录态守卫：account/usage 不可用时字段必须全空。
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
      reasonCode: "signed_out",
      money: {
        currency: "",
        balanceAmount: "",
        consumedAmount: "",
        displayRate: "",
      },
      savings: {
        status: "unavailable",
        // Rust 侧 unavailable 的 reasonCode 只会是这三个值（默认 logs_unavailable）。
        reasonCode: "logs_unavailable",
        officialAmount: "",
        siteAmount: "",
        savedAmount: "",
        referenceRate: "",
        priceRate: "",
        recordLimit: 100,
        scannedCount: 0,
        includedCount: 0,
        excludedCount: 0,
        oldestAtEpochMs: 0,
        newestAtEpochMs: 0,
      },
      usageLog,
    };
    // 非登录态携带可用用量报告 → 拒绝。
    expect(
      decodeAccountProjection(signedOut, signedOut.requestId),
    ).toBeNull();
    // 未登录但报告为 unavailable → 通过（报告以 unavailable 状态随投影下发）。
    const unavailable = {
      ...signedOut,
      usageLog: { ...usageLog, status: "unavailable" as const, records: [], recordCount: 0 },
    };
    expect(
      decodeAccountProjection(unavailable, unavailable.requestId),
    ).not.toBeNull();
    // 记录数不一致 → 拒绝。
    const mismatched = {
      ...signedIn(),
      schemaVersion: 6,
      money: signedOut.money,
      savings: signedOut.savings,
      usageLog: { ...usageLog, recordCount: 3 },
    };
    expect(
      decodeAccountProjection(mismatched, mismatched.requestId),
    ).toBeNull();
    // 金额非法文本 → 拒绝。
    const badAmount = {
      ...signedIn(),
      schemaVersion: 6,
      money: signedOut.money,
      savings: signedOut.savings,
      usageLog: {
        ...usageLog,
        records: [
          { ...usageLog.records[0], amount: "not-a-number" },
          usageLog.records[1],
        ],
      },
    };
    expect(
      decodeAccountProjection(badAmount, badAmount.requestId),
    ).toBeNull();
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

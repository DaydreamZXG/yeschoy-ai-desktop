import type { ComparisonContext, ComparisonEvidence } from "./comparison";

// Fictional arithmetic fixture. Never import into the application entry graph.
export const contextFixture: ComparisonContext = {
  accountId: "test-account",
  periodStartMs: 1_700_000_000_000,
  periodEndMs: 1_700_086_400_000,
  nowMs: 1_700_010_000_000,
};

export function evidenceFixture(): ComparisonEvidence {
  return {
    schemaVersion: 1,
    calculationVersion: "normalized-token-lines-v1",
    accountId: contextFixture.accountId,
    periodStartMs: contextFixture.periodStartMs,
    periodEndMs: contextFixture.periodEndMs,
    generatedAtMs: contextFixture.nowMs,
    expiresAtMs: contextFixture.nowMs + 60_000,
    comparisonFx: "6.75",
    excluded: [],
    rows: [
      {
        id: "fictional-request-1",
        modelId: "fictional-model-full-id-2026-08-31",
        officialModelId: "fictional-official-model-1",
        occurredAtMs: contextFixture.periodStartMs + 10_000,
        usageSource: "upstream_reported",
        modelMatch: "verified",
        billingDimensionsComplete: true,
        funding: "wallet",
        settledNetCnyMicros: "3000000",
        officialCurrency: "USD",
        officialTotalMicros: "1560000",
        priceVersion: "fictional-price-v1",
        priceEffectiveAtMs: contextFixture.periodStartMs - 10_000,
        sourceCheckedAtMs: contextFixture.periodStartMs,
        officialSourceUrl: "https://example.com/fictional-pricing",
        fxVersion: "fixed-6.75-v1",
        tierLabel: "Fictional text-only, standard context tier",
        usage: {
          inputTotal: "1000000",
          input: "200000",
          output: "100000",
          cache_read: "800000",
          cache_write_5m: "0",
          cache_write_1h: "0",
        },
        lines: [
          {
            kind: "input",
            quantity: "200000",
            pricePerMillionMicros: "2000000",
            amountMicros: "400000",
          },
          {
            kind: "cache_read",
            quantity: "800000",
            pricePerMillionMicros: "200000",
            amountMicros: "160000",
          },
          {
            kind: "output",
            quantity: "100000",
            pricePerMillionMicros: "10000000",
            amountMicros: "1000000",
          },
        ],
      },
    ],
  };
}

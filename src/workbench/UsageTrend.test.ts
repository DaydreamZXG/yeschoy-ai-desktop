import { describe, expect, it } from "vitest";
import { dailySpend } from "./UsageTrend";

const DAY = 86_400_000;
const record = (at: number, amount: string) => ({
  toolId: "",
  modelId: "m",
  observedAtEpochMs: at,
  promptTokens: 10,
  completionTokens: 5,
  cacheTokens: 0,
  amount,
});

describe("dailySpend", () => {
  it("buckets the last 14 days and ignores older or unpriced records", () => {
    const now = 100 * DAY + 3_600_000;
    const days = dailySpend(
      [
        record(now, "1.5"),
        record(now - 1000, "0.5"),
        record(now - DAY, ""),
        record(now - 20 * DAY, "9"),
      ],
      now,
    );
    expect(days).toHaveLength(14);
    expect(days[13].amount).toBeCloseTo(2);
    expect(days[12]).toMatchObject({ amount: 0, tokens: 15 });
    expect(days.reduce((s, d) => s + d.amount, 0)).toBeCloseTo(2);
  });
});

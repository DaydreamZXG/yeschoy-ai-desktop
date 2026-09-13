import { describe, expect, it } from "vitest";
import {
  aggregateUsage,
  isUsageLogReport,
  type UsageRecord,
} from "./usage";

function record(overrides: Partial<UsageRecord>): UsageRecord {
  return {
    toolId: "codex_desktop",
    modelId: "gpt-6-astra",
    observedAtEpochMs: 1_788_195_600_000,
    promptTokens: 100,
    completionTokens: 20,
    cacheTokens: 0,
    amount: "1",
    ...overrides,
  };
}

function report(overrides: Record<string, unknown> = {}) {
  return {
    status: "available",
    reasonCode: "none",
    recordCount: 1,
    scannedCount: 1,
    windowDays: 30,
    truncated: false,
    oldestAtEpochMs: 1_788_195_000_000,
    newestAtEpochMs: 1_788_195_600_000,
    records: [record({})],
    ...overrides,
  };
}

describe("usage log report boundary", () => {
  it("accepts a well-formed report", () => {
    expect(isUsageLogReport(report())).toBe(true);
  });

  it("rejects status drift, truncated non-boolean and record overflow", () => {
    expect(isUsageLogReport(report({ status: "partial" }))).toBe(false);
    expect(isUsageLogReport(report({ truncated: "yes" }))).toBe(false);
    expect(
      isUsageLogReport(report({ records: new Array(501).fill(record({})) })),
    ).toBe(false);
  });

  it("rejects records with unsafe text, negative tokens or bad amounts", () => {
    expect(
      isUsageLogReport(
        report({
          records: [record({ modelId: "bad\nname" })],
        }),
      ),
    ).toBe(false);
    expect(
      isUsageLogReport(
        report({
          records: [record({ promptTokens: -1 })],
        }),
      ),
    ).toBe(false);
    expect(
      isUsageLogReport(
        report({
          records: [record({ amount: "1e3" })],
        }),
      ),
    ).toBe(false);
    // 空串金额合法（换算参数缺失，不显示金额）。
    expect(
      isUsageLogReport(
        report({
          records: [record({ amount: "" })],
        }),
      ),
    ).toBe(true);
  });

  it("aggregates by tool x model with epsilon-tolerant amounts", () => {
    const rows = aggregateUsage([
      record({ amount: "0.1", promptTokens: 10 }),
      record({ amount: "0.2", promptTokens: 5 }),
      record({
        toolId: "claude_code",
        modelId: "claude-sonnet-4-6",
        amount: "3.5",
      }),
      record({ modelId: "claude-sonnet-4-6", amount: "" }),
    ]);
    // 0.1 + 0.2 的浮点和应为 0.30000000000000004 → EPSILON 容差收敛为 0.3。
    expect(rows.find((r) => r.modelId === "gpt-6-astra")?.amount).toBe(0.3);
    expect(
      rows.find((r) => r.modelId === "gpt-6-astra")?.requests,
    ).toBe(2);
    expect(
      rows.find((r) => r.modelId === "gpt-6-astra")?.promptTokens,
    ).toBe(15);
    // claude_code + claude-sonnet-4-6 与 codex_desktop + 同模型是不同行。
    expect(rows).toHaveLength(3);
    // 任一记录金额缺失 → 该行金额为 null（不显示，不猜）。
    expect(
      rows.find(
        (r) =>
          r.toolId === "codex_desktop" && r.modelId === "claude-sonnet-4-6",
      )?.amount,
    ).toBeNull();
  });
});

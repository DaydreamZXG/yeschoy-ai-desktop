import { describe, expect, it } from "vitest";
import type { UsageLogReport, UsageRecord } from "../account/usage";
import {
  covers,
  daysOf,
  eatersOf,
  latestOf,
  spentSince,
  startOfDay,
} from "./ledgerInfo";

const NOW = new Date(2026, 10, 21, 15, 0).getTime();
const at = (daysAgo: number, hour = 10) =>
  new Date(2026, 10, 21 - daysAgo, hour).getTime();
const rec = (modelId: string, when: number, amount: string): UsageRecord => ({
  toolId: "claude_code",
  modelId,
  observedAtEpochMs: when,
  promptTokens: 10,
  completionTokens: 5,
  cacheTokens: 0,
  amount,
});
const log = (
  records: UsageRecord[],
  extra: Partial<UsageLogReport> = {},
): UsageLogReport => ({
  status: records.length ? "available" : "empty",
  reasonCode: "none",
  recordCount: records.length,
  scannedCount: records.length,
  windowDays: 30,
  truncated: false,
  oldestAtEpochMs: Math.min(...records.map((r) => r.observedAtEpochMs), NOW),
  newestAtEpochMs: Math.max(...records.map((r) => r.observedAtEpochMs), 0),
  records,
  ...extra,
});

describe("spentSince", () => {
  const today = startOfDay(NOW);

  it("adds up what the station reported", () => {
    const report = log([
      rec("gpt-5", at(0), "0.42"),
      rec("gpt-5", at(0, 12), "0.03"),
      rec("gpt-5", at(1), "5"),
    ]);
    expect(spentSince(report, today)).toBe(0.45);
    expect(spentSince(report, 0)).toBe(5.45);
  });

  it("an empty ledger is a real zero", () => {
    expect(spentSince(log([]), today)).toBe(0);
  });

  it("one record without an amount makes the total unknown, not smaller", () => {
    const report = log([rec("gpt-5", at(0), "0.42"), rec("gpt-5", at(0), "")]);
    expect(spentSince(report, today)).toBeNull();
  });

  it("is unknown when the station did not provide the ledger", () => {
    expect(spentSince(log([], { status: "unavailable" }), today)).toBeNull();
  });

  it("a truncated ledger only answers for the time it actually covers", () => {
    const report = log([rec("gpt-5", at(0), "1"), rec("gpt-5", at(2), "2")], {
      truncated: true,
    });
    expect(covers(report, today)).toBe(true);
    expect(spentSince(report, today)).toBe(1);
    // 三天前起：最早一笔是两天前的，更早的没读到。
    expect(spentSince(report, at(3))).toBeNull();
  });
});

describe("daysOf", () => {
  it("buckets by local day, today last, quiet days are zero", () => {
    const days = daysOf(
      log([
        rec("gpt-5", at(0, 1), "1"),
        rec("gpt-5", at(0, 14), "2"),
        rec("gpt-5", at(2, 23), "4"),
      ]),
      NOW,
    );
    expect(days).toHaveLength(7);
    expect(days.map((day) => day.amount)).toEqual([0, 0, 0, 0, 4, 0, 3]);
    expect(days[6].start).toBe(startOfDay(NOW));
    expect(days[6].requests).toBe(2);
  });

  it("days the truncated ledger does not reach are unknown, not zero", () => {
    const days = daysOf(
      log([rec("gpt-5", at(0), "1"), rec("gpt-5", at(2, 12), "2")], {
        truncated: true,
      }),
      NOW,
    );
    // 两天前那天只读到中午之后，那天和更早的都不知道。
    expect(days.map((day) => day.amount)).toEqual([
      null,
      null,
      null,
      null,
      null,
      0,
      1,
    ]);
  });

  it("a day with an unpriced record is unknown", () => {
    const days = daysOf(log([rec("gpt-5", at(1), "")]), NOW);
    expect(days[5].amount).toBeNull();
    expect(days[6].amount).toBe(0);
  });

  it("nothing is known without a ledger", () => {
    const days = daysOf(log([], { status: "unavailable" }), NOW);
    expect(days.every((day) => day.amount === null)).toBe(true);
  });
});

describe("eatersOf", () => {
  it("groups by character, biggest spender first", () => {
    const eaters = eatersOf([
      rec("deepseek-v4-flash", at(0), "0.1"),
      rec("claude-sonnet-4-5", at(0), "3"),
      rec("claude-opus-4-1", at(1), "2"),
      rec("some-unknown-model", at(1), "0.5"),
    ]);
    expect(eaters.map((eater) => [eater.who, eater.amount])).toEqual([
      ["cla", 5],
      ["new", 0.5],
      ["ds", 0.1],
    ]);
  });

  it("falls back to request counts when someone's spend is unknown", () => {
    const eaters = eatersOf([
      rec("claude-sonnet-4-5", at(0), "30"),
      rec("deepseek-v4-flash", at(0), ""),
      rec("deepseek-v4-flash", at(0), "0.1"),
    ]);
    expect(eaters.map((eater) => eater.who)).toEqual(["ds", "cla"]);
    expect(eaters[0].amount).toBeNull();
    expect(eaters[1].amount).toBe(30);
  });
});

describe("latestOf", () => {
  it("newest first, capped", () => {
    const records = [1, 5, 3, 2, 4, 7, 6, 0].map((n) =>
      rec(`m${n}`, at(n), "1"),
    );
    expect(latestOf(records).map((record) => record.modelId)).toEqual([
      "m0",
      "m1",
      "m2",
      "m3",
      "m4",
      "m5",
    ]);
  });
});

import { describe, expect, it } from "vitest";
import type { AccountModel } from "../account/session";
import {
  billingTiers,
  chooseBillingGroup,
  groupPrice,
  hundredMillionTokenEstimate,
} from "./billing";

export const timedRule =
  '(((weekday("Asia/Shanghai") >= 1 && weekday("Asia/Shanghai") <= 5) && ((hour("Asia/Shanghai") >= 9 && hour("Asia/Shanghai") < 12) || (hour("Asia/Shanghai") >= 14 && hour("Asia/Shanghai") < 18))) ? tier("高峰期", p * 3 + cr * 0.1 + c * 9) : tier("非高峰期", p * 1.5 + cr * 0.05 + c * 4.5))';
const fixture = (
  billingMode: AccountModel["billingMode"] = "tiered_expr",
): AccountModel => ({
  id: "deepseek-v4-flash",
  description: "",
  billingMode,
  pricingAvailable: false,
  officialInputCnyPerMillion: "",
  officialOutputCnyPerMillion: "",
  actualInputCnyPerMillion: "",
  actualOutputCnyPerMillion: "",
  billing: {
    groups: [
      { id: "default", description: "", ratio: 1 },
      { id: "特价", description: "", ratio: 0.35 },
    ],
    baseInputUsd: 3,
    baseOutputUsd: 9,
    requestUsd: 0.01,
    expression: timedRule,
  },
});

describe("billing display follows the selected NewAPI group", () => {
  it.each(["deepseek-v4-flash", "deepseek-v4.1-flash"])(
    "estimates the published multiline DeepSeek rule: %s",
    (id) => {
      const model = fixture();
      model.id = id;
      model.billing!.expression =
        '(((weekday("Asia/Shanghai") >= 1 && weekday("Asia/Shanghai") <= 5) &&\n  ((hour("Asia/Shanghai") >= 9 && hour("Asia/Shanghai") < 12) ||\n   (hour("Asia/Shanghai") >= 14 && hour("Asia/Shanghai") < 18)))\n ? tier("高峰期", p * 2 + cr * 0.04 + c * 8)\n : tier("非高峰期", p * 1 + cr * 0.02 + c * 4))';
      const estimate = hundredMillionTokenEstimate(
        model,
        model.billing!.groups[1],
        "1",
      )!;
      expect(estimate.official.minimum).toBeCloseTo(3.35033384);
      expect(estimate.official.maximum).toBeCloseTo(6.70066768);
      expect(estimate.yeschoy.minimum).toBeCloseTo(3.35033384 * 0.35);
      expect(estimate.yeschoy.maximum).toBeCloseTo(6.70066768 * 0.35);
    },
  );

  it.each(["1", "7", ""])(
    "uses 6.75 official FX and 1:1 site pricing, independent of %s",
    (fx) => {
      const model = fixture();
      model.id = "gpt-6-astra";
      model.billing!.expression =
        'len <= 272000 ? tier("standard", p * 10 + c * 50 + cr * 1) : tier("long_context", p * 20 + c * 75 + cr * 2)';
      const estimate = hundredMillionTokenEstimate(
        model,
        { id: "special", ratio: 0.4, description: "" },
        fx,
      )!;
      expect(estimate.official.minimum).toBeCloseTo(772.84293411);
      expect(estimate.official.maximum).toBeCloseTo(1517.29796933);
      expect(estimate.yeschoy.minimum).toBeCloseTo(45.79809980);
      expect(estimate.yeschoy.maximum).toBeCloseTo(89.91395374);
      expect(estimate.savingPercent).toBeCloseTo(94.074074);
    },
  );

  it("selects a named standard tier rather than the cheapest or first tier", () => {
    const value = fixture();
    value.id = "gpt-6-astra";
    value.billing!.expression = 'len > 1 ? tier("promo", p * 1 + c * 1 + cr * 0.1) : tier("standard", p * 10 + c * 50 + cr * 1)';
    const estimate = hundredMillionTokenEstimate(value, { id: "test", ratio: 0.4, description: "" }, "1", "reference")!;
    expect(estimate.official.minimum).toBeCloseTo(772.84293411);
    expect(estimate.official.maximum).toBe(estimate.official.minimum);
    expect(estimate.yeschoy.minimum).toBeCloseTo(45.79809980);
  });
  it("does not choose a cheap off-peak tier when no standard tier is declared", () => {
    const value = fixture();
    const estimate = hundredMillionTokenEstimate(value, value.billing!.groups[1], "1", "reference")!;
    const range = hundredMillionTokenEstimate(value, value.billing!.groups[1], "1")!;
    expect(estimate.official.minimum).toBe(range.official.maximum);
    expect(estimate.official.maximum).toBe(estimate.official.minimum);
  });
  it("uses site cache rates in the 100M-token comparison instead of charging cached input as new input", () => {
    const model = fixture("ratio");
    model.billing = {
      ...model.billing!,
      baseInputUsd: 0.2,
      baseOutputUsd: 1.25,
      cacheReadUsd: 0.02,
      cacheWriteUsd: 0.25,
      expression: "",
    };
    const group = { id: "special", description: "", ratio: 0.16 };
    const result = hundredMillionTokenEstimate(model, group, "1")!;
    expect(result.cacheFallback).toBe(false);
    expect(result.official.minimum).toBeCloseTo(2.33196114);
    expect(result.yeschoy.minimum).toBeCloseTo(2.33196114 * 0.16);
    expect(result.savingPercent).toBeCloseTo(84);
  });
  it("reads both time tiers without treating the model ratio as the dynamic price", () => {
    const model = fixture(),
      group = model.billing!.groups[1];
    const result = groupPrice(model, group, "7");
    expect(result.rows).toEqual([
      { name: "高峰期", rates: { p: 3, cr: 0.1, c: 9 } },
      { name: "非高峰期", rates: { p: 1.5, cr: 0.05, c: 4.5 } },
    ]);
    expect(result.rows[0].rates.p * result.multiplier).toBeCloseTo(7.35);
    expect(result.rows[1].rates.c * result.multiplier).toBeCloseTo(11.025);
  });

  it("preserves zero and distinguishes unavailable FX and multiplier", () => {
    expect(
      groupPrice(
        fixture("ratio"),
        { id: "free", description: "", ratio: 0 },
        "7",
      ).multiplier,
    ).toBe(0);
    const usd = groupPrice(
      fixture("ratio"),
      { id: "x", description: "", ratio: 0.5 },
      "",
    );
    expect(usd.currency).toBe("USD");
    expect(usd.multiplier).toBe(0.5);
    expect(
      groupPrice(fixture(), { id: "x", description: "", ratio: null }, "7")
        .multiplier,
    ).toBeNaN();
  });

  it("does not conflate per-request and per-token prices", () => {
    const result = groupPrice(
      fixture("per_request"),
      fixture().billing!.groups[1],
      "1",
    );
    expect(result.unit).toBe("request");
    expect(result.rows[0].rates.request * result.multiplier).toBeCloseTo(
      0.0035,
    );
  });

  it("compares a transparent cache-heavy 100M token example", () => {
    const result = hundredMillionTokenEstimate(
      fixture(),
      fixture().billing!.groups[1],
      "7",
    );
    expect(result).toMatchObject({
      currency: "CNY",
      cacheFallback: false,
      tiered: true,
      savingPercent: 65,
    });
    expect(result?.official.minimum).toBeCloseTo(47.29133858);
    expect(result?.official.maximum).toBeCloseTo(94.58267717);
    expect(result?.yeschoy.minimum).toBeCloseTo(47.29133858 * 0.35);
    expect(result?.yeschoy.maximum).toBeCloseTo(94.58267717 * 0.35);
  });

  it("uses the input rate as a disclosed conservative cache fallback", () => {
    const model = fixture("ratio");
    const result = hundredMillionTokenEstimate(
      model,
      model.billing!.groups[1],
      "7",
    );
    expect(result).toMatchObject({
      cacheFallback: true,
      tiered: false,
    });
    expect(result?.official.minimum).toBeCloseTo(2107.06543261);
    expect(result?.official.maximum).toBeCloseTo(2107.06543261);
    expect(result?.yeschoy.minimum).toBeCloseTo(2107.06543261 * 0.35);
    expect(result?.yeschoy.maximum).toBeCloseTo(2107.06543261 * 0.35);
  });

  it("does not invent a token estimate without FX, group ratio or token rates", () => {
    const model = fixture();
    expect(
      hundredMillionTokenEstimate(model, model.billing!.groups[1], ""),
    ).toBeNull();
    expect(
      hundredMillionTokenEstimate(
        model,
        { id: "unknown", description: "", ratio: null },
        "7",
      ),
    ).toBeNull();
    expect(
      hundredMillionTokenEstimate(
        fixture("per_request"),
        model.billing!.groups[1],
        "7",
      ),
    ).toBeNull();
  });

  it("keeps a valid choice on refresh and replaces an unavailable group on model change", () => {
    expect(chooseBillingGroup(fixture(), "特价")).toBe("特价");
    expect(chooseBillingGroup(fixture(), "不存在")).toBe("default");
    const other = fixture();
    other.billing!.groups = [{ id: "另一个分组", description: "", ratio: 0.8 }];
    expect(chooseBillingGroup(other, "特价")).toBe("另一个分组");
    expect(chooseBillingGroup(undefined, "特价")).toBe("");
  });

  it.each([
    'tier("base", p * 3) * 5', // never silently omit a request multiplier
    'tier("base", p * c)',
    'tier("base", p * -3)',
    'tier("base", p * 3 + 2)',
    'tier("base", p / 0)',
    'tier("base", p * 1e999)',
    'tier("base", p * 3); fetch("https://example.test")',
    'tier("base", p * 3)|||when(header("x")) * 5',
    'param("tier") == "fast" ? tier("a", p * 3) : tier("b", p * 2)',
    "(".repeat(100) + "p" + ")".repeat(100),
  ])(
    "keeps unsupported or dangerous rules out of fixed-price claims: %s",
    (rule) => {
      expect(billingTiers(rule)).toEqual([]);
    },
  );

  it("supports constants, parentheses and long-context tier labels", () => {
    expect(
      billingTiers(
        'v1:len <= 200000 ? tier("普通 用量", 3 * p + c * (6 / 2)) : tier("长上下文", p * 6)',
      ),
    ).toEqual([
      { name: "普通 用量", rates: { p: 3, c: 3 } },
      { name: "长上下文", rates: { p: 6 } },
    ]);
  });
});

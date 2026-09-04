import { describe, expect, it } from "vitest";
import type { AccountModel } from "../account/session";
import { billingTiers, chooseBillingGroup, groupPrice } from "./billing";

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

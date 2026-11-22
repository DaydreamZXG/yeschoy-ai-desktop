import { beforeEach, describe, expect, it } from "vitest";
import type { AccountModel } from "../account/session";
import {
  discountOf,
  pickedModel,
  pickedPlan,
  plansOf,
  pricesOf,
  readCardPicks,
  storeCardPicks,
  tieredPricesOf,
  yuan,
} from "./cardInfo";

const model = (
  groups: [string, number | null][],
  prices: [string, string] | null = ["21", "105"],
): AccountModel => ({
  id: "claude-sonnet-4.5",
  description: "",
  billingMode: "ratio",
  pricingAvailable: !!prices,
  officialInputCnyPerMillion: prices?.[0] ?? "",
  officialOutputCnyPerMillion: prices?.[1] ?? "",
  actualInputCnyPerMillion: "",
  actualOutputCnyPerMillion: "",
  billing: {
    groups: groups.map(([id, ratio]) => ({ id, description: "", ratio })),
    baseInputUsd: 3,
    baseOutputUsd: 15,
    requestUsd: null,
    expression: "",
  },
});

describe("discountOf", () => {
  it("writes the station's multiplier the way people say it", () => {
    expect(discountOf(0.3)).toEqual({ kind: "off", tenths: 3 });
    expect(discountOf(0.65)).toEqual({ kind: "off", tenths: 6.5 });
    expect(discountOf(0.33)).toEqual({ kind: "off", tenths: 3.5 });
    expect(discountOf(1)).toEqual({ kind: "full" });
    expect(discountOf(0.98)).toEqual({ kind: "full" });
    expect(discountOf(1.5)).toEqual({ kind: "over", times: 1.5 });
    // 只贵 3%、4% 的不能写成「1 倍」。
    expect(discountOf(1.03)).toEqual({ kind: "over", times: 1.03 });
    expect(discountOf(1.04)).toEqual({ kind: "over", times: 1.04 });
    expect(discountOf(1.09)).toEqual({ kind: "over", times: 1.09 });
    expect(discountOf(1.1)).toEqual({ kind: "over", times: 1.1 });
    expect(discountOf(0)).toEqual({ kind: "free" });
  });

  it("never rounds a small multiplier down to nothing", () => {
    expect(discountOf(0.03)).toEqual({ kind: "off", tenths: 0.3 });
    expect(discountOf(0.001)).toEqual({ kind: "off", tenths: 0.1 });
  });

  it("stays silent when the station gave no usable number", () => {
    expect(discountOf(null)).toBeNull();
    expect(discountOf(undefined)).toBeNull();
    expect(discountOf(Number.NaN)).toBeNull();
    expect(discountOf(-1)).toBeNull();
  });
});

describe("plansOf", () => {
  it("marks the one plan that is clearly cheapest", () => {
    const plans = plansOf(
      model([
        ["default", 0.3],
        ["特价", 0.15],
        ["官转", 0.8],
      ]),
    );
    expect(plans.map((plan) => plan.cheapest)).toEqual([false, true, false]);
  });

  it("marks nothing when it cannot tell", () => {
    expect(plansOf(model([["default", 0.3]]))[0].cheapest).toBe(false);
    expect(
      plansOf(
        model([
          ["a", 0.3],
          ["b", 0.3],
        ]),
      ).some((plan) => plan.cheapest),
    ).toBe(false);
    expect(
      plansOf(
        model([
          ["a", 0.3],
          ["b", null],
        ]),
      ).some((plan) => plan.cheapest),
    ).toBe(false);
    expect(plansOf(undefined)).toEqual([]);
  });
});

describe("tieredPricesOf", () => {
  const tiered = (
    expression: string,
    patch: Partial<AccountModel> = {},
  ): AccountModel => ({
    ...model([["default", 0.7]], null),
    billingMode: "tiered_expr",
    ...patch,
    billing: {
      groups: [{ id: "default", description: "", ratio: 0.7 }],
      baseInputUsd: null,
      baseOutputUsd: null,
      requestUsd: null,
      expression,
      ...patch.billing,
    },
  });
  // 旧站真的这么报的两条。
  const PEAK =
    '(((weekday("Asia/Shanghai") >= 1 && weekday("Asia/Shanghai") <= 5) && ((hour("Asia/Shanghai") >= 9 && hour("Asia/Shanghai") < 12) || (hour("Asia/Shanghai") >= 14 && hour("Asia/Shanghai") < 18))) ? tier("高峰期", p * 2 + cr * 0.04 + c * 8) : tier("非高峰期", p * 1 + cr * 0.02 + c * 4))';

  it("lists one price per tier, in yuan, with the plan's multiplier applied", () => {
    expect(tieredPricesOf(tiered(PEAK), 0.5, "7")).toEqual({
      unit: "tokens",
      tiers: [
        {
          name: "高峰期",
          officialInput: 14,
          officialOutput: 56,
          planInput: 7,
          planOutput: 28,
        },
        {
          name: "非高峰期",
          officialInput: 7,
          officialOutput: 28,
          planInput: 3.5,
          planOutput: 14,
        },
      ],
    });
  });

  it("leaves a single tier unnamed", () => {
    expect(
      tieredPricesOf(tiered('tier("base", p * 5 + c * 25)'), null, "1"),
    ).toEqual({
      unit: "tokens",
      tiers: [
        {
          name: "",
          officialInput: 5,
          officialOutput: 25,
          planInput: null,
          planOutput: null,
        },
      ],
    });
  });

  it("prices a model that charges per request", () => {
    expect(
      tieredPricesOf(tiered('tier("base", fixed(0.05))'), 0.5, "1"),
    ).toEqual({
      unit: "request",
      tiers: [{ name: "", official: 0.05, plan: 0.025 }],
    });
    expect(
      tieredPricesOf(
        tiered("", {
          billingMode: "per_request",
          billing: { requestUsd: 0.2 } as AccountModel["billing"] & object,
        }),
        1,
        "7",
      ),
    ).toMatchObject({ unit: "request", tiers: [{ official: 0.2 * 7 }] });
  });

  it("says nothing when the rule can't be read or the rate is missing", () => {
    expect(tieredPricesOf(tiered(PEAK), 0.5, "")).toBeNull();
    expect(tieredPricesOf(tiered(PEAK), 0.5, "0")).toBeNull();
    expect(tieredPricesOf(tiered('tier("a", p * 3 + 2)'), 0.5, "1")).toBeNull();
    // 只报了输入价的一档：凑不出一行，整个不写。
    expect(tieredPricesOf(tiered('tier("a", p * 3)'), 0.5, "1")).toBeNull();
    expect(tieredPricesOf(model([["default", 0.3]]), 0.3, "1")).toBeNull();
    expect(tieredPricesOf(undefined, 0.3, "1")).toBeNull();
  });
});

describe("pricesOf", () => {
  it("prices a plan from the station's own list price and multiplier", () => {
    expect(pricesOf(model([["default", 0.3]]), 0.3)).toEqual({
      officialInput: 21,
      officialOutput: 105,
      planInput: 21 * 0.3,
      planOutput: 105 * 0.3,
    });
  });

  it("keeps an unknown price unknown instead of showing zero", () => {
    expect(pricesOf(model([["default", 0.3]], null), 0.3)).toBeNull();
    expect(pricesOf(model([["default", 0.3]], ["", ""]), 0.3)).toBeNull();
    expect(pricesOf(undefined, 0.3)).toBeNull();
    expect(pricesOf(model([["default", null]]), null)).toMatchObject({
      officialInput: 21,
      planInput: null,
      planOutput: null,
    });
  });

  it("formats money without hiding small amounts", () => {
    expect(yuan(21)).toBe("¥21.00");
    expect(yuan(6.3)).toBe("¥6.30");
    expect(yuan(0.014)).toBe("¥0.014");
  });
});

describe("card picks", () => {
  beforeEach(() => localStorage.clear());

  it("remembers what was picked on the card", () => {
    storeCardPicks("", {
      model: { cla: "claude-opus-4.1" },
      plan: { m: "特价" },
    });
    expect(readCardPicks("")).toEqual({
      model: { cla: "claude-opus-4.1" },
      plan: { m: "特价" },
    });
  });

  it("keeps each station's picks apart: same model name, different plans", () => {
    storeCardPicks("a.example", { model: {}, plan: { "gpt-5": "特价" } });
    storeCardPicks("b.example", { model: {}, plan: { "gpt-5": "default" } });
    expect(readCardPicks("a.example").plan).toEqual({ "gpt-5": "特价" });
    expect(readCardPicks("b.example").plan).toEqual({ "gpt-5": "default" });
    expect(readCardPicks("c.example")).toEqual({ model: {}, plan: {} });
    expect(readCardPicks("")).toEqual({ model: {}, plan: {} });
  });

  it("forgets the stations used longest ago, not the latest ones", () => {
    for (let n = 0; n < 25; n += 1)
      storeCardPicks(`s${n}.example`, { model: { gpt: `m${n}` }, plan: {} });
    expect(readCardPicks("s0.example").model).toEqual({});
    expect(readCardPicks("s24.example").model).toEqual({ gpt: "m24" });
    expect(readCardPicks("s5.example").model).toEqual({ gpt: "m5" });
  });

  it("ignores a damaged record", () => {
    localStorage.setItem("kaifan-card-picks", '{"":{"model":[1],"plan":{}}}');
    expect(readCardPicks("")).toEqual({ model: {}, plan: {} });
    localStorage.setItem("kaifan-card-picks", "not json");
    expect(readCardPicks("")).toEqual({ model: {}, plan: {} });
    localStorage.setItem("kaifan-card-picks", "[1]");
    expect(readCardPicks("")).toEqual({ model: {}, plan: {} });
    // 坏掉的记录不挡着新的选择存进去。
    storeCardPicks("", { model: { gpt: "gpt-5" }, plan: {} });
    expect(readCardPicks("").model).toEqual({ gpt: "gpt-5" });
  });

  it("drops a pick the station no longer offers", () => {
    const picks = {
      model: { cla: "gone" } as const,
      plan: { "claude-sonnet-4.5": "停售" },
    };
    expect(pickedModel("cla", ["a", "b"], picks)).toBe("a");
    expect(pickedModel("cla", ["a", "gone"], picks)).toBe("gone");
    expect(pickedModel("cla", undefined, picks)).toBe("");
    const m = model([["default", 0.3]]);
    expect(pickedPlan(m, picks)).toBeUndefined();
    expect(pickedPlan(m, { model: {}, plan: { [m.id]: "default" } })).toBe(
      "default",
    );
  });
});

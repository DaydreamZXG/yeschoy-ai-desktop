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
    storeCardPicks({ model: { cla: "claude-opus-4.1" }, plan: { m: "特价" } });
    expect(readCardPicks()).toEqual({
      model: { cla: "claude-opus-4.1" },
      plan: { m: "特价" },
    });
  });

  it("ignores a damaged record", () => {
    localStorage.setItem("kaifan-card-picks", '{"model":[1],"plan":{}}');
    expect(readCardPicks()).toEqual({ model: {}, plan: {} });
    localStorage.setItem("kaifan-card-picks", "not json");
    expect(readCardPicks()).toEqual({ model: {}, plan: {} });
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

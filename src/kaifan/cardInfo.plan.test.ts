import { describe, expect, it } from "vitest";
import type { AccountModel } from "../account/session";
import { currencyOf, planLabelOf, plansOf, pricesOf, yuan } from "./cardInfo";

/** 按中立约定接入的站：方案有自己的名字和价，货币由站点说。 */
function model(currency: "CNY" | "USD"): AccountModel {
  return {
    id: "claude",
    description: "",
    billingMode: "ratio",
    pricingAvailable: true,
    officialInputCnyPerMillion: "20",
    officialOutputCnyPerMillion: "100",
    actualInputCnyPerMillion: "10",
    actualOutputCnyPerMillion: "60",
    billing: {
      groups: [
        {
          id: "7",
          description: "",
          ratio: 0.5,
          plan: {
            label: "标准",
            currency,
            inputPerMillion: 10,
            outputPerMillion: 60,
            appOnly: false,
            subscription: false,
          },
        },
        {
          id: "9",
          description: "",
          ratio: 0.25,
          plan: {
            label: "种菜线路",
            currency,
            inputPerMillion: 5,
            outputPerMillion: 25,
            appOnly: true,
            subscription: true,
          },
        },
      ],
      baseInputUsd: 20,
      baseOutputUsd: 100,
      requestUsd: null,
      expression: "",
    },
  };
}

describe("按中立约定接入的站的方案", () => {
  it("方案显示站点起的名字，不是一串数字", () => {
    const models = [model("CNY")];
    expect(planLabelOf(models, "9")).toBe("种菜线路");
    expect(planLabelOf(models, "404")).toBeNull();
  });

  it("月卡和客户端专属价各自标出来，几折照旧", () => {
    const [standard, garden] = plansOf(model("CNY"));
    expect(standard).toMatchObject({ subscription: false, appOnly: false });
    expect(garden).toMatchObject({
      subscription: true,
      appOnly: true,
      cheapest: true,
    });
  });

  it("方案的价用站点直接给的，不拿官方价乘倍率去推", () => {
    const [standard] = plansOf(model("CNY"));
    // 输出价 60 不是 100 × 0.5：站点给了就照站点的。
    expect(
      pricesOf(model("CNY"), standard.ratio, standard.pricing),
    ).toMatchObject({ planInput: 10, planOutput: 60, officialOutput: 100 });
  });

  it("站点用美元计价就写美元", () => {
    expect(currencyOf(model("USD"))).toBe("USD");
    expect(currencyOf(model("CNY"))).toBe("CNY");
    expect(currencyOf(undefined)).toBe("CNY");
    expect(yuan(1.5, "USD")).toBe("$1.50");
    expect(yuan(1.5)).toBe("¥1.50");
  });
});

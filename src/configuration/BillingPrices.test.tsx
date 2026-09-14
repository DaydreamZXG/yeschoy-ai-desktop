import {
  cleanup,
  fireEvent,
  render,
  screen,
  within,
} from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import i18n from "i18next";
import zh from "../i18n/locales/zh.json";
import type { AccountModel } from "../account/session";
import { BillingGroupPicker, BillingPrices } from "./BillingGroupPicker";

beforeEach(async () => {
  i18n.addResourceBundle("zh", "translation", zh, true, true);
  await i18n.changeLanguage("zh");
});
afterEach(cleanup);
const model = (ratio: number, priced = false): AccountModel => ({
  id: "example-model",
  description: "",
  billingMode: "ratio",
  pricingAvailable: priced,
  officialInputCnyPerMillion: priced ? "2" : "",
  officialOutputCnyPerMillion: priced ? "8" : "",
  actualInputCnyPerMillion: priced ? "1" : "",
  actualOutputCnyPerMillion: priced ? "4" : "",
  billing: {
    baseInputUsd: 0.2,
    baseOutputUsd: 1.25,
    cacheReadUsd: 0.02,
    requestUsd: null,
    expression: "",
    groups: [{ id: "example", ratio, description: "" }],
  },
});
describe("pricing plan choices", () => {
  it("shows comparable costs, keeps descriptions in details, and does not auto-select", () => {
    const value = model(0.4);
    value.billing!.groups = [
      { id: "default", ratio: 0.7, description: "标准入口" },
      {
        id: "DeepSeek Flash",
        ratio: 0.35,
        description: "模型：其他模型；3.5折",
      },
    ];
    let chosen = "default";
    render(
      <BillingGroupPicker
        model={value}
        selected={chosen}
        fx="1"
        onChange={(id) => {
          chosen = id;
        }}
      />,
    );
    const standard = screen.getByRole("radio", { name: /标准分组/ });
    expect(standard).toBeChecked();
    expect(chosen).toBe("default");
    const cheapest = screen.getByRole("radio", { name: /DeepSeek Flash/ });
    expect(cheapest).toHaveAccessibleName(/价格最低/);
    expect(cheapest).not.toHaveAccessibleName(/其他模型|3.5折/);
    const card = cheapest.closest(".billing-plan-card")!;
    expect(within(card as HTMLElement).getByText("¥0.82")).toBeInTheDocument();
    expect(card.querySelector("details")).not.toHaveAttribute("open");
    fireEvent.click(within(card as HTMLElement).getByText("详情"));
    expect(chosen).toBe("default");
    fireEvent.click(cheapest);
    expect(chosen).toBe("DeepSeek Flash");
  });
  it("does not claim lowest price when another plan is unpriced and respects disabled state", () => {
    const value = model(0.4);
    value.billing!.groups.push({ id: "unknown", ratio: null, description: "" });
    render(
      <BillingGroupPicker
        model={value}
        selected="example"
        fx="1"
        disabled
        onChange={() => {
          throw Error("must not change");
        }}
      />,
    );
    expect(screen.queryByText("价格最低")).not.toBeInTheDocument();
    expect(screen.getByText("价格待确认")).toBeInTheDocument();
    for (const radio of screen.getAllByRole("radio"))
      expect(radio).toBeDisabled();
  });
});

describe("rounded price comparisons remain honest", () => {
  it("never turns a very small positive price into zero or a full discount", () => {
    render(<BillingPrices model={model(0.0001)} selected="example" fx="1" />);
    expect(
      document.querySelector(".billing-comparison-card.is-yeschoy strong"),
    ).toHaveTextContent("< ¥0.01");
    expect(screen.getByText("约省 99.9%")).toBeInTheDocument();
    expect(screen.queryByText("约省 100%")).not.toBeInTheDocument();
  });
  it("keeps an explicitly zero price distinguishable from missing or rounded data", () => {
    render(<BillingPrices model={model(0)} selected="example" fx="1" />);
    expect(
      document.querySelector(".billing-comparison-card.is-yeschoy strong"),
    ).toHaveTextContent(/^¥0$/);
    expect(screen.getByText("约省 100%")).toBeInTheDocument();
  });
});
describe("#12 per-million prices are shown directly (PRD 6.4)", () => {
  it("renders the official and actual input/output rows per million tokens", () => {
    render(<BillingPrices model={model(1, true)} selected="example" fx="1" />);
    const table = document.querySelector("table.per-million-prices")!;
    expect(table).toHaveTextContent("每百万 tokens");
    expect(table).toHaveTextContent("官网参考价");
    expect(table).toHaveTextContent("野菜API实际价");
    expect(table).toHaveTextContent("输入");
    expect(table).toHaveTextContent("输出");
    expect(table).toHaveTextContent("¥2");
    expect(table).toHaveTextContent("¥8");
    expect(table).toHaveTextContent("¥1");
    expect(table).toHaveTextContent("¥4");
    // 1 亿估算降级为折叠示例
    const estimate = document.querySelector("details.billing-estimate")!;
    expect(estimate).not.toHaveAttribute("open");
    expect(screen.getByText("1 亿 Token 费用参考")).toBeInTheDocument();
  });
  it("keeps the estimate collapsed and uses billing rates when comparison fields are absent", () => {
    render(<BillingPrices model={model(1, false)} selected="example" fx="1" />);
    expect(screen.queryByText("暂不可用")).not.toBeInTheDocument();
    expect(document.querySelector("table.per-million-prices")).not.toBeNull();
    expect(
      document.querySelector("details.billing-estimate"),
    ).not.toHaveAttribute("open");
  });
  it("shows the confirmed GPT CNY comparison and total savings", () => {
    const value = model(0.4);
    value.id = "gpt-6-astra";
    value.billingMode = "tiered_expr";
    value.billing!.expression =
      'len <= 272000 ? tier("standard", p * 10 + c * 50 + cr * 1) : tier("long", p * 20 + c * 75 + cr * 2)';
    render(<BillingPrices model={value} selected="example" fx="1" />);
    expect(screen.getByText("¥772.84 – ¥1,517.3")).toBeInTheDocument();
    expect(screen.getByText("¥45.8 – ¥89.91")).toBeInTheDocument();
    expect(
      screen.getByText("费用参考，实际费用随使用情况变化"),
    ).toBeInTheDocument();
    expect(
      screen.getByText("约 0.69% 新输入 + 99.14% 缓存读取 + 0.17% 输出"),
    ).toBeInTheDocument();
    expect(screen.getByText("约省 94.1%")).toBeInTheDocument();
    expect(screen.queryByText("约省 60%")).not.toBeInTheDocument();
    expect(screen.queryByText("暂不可用")).not.toBeInTheDocument();
  });
  it("marks empty per-million values as unknown instead of inventing zero", () => {
    const partial = { ...model(1, true), actualInputCnyPerMillion: "" };
    render(<BillingPrices model={partial} selected="example" fx="1" />);
    const cells = Array.from(
      document.querySelectorAll("table.per-million-prices td"),
    ).map((td) => td.textContent);
    expect(cells).toContain("—");
    expect(cells).toContain("¥2");
  });
});

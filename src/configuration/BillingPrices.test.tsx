import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import i18n from "i18next";
import zh from "../i18n/locales/zh.json";
import type { AccountModel } from "../account/session";
import { BillingPrices } from "./BillingGroupPicker";

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
describe("rounded price comparisons remain honest", () => {
  it("never turns a very small positive price into zero or a full discount", () => {
    render(<BillingPrices model={model(0.0001)} selected="example" fx="1" />);
    expect(screen.getByText("< ¥0.01")).toBeInTheDocument();
    expect(screen.getByText("约省 99.9%")).toBeInTheDocument();
    expect(screen.queryByText("约省 100%")).not.toBeInTheDocument();
  });
  it("keeps an explicitly zero price distinguishable from missing or rounded data", () => {
    render(<BillingPrices model={model(0)} selected="example" fx="1" />);
    expect(screen.getByText("¥0")).toBeInTheDocument();
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
    expect(
      screen.getByText("1 亿 Token 费用参考"),
    ).toBeInTheDocument();
  });
  it("keeps the estimate collapsed and shows unavailable note without prices", () => {
    render(<BillingPrices model={model(1, false)} selected="example" fx="1" />);
    expect(screen.getByText("暂不可用")).toBeInTheDocument();
    expect(document.querySelector("table.per-million-prices")).toBeNull();
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

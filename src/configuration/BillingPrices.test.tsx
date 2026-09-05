import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import type { AccountModel } from "../account/session";
import { BillingPrices } from "./BillingGroupPicker";

afterEach(cleanup);
const model = (ratio: number): AccountModel => ({
  id: "example-model",
  description: "",
  billingMode: "ratio",
  pricingAvailable: false,
  officialInputCnyPerMillion: "",
  officialOutputCnyPerMillion: "",
  actualInputCnyPerMillion: "",
  actualOutputCnyPerMillion: "",
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

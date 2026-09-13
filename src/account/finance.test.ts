import { describe, expect, it } from "vitest";
import {
  balanceAlert,
  LOW_BALANCE_THRESHOLD_CNY,
  type AccountMoney,
} from "./finance";

const money = (balanceAmount: string, currency = "CNY"): AccountMoney => ({
  currency: currency as AccountMoney["currency"],
  balanceAmount,
  consumedAmount: "0",
  displayRate: "6.75",
});

describe("balanceAlert (PRD 6.2 single-threshold guidance)", () => {
  it("is null when money is unavailable or not CNY", () => {
    expect(balanceAlert(undefined)).toBeNull();
    expect(balanceAlert(money("3.20", "USD"))).toBeNull();
    expect(balanceAlert(money("", ""))).toBeNull();
  });

  it("is null above the threshold and low at or below it", () => {
    expect(
      balanceAlert(money(String(LOW_BALANCE_THRESHOLD_CNY + 0.01))),
    ).toBeNull();
    expect(balanceAlert(money(String(LOW_BALANCE_THRESHOLD_CNY)))).toEqual({
      level: "low",
      amount: "5",
    });
    expect(balanceAlert(money("3.20"))).toEqual({
      level: "low",
      amount: "3.20",
    });
  });

  it("is depleted at or below zero, including negatives", () => {
    expect(balanceAlert(money("0"))).toEqual({ level: "depleted", amount: "0" });
    expect(balanceAlert(money("-0.50"))).toEqual({
      level: "depleted",
      amount: "-0.50",
    });
  });

  it("ignores malformed amounts instead of warning wrongly", () => {
    expect(balanceAlert(money("not-a-number"))).toBeNull();
  });
});

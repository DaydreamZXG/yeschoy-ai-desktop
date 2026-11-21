import { render, screen, waitFor, within } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import "../i18n";
import type { UsageLogReport, UsageRecord } from "../account/usage";
import { Ledger } from "./Ledger";

const NOW = new Date(2026, 10, 21, 15, 0).getTime();
const at = (daysAgo: number, hour = 10) =>
  new Date(2026, 10, 21 - daysAgo, hour).getTime();
const rec = (
  modelId: string,
  when: number,
  amount: string,
  toolId = "claude_code",
): UsageRecord => ({
  toolId,
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
const money = {
  currency: "CNY" as const,
  balanceAmount: "23.5",
  consumedAmount: "108.2",
  displayRate: "7.2",
};
const show = (props: Partial<Parameters<typeof Ledger>[0]> = {}) =>
  render(
    <Ledger
      open
      who="cla"
      reach="ok"
      money={money}
      appName={(id) => `应用:${id}`}
      onRecharge={vi.fn()}
      onConnect={vi.fn()}
      onClose={vi.fn()}
      now={NOW}
      {...props}
    />,
  );
const says = () => screen.getByTestId("kaifan-ledger-says");

describe("Ledger", () => {
  it("余额、七天、排行、最近几笔，都是站点给的数", async () => {
    show({
      log: log([
        rec("claude-sonnet-4-5", at(0), "0.42"),
        rec("claude-sonnet-4-5", at(1), "3"),
        rec("deepseek-v4-flash", at(2), "0.08", ""),
      ]),
    });
    expect(screen.getByTestId("kaifan-ledger-balance")).toHaveTextContent(
      "¥23.50",
    );
    expect(screen.getByTestId("kaifan-ledger-balance")).toHaveTextContent(
      "一共用掉 ¥108.20",
    );
    expect(screen.getByTestId("kaifan-ledger-week")).toHaveTextContent("¥3.50");
    const eaters = within(screen.getByTestId("kaifan-ledger-eaters"))
      .getAllByRole("listitem")
      .map((row) => row.textContent);
    expect(eaters[0]).toContain("¥3.42");
    expect(eaters[0]).toContain("2 次");
    expect(eaters[1]).toContain("¥0.08");
    const latest = within(
      screen.getByTestId("kaifan-ledger-latest"),
    ).getAllByRole("listitem");
    expect(latest).toHaveLength(3);
    expect(latest[0]).toHaveTextContent("¥0.42");
    expect(latest[0]).toHaveTextContent("应用:claude_code");
    // 没法归到哪个应用的那一笔，不编一个应用名。
    expect(latest[2]).not.toHaveTextContent("应用:");
    await waitFor(() =>
      expect(says()).toHaveTextContent("今天花了 ¥0.42。余额还有 ¥23.50。"),
    );
  });

  it("没带金额的记录：写「看不到」，不写 0，合计也不装作知道", async () => {
    show({
      log: log([
        rec("claude-sonnet-4-5", at(0), ""),
        rec("claude-sonnet-4-5", at(1), "3"),
      ]),
    });
    expect(screen.getByTestId("kaifan-ledger-week")).toHaveTextContent(
      "没读全",
    );
    expect(
      within(screen.getByTestId("kaifan-ledger-eaters")).getByRole("listitem"),
    ).toHaveTextContent("看不到");
    expect(
      within(screen.getByTestId("kaifan-ledger-latest")).getAllByRole(
        "listitem",
      )[0],
    ).toHaveTextContent("看不到");
    // 今天那一格看不到；前几天明细读全了、确实没花，才是 0。
    const days = screen
      .getByTestId("kaifan-ledger")
      .querySelectorAll(".days li");
    expect(days[6]).toHaveClass("na");
    expect(days[6]).toHaveTextContent("看不到");
    expect(days[4]).not.toHaveClass("na");
    expect(days[4]).toHaveTextContent("¥0.00");
    await waitFor(() =>
      expect(says()).toHaveTextContent("今天花了多少我看不到。"),
    );
  });

  it("明细被截断：读不到的那几天画成看不到，排行写明从哪天算起", () => {
    show({
      log: log([rec("gpt-5", at(0), "1"), rec("gpt-5", at(2, 12), "2")], {
        truncated: true,
      }),
    });
    const days = screen
      .getByTestId("kaifan-ledger")
      .querySelectorAll(".days li");
    expect(days).toHaveLength(7);
    expect([...days].map((day) => day.classList.contains("na"))).toEqual([
      true,
      true,
      true,
      true,
      true,
      false,
      false,
    ]);
    expect(screen.getByTestId("kaifan-ledger-week")).toHaveTextContent(
      "没读全",
    );
    expect(screen.getByTestId("kaifan-ledger")).toHaveTextContent("11/19 之后");
  });

  it("一笔都没有：今天是真的 0，说「还没开吃」", async () => {
    show({ log: log([]) });
    expect(screen.getByTestId("kaifan-ledger-week")).toHaveTextContent("¥0.00");
    expect(screen.getByTestId("kaifan-ledger")).toHaveTextContent(
      "这段时间还没人开吃。",
    );
    await waitFor(() =>
      expect(says()).toHaveTextContent("今天还没开吃。余额还有 ¥23.50。"),
    );
  });

  it("站点给了余额但不给明细：余额照写，明细直说没有", () => {
    show({ log: log([], { status: "unavailable" }) });
    expect(screen.getByTestId("kaifan-ledger-balance")).toHaveTextContent(
      "¥23.50",
    );
    expect(screen.getByTestId("kaifan-ledger-nolog")).toBeInTheDocument();
    expect(screen.queryByTestId("kaifan-ledger-week")).toBeNull();
  });

  it("用密钥连的站什么都不给：直说看不到，不显示 0", async () => {
    show({ money: undefined, log: undefined });
    expect(screen.queryByTestId("kaifan-ledger-balance")).toBeNull();
    expect(screen.getByTestId("kaifan-ledger")).not.toHaveTextContent("0");
    await waitFor(() =>
      expect(says()).toHaveTextContent(
        "这个站没把余额和账单告诉我，我看不到。",
      ),
    );
  });

  it("没连站点：不给充值，给「连接站点」", () => {
    const onConnect = vi.fn();
    show({ reach: "out", money: undefined, onConnect });
    expect(screen.queryByTestId("kaifan-ledger-recharge")).toBeNull();
    screen.getByRole("button", { name: "连接站点" }).click();
    expect(onConnect).toHaveBeenCalled();
  });

  it("站点连不上：不拿上一次的数出来充数", () => {
    show({ reach: "off", log: log([rec("gpt-5", at(0), "1")]) });
    expect(screen.queryByTestId("kaifan-ledger-balance")).toBeNull();
    expect(screen.queryByTestId("kaifan-ledger-recharge")).toBeNull();
  });
});

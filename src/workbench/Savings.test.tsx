import {
  cleanup,
  fireEvent,
  render,
  screen,
  within,
} from "@testing-library/react";
import { afterEach, beforeAll, describe, expect, it, vi } from "vitest";
import i18n from "i18next";
import zh from "../i18n/locales/zh.json";
import type { RecentSavings } from "../account/finance";
import type { AccountProjection } from "../account/session";
import type { AccountSessionController } from "../account/useAccountSession";
import { SavingsCard, SavingsDetails } from "./Savings";
import { AccountView } from "./AccountView";
import { AccountSummary } from "./WorkbenchChrome";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
beforeAll(async () => {
  await i18n.init({
    lng: "zh",
    resources: { zh: { translation: zh } },
    interpolation: { escapeValue: false },
  });
});
afterEach(cleanup);
const savings: RecentSavings = {
  status: "available",
  reasonCode: "none",
  officialAmount: "140",
  siteAmount: "20",
  savedAmount: "120",
  referenceRate: "7",
  priceRate: "2",
  recordLimit: 100,
  scannedCount: 100,
  includedCount: 80,
  excludedCount: 20,
  oldestAtEpochMs: 1000,
  newestAtEpochMs: 2000,
};
function projection(): AccountProjection {
  return {
    requestId: "ui-finance",
    schemaVersion: 3,
    status: "signed_in",
    userCode: "",
    pollAfterSeconds: 0,
    expiresAtEpochMs: 0,
    observedAtEpochMs: 3000,
    account: {
      available: true,
      displayName: "普通用户",
      username: "member",
      balanceQuota: "13295000",
      usedQuota: "165330000",
      requestCount: "3274",
      quotaPerUnit: "500000",
    },
    usage: {
      available: true,
      consumedQuota: "1",
      requestRate: "3",
      tokenCount: "999999",
    },
    models: [],
    comparisonFx: "1",
    reasonCode: "none",
    money: {
      currency: "CNY",
      balanceAmount: "26.59",
      consumedAmount: "330.66",
      displayRate: "1",
    },
    savings,
  };
}
function controller(): AccountSessionController {
  return {
    projection: projection(),
    loading: false,
    lastError: null,
    refresh: vi.fn(),
    beginAuthorization: vi.fn(),
    cancelAuthorization: vi.fn(),
    logout: vi.fn(),
    openWallet: vi.fn(),
  };
}

describe("clear account currency and savings UX", () => {
  it("shows RMB and recent savings on overview, not recent TPM as cumulative tokens", () => {
    render(
      <AccountSummary
        detected={2}
        onOpenAccount={vi.fn()}
        accountProjection={projection()}
      />,
    );
    expect(screen.getByText("¥26.59")).toBeInTheDocument();
    const card = screen.getByText("近期费用对比").closest("article")!;
    for (const [label, amount] of [
      ["官网应收", "¥140.00"],
      ["野菜收取", "¥20.00"],
      ["预计省下", "¥120.00"],
    ]) {
      const row = within(card).getByText(label).closest("div")!;
      expect(within(row).getByText(amount)).toBeVisible();
    }
    expect(within(card).getByText("估算")).toBeInTheDocument();
    expect(within(card).getByText("基于 80 笔可比较记录")).toBeInTheDocument();
    expect(screen.getByText("人民币额度")).toBeInTheDocument();
    expect(screen.getByText("累计请求")).toBeInTheDocument();
    expect(screen.queryByText("累计用量")).not.toBeInTheDocument();
    expect(screen.queryByText(/US\$/)).not.toBeInTheDocument();
  });
  it("retains actual cumulative consumption and opens the calculation with keyboard focus", () => {
    render(
      <AccountView
        lineId="mainland_optimized"
        onLineChange={vi.fn()}
        session={controller()}
      />,
    );
    expect(screen.getByText("¥330.66")).toBeInTheDocument();
    const details = screen.getByText("节省金额怎么算的？").closest("details")!;
    expect(details.open).toBe(false);
    fireEvent.click(screen.getByRole("button", { name: "怎么算的" }));
    expect(details.open).toBe(true);
    expect(details.querySelector("summary")).toHaveFocus();
    expect(
      within(details).getByText(/80 笔参与比较，20 笔未计入/),
    ).toBeInTheDocument();
    expect(within(details).getByText(/不是累计或本月节省/)).toBeInTheDocument();
    expect(within(details).getByText(/不是官网实际账单/)).toBeInTheDocument();
  });
  it("explains that account logout does not silently rewrite connected apps", () => {
    const session = controller();
    render(
      <AccountView
        lineId="mainland_optimized"
        onLineChange={vi.fn()}
        session={session}
      />,
    );
    fireEvent.click(screen.getByRole("button", { name: "退出登录" }));
    expect(screen.getByText("仅退出野菜API账户？")).toBeInTheDocument();
    expect(screen.getByText(/不会改动 Codex、Claude/)).toBeInTheDocument();
    expect(session.logout).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: "仅退出账户" }));
    expect(session.logout).toHaveBeenCalledTimes(1);
  });
  it("shows a higher-than-reference result truthfully instead of claiming savings", () => {
    render(
      <SavingsCard
        savings={{
          ...savings,
          officialAmount: "18",
          siteAmount: "20",
          savedAmount: "-2",
        }}
      />,
    );
    expect(screen.getByText("高于参考估算")).toBeInTheDocument();
    expect(screen.getByText("¥18.00")).toBeInTheDocument();
    expect(screen.getByText("¥20.00")).toBeInTheDocument();
    expect(screen.getByText("¥2.00")).toBeInTheDocument();
    expect(screen.queryByText("预计省下")).not.toBeInTheDocument();
  });
  it("keeps unavailable amounts unknown and offers an explanation", () => {
    render(
      <>
        <SavingsCard />
        <SavingsDetails />
      </>,
    );
    expect(screen.getAllByText("—")).toHaveLength(3);
    expect(screen.getByText("暂时无法估算")).toBeInTheDocument();
    expect(screen.queryByText("¥0.00")).not.toBeInTheDocument();
  });
  it("shows genuine zero savings as equal costs, not missing data", () => {
    render(
      <SavingsCard
        savings={{
          ...savings,
          officialAmount: "20",
          siteAmount: "20",
          savedAmount: "0",
        }}
      />,
    );
    expect(screen.getAllByText("¥20.00")).toHaveLength(2);
    expect(screen.getByText("¥0.00")).toBeInTheDocument();
    expect(screen.getByText("与参考估算持平")).toBeInTheDocument();
    expect(screen.queryByText("—")).not.toBeInTheDocument();
  });
  it("keeps all three values unknown while loading or without comparable history", () => {
    const view = render(<SavingsCard loading />);
    expect(screen.getByText("正在读取账单…")).toBeInTheDocument();
    expect(screen.getAllByText("—")).toHaveLength(3);
    const unknown = {
      ...savings,
      officialAmount: "",
      siteAmount: "",
      savedAmount: "",
      referenceRate: "",
      priceRate: "",
      includedCount: 0,
      oldestAtEpochMs: 0,
      newestAtEpochMs: 0,
    };
    view.rerender(
      <SavingsCard
        savings={{
          ...unknown,
          status: "empty",
          reasonCode: "no_history",
          scannedCount: 0,
          excludedCount: 0,
        }}
      />,
    );
    expect(screen.getByText("有消费记录后显示")).toBeInTheDocument();
    expect(screen.getAllByText("—")).toHaveLength(3);
    view.rerender(
      <SavingsCard
        savings={{
          ...unknown,
          status: "no_comparable_records",
          reasonCode: "missing_basis",
          scannedCount: 100,
          excludedCount: 100,
        }}
      />,
    );
    expect(screen.getByText("暂无可比较记录")).toBeInTheDocument();
    expect(screen.getAllByText("—")).toHaveLength(3);
    expect(screen.queryByText("¥0.00")).not.toBeInTheDocument();
  });
  it("labels old data on refresh failure without changing displayed currency", () => {
    render(
      <AccountView
        lineId="global_accelerated"
        onLineChange={vi.fn()}
        session={{ ...controller(), lastError: "network_error" }}
      />,
    );
    expect(screen.getByText(/以下是上次读取的数据/)).toBeInTheDocument();
    expect(screen.getByText("¥26.59")).toBeInTheDocument();
    expect(
      screen.getByText(/Cloudflare 全球线路，海外可优先尝试/),
    ).toBeInTheDocument();
    expect(screen.getByText(/不改变计费分组和倍率/)).toBeInTheDocument();
  });
  it("switches a network route through the existing handler without initiating authorization", () => {
    const session = controller(),
      change = vi.fn();
    render(
      <AccountView
        lineId="mainland_optimized"
        onLineChange={change}
        session={session}
      />,
    );
    fireEvent.change(screen.getByRole("combobox"), {
      target: { value: "global_accelerated" },
    });
    expect(change).toHaveBeenCalledWith("global_accelerated");
    expect(session.beginAuthorization).not.toHaveBeenCalled();
  });
});

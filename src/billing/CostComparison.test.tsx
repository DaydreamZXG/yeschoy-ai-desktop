import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
  within,
} from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import i18n from "i18next";
import { CostComparison } from "./CostComparison";
import { comparisonCopies } from "./copy";
import { contextFixture as context, evidenceFixture } from "./test-fixtures";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
const native = vi.mocked(invoke);
beforeEach(async () => {
  for (const language of Object.keys(comparisonCopies))
    i18n.addResourceBundle(language, "translation", { testLanguage: language });
  await i18n.changeLanguage("zh");
  native.mockClear();
});
afterEach(() => {
  cleanup();
  vi.useRealTimers();
  vi.restoreAllMocks();
});

describe("auditable comparison presentation", () => {
  it.each(["zh", "en", "zh-TW", "ja"] as const)(
    "shows an honest disconnected state in %s, without fetching or native side effects",
    async (language) => {
      await i18n.changeLanguage(language);
      const fetch = vi.spyOn(globalThis, "fetch");
      const { container } = render(<CostComparison />);
      const copy = comparisonCopies[language];
      expect(screen.getByText(copy.unavailable)).toBeInTheDocument();
      expect(screen.getAllByText("—")).toHaveLength(3);
      expect(container.querySelector(".cost-totals")).not.toHaveTextContent(
        /¥|%/,
      );
      expect(container.querySelector(".cost-method")).not.toHaveAttribute(
        "open",
      );
      fireEvent.click(screen.getByText(copy.method));
      expect(container.querySelector(".cost-method")).toHaveAttribute("open");
      expect(screen.getByText(copy.rounding)).toBeVisible();
      expect(fetch).not.toHaveBeenCalled();
      expect(native).not.toHaveBeenCalled();
      expect(container.textContent).not.toMatch(
        /NewAPI|契约|契約|适配器|適配器|原生桥|候选版|backend|contract|adapter|mock/i,
      );
    },
  );
  it("keeps all four translations complete", () => {
    for (const copy of Object.values(comparisonCopies)) {
      expect(Object.keys(copy).sort()).toEqual(
        Object.keys(comparisonCopies.zh).sort(),
      );
      expect(Object.values(copy).every((value) => value.length > 0)).toBe(true);
    }
  });
  it("shows source, full IDs, usage, exact unit prices and rounding rules for the same cohort", () => {
    const evidence = evidenceFixture();
    const { container } = render(
      <CostComparison evidence={evidence} context={context} />,
    );
    const totals = within(
      container.querySelector(".cost-totals") as HTMLElement,
    );
    for (const amount of ["¥10.53", "¥3.00", "¥7.53", "71.5%"])
      expect(totals.getByText(amount)).toBeInTheDocument();
    expect(screen.getByText("已纳入 1/1 笔")).toBeInTheDocument();
    expect(
      container.querySelector('[data-direction="saved"]'),
    ).toBeInTheDocument();
    fireEvent.click(screen.getByText(evidence.rows[0].modelId));
    expect(screen.getByText(evidence.rows[0].officialModelId)).toBeVisible();
    expect(screen.getByText(evidence.rows[0].officialSourceUrl)).toBeVisible();
    expect(screen.getByText(evidence.rows[0].id)).toBeVisible();
    expect(screen.getByText("3 CNY")).toBeVisible();
    expect(screen.getByText("1.56 USD")).toBeVisible();
    expect(screen.getByText("800,000")).toBeVisible();
    expect(screen.getByText("0.16")).toBeVisible();
    // An untrusted source URL is text; the component cannot open arbitrary sites.
    expect(container.querySelector("a")).toBeNull();
  });
  it("visibly separates exclusions and pending records from the comparison", () => {
    const evidence = evidenceFixture();
    evidence.excluded = [
      {
        id: "e1",
        modelId: "unmapped",
        occurredAtMs: evidence.rows[0].occurredAtMs,
        reason: "model_unverified",
        settledNetCnyMicros: "9000000",
      },
      {
        id: "e2",
        modelId: "pending",
        occurredAtMs: evidence.rows[0].occurredAtMs,
        reason: "pending_settlement",
        settledNetCnyMicros: null,
      },
    ];
    const { container } = render(
      <CostComparison evidence={evidence} context={context} />,
    );
    expect(screen.getByText("已纳入 1/3 笔")).toBeInTheDocument();
    expect(screen.getByText("¥9.00")).toBeInTheDocument();
    expect(screen.getByText("模型尚无法对应")).toBeInTheDocument();
    expect(screen.getByText(/尚有待结算记录 \(1\)/)).toBeInTheDocument();
    expect(container.querySelector(".cost-totals")).toHaveTextContent("¥3.00");
    expect(container.querySelector(".cost-totals")).not.toHaveTextContent(
      "¥12.00",
    );
  });
  it("does not present higher costs as a saving or hide them", () => {
    const evidence = evidenceFixture();
    evidence.rows[0].settledNetCnyMicros = "20000000";
    const { container } = render(
      <CostComparison evidence={evidence} context={context} />,
    );
    expect(screen.getByText(comparisonCopies.zh.more)).toBeInTheDocument();
    expect(
      screen.queryByText(comparisonCopies.zh.saved),
    ).not.toBeInTheDocument();
    expect(screen.getByText("¥9.47")).toBeInTheDocument();
    expect(screen.getByText("89.9%")).toBeInTheDocument();
    expect(container.querySelector('[data-direction="saved"]')).toBeNull();
  });
  it("does not render an all-excluded period as zero spend", () => {
    const evidence = evidenceFixture();
    evidence.rows = [];
    const { container } = render(
      <CostComparison evidence={evidence} context={context} />,
    );
    expect(screen.getAllByText("—")).toHaveLength(3);
    expect(screen.getByText(comparisonCopies.zh.noneBody)).toBeVisible();
    expect(container.textContent).not.toMatch(/¥0.00|100%/);
  });
  it.each(["invalid", "expired"] as const)(
    "withholds all amounts when evidence is %s",
    (status) => {
      const evidence = evidenceFixture();
      if (status === "invalid") evidence.rows[0].officialTotalMicros = "1";
      else evidence.expiresAtMs = context.nowMs;
      // Keep an otherwise valid expiry window when checking expiration.
      if (status === "expired") evidence.generatedAtMs -= 1;
      const { container } = render(
        <CostComparison evidence={evidence} context={context} />,
      );
      expect(screen.getByText(comparisonCopies.zh[status])).toBeVisible();
      expect(screen.getAllByText("—")).toHaveLength(3);
      expect(container.textContent).not.toContain(evidence.rows[0].modelId);
      expect(container.querySelector(".cost-totals")).not.toHaveTextContent(
        /¥|%/,
      );
    },
  );
  it("removes values when their validity ends even without another request", () => {
    vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout", "performance"] });
    const evidence = evidenceFixture();
    evidence.expiresAtMs = context.nowMs + 1000;
    render(<CostComparison evidence={evidence} context={context} />);
    expect(screen.getByText("¥10.53")).toBeInTheDocument();
    act(() => {
      vi.advanceTimersByTime(1000);
    });
    expect(screen.getByText(comparisonCopies.zh.expired)).toBeVisible();
    expect(screen.queryByText("¥10.53")).not.toBeInTheDocument();
  });
  it("drops old values immediately on account, period or evidence change", () => {
    const evidence = evidenceFixture();
    const { rerender } = render(
      <CostComparison evidence={evidence} context={context} />,
    );
    expect(screen.getByText("¥10.53")).toBeInTheDocument();
    rerender(
      <CostComparison
        evidence={evidence}
        context={{ ...context, accountId: "other" }}
      />,
    );
    expect(screen.queryByText("¥10.53")).not.toBeInTheDocument();
    rerender(
      <CostComparison
        evidence={evidence}
        context={{ ...context, periodEndMs: context.periodEndMs + 1 }}
      />,
    );
    expect(screen.queryByText("¥10.53")).not.toBeInTheDocument();
    rerender(<CostComparison evidence={undefined} context={context} />);
    expect(screen.getByText(comparisonCopies.zh.unavailable)).toBeVisible();
  });
});

import { useMemo, type CSSProperties } from "react";
import type { UsageRecord } from "../account/usage";
import { formatMoney } from "../account/finance";
import { useWorkbenchCopy } from "./copy";

const DAYS = 14;
const DAY_MS = 86_400_000;

/** 把 30 天明细按天汇总成最近 14 天的柱状图。只做展示，金额沿用原生侧换算好的值。 */
export function dailySpend(records: UsageRecord[], now: number) {
  const end = Math.floor(now / DAY_MS) * DAY_MS + DAY_MS;
  const days = Array.from({ length: DAYS }, (_, i) => ({
    start: end - (DAYS - i) * DAY_MS,
    amount: 0,
    tokens: 0,
  }));
  for (const record of records) {
    const index = Math.floor(
      (record.observedAtEpochMs - days[0].start) / DAY_MS,
    );
    if (index < 0 || index >= DAYS) continue;
    const amount = Number(record.amount);
    if (record.amount !== "" && Number.isFinite(amount))
      days[index].amount += amount;
    days[index].tokens += record.promptTokens + record.completionTokens;
  }
  return days;
}

export function UsageTrend({
  records,
  currency,
  locale,
}: {
  records: UsageRecord[];
  currency?: string;
  locale?: string;
}) {
  const c = useWorkbenchCopy();
  const now = useMemo(
    () =>
      records.reduce((max, r) => Math.max(max, r.observedAtEpochMs), 0) ||
      Date.now(),
    [records],
  );
  const days = useMemo(() => dailySpend(records, now), [records, now]);
  const byAmount = days.some((d) => d.amount > 0);
  const value = (d: (typeof days)[number]) => (byAmount ? d.amount : d.tokens);
  const peak = Math.max(...days.map(value), 0);
  const date = new Intl.DateTimeFormat(locale, {
    month: "numeric",
    day: "numeric",
  });
  return (
    <section className="usage-trend" aria-label={c.usageTrendTitle}>
      <header className="usage-trend-heading">
        <h2>{c.usageTrendTitle}</h2>
        <small>{c.usageTrendNote}</small>
      </header>
      {peak === 0 ? (
        <p className="usage-note">{c.usageTrendEmpty}</p>
      ) : (
        <ol className="usage-trend-bars">
          {days.map((d, index) => {
            const label = date.format(d.start);
            const shown = byAmount
              ? formatMoney(d.amount.toFixed(2), currency, locale)
              : new Intl.NumberFormat(locale, { notation: "compact" }).format(
                  d.tokens,
                );
            return (
              <li
                key={d.start}
                style={{ "--i": index } as CSSProperties}
                title={c.usageTrendDayLabel
                  .replace("{{date}}", label)
                  .replace("{{amount}}", shown)}
              >
                <span
                  className="usage-trend-bar"
                  data-empty={value(d) === 0 || undefined}
                  style={{ height: `${Math.max(4, (value(d) / peak) * 100)}%` }}
                />
                <small>{label}</small>
              </li>
            );
          })}
        </ol>
      )}
    </section>
  );
}

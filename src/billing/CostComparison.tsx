import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { ArrowDownUp, CircleHelp } from "lucide-react";
import {
  verifyComparison,
  formatCnyMicros,
  formatPercentTenths,
  type ComparisonContext,
} from "./comparison";
import { useComparisonCopy } from "./copy";
import "./comparison.css";

function exactMicros(value: string): string {
  const n = BigInt(value);
  const fraction = (n % 1_000_000n)
    .toString()
    .padStart(6, "0")
    .replace(/0+$/, "");
  return `${n / 1_000_000n}${fraction ? `.${fraction}` : ""}`;
}

export function CostComparison({
  evidence,
  context,
}: {
  evidence?: unknown;
  context?: ComparisonContext;
}) {
  const c = useComparisonCopy();
  const { i18n } = useTranslation();
  const locale = i18n.resolvedLanguage ?? "zh";
  const [clock, setClock] = useState({ context, now: context?.nowMs ?? 0 });
  const result = verifyComparison(
    evidence,
    context
      ? {
          ...context,
          nowMs: clock.context === context ? clock.now : context.nowMs,
        }
      : undefined,
  );
  useEffect(() => {
    if (!context) return;
    const initial = verifyComparison(evidence, context);
    if (!("evidence" in initial)) return;
    const started = performance.now();
    let timer: ReturnType<typeof setTimeout>;
    const tick = () => {
      const now = context.nowMs + Math.floor(performance.now() - started);
      setClock({ context, now });
      if (now < initial.evidence.expiresAtMs)
        timer = setTimeout(
          tick,
          Math.min(initial.evidence.expiresAtMs - now, 2_147_483_647),
        );
    };
    tick();
    return () => clearTimeout(timer);
  }, [evidence, context]);
  const verified = "evidence" in result ? result : null;
  const comparable =
    verified && verified.status !== "not_comparable" ? verified : null;
  const direction = comparable
    ? comparable.differenceCnyMicros > 0n
      ? "saved"
      : comparable.differenceCnyMicros < 0n
        ? "more"
        : "same"
    : "saved";
  const difference = comparable
    ? comparable.differenceCnyMicros < 0n
      ? -comparable.differenceCnyMicros
      : comparable.differenceCnyMicros
    : null;
  const percentage = comparable
    ? formatPercentTenths(comparable.percentTenths)
    : null;
  const date = (ms: number) =>
    new Intl.DateTimeFormat(locale, {
      dateStyle: "short",
      timeStyle: "short",
    }).format(ms);
  const message =
    result.status === "not_comparable"
      ? "none"
      : result.status === "invalid" || result.status === "expired"
        ? result.status
        : "unavailable";
  return (
    <section
      className="cost-comparison"
      aria-labelledby="cost-comparison-title"
      data-status={result.status}
    >
      <div className="workbench-section-heading">
        <h2 id="cost-comparison-title">
          <ArrowDownUp aria-hidden="true" />
          {c.title}
        </h2>
        {verified ? (
          <span className="cost-coverage">
            {c.coverage} {verified.evidence.rows.length}/
            {verified.evidence.rows.length + verified.evidence.excluded.length}{" "}
            {c.requests}
          </span>
        ) : (
          <span className="status-badge">{c[message]}</span>
        )}
      </div>
      {verified && (
        <p className="cost-period">
          {c.period}：{date(verified.evidence.periodStartMs)} {c.to}{" "}
          {date(verified.evidence.periodEndMs)}
        </p>
      )}
      <dl className="cost-totals">
        <div>
          <dt>{c.official}</dt>
          <dd>
            {comparable
              ? formatCnyMicros(comparable.officialCnyMicros, locale)
              : "—"}
          </dd>
        </div>
        <div>
          <dt>{c.actual}</dt>
          <dd>
            {comparable
              ? formatCnyMicros(comparable.actualCnyMicros, locale)
              : "—"}
          </dd>
        </div>
        <div data-direction={comparable ? direction : "unknown"}>
          <dt>{c[direction]}</dt>
          <dd>
            {difference === null ? "—" : formatCnyMicros(difference, locale)}
          </dd>
          {percentage && <small>{percentage}</small>}
        </div>
      </dl>
      {!comparable && (
        <p className="cost-notice" role="status">
          {c[`${message}Body`]}
        </p>
      )}
      {comparable && <p className="cost-assumption">{c.estimate}</p>}
      {verified && verified.evidence.excluded.length > 0 && (
        <div className="cost-exclusions">
          <p>
            {c.excluded}：
            <strong>
              {formatCnyMicros(verified.excludedKnownCnyMicros, locale)}
            </strong>
            {verified.excludedUnknownCharges > 0 && (
              <span>
                {" "}
                · {c.pending} ({verified.excludedUnknownCharges})
              </span>
            )}
          </p>
          <ul>
            {verified.evidence.excluded.map((row) => (
              <li key={row.id}>
                <code>{row.modelId}</code>
                <span>{c[row.reason]}</span>
              </li>
            ))}
          </ul>
        </div>
      )}
      <details className="cost-method">
        <summary>
          <CircleHelp aria-hidden="true" />
          {c.method}
        </summary>
        <p>{c.methodBody}</p>
        <p>{c.rounding}</p>
        <p>{c.estimate}</p>
        <p>{c.missing}</p>
      </details>
      {comparable && (
        <div className="cost-receipts">
          <h3>{c.details}</h3>
          {comparable.evidence.rows.map((row) => (
            <details className="cost-receipt" key={row.id}>
              <summary>
                <code>{row.modelId}</code>
                <span>
                  {formatCnyMicros(BigInt(row.settledNetCnyMicros), locale)}
                </span>
              </summary>
              <dl className="cost-metadata">
                <div>
                  <dt>{c.request}</dt>
                  <dd>
                    <code>{row.id}</code>
                  </dd>
                </div>
                <div>
                  <dt>{c.requestedAt}</dt>
                  <dd>{date(row.occurredAtMs)}</dd>
                </div>
                <div>
                  <dt>{c.actual}</dt>
                  <dd>{exactMicros(row.settledNetCnyMicros)} CNY</dd>
                </div>
                <div>
                  <dt>{c.officialModel}</dt>
                  <dd>
                    <code>{row.officialModelId}</code>
                  </dd>
                </div>
                <div>
                  <dt>{c.conditions}</dt>
                  <dd>{row.tierLabel}</dd>
                </div>
                <div>
                  <dt>{c.version}</dt>
                  <dd>{row.priceVersion}</dd>
                </div>
                <div>
                  <dt>{c.effective}</dt>
                  <dd>{date(row.priceEffectiveAtMs)}</dd>
                </div>
                <div>
                  <dt>{c.checked}</dt>
                  <dd>{date(row.sourceCheckedAtMs)}</dd>
                </div>
                <div>
                  <dt>{c.source}</dt>
                  <dd>{row.officialSourceUrl}</dd>
                </div>
                <div>
                  <dt>{c.fxVersion}</dt>
                  <dd>{row.fxVersion} · 1 USD = 6.75 CNY</dd>
                </div>
              </dl>
              <div className="cost-table-scroll">
                <table>
                  <caption>
                    {c.currency}：{row.officialCurrency}
                  </caption>
                  <thead>
                    <tr>
                      <th>{c.qty}</th>
                      <th>token</th>
                      <th>{c.rate}</th>
                      <th>{c.amount}</th>
                    </tr>
                  </thead>
                  <tbody>
                    {row.lines.map((line) => (
                      <tr key={line.kind}>
                        <th scope="row">{c[line.kind]}</th>
                        <td>
                          {new Intl.NumberFormat(locale).format(
                            BigInt(line.quantity),
                          )}
                        </td>
                        <td>{exactMicros(line.pricePerMillionMicros)}</td>
                        <td>{exactMicros(line.amountMicros)}</td>
                      </tr>
                    ))}
                  </tbody>
                  <tfoot>
                    <tr>
                      <th scope="row" colSpan={3}>
                        {c.total}
                      </th>
                      <td>
                        {exactMicros(row.officialTotalMicros)}{" "}
                        {row.officialCurrency}
                      </td>
                    </tr>
                  </tfoot>
                </table>
              </div>
            </details>
          ))}
        </div>
      )}
    </section>
  );
}

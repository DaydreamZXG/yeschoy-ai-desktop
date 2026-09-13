import { useId } from "react";
import type { AccountModel } from "../account/session";
import { CheckCircle2, Landmark, Sprout } from "lucide-react";
import { useConfigurationCopy } from "./copy";
import { hundredMillionTokenEstimate } from "./billing";

export const groupLabel = (id: string, defaultLabel = "标准分组") =>
  id === "default" ? defaultLabel : id;

// #19 分组显示名走目录映射：服务端 usable_group 已把显示名存进 description，
// 展示优先用目录显示名；default 分组仍用本地化的「标准分组」，查不到时回退 id。
export function groupDisplayName(
  id: string,
  groups: ReadonlyArray<{ id: string; description: string }> | undefined,
  defaultLabel: string,
): string {
  if (id === "default") return defaultLabel;
  const description = groups?.find((g) => g.id === id)?.description.trim();
  return description || id;
}

export function BillingGroupPicker({
  model,
  selected,
  onChange,
  disabled = false,
}: {
  model?: AccountModel;
  selected: string;
  onChange: (id: string) => void;
  disabled?: boolean;
}) {
  const c = useConfigurationCopy();
  const groups = model?.billing?.groups ?? [];
  const radioName = useId();
  return (
    <fieldset className="billing-group-picker" disabled={disabled}>
      <legend>{c.chooseGroupLegend}</legend>
      <p>{c.groupIntro}</p>
      {groups.length ? (
        <div className="billing-group-grid">
          {groups.map((group) => (
            <label
              key={group.id}
              className={
                selected === group.id
                  ? "billing-group-option is-selected"
                  : "billing-group-option"
              }
            >
              <input
                type="radio"
                name={radioName}
                value={group.id}
                checked={selected === group.id}
                onChange={() => onChange(group.id)}
              />
              <span className="billing-group-info">
                <strong>
                  {groupDisplayName(group.id, groups, c.defaultGroup)}
                </strong>
                {group.id === "default" ? (
                  group.description ? (
                    <small>{group.description}</small>
                  ) : null
                ) : group.description ? (
                  <small>
                    <code>{group.id}</code>
                  </small>
                ) : null}
              </span>
              <span className="billing-group-ratio">
                {group.ratio === null ? c.ratioPending : `${group.ratio}×`}
              </span>
              {selected === group.id && <CheckCircle2 aria-hidden="true" />}
            </label>
          ))}
        </div>
      ) : (
        <p className="account-inline-warning">
          {model ? c.groupsMissing : c.groupsEmptyHint}
        </p>
      )}
    </fieldset>
  );
}

function price(amount: number) {
  if (!Number.isFinite(amount)) return "—";
  if (amount > 0 && amount < 0.01) return "< ¥0.01";
  return `¥${new Intl.NumberFormat("zh-CN", {
    minimumFractionDigits: 0,
    maximumFractionDigits: Math.abs(amount) < 10 ? 2 : 0,
  }).format(amount)}`;
}

function priceRange(range: { minimum: number; maximum: number }) {
  if (Math.abs(range.maximum - range.minimum) < 0.005)
    return price(range.minimum);
  return `${price(range.minimum)} – ${price(range.maximum)}`;
}

function perMillionPrice(text: string) {
  if (text === "") return "—";
  const value = Number(text);
  return Number.isFinite(value) ? price(value) : "—";
}

export function BillingPrices({
  model,
  selected,
  fx,
}: {
  model?: AccountModel;
  selected: string;
  fx: string;
}) {
  const c = useConfigurationCopy();
  const group = model?.billing?.groups.find((g) => g.id === selected);
  if (!model || !group) return null;
  const estimate = hundredMillionTokenEstimate(model, group, fx);
  // #12 价格口径：perMillion 字段（元/百万 tokens）直显为第一层级，
  // 「1 亿 Token 费用参考」降级为折叠示例。
  const hasPerMillion = model.pricingAvailable;
  return (
    <section
      className="billing-prices"
      aria-label={c.pricesLabel}
      aria-live="polite"
    >
      {hasPerMillion ? (
        <table className="per-million-prices">
          <caption>{c.perMillionTokens}</caption>
          <thead>
            <tr>
              <th scope="col" />
              <th scope="col">{c.officialPrice}</th>
              <th scope="col">{c.actualPrice}</th>
            </tr>
          </thead>
          <tbody>
            <tr>
              <th scope="row">{c.inputPrice}</th>
              <td>{perMillionPrice(model.officialInputCnyPerMillion)}</td>
              <td>{perMillionPrice(model.actualInputCnyPerMillion)}</td>
            </tr>
            <tr>
              <th scope="row">{c.outputPrice}</th>
              <td>{perMillionPrice(model.officialOutputCnyPerMillion)}</td>
              <td>{perMillionPrice(model.actualOutputCnyPerMillion)}</td>
            </tr>
          </tbody>
        </table>
      ) : (
        <p className="billing-price-note">{c.priceUnavailable}</p>
      )}
      <details className="billing-estimate">
        <summary>{c.estimateSummary}</summary>
        <p className="billing-example-note">{c.estimateExampleNote}</p>
        {estimate?.savingPercent !== null &&
          estimate?.savingPercent !== undefined && (
            <span className="billing-saving-badge">
              {c.estimateSaving.replace(
                "{{percent}}",
                String(
                  estimate.savingPercent >= 99 && estimate.savingPercent < 100
                    ? Math.floor(estimate.savingPercent * 10) / 10
                    : Math.round(estimate.savingPercent),
                ),
              )}
            </span>
          )}
        {estimate ? (
          <>
            <p className="billing-example-formula">{c.estimateFormula}</p>
            <div className="billing-comparison-grid">
              <div className="billing-comparison-card is-official">
                <Landmark aria-hidden="true" />
                <span>{c.estimateOfficialLabel}</span>
                <strong>{priceRange(estimate.official)}</strong>
              </div>
              <div className="billing-comparison-card is-yeschoy">
                <Sprout aria-hidden="true" />
                <span>{c.estimateYeschoyLabel}</span>
                <strong>{priceRange(estimate.yeschoy)}</strong>
              </div>
            </div>
            <div className="billing-price-details">
              <p className="billing-price-note">
                {c.estimateNote
                  .replace(
                    "{{group}}",
                    groupDisplayName(
                      group.id,
                      model.billing?.groups,
                      c.defaultGroup,
                    ),
                  )
                  .replace("{{fx}}", fx)
                  .replace(
                    "{{tiered}}",
                    estimate.tiered ? c.estimateTieredNote : "",
                  )
                  .replace(
                    "{{cacheNote}}",
                    estimate.cacheFallback
                      ? c.estimateCacheFallbackNote
                      : c.estimateCacheExcludedNote,
                  )}
              </p>
            </div>
          </>
        ) : (
          <p className="billing-price-note">{c.estimateUnavailable}</p>
        )}
      </details>
    </section>
  );
}

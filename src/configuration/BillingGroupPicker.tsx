import { useId } from "react";
import type { AccountModel } from "../account/session";
import type { ActivationToolId } from "./activation";
import { Landmark, Sprout } from "lucide-react";
import { useConfigurationCopy } from "./copy";
import {
  billingConversion,
  groupPrice,
  groupSupportsTool,
  hundredMillionTokenEstimate,
} from "./billing";

export const groupLabel = (id: string, defaultLabel = "标准分组") =>
  id === "default" ? defaultLabel : id;

// 线上 usable_group 的形状是「键 → 一句话」，不是「id → 显示名」：
//   "Qwen / GLM"        → "模型：qwen3.8-max、glm-5.3； 6.5折"
//   "限时国模特价渠道"   → "限时特价"
// 所以键本身就是给人看的名字，description 是补充说明。价格方案卡按这个
// 事实显示键（见 BillingGroupPicker），description 留在「计费详情」里。
//
// 这个函数只用在句子里（例如计费说明），那里一句话比一个名字更合适。
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
  fx = "",
  toolId,
  toolName,
}: {
  model?: AccountModel;
  selected: string;
  onChange: (id: string) => void;
  disabled?: boolean;
  fx?: string;
  toolId?: ActivationToolId;
  toolName?: string;
}) {
  const c = useConfigurationCopy();
  const groups = model?.billing?.groups ?? [];
  const radioName = useId();
  const plans = groups.map((group) => ({
    group,
    estimate: model ? hundredMillionTokenEstimate(model, group, fx, "reference") : null,
  }));
  const allPriced =
    plans.length > 1 &&
    plans.every(
      ({ estimate }) =>
        estimate &&
        Number.isFinite(estimate.yeschoy.minimum) &&
        Number.isFinite(estimate.yeschoy.maximum),
    );
  return (
    <fieldset className="billing-group-picker" disabled={disabled}>
      <legend>{c.chooseGroupLegend}</legend>
      <p>{c.groupIntro}</p>
      {groups.length ? (
        <div className="billing-group-grid">
          {plans.map(({ group, estimate }) => {
            const unusable =
              !!toolId &&
              !!group.supportedEndpointTypes?.length &&
              !groupSupportsTool(model, group, toolId);
            return (
              <div
                key={group.id}
                className={
                  unusable
                    ? "billing-plan-card is-incompatible"
                    : "billing-plan-card"
                }
              >
                <label
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
                    disabled={unusable}
                    onChange={() => !unusable && onChange(group.id)}
                  />
                  <span className="billing-group-info">
                  {/* 显示分组的 **键**，不是它的 description。
                      线上数据里 usable_group 是「键 → 一句话」：键是
                      `Qwen / GLM`、`限时国模特价渠道` 这种可读名字，值是
                      「模型：qwen3.8-max、glm-5.3； 6.5折」这种句子。
                      之前按代码注释以为 description 存的是显示名，结果卡片
                      标题变成截断的「模型：qwen3.8-m...」，在配 glm-5.3 的
                      页面上看起来像是别的模型的方案。句子留在「计费详情」里。

                      名字仍然最弱、单行截断：它是后台自由文本，长度不受控，
                      而价格是算出来的、可信，价格才是主角。 */}
                  <strong title={groupLabel(group.id, c.defaultGroup)}>
                    {groupLabel(group.id, c.defaultGroup)}
                  </strong>
                  <span className="billing-plan-price">
                    {estimate
                      ? c.estimateAmount.replace("{{amount}}", price(estimate.yeschoy.minimum))
                      : c.planPriceUnavailable}
                  </span>
                  <small>{c.planPriceUnit}</small>
                  {unusable && toolName ? (
                    <small className="billing-plan-unusable">
                      {c.groupNotForThisApp.replace("{{app}}", toolName)}
                    </small>
                  ) : null}
                  <span className="billing-plan-badges">
                    {selected === group.id && <small>{c.planSelected}</small>}
                    {allPriced &&
                      estimate &&
                      plans.every(
                        (plan) =>
                          estimate.yeschoy.minimum <=
                            plan.estimate!.yeschoy.minimum &&
                          estimate.yeschoy.maximum <=
                            plan.estimate!.yeschoy.maximum,
                      ) && <small>{c.planLowest}</small>}
                  </span>
                  </span>
                </label>
                <details className="billing-plan-details">
                  <summary>{c.planDetails}</summary>
                  <p>
                    {c.planRatio}:{" "}
                    {group.ratio === null ? c.ratioPending : `${group.ratio}×`}
                  </p>
                  {group.description && (
                    <p>
                      {c.planDescription}: {group.description}
                    </p>
                  )}
                </details>
              </div>
            );
          })}
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
    minimumFractionDigits: 2,
    maximumFractionDigits: 2,
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
  const estimate = hundredMillionTokenEstimate(model, group, fx, "reference");
  const range = hundredMillionTokenEstimate(model, group, fx);
  const conversion = billingConversion(model, fx);
  const quoted = groupPrice(model, group, fx);
  const useRates =
    !model.pricingAvailable || conversion.reference !== conversion.site;
  const canQuote =
    useRates &&
    quoted.unit === "tokens" &&
    quoted.rows.length > 0 &&
    group.ratio !== null &&
    Number.isFinite(group.ratio) &&
    group.ratio >= 0 &&
    [conversion.reference, conversion.site].every(
      (n) => Number.isFinite(n) && n > 0,
    ) &&
    quoted.rows.every(({ rates }) =>
      [rates.p, rates.c].every((n) => Number.isFinite(n) && n >= 0),
    );
  const quotedPrice = (field: "p" | "c" | "cr" | "cc", actual: boolean) => {
    const amounts = quoted.rows.map(
      ({ rates }) =>
        rates[field] *
        (actual ? conversion.site * group.ratio! : conversion.reference),
    );
    return priceRange({
      minimum: Math.min(...amounts),
      maximum: Math.max(...amounts),
    });
  };
  const cachePrice = (field: "cr" | "cc", actual: boolean) => {
    // Cache quotes come from the billing rule, even when input/output prices
    // are supplied directly in CNY. Missing tiers must not become zero.
    const rate = actual
      ? conversion.site * (group.ratio ?? NaN)
      : conversion.reference;
    if (
      quoted.unit !== "tokens" ||
      !quoted.rows.length ||
      !Number.isFinite(rate) ||
      rate < 0 ||
      ![conversion.reference, conversion.site].every(n => Number.isFinite(n) && n > 0) ||
      !quoted.rows.every(({ rates }) => Number.isFinite(rates[field]) && rates[field] >= 0)
    ) return c.cachePriceUnavailable;
    return quotedPrice(field, actual);
  };
  const hasCacheWrite = quoted.unit === "tokens" &&
    quoted.rows.some(({ rates }) => rates.cc !== undefined);
  // #12 价格口径：perMillion 字段（元/百万 tokens）直显为第一层级，
  // 「1 亿 Token 费用参考」降级为折叠示例。
  const hasPerMillion = model.pricingAvailable || canQuote;
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
              <td>
                {canQuote
                  ? quotedPrice("p", false)
                  : perMillionPrice(model.officialInputCnyPerMillion)}
              </td>
              <td>
                {canQuote
                  ? quotedPrice("p", true)
                  : perMillionPrice(model.actualInputCnyPerMillion)}
              </td>
            </tr>
            <tr>
              <th scope="row">{c.outputPrice}</th>
              <td>
                {canQuote
                  ? quotedPrice("c", false)
                  : perMillionPrice(model.officialOutputCnyPerMillion)}
              </td>
              <td>
                {canQuote
                  ? quotedPrice("c", true)
                  : perMillionPrice(model.actualOutputCnyPerMillion)}
              </td>
            </tr>
            <tr>
              <th scope="row">{c.cacheReadPrice}</th>
              <td>{cachePrice("cr", false)}</td>
              <td>{cachePrice("cr", true)}</td>
            </tr>
            {hasCacheWrite && (
              <tr>
                <th scope="row">{c.cacheWritePrice}</th>
                <td>{cachePrice("cc", false)}</td>
                <td>{cachePrice("cc", true)}</td>
              </tr>
            )}
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
                    : Math.round(estimate.savingPercent * 10) / 10,
                ),
              )}
            </span>
          )}
        {estimate ? (
          <>
            <div className="billing-comparison-grid">
              <div className="billing-comparison-card is-official">
                <Landmark aria-hidden="true" />
                <span>{c.estimateOfficialLabel}</span>
                <strong>{c.estimateAmount.replace("{{amount}}", price(estimate.official.minimum))}</strong>
              </div>
              <div className="billing-comparison-card is-yeschoy">
                <Sprout aria-hidden="true" />
                <span>{c.estimateYeschoyLabel}</span>
                <strong>{c.estimateAmount.replace("{{amount}}", price(estimate.yeschoy.minimum))}</strong>
              </div>
            </div>
            <details className="billing-price-details">
              <summary>{c.planDetails}</summary>
              <p className="billing-example-formula">{c.estimateFormula}</p>
              {range?.tiered && <p className="billing-price-note">
                {c.estimateOfficialLabel}: {priceRange(range.official)}；
                {c.estimateYeschoyLabel}: {priceRange(range.yeschoy)}
              </p>}
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
                  .replace("{{referenceFx}}", String(conversion.reference))
                  .replace("{{siteFx}}", String(conversion.site))
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
            </details>
          </>
        ) : (
          <p className="billing-price-note">{c.estimateUnavailable}</p>
        )}
      </details>
    </section>
  );
}

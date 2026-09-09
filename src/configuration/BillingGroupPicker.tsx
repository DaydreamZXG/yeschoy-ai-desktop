import { useId } from "react";
import type { AccountModel } from "../account/session";
import { CheckCircle2, Landmark, Sprout } from "lucide-react";
import { hundredMillionTokenEstimate } from "./billing";

export const groupLabel = (id: string) => (id === "default" ? "标准分组" : id);

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
  const groups = model?.billing?.groups ?? [];
  const radioName = useId();
  return (
    <fieldset className="billing-group-picker" disabled={disabled}>
      <legend>选择计费分组</legend>
      <p>
        同一个模型，不同分组有不同价格。下面只列出这个模型已开放的分组；分组下方的说明是分组自身的介绍，不一定列全。
      </p>
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
                <strong>{groupLabel(group.id)}</strong>
                {group.description && <small>{group.description}</small>}
              </span>
              <span className="billing-group-ratio">
                {group.ratio === null ? "倍率待查询" : `${group.ratio}×`}
              </span>
              {selected === group.id && <CheckCircle2 aria-hidden="true" />}
            </label>
          ))}
        </div>
      ) : (
        <p className="account-inline-warning">
          {model
            ? "暂未读到这个模型的可用分组，请刷新账户数据。"
            : "选择模型后，可查看对应计费分组。"}
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

export function BillingPrices({
  model,
  selected,
  fx,
}: {
  model?: AccountModel;
  selected: string;
  fx: string;
}) {
  const group = model?.billing?.groups.find((g) => g.id === selected);
  if (!model || !group) return null;
  const estimate = hundredMillionTokenEstimate(model, group, fx);
  return (
    <section
      className="billing-prices"
      aria-label="所选分组价格"
      aria-live="polite"
    >
      <header>
        <div>
          <strong>1 亿 Token 费用参考</strong>
          <p>缓存型示例，不是账单预测</p>
        </div>
        {estimate?.savingPercent !== null &&
          estimate?.savingPercent !== undefined && (
            <span className="billing-saving-badge">
              约省{" "}
              {estimate.savingPercent >= 99 && estimate.savingPercent < 100
                ? Math.floor(estimate.savingPercent * 10) / 10
                : Math.round(estimate.savingPercent)}
              %
            </span>
          )}
      </header>
      {estimate ? (
        <>
          <p className="billing-example-formula">
            1,000 万新输入 + 8,000 万缓存读取 + 1,000 万输出
          </p>
          <div className="billing-comparison-grid">
            <div className="billing-comparison-card is-official">
              <Landmark aria-hidden="true" />
              <span>使用官网预计</span>
              <strong>{priceRange(estimate.official)}</strong>
            </div>
            <div className="billing-comparison-card is-yeschoy">
              <Sprout aria-hidden="true" />
              <span>使用野菜预计</span>
              <strong>{priceRange(estimate.yeschoy)}</strong>
            </div>
          </div>
          <details className="billing-price-details">
            <summary>这个价格怎么算？</summary>
            <p className="billing-price-note">
              按 {groupLabel(group.id)} 当前倍率和网站参考换算值 {fx} 估算
              {estimate.tiered ? "；不同请求档位会形成以上区间" : ""}。
              该换算值不是市场汇率。
              {estimate.cacheFallback
                ? "该模型没有单独的缓存读取价，缓存部分按输入价保守估算。"
                : "不含另行发生的缓存写入，实际费用以请求命中的计费档位为准。"}
            </p>
          </details>
        </>
      ) : (
        <p className="billing-price-note">
          当前规则无法可靠换算为 Token
          费用，暂不展示估算；实际费用以网站账单为准。
        </p>
      )}
    </section>
  );
}

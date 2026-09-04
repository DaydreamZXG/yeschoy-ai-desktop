import type { AccountModel } from "../account/session";
import { CheckCircle2 } from "lucide-react";
import { groupPrice } from "./billing";

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
  return (
    <fieldset className="billing-group-picker" disabled={disabled}>
      <legend>选择计费分组</legend>
      <p>同一个模型，不同分组有不同价格。倍率来自你的账户。</p>
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
                name="billing-group"
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

const rateLabels: Record<string, string> = {
  p: "输入",
  c: "输出",
  cr: "缓存读取",
  cc: "缓存写入",
  cc1h: "缓存写入（1 小时）",
  img: "图片输入",
  img_o: "图片输出",
  ai: "音频输入",
  ao: "音频输出",
  request: "每次请求",
};
function price(amount: number, currency: string) {
  if (!Number.isFinite(amount)) return "—";
  return `${currency === "CNY" ? "¥" : "US$"}${new Intl.NumberFormat("zh-CN", { minimumFractionDigits: 2, maximumFractionDigits: 8 }).format(amount)}`;
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
  const { rows, currency, multiplier, unit } = groupPrice(model, group, fx);
  const dynamic = model.billingMode === "tiered_expr";
  const exchange = currency === "CNY" ? Number(fx) : 1;
  return (
    <section
      className="billing-prices"
      aria-label="所选分组价格"
      aria-live="polite"
    >
      <header>
        <strong>{groupLabel(group.id)} · 价格明细</strong>
        <span>{unit === "request" ? "每次请求" : "每百万 tokens"}</span>
      </header>
      {dynamic && (
        <p className="billing-price-note">
          按分时或阶梯规则计费，下表列出各档单价；实际采用请求命中的档位。
        </p>
      )}
      {rows.length ? (
        rows.map((row, index) => (
          <div className="billing-tier" key={`${row.name}-${index}`}>
            {dynamic && <h4>{row.name}</h4>}
            <table>
              <thead>
                <tr>
                  <th>用量类型</th>
                  <th>
                    {dynamic || unit === "request"
                      ? "基础价（倍率前）"
                      : "官网参考价"}
                  </th>
                  <th>本分组价</th>
                </tr>
              </thead>
              <tbody>
                {Object.entries(row.rates)
                  .filter(([key]) => key !== "constant")
                  .map(([key, value]) => (
                    <tr key={key}>
                      <th scope="row">{rateLabels[key] ?? key}</th>
                      <td>{price(value * exchange, currency)}</td>
                      <td>{price(value * multiplier, currency)}</td>
                    </tr>
                  ))}
              </tbody>
            </table>
          </div>
        ))
      ) : (
        <p className="billing-price-note">
          当前规则暂不能换算为固定单价，可以继续接入；费用按网站规则结算。
        </p>
      )}
      {group.ratio === null && (
        <p className="billing-price-note">
          暂未读到当前账户的倍率，不显示推测价格。
        </p>
      )}
      {model.billingMode === "ratio" &&
        group.ratio !== null &&
        group.ratio < 1 &&
        rows.length > 0 && (
          <p className="billing-saving">
            同等基础输入／输出用量，比参考价节省{" "}
            {new Intl.NumberFormat("zh-CN", {
              maximumFractionDigits: 2,
            }).format((1 - group.ratio) * 100)}
            %
          </p>
        )}
      <p className="billing-price-note">
        {currency === "CNY"
          ? `换算汇率来自 NewAPI：1 USD = ${fx} CNY。`
          : "暂未读到换算汇率，先显示美元价格。"}
        倍率作用于基础计费，不代表所有请求都享有同一个官网折扣。
      </p>
      {dynamic && model.billing?.expression && (
        <details className="billing-rule">
          <summary>查看完整计费规则</summary>
          <code>{model.billing.expression}</code>
        </details>
      )}
    </section>
  );
}

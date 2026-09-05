import { ArrowDownRight, ArrowUpRight, ReceiptText } from "lucide-react";
import {
  formatMoney,
  savingsPresentation,
  type RecentSavings,
} from "../account/finance";

export function SavingsCard({
  savings,
  onDetails,
  loading = false,
  detailsLabel = "怎么算的",
}: {
  savings?: RecentSavings;
  onDetails?: () => void;
  loading?: boolean;
  detailsLabel?: string;
}) {
  const { value, note, negative } = savingsPresentation(savings);
  const available = savings?.status === "available";
  return (
    <article
      className={`summary-card savings-card${negative ? " savings-negative" : ""}`}
    >
      <span className="summary-label">
        <ReceiptText aria-hidden="true" />
        近期费用对比
      </span>
      <dl className="savings-comparison" aria-label="同一批记录的费用对比">
        <div>
          <dt>
            官网应收 <small>估算</small>
          </dt>
          <dd>
            {formatMoney(available ? savings.officialAmount : undefined, "CNY")}
          </dd>
        </div>
        <div>
          <dt>野菜收取</dt>
          <dd>
            {formatMoney(available ? savings.siteAmount : undefined, "CNY")}
          </dd>
        </div>
        <div className="savings-total">
          <dt>{negative ? "高于参考估算" : "预计省下"}</dt>
          <dd>{value}</dd>
        </div>
      </dl>
      <div className="summary-bottom">
        <span>{loading && !savings ? "正在读取账单…" : note}</span>
        {onDetails && (
          <button type="button" className="text-button" onClick={onDetails}>
            {detailsLabel}
            <ArrowUpRight aria-hidden="true" />
          </button>
        )}
      </div>
    </article>
  );
}

export function SavingsDetails({ savings }: { savings?: RecentSavings }) {
  return (
    <details className="savings-receipt" id="savings-basis">
      <summary>
        <span>
          <ReceiptText aria-hidden="true" />
          节省金额怎么算的？
        </span>
        <span className="savings-scope">最近最多 100 笔</span>
      </summary>
      <div className="savings-receipt-body">
        {savings?.status === "available" ? (
          <>
            <div className="savings-equation">
              <div>
                <span>官网参考额</span>
                <strong>{formatMoney(savings.officialAmount, "CNY")}</strong>
              </div>
              <span className="savings-minus" aria-hidden="true">
                −
              </span>
              <div>
                <span>野菜计费额</span>
                <strong>{formatMoney(savings.siteAmount, "CNY")}</strong>
              </div>
              <ArrowDownRight aria-hidden="true" />
              <div>
                <span>
                  {Number(savings.savedAmount) < 0
                    ? "差额（高于参考）"
                    : "预计节省"}
                </span>
                <strong>{formatMoney(savings.savedAmount, "CNY")}</strong>
              </div>
            </div>
            <p>
              最近 {savings.scannedCount} 笔消费中，{savings.includedCount}{" "}
              笔参与比较，{savings.excludedCount} 笔未计入。不是累计或本月节省。
            </p>
            {!!savings.oldestAtEpochMs && (
              <p>
                可比较记录：
                {new Date(savings.oldestAtEpochMs).toLocaleDateString(
                  "zh-CN",
                )}{" "}
                —{" "}
                {new Date(savings.newestAtEpochMs).toLocaleDateString("zh-CN")}
                。
              </p>
            )}
            <dl className="savings-formula">
              <div>
                <dt>官网参考额</dt>
                <dd>
                  逐笔计费额度 ÷ 该笔记录倍率 × 当前参考换算值{" "}
                  {savings.referenceRate}，再求和
                </dd>
              </div>
              <div>
                <dt>野菜计费额</dt>
                <dd>
                  同一批计费额度 × 当前网站价格系数 {savings.priceRate}，再求和
                </dd>
              </div>
            </dl>
          </>
        ) : (
          <p>
            {savings?.status === "empty"
              ? "还没有消费记录，使用模型后就能在这里查看。"
              : savings?.status === "no_comparable_records"
                ? `最近 ${savings.scannedCount} 笔记录暂时没有完整的可比较计费依据。`
                : "暂时未取得完整的账单或价格设置，刷新账户后可重试。"}
            不会将缺失数据当作省了 0 元。
          </p>
        )}
        <p>
          计费额度由实际扣费数除以网站额度单位得到；优先采用该笔专属倍率，否则使用分组倍率。缓存已包含在实际扣费中，不假设固定缓存比例。
        </p>
        <p>
          缺少依据、套餐消费及特殊附加费不参与比较。按网站维护的官网参考价和当前价格设置估算，不是官网实际账单，也不等于充值现金优惠。换算值不是市场汇率。
        </p>
      </div>
    </details>
  );
}

import { ArrowDownRight, ArrowUpRight, ReceiptText } from "lucide-react";
import { useTranslation } from "react-i18next";
import {
  formatMoney,
  savingsPresentation,
  type RecentSavings,
} from "../account/finance";

export function SavingsCard({
  savings,
  onDetails,
  loading = false,
  detailsLabel,
}: {
  savings?: RecentSavings;
  onDetails?: () => void;
  loading?: boolean;
  detailsLabel?: string;
}) {
  const { t } = useTranslation();
  const { value, note, negative } = savingsPresentation(savings);
  const available = savings?.status === "available";
  const skeleton = loading && !savings;
  return (
    <article
      className={`summary-card savings-card${negative ? " savings-negative" : ""}`}
    >
      <span className="summary-label">
        <ReceiptText aria-hidden="true" />
        {t("savings.cardTitle")}
      </span>
      <dl
        className="savings-comparison"
        aria-label={t("savings.comparisonLabel")}
      >
        <div>
          <dt>
            {t("savings.officialDue")} <small>{t("savings.estimateTag")}</small>
          </dt>
          <dd className={skeleton ? "skeleton skeleton-value" : undefined}>
            {formatMoney(available ? savings.officialAmount : undefined, "CNY")}
          </dd>
        </div>
        <div>
          <dt>{t("savings.siteCharged")}</dt>
          <dd className={skeleton ? "skeleton skeleton-value" : undefined}>
            {formatMoney(available ? savings.siteAmount : undefined, "CNY")}
          </dd>
        </div>
        <div className="savings-total">
          <dt>
            {t(negative ? "savings.aboveReference" : "savings.expectedSaving")}
          </dt>
          <dd className={skeleton ? "skeleton skeleton-value" : undefined}>
            {value}
          </dd>
        </div>
      </dl>
      <div className="summary-bottom">
        <span>{loading && !savings ? t("savings.loadingBill") : note}</span>
        {onDetails && (
          <button type="button" className="text-button" onClick={onDetails}>
            {detailsLabel ?? t("savings.detailsLabel")}
            <ArrowUpRight aria-hidden="true" />
          </button>
        )}
      </div>
    </article>
  );
}

export function SavingsDetails({ savings }: { savings?: RecentSavings }) {
  const { t, i18n } = useTranslation();
  // 日期跟着界面语言走。以前写死 `zh-CN`，英文界面上会冒出一个中文日期。
  const day = (epochMs: number) =>
    new Date(epochMs).toLocaleDateString(i18n.language);
  return (
    <details className="savings-receipt" id="savings-basis">
      <summary>
        <span>
          <ReceiptText aria-hidden="true" />
          {t("savings.receiptSummary")}
        </span>
        <span className="savings-scope">{t("savings.scope")}</span>
      </summary>
      <div className="savings-receipt-body">
        {savings?.status === "available" ? (
          <>
            <div className="savings-equation">
              <div>
                <span>{t("savings.officialReference")}</span>
                <strong>{formatMoney(savings.officialAmount, "CNY")}</strong>
              </div>
              <span className="savings-minus" aria-hidden="true">
                −
              </span>
              <div>
                <span>{t("savings.siteBilled")}</span>
                <strong>{formatMoney(savings.siteAmount, "CNY")}</strong>
              </div>
              <ArrowDownRight aria-hidden="true" />
              <div>
                <span>
                  {t(
                    Number(savings.savedAmount) < 0
                      ? "savings.differenceAbove"
                      : "savings.savingEstimate",
                  )}
                </span>
                <strong>{formatMoney(savings.savedAmount, "CNY")}</strong>
              </div>
            </div>
            <p>
              {t("savings.counts", {
                scanned: savings.scannedCount,
                included: savings.includedCount,
                excluded: savings.excludedCount,
              })}
            </p>
            {!!savings.oldestAtEpochMs && (
              <p>
                {t("savings.range", {
                  from: day(savings.oldestAtEpochMs),
                  to: day(savings.newestAtEpochMs),
                })}
              </p>
            )}
            <dl className="savings-formula">
              <div>
                <dt>{t("savings.officialReference")}</dt>
                <dd>
                  {t("savings.formulaOfficial", {
                    rate: savings.referenceRate,
                  })}
                </dd>
              </div>
              <div>
                <dt>{t("savings.siteBilled")}</dt>
                <dd>{t("savings.formulaSite", { rate: savings.priceRate })}</dd>
              </div>
            </dl>
          </>
        ) : (
          <p>
            {savings?.status === "empty"
              ? t("savings.emptyRecords")
              : savings?.status === "no_comparable_records"
                ? t("savings.noComparable", { count: savings.scannedCount })
                : t("savings.unavailable")}{" "}
            {t("savings.neverZero")}
          </p>
        )}
        <p>{t("savings.basisNote")}</p>
        <p>{t("savings.excludedNote")}</p>
      </div>
    </details>
  );
}

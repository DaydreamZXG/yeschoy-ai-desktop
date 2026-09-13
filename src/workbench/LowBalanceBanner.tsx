import { useTranslation } from "react-i18next";
import { ArrowUpRight, CircleAlert } from "lucide-react";
import { formatMoney, type BalanceAlert } from "../account/finance";
import { useWorkbenchCopy } from "./copy";

/**
 * PRD 6.2 recharge guidance banner. Shown on the home and setup pages when the
 * CNY balance falls to the low-balance threshold; a depleted balance upgrades
 * the tone but never blocks setup.
 */
export function LowBalanceBanner({
  alert,
  onRecharge,
}: {
  alert: NonNullable<BalanceAlert>;
  onRecharge: () => void;
}) {
  const c = useWorkbenchCopy();
  const { i18n } = useTranslation();
  const locale = i18n.resolvedLanguage ?? i18n.language;
  const depleted = alert.level === "depleted";
  return (
    <p className="workbench-notice low-balance-banner" data-level={alert.level} role="status">
      <CircleAlert aria-hidden="true" />
      {depleted
        ? c.balanceDepletedBody
        : c.balanceLowBody.replace(
            "{{amount}}",
            formatMoney(alert.amount, "CNY", locale),
          )}
      <button type="button" onClick={onRecharge}>
        {c.rechargeNow}
        <ArrowUpRight aria-hidden="true" />
      </button>
    </p>
  );
}

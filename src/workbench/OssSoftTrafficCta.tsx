import { ArrowUpRight } from "lucide-react";
import { useTranslation } from "react-i18next";
import { REFERENCE_STATION_ORIGIN } from "../distribution/softTraffic";

export function OssSoftTrafficCta({
  onTryReference,
}: {
  onTryReference: () => void;
}) {
  const { t } = useTranslation();
  return (
    <p className="oss-soft-traffic" data-testid="oss-soft-traffic-cta">
      <button type="button" className="text-button" onClick={onTryReference}>
        {t("workbench.ossSoftTrafficCta")}
        <ArrowUpRight aria-hidden="true" />
      </button>
      <a href={REFERENCE_STATION_ORIGIN} target="_blank" rel="noreferrer">
        {t("workbench.ossReferenceAttribution")}
      </a>
    </p>
  );
}

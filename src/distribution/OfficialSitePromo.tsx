import { useState } from "react";
import { useTranslation } from "react-i18next";
import {
  OFFICIAL_SITE_ORIGIN,
  dismissOfficialSitePromo,
  showOfficialSitePromo,
} from "./officialSitePromo";

/** Settings-footer one-liner for official builds; dismiss persists locally. */
export function OfficialSitePromo() {
  const { t } = useTranslation();
  const [visible, setVisible] = useState(showOfficialSitePromo);

  if (!visible) return null;

  return (
    <p className="official-site-promo" data-testid="official-site-promo">
      <a href={OFFICIAL_SITE_ORIGIN} target="_blank" rel="noreferrer">
        {t("workbench.officialSitePromo")}
      </a>
      <button
        type="button"
        className="text-button"
        data-testid="official-site-promo-dismiss"
        onClick={() => {
          dismissOfficialSitePromo();
          setVisible(false);
        }}
      >
        {t("workbench.officialSitePromoDismiss")}
      </button>
    </p>
  );
}

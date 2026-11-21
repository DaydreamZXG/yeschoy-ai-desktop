import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { ROSTER, chibiOf } from "./roster";
import { hostOf } from "./station";
import { useTyped } from "./useTyped";

interface Props {
  /** 等着确认的站点；没有就不显示。 */
  origin: string | null;
  onAnswer: (accept: boolean) => Promise<void>;
}

/**
 * 网页拉起了客户端，而这个站点是第一次见：阿青先问一句。
 * 分不清钓鱼站的人最容易顺手点确认，所以默认停在「不连接」上，Esc 也是不连接。
 */
export function TrustAsk({ origin, onAnswer }: Props) {
  const { t } = useTranslation();
  const [busy, setBusy] = useState(false);
  const decline = useRef<HTMLButtonElement>(null);
  const host = origin ? hostOf(origin) : "";
  const typed = useTyped(origin ? t("kaifan.trust.ask", { host }) : "");

  useEffect(() => {
    if (origin) decline.current?.focus();
  }, [origin]);

  const answer = async (accept: boolean) => {
    setBusy(true);
    await onAnswer(accept);
    setBusy(false);
  };

  return (
    <div
      className={origin ? "sheet on top" : "sheet top"}
      data-testid="kaifan-trust"
      aria-hidden={!origin}
      onKeyDown={(event) => {
        if (event.key === "Escape" && origin && !busy) void answer(false);
      }}
    >
      <div
        className="panel glass cnp"
        role="alertdialog"
        aria-modal="true"
        aria-label={t("kaifan.trust.title")}
      >
        <header className="dh">
          <img src={chibiOf("gpt", "idle")} alt="" />
          <div>
            <small
              className="who"
              style={{ ["--hc" as string]: ROSTER.gpt.color }}
            >
              {t("roster.gpt.name")}
              <small>{ROSTER.gpt.brand}</small>
            </small>
            <p aria-live="polite">{typed}</p>
          </div>
        </header>
        <p className="quote">
          <b>{host}</b>
          <br />
          {t("kaifan.trust.note")}
        </p>
        <p className="hint">{t("kaifan.trust.warn")}</p>
        <div className="ft">
          <i />
          <button
            type="button"
            className="go"
            ref={decline}
            disabled={busy}
            onClick={() => void answer(false)}
          >
            {t("kaifan.trust.decline")}
          </button>
          <button
            type="button"
            className="go alt"
            disabled={busy}
            onClick={() => void answer(true)}
          >
            {t("kaifan.trust.accept")}
          </button>
        </div>
      </div>
    </div>
  );
}

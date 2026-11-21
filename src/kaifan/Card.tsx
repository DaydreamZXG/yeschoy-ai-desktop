import { useEffect, useMemo, useRef } from "react";
import { useTranslation } from "react-i18next";
import type { AccountModel } from "../account/session";
import {
  DEFAULT_BILLING_GROUP,
  chooseBillingGroup,
} from "../configuration/billing";
import type { ToolConnection } from "../configuration/connections";
import { modelDisplayName } from "../model-profiles/profile";
import { WORKBENCH_APPS, appDisplayName } from "../workbench/appCatalog";
import {
  pickedModel,
  pickedPlan,
  plansOf,
  pricesOf,
  yuan,
  type CardPicks,
  type Discount,
} from "./cardInfo";
import { ROSTER, chibiOf, type RosterId } from "./roster";
import { sound } from "./sound";
import { workerOf } from "./stageState";
import { useTyped } from "./useTyped";

interface Props {
  open: boolean;
  who: RosterId;
  /** 这个站上归她的型号。 */
  ids: string[];
  /** 这个站上的全部模型（带价格）。 */
  models: AccountModel[];
  picks: CardPicks;
  connections?: ToolConnection[];
  onModel: (modelId: string) => void;
  onPlan: (modelId: string, planId: string) => void;
  /** 「让她去接入」。 */
  onJoin: () => void;
  onClose: () => void;
}

/**
 * 角色名片：她现在用哪个型号、走哪个方案、几折，在哪些应用里干活。
 *
 * 数都是站点自己报的。站点没报的照实说没有，不补一个 0，也不猜。
 */
export function Card({
  open,
  who,
  ids,
  models,
  picks,
  connections,
  onModel,
  onPlan,
  onJoin,
  onClose,
}: Props) {
  const { t } = useTranslation();
  const name = t(`roster.${who}.name`);
  const modelId = pickedModel(who, ids, picks);
  const model = models.find((item) => item.id === modelId);
  const plans = useMemo(() => plansOf(model), [model]);
  const planId = model
    ? (pickedPlan(model, picks) ?? chooseBillingGroup(model, ""))
    : "";
  const plan = plans.find((item) => item.id === planId);
  const prices = pricesOf(model, plan?.ratio);

  const planName = (id: string) =>
    id === DEFAULT_BILLING_GROUP ? t("configuration.defaultGroupLabel") : id;
  // 几折只在站点给了官方价时才说：没有官方价，倍率就不知道是相对什么打的折。
  const discount = (value: Discount | null | undefined) => {
    if (!value || !prices) return "";
    // 中文说「几折」（十分之几），别的语言说百分之几：倍数写在文案里。
    if (value.kind === "off")
      return t("kaifan.card.off", {
        n:
          Math.round(value.tenths * Number(t("kaifan.card.offScale")) * 10) /
          10,
      });
    if (value.kind === "over") return t("kaifan.card.over", { n: value.times });
    return t(`kaifan.card.${value.kind}`);
  };
  const multiplier = (ratio: number | null) =>
    ratio === null
      ? t("kaifan.card.noRatio")
      : t("kaifan.card.ratio", { n: Number(ratio.toFixed(3)) });

  const now = discount(plan?.discount);
  const says = !open
    ? ""
    : who === "new"
      ? t("kaifan.card.say.new")
      : !model
        ? t("kaifan.card.say.none")
        : !plans.length
          ? t("kaifan.card.say.noPlans")
          : !now
            ? t("kaifan.card.say.noPrice", { plan: planName(planId) })
            : plans.length === 1
              ? t("kaifan.card.say.one", { discount: now })
              : t("kaifan.card.say.many", {
                  plan: planName(planId),
                  discount: now,
                });
  const typed = useTyped(says);

  // 她在哪些应用里干活，各自用的是哪个型号和方案（接入时存下来的那份）。
  const jobs = WORKBENCH_APPS.flatMap((app) => {
    const saved = connections?.find((item) => item.toolId === app.id);
    return saved && workerOf(saved) === who
      ? [
          {
            id: app.id,
            name: appDisplayName(app.id, app.name, t),
            modelId: saved.modelId,
            planId: saved.billingGroup,
          },
        ]
      : [];
  });
  // 名片上改的是「下次接入用什么」。已经接好的应用不会被悄悄改掉。
  const stale = jobs.some(
    (job) => job.modelId !== modelId || (job.planId && job.planId !== planId),
  );

  const panel = useRef<HTMLElement>(null);
  useEffect(() => {
    if (!open) return;
    const opener = document.activeElement;
    panel.current?.focus();
    return () => {
      if (opener instanceof HTMLElement && opener.isConnected) opener.focus();
    };
  }, [open]);

  return (
    <div
      className={open ? "drw on" : "drw"}
      data-testid="kaifan-card"
      {...(open ? {} : { inert: "" })}
      onClick={(event) => {
        if (event.target === event.currentTarget) onClose();
      }}
      onKeyDown={(event) => {
        if (event.key === "Escape") onClose();
      }}
    >
      <aside
        className="dr glass"
        role="dialog"
        aria-modal="true"
        aria-label={t("kaifan.card.title", { name })}
        tabIndex={-1}
        ref={panel}
        style={{ ["--hc" as string]: ROSTER[who].color }}
      >
        <button
          type="button"
          className="x"
          aria-label={t("kaifan.card.close")}
          onClick={onClose}
        >
          ×
        </button>
        <header className="dh">
          <img src={chibiOf(who, "happy")} alt="" />
          <div>
            <small className="who">
              {name}
              {who !== "new" && <small>{ROSTER[who].brand}</small>}
            </small>
            <p aria-live="polite" data-testid="kaifan-card-says">
              {typed}
            </p>
          </div>
        </header>
        <div className="db">
          {who !== "new" && (
            <section>
              <h4>{t("kaifan.card.good")}</h4>
              <div className="opts">
                {[1, 2, 3].map((index) => (
                  <span className="chip" key={index}>
                    {t(`roster.${who}.good${index}`)}
                  </span>
                ))}
              </div>
            </section>
          )}
          {ids.length > 0 && (
            <section>
              <h4>{t("kaifan.card.model")}</h4>
              <div className="opts">
                {ids.map((id) => {
                  const note = models.find(
                    (item) => item.id === id,
                  )?.description;
                  return (
                    <button
                      key={id}
                      type="button"
                      className="op"
                      aria-pressed={id === modelId}
                      onClick={() => {
                        sound.tick();
                        onModel(id);
                      }}
                    >
                      <b>{modelDisplayName(id)}</b>
                      {note && <small>{note}</small>}
                    </button>
                  );
                })}
              </div>
            </section>
          )}
          {plans.length > 0 && (
            <section>
              <h4>
                {t("kaifan.card.plan")}
                {plans.length > 1 && <small>{t("kaifan.card.planHint")}</small>}
              </h4>
              <div className="opts">
                {plans.map((item) => (
                  <button
                    key={item.id}
                    type="button"
                    className="op"
                    aria-pressed={item.id === planId}
                    disabled={plans.length === 1}
                    onClick={() => {
                      sound.tick();
                      onPlan(modelId, item.id);
                    }}
                  >
                    {item.cheapest && <em>{t("kaifan.card.cheapest")}</em>}
                    <b>{planName(item.id)}</b>
                    <small>
                      {discount(item.discount) || multiplier(item.ratio)}
                    </small>
                  </button>
                ))}
              </div>
              {prices && <p className="nt">{t("kaifan.card.planNote")}</p>}
            </section>
          )}
          {model && (
            <details>
              <summary className="lnk">{t("kaifan.card.prices")}</summary>
              {prices ? (
                <div className="kv" data-testid="kaifan-card-prices">
                  <span>{t("kaifan.card.perMillion")}</span>
                  <b>{t("kaifan.card.input")}</b>
                  <b>{t("kaifan.card.output")}</b>
                  <span>{t("kaifan.card.official")}</span>
                  <b>{yuan(prices.officialInput)}</b>
                  <b>{yuan(prices.officialOutput)}</b>
                  <span>{t("kaifan.card.thisPlan")}</span>
                  <b>
                    {prices.planInput === null
                      ? t("kaifan.card.unknown")
                      : yuan(prices.planInput)}
                  </b>
                  <b>
                    {prices.planOutput === null
                      ? t("kaifan.card.unknown")
                      : yuan(prices.planOutput)}
                  </b>
                </div>
              ) : (
                <p className="nt" data-testid="kaifan-card-prices">
                  {t("kaifan.card.noPrices")}
                </p>
              )}
            </details>
          )}
          <section>
            <h4>{t("kaifan.card.jobs")}</h4>
            {jobs.length ? (
              <ul className="jobs">
                {jobs.map((job) => (
                  <li key={job.id}>
                    <b>{job.name}</b>
                    <span>
                      {modelDisplayName(job.modelId)}
                      {job.planId && ` · ${planName(job.planId)}`}
                    </span>
                  </li>
                ))}
              </ul>
            ) : (
              <p className="nt">{t("kaifan.card.noJobs")}</p>
            )}
            {stale && <p className="nt warn">{t("kaifan.card.stale")}</p>}
          </section>
        </div>
        {model && (
          <footer className="df">
            <button
              type="button"
              className="go"
              data-testid="kaifan-card-join"
              onClick={onJoin}
            >
              {t("kaifan.card.join", { name })}
            </button>
          </footer>
        )}
      </aside>
    </div>
  );
}

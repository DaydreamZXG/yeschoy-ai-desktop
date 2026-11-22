import { Fragment, useMemo } from "react";
import { useTranslation } from "react-i18next";
import type { AccountModel } from "../account/session";
import type { ActivationToolId } from "../configuration/activation";
import {
  DEFAULT_BILLING_GROUP,
  chooseBillingGroup,
} from "../configuration/billing";
import type { ToolConnection } from "../configuration/connections";
import { modelDisplayName } from "../model-profiles/profile";
import { WORKBENCH_APPS, appDisplayName } from "../workbench/appCatalog";
import {
  joinModel,
  pickedPlan,
  currencyOf,
  planLabelOf,
  plansOf,
  pricesOf,
  tieredPricesOf,
  yuan,
  type CardPicks,
  type Discount,
} from "./cardInfo";
import { Drawer } from "./Drawer";
import type { RosterId } from "./roster";
import { sound } from "./sound";
import { workerOf } from "./stageState";

interface Props {
  open: boolean;
  who: RosterId;
  /** 这个站上归她的型号。 */
  ids: string[];
  /** 这个站上的全部模型（带价格）。 */
  models: AccountModel[];
  picks: CardPicks;
  /** 站点报的汇率（一美元合多少元）。分档收费的型号靠它算成元；没给就是空串。 */
  fx?: string;
  connections?: ToolConnection[];
  onModel: (modelId: string) => void;
  onPlan: (modelId: string, planId: string) => void;
  /** 「让她去接入」。 */
  /** 去接入屏。带上名片正说着的那个应用，两边讲的才是同一次接入。 */
  onJoin: (app?: ActivationToolId) => void;
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
  fx = "",
  connections,
  onModel,
  onPlan,
  onJoin,
  onClose,
}: Props) {
  const { t } = useTranslation();
  const name = t(`roster.${who}.name`);
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
            saved,
          },
        ]
      : [];
  });
  // 名片上写的是「她再去接入会用什么」，和接入屏问的是同一处：没挑过就沿用
  // 接好的那份，不拿一个默认的出来说「要重新接入」。
  // 她接着好几个应用时说的是头一个，点「去接入」也是带着这一个过去。
  const first = jobs[0];
  const modelId = joinModel(who, ids, first?.saved, picks);
  const model = models.find((item) => item.id === modelId);
  const plans = useMemo(() => plansOf(model), [model]);
  const kept = first
    ? [
        { modelId: first.modelId, billingGroup: first.planId },
        ...(first.saved.models ?? []),
      ].find((item) => item.modelId === modelId)?.billingGroup
    : undefined;
  const planId = model
    ? (pickedPlan(model, picks) ??
      (kept && plans.some((item) => item.id === kept)
        ? kept
        : chooseBillingGroup(model, "")))
    : "";
  const plan = plans.find((item) => item.id === planId);
  const prices = pricesOf(model, plan?.ratio, plan?.pricing);
  // 站点没给单一的官方价时，看它是不是分档或者按次收费的。
  const tiered = prices ? null : tieredPricesOf(model, plan?.ratio, fx);
  const priced = !!prices || !!tiered;
  const currency = currencyOf(model);
  const money = (value: number) => yuan(value, currency);
  const amount = (value: number | null) =>
    value === null ? t("kaifan.card.unknown") : money(value);

  const planName = (id: string) =>
    planLabelOf(models, id) ??
    (id === DEFAULT_BILLING_GROUP ? t("configuration.defaultGroupLabel") : id);
  // 几折只在站点给了官方价时才说：没有官方价，倍率就不知道是相对什么打的折。
  const discount = (value: Discount | null | undefined) => {
    if (!value || !priced) return "";
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
  const says =
    who === "new"
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

  // 名片上改的是「下次接入用什么」。已经接好的应用不会被悄悄改掉；只有
  // 再接入一次真的会换型号或方案时才提醒。
  const stale = jobs.some((job) => {
    const next = joinModel(who, ids, job.saved, picks);
    if (next !== job.modelId) return true;
    const wanted = pickedPlan(
      models.find((item) => item.id === next),
      picks,
    );
    return !!wanted && !!job.planId && wanted !== job.planId;
  });

  return (
    <Drawer
      open={open}
      testId="kaifan-card"
      label={t("kaifan.card.title", { name })}
      who={who}
      says={says}
      onClose={onClose}
      foot={
        model && (
          <button
            type="button"
            className="go"
            data-testid="kaifan-card-join"
            onClick={() => onJoin(first?.id)}
          >
            {t("kaifan.card.join", { name })}
          </button>
        )
      }
    >
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
              const note = models.find((item) => item.id === id)?.description;
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
                {item.subscription && <i>{t("kaifan.card.subscription")}</i>}
                {item.appOnly && <i>{t("kaifan.card.appOnly")}</i>}
                <small>
                  {discount(item.discount) || multiplier(item.ratio)}
                </small>
              </button>
            ))}
          </div>
          {priced && <p className="nt">{t("kaifan.card.planNote")}</p>}
        </section>
      )}
      {model && prices && (
        <details>
          <summary className="lnk">{t("kaifan.card.prices")}</summary>
          <div className="kv" data-testid="kaifan-card-prices">
            <span>{t("kaifan.card.perMillion")}</span>
            <b>{t("kaifan.card.input")}</b>
            <b>{t("kaifan.card.output")}</b>
            <span>{t("kaifan.card.official")}</span>
            <b>{money(prices.officialInput)}</b>
            <b>{money(prices.officialOutput)}</b>
            <span>{t("kaifan.card.thisPlan")}</span>
            <b>{amount(prices.planInput)}</b>
            <b>{amount(prices.planOutput)}</b>
          </div>
        </details>
      )}
      {model && tiered && (
        <details>
          <summary className="lnk">{t("kaifan.card.prices")}</summary>
          {tiered.unit === "tokens" ? (
            <div className="kv" data-testid="kaifan-card-prices">
              <span>{t("kaifan.card.perMillion")}</span>
              <b>{t("kaifan.card.input")}</b>
              <b>{t("kaifan.card.output")}</b>
              {tiered.tiers.map((tier, index) => (
                <Fragment key={index}>
                  {tier.name && <i>{tier.name}</i>}
                  <span>{t("kaifan.card.official")}</span>
                  <b>{money(tier.officialInput)}</b>
                  <b>{money(tier.officialOutput)}</b>
                  <span>{t("kaifan.card.thisPlan")}</span>
                  <b>{amount(tier.planInput)}</b>
                  <b>{amount(tier.planOutput)}</b>
                </Fragment>
              ))}
            </div>
          ) : (
            <div className="kv kv1" data-testid="kaifan-card-prices">
              <span>{t("kaifan.card.perRequest")}</span>
              <b />
              {tiered.tiers.map((tier, index) => (
                <Fragment key={index}>
                  {tier.name && <i>{tier.name}</i>}
                  <span>{t("kaifan.card.official")}</span>
                  <b>{money(tier.official)}</b>
                  <span>{t("kaifan.card.thisPlan")}</span>
                  <b>{amount(tier.plan)}</b>
                </Fragment>
              ))}
            </div>
          )}
          {tiered.tiers.length > 1 && (
            <p className="nt">{t("kaifan.card.tierNote")}</p>
          )}
        </details>
      )}
      {model && !priced && (
        <p className="nt" data-testid="kaifan-card-prices">
          {t("kaifan.card.noPrices")}
        </p>
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
    </Drawer>
  );
}

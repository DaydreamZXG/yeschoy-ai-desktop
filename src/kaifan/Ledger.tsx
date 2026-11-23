import { useMemo } from "react";
import { useTranslation } from "react-i18next";
import { formatMoney, type AccountMoney } from "../account/finance";
import type { UsageLogReport } from "../account/usage";
import { modelDisplayName } from "../model-profiles/profile";
import { Drawer } from "./Drawer";
import {
  covers,
  daysOf,
  eatersOf,
  latestOf,
  spentSince,
  startOfDay,
} from "./ledgerInfo";
import { ROSTER, chibiOf, type RosterId } from "./roster";

interface Props {
  open: boolean;
  who: RosterId;
  /** 站点现在的情况：连着、没连、连不上。 */
  reach: "ok" | "out" | "off" | "wait";
  /** 正在读最新的数：读回来之前不把手里上一次的数当成现在的摆出来。 */
  reading?: boolean;
  money?: AccountMoney;
  log?: UsageLogReport;
  /** 应用的名字，按应用 ID 查；最近几笔里写是哪个应用花的。 */
  appName: (toolId: string) => string;
  onRecharge: () => void;
  onConnect: () => void;
  onClose: () => void;
  /** 现在几点；测试里固定住。 */
  now?: number;
}

/**
 * 账本：余额、最近七天、谁用得最多、最近几笔。
 *
 * 数全是站点给的。站点没给的（余额、某一天、某一笔的金额）写「不知道」，
 * 不写 0，也不估。
 */
export function Ledger({
  open,
  who,
  reach,
  reading = false,
  money,
  log,
  appName,
  onRecharge,
  onConnect,
  onClose,
  now = Date.now(),
}: Props) {
  const { t, i18n } = useTranslation();
  const locale = i18n.language;
  const unknown = t("kaifan.ledger.unknown");
  const cash = (amount: number | string | null | undefined) => {
    if (amount === null || amount === undefined) return null;
    const shown = formatMoney(String(amount), money?.currency, locale);
    return shown === "—" ? null : shown;
  };

  const balance = cash(money?.balanceAmount);
  const used = cash(money?.consumedAmount);
  const readable = log?.status === "available" || log?.status === "empty";
  const view = useMemo(() => {
    if (!log || !readable) return null;
    const days = daysOf(log, now);
    return {
      days,
      today: spentSince(log, startOfDay(now)),
      week: spentSince(log, days[0].start),
      eaters: eatersOf(log.records),
      latest: latestOf(log.records),
      // 明细没读全时，排行只算得到最早那一笔之后的。
      whole: covers(log, now - log.windowDays * 86_400_000),
    };
  }, [log, now, readable]);
  const nothing = !balance && !view;

  const today = view ? cash(view.today) : null;
  const says =
    reach === "out"
      ? t("kaifan.ledger.saysOut")
      : reach === "off"
        ? t("kaifan.ledger.saysOff")
        : reach === "wait"
          ? t("kaifan.ledger.saysWait")
          : reading
            ? t("kaifan.ledger.saysReading")
            : nothing
              ? t("kaifan.ledger.saysNothing")
              : [
                  !view || view.today === null || !today
                    ? t("kaifan.ledger.saysTodayUnknown")
                    : view.today === 0
                      ? t("kaifan.ledger.saysTodayNone")
                      : t("kaifan.ledger.saysToday", { amount: today }),
                  balance
                    ? t("kaifan.ledger.saysBalance", { amount: balance })
                    : t("kaifan.ledger.saysBalanceUnknown"),
                ].join("");

  const top = view
    ? Math.max(0, ...view.days.map((day) => day.amount ?? 0))
    : 0;
  // 有人的金额看不到时排行是按次数排的，条的长短也按次数画，不混着比。
  const priced = !!view?.eaters.every((eater) => eater.amount !== null);
  const weight = (eater: { amount: number | null; requests: number }) =>
    priced ? eater.amount! : eater.requests;
  const most = view ? Math.max(0, ...view.eaters.map(weight)) : 0;
  const date = (at: number, options: Intl.DateTimeFormatOptions) =>
    new Intl.DateTimeFormat(locale, options).format(at);

  return (
    <Drawer
      open={open}
      testId="kaifan-ledger"
      label={t("kaifan.nav.ledger")}
      who={who}
      mood={
        reach === "ok" && reading
          ? "idle"
          : reach === "ok" && !nothing
            ? "happy"
            : "panic"
      }
      says={says}
      onClose={onClose}
      foot={
        reach === "out" ? (
          <button type="button" className="go" onClick={onConnect}>
            {t("kaifan.home.outGo")}
          </button>
        ) : reach === "ok" ? (
          <button
            type="button"
            className="go"
            data-testid="kaifan-ledger-recharge"
            onClick={onRecharge}
          >
            {t("kaifan.ledger.recharge")}
          </button>
        ) : undefined
      }
    >
      {reach !== "ok" || reading || nothing ? (
        <p className="quote">{t("kaifan.ledger.wouldShow")}</p>
      ) : (
        <>
          <section>
            <h4>{t("kaifan.ledger.balance")}</h4>
            <p className="big" data-testid="kaifan-ledger-balance">
              {balance ?? unknown}
              {used && (
                <small>{t("kaifan.ledger.used", { amount: used })}</small>
              )}
            </p>
          </section>
          {!view ? (
            <p className="nt" data-testid="kaifan-ledger-nolog">
              {t("kaifan.ledger.noLog")}
            </p>
          ) : (
            <>
              <section>
                <h4>
                  {t("kaifan.ledger.week")}
                  <small data-testid="kaifan-ledger-week">
                    {cash(view.week) ?? t("kaifan.ledger.partial")}
                  </small>
                </h4>
                <ol className="days">
                  {view.days.map((day, index) => {
                    const amount = cash(day.amount);
                    const last = index === view.days.length - 1;
                    const label = date(day.start, {
                      month: "numeric",
                      day: "numeric",
                    });
                    return (
                      <li
                        key={day.start}
                        className={
                          [day.amount === null && "na", last && "now"]
                            .filter(Boolean)
                            .join(" ") || undefined
                        }
                        title={`${label} · ${amount ?? unknown}`}
                        style={{
                          ["--bar" as string]:
                            day.amount && top ? day.amount / top : 0,
                        }}
                      >
                        {/* 花了钱的那几天把数写在柱子上：不用把鼠标停上去才看得到。 */}
                        {!!day.amount && amount && (
                          <em aria-hidden="true">{amount}</em>
                        )}
                        <i aria-hidden="true" />
                        <span>
                          <span className="sr">{label} </span>
                          <b className="sr">{amount ?? unknown}</b>
                          <span aria-hidden="true">
                            {last
                              ? t("kaifan.ledger.today")
                              : date(day.start, { day: "numeric" })}
                          </span>
                        </span>
                      </li>
                    );
                  })}
                </ol>
              </section>
              <section>
                <h4>
                  {t("kaifan.ledger.eaters")}
                  <small>
                    {view.whole
                      ? t("kaifan.ledger.window", { count: log!.windowDays })
                      : t("kaifan.ledger.since", {
                          date: date(log!.oldestAtEpochMs, {
                            month: "numeric",
                            day: "numeric",
                          }),
                        })}
                  </small>
                </h4>
                {view.eaters.length ? (
                  <ul className="rows" data-testid="kaifan-ledger-eaters">
                    {view.eaters.map((eater) => (
                      <li
                        key={eater.who}
                        className="rw"
                        style={{ ["--c" as string]: ROSTER[eater.who].color }}
                      >
                        <img src={chibiOf(eater.who, "idle")} alt="" />
                        <span>
                          {t(`roster.${eater.who}.name`)}
                          <small>
                            {t("kaifan.ledger.requests", {
                              count: eater.requests,
                            })}
                          </small>
                          <i
                            className="br"
                            style={{
                              width: most
                                ? `${Math.max(3, (weight(eater) / most) * 100)}%`
                                : "3%",
                            }}
                          />
                        </span>
                        <b>{cash(eater.amount) ?? unknown}</b>
                      </li>
                    ))}
                  </ul>
                ) : (
                  <p className="nt">{t("kaifan.ledger.noRecords")}</p>
                )}
              </section>
              {view.latest.length > 0 && (
                <section>
                  <h4>{t("kaifan.ledger.latest")}</h4>
                  <ul className="txs" data-testid="kaifan-ledger-latest">
                    {view.latest.map((record, index) => (
                      <li key={`${record.observedAtEpochMs}-${index}`}>
                        <b>{modelDisplayName(record.modelId)}</b>
                        <span>
                          {date(record.observedAtEpochMs, {
                            month: "numeric",
                            day: "numeric",
                            hour: "2-digit",
                            minute: "2-digit",
                          })}
                          {record.toolId && ` · ${appName(record.toolId)}`}
                        </span>
                        <em>{cash(record.amount) ?? unknown}</em>
                      </li>
                    ))}
                  </ul>
                </section>
              )}
            </>
          )}
        </>
      )}
    </Drawer>
  );
}

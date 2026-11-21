import type { UsageLogReport, UsageRecord } from "../account/usage";
import { characterOfModel, type RosterId } from "./roster";

/**
 * 账本上的数。全部来自站点给的用量明细，这里只做加总。
 *
 * 两条规矩：
 * - 一笔记录没带金额，含它的合计就是「不知道」（null），不当成 0 加进去。
 * - 明细有条数上限。被截断时，比最早那一笔还早的时间段没读全，也是「不知道」。
 */

/** 明细有没有读全从 `from` 起的这段时间。 */
export function covers(log: UsageLogReport, from: number): boolean {
  return !log.truncated || log.oldestAtEpochMs <= from;
}

function sum(records: UsageRecord[]): number | null {
  let total = 0;
  for (const record of records) {
    const amount = Number(record.amount);
    if (record.amount === "" || !Number.isFinite(amount)) return null;
    total += amount;
  }
  return Number(total.toFixed(6));
}

/** 从 `from` 到现在一共花了多少；这段时间没读全、或者有记录没带金额，就是 null。 */
export function spentSince(log: UsageLogReport, from: number): number | null {
  if (log.status === "empty") return 0;
  if (log.status !== "available" || !covers(log, from)) return null;
  return sum(log.records.filter((record) => record.observedAtEpochMs >= from));
}

/** 本地时间今天零点。 */
export function startOfDay(now: number): number {
  const day = new Date(now);
  day.setHours(0, 0, 0, 0);
  return day.getTime();
}

export interface LedgerDay {
  start: number;
  /** 这一天花了多少；没读全或有记录没带金额时是 null。 */
  amount: number | null;
  requests: number;
}

/** 最近 `count` 天（按本地日期），最后一个是今天。 */
export function daysOf(
  log: UsageLogReport,
  now: number,
  count = 7,
): LedgerDay[] {
  const today = new Date(startOfDay(now));
  return Array.from({ length: count }, (_, index) => {
    // 用日历日期往回数，夏令时换表的那天也不会错一格。
    const from = new Date(today);
    from.setDate(today.getDate() - (count - 1 - index));
    const to = new Date(from);
    to.setDate(from.getDate() + 1);
    const start = from.getTime();
    const records =
      log.status === "available"
        ? log.records.filter(
            (record) =>
              record.observedAtEpochMs >= start &&
              record.observedAtEpochMs < to.getTime(),
          )
        : [];
    const known =
      log.status === "empty" ||
      (log.status === "available" && covers(log, start));
    return {
      start,
      amount: known ? sum(records) : null,
      requests: records.length,
    };
  });
}

export interface Eater {
  who: RosterId;
  /** 她花了多少；有记录没带金额时是 null。 */
  amount: number | null;
  requests: number;
}

/**
 * 谁吃得最多：把明细按角色加起来，花得多的排前面。
 * 只要有一位的金额不知道，就没法按钱比，改按次数排。
 */
export function eatersOf(records: UsageRecord[]): Eater[] {
  const groups = new Map<RosterId, UsageRecord[]>();
  for (const record of records) {
    const who = characterOfModel(record.modelId);
    groups.set(who, [...(groups.get(who) ?? []), record]);
  }
  const eaters = [...groups].map(([who, list]) => ({
    who,
    amount: sum(list),
    requests: list.length,
  }));
  const priced = eaters.every((eater) => eater.amount !== null);
  return eaters.sort((a, b) =>
    priced
      ? b.amount! - a.amount! || b.requests - a.requests
      : b.requests - a.requests,
  );
}

/** 最近的几笔，新的在前。 */
export function latestOf(records: UsageRecord[], count = 6): UsageRecord[] {
  return [...records]
    .sort((a, b) => b.observedAtEpochMs - a.observedAtEpochMs)
    .slice(0, count);
}

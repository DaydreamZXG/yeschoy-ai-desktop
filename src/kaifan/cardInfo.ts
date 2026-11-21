import type { AccountModel, BillingGroup } from "../account/session";
import { hasReviewedCapabilities } from "../model-profiles/profile";
import type { RosterId } from "./roster";

/**
 * 名片上的数：几折、哪个方案最省、每百万 tokens 多少钱。
 *
 * 只用站点自己报出来的东西（方案的倍率、它标的官方价）。站点怎么卖额度、
 * 充值有没有优惠，客户端看不到，所以这里不去猜「你实际省了多少」。
 */

export type Discount =
  | { kind: "free" }
  | { kind: "off"; tenths: number }
  | { kind: "full" }
  | { kind: "over"; times: number };

/** 倍率写成人话。站点没给、或者给的不是个正常的数，就是 null：不写。 */
export function discountOf(ratio: number | null | undefined): Discount | null {
  if (ratio === null || ratio === undefined) return null;
  if (!Number.isFinite(ratio) || ratio < 0) return null;
  if (ratio === 0) return { kind: "free" };
  if (ratio > 1.025)
    return { kind: "over", times: Math.round(ratio * 10) / 10 };
  // 取到半折；不到一折的写到小数点后一位，免得写成「0 折」。
  const half = Math.round(ratio * 20) / 2;
  if (half >= 10) return { kind: "full" };
  if (half < 1)
    return { kind: "off", tenths: Math.max(0.1, Math.round(ratio * 100) / 10) };
  return { kind: "off", tenths: half };
}

export interface Plan {
  id: string;
  ratio: number | null;
  discount: Discount | null;
  /** 几个方案里明确最便宜的那个。只有一个、或者分不出高低时都不标。 */
  cheapest: boolean;
}

export function plansOf(model: AccountModel | undefined): Plan[] {
  const groups: BillingGroup[] = model?.billing?.groups ?? [];
  const known = groups
    .map((group) => group.ratio)
    .filter(
      (ratio): ratio is number =>
        ratio !== null && Number.isFinite(ratio) && ratio >= 0,
    );
  const lowest = known.length ? Math.min(...known) : null;
  const clear =
    groups.length > 1 &&
    known.length === groups.length &&
    known.filter((ratio) => ratio === lowest).length === 1;
  return groups.map((group) => ({
    id: group.id,
    ratio: group.ratio,
    discount: discountOf(group.ratio),
    cheapest: clear && group.ratio === lowest,
  }));
}

export interface Prices {
  officialInput: number;
  officialOutput: number;
  /** 选中的方案下的价；这个方案的倍率站点没给时是 null。 */
  planInput: number | null;
  planOutput: number | null;
}

/** 每百万 tokens 的价（元）。站点没给官方价就是 null，界面照实说没有。 */
export function pricesOf(
  model: AccountModel | undefined,
  ratio: number | null | undefined,
): Prices | null {
  if (!model?.pricingAvailable) return null;
  const input = model.officialInputCnyPerMillion.trim();
  const output = model.officialOutputCnyPerMillion.trim();
  // 空串是「没给」，不是 0。
  if (!input || !output) return null;
  const officialInput = Number(input);
  const officialOutput = Number(output);
  if (
    !Number.isFinite(officialInput) ||
    officialInput < 0 ||
    !Number.isFinite(officialOutput) ||
    officialOutput < 0
  )
    return null;
  const usable =
    ratio !== null &&
    ratio !== undefined &&
    Number.isFinite(ratio) &&
    ratio >= 0;
  return {
    officialInput,
    officialOutput,
    planInput: usable ? officialInput * ratio : null,
    planOutput: usable ? officialOutput * ratio : null,
  };
}

export function yuan(amount: number): string {
  return `¥${new Intl.NumberFormat("zh-CN", {
    minimumFractionDigits: 2,
    maximumFractionDigits: amount > 0 && amount < 0.1 ? 4 : 2,
  }).format(amount)}`;
}

/** 在名片上挑过的：每个人用哪个型号、每个型号走哪个方案。 */
export interface CardPicks {
  model: Partial<Record<RosterId, string>>;
  plan: Record<string, string>;
}

const KEY = "kaifan-card-picks";
const text = (value: unknown) =>
  !!value &&
  typeof value === "object" &&
  !Array.isArray(value) &&
  Object.values(value).every(
    (item) => typeof item === "string" && item.length <= 200,
  );

export function readCardPicks(): CardPicks {
  try {
    const raw = JSON.parse(localStorage.getItem(KEY) ?? "null");
    if (raw && text(raw.model) && text(raw.plan))
      return { model: raw.model, plan: raw.plan };
  } catch {
    // 读不了就当没挑过。
  }
  return { model: {}, plan: {} };
}

export function storeCardPicks(picks: CardPicks) {
  try {
    localStorage.setItem(KEY, JSON.stringify(picks));
  } catch {
    // 存不了只是下次要重新挑。
  }
}

/** 在名片上给她挑过、而且站上还有的型号；没挑过就是 undefined。 */
export function chosenModel(
  who: RosterId,
  ids: string[] | undefined,
  picks: CardPicks,
): string | undefined {
  const picked = picks.model[who];
  return picked && ids?.includes(picked) ? picked : undefined;
}

/** 她现在用哪个型号：挑过的；没挑过就用资料齐的那个，再不行是站点列的第一个。 */
export function pickedModel(
  who: RosterId,
  ids: string[] | undefined,
  picks: CardPicks,
): string {
  if (!ids?.length) return "";
  return (
    chosenModel(who, ids, picks) ??
    ids.find((id) => hasReviewedCapabilities(id)) ??
    ids[0]
  );
}

/** 这个型号挑过的方案；站点已经不提供了就不算。 */
export function pickedPlan(
  model: AccountModel | undefined,
  picks: CardPicks,
): string | undefined {
  if (!model) return undefined;
  const picked = picks.plan[model.id];
  return picked && model.billing?.groups.some((group) => group.id === picked)
    ? picked
    : undefined;
}

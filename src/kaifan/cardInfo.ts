import type { AccountModel, BillingGroup } from "../account/session";
import { billingTiers } from "../configuration/billing";
import type { ToolConnection } from "../configuration/connections";
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
  // 只贵一点点的（1.03、1.04）写到两位小数：取一位会写成「1 倍」，等于说原价。
  if (ratio > 1.025)
    return {
      kind: "over",
      times:
        Math.round(ratio * (ratio < 1.095 ? 100 : 10)) /
        (ratio < 1.095 ? 100 : 10),
    };
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

/** 分档收费的型号：一档一个价。按 tokens 算和按次算的不会混在一个型号里。 */
export type TieredPrices =
  | { unit: "tokens"; tiers: ({ name: string } & Prices)[] }
  | {
      unit: "request";
      tiers: { name: string; official: number; plan: number | null }[];
    };

/**
 * 站点按档收费（高峰和平时、长对话和短对话）或者按次收费的型号的价（元）。
 *
 * 这种型号站点不给一个单一的官方价，给的是一条算式。算式读得懂就一档一档列出来；
 * 读不懂、或者汇率没给，就是 null，界面照实说没有。档位的名字是站点起的，原样写。
 */
export function tieredPricesOf(
  model: AccountModel | undefined,
  ratio: number | null | undefined,
  fx: string,
): TieredPrices | null {
  const billing = model?.billing;
  const rate = Number(fx);
  if (!billing || !fx.trim() || !Number.isFinite(rate) || rate <= 0)
    return null;
  const rows =
    model.billingMode === "tiered_expr"
      ? billingTiers(billing.expression)
      : model.billingMode === "per_request" && billing.requestUsd !== null
        ? [{ name: "", rates: { request: billing.requestUsd } }]
        : [];
  if (!rows.length) return null;
  const usable =
    ratio !== null &&
    ratio !== undefined &&
    Number.isFinite(ratio) &&
    ratio >= 0;
  const plan = (official: number) => (usable ? official * ratio : null);
  // 只有一档就不用写名字。
  const name = (row: { name: string }) => (rows.length > 1 ? row.name : "");
  const priced = (value: number | undefined): value is number =>
    value !== undefined && Number.isFinite(value) && value >= 0;
  if (rows.every((row) => priced(row.rates.request)))
    return {
      unit: "request",
      tiers: rows.map((row) => ({
        name: name(row),
        official: row.rates.request * rate,
        plan: plan(row.rates.request * rate),
      })),
    };
  if (rows.every((row) => priced(row.rates.p) && priced(row.rates.c)))
    return {
      unit: "tokens",
      tiers: rows.map((row) => ({
        name: name(row),
        officialInput: row.rates.p * rate,
        officialOutput: row.rates.c * rate,
        planInput: plan(row.rates.p * rate),
        planOutput: plan(row.rates.c * rate),
      })),
    };
  return null;
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

/** 最多记这么多个站点的选择，再多就丢最早的。 */
const STATIONS = 20;

function stored(): Record<string, unknown> {
  try {
    const raw = JSON.parse(localStorage.getItem(KEY) ?? "null");
    if (raw && typeof raw === "object" && !Array.isArray(raw)) return raw;
  } catch {
    // 读不了就当没挑过。
  }
  return {};
}

/**
 * 在这个站点上挑过的型号和方案。
 *
 * 按站点分开记：两个站可以有同名的型号，方案和价格却不是一回事，
 * 在甲站挑的方案不能带到乙站去。`station` 是站点的主机名，内置站点是空串。
 */
export function readCardPicks(station: string): CardPicks {
  const raw = stored()[station] as Partial<CardPicks> | undefined;
  return raw && text(raw.model) && text(raw.plan)
    ? { model: raw.model!, plan: raw.plan! }
    : { model: {}, plan: {} };
}

export function storeCardPicks(station: string, picks: CardPicks) {
  try {
    const all = stored();
    // 重新放进去，让最近用的站点排在后面，超了先丢最早的。
    delete all[station];
    all[station] = picks;
    localStorage.setItem(
      KEY,
      JSON.stringify(Object.fromEntries(Object.entries(all).slice(-STATIONS))),
    );
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

/**
 * 她去接入这个应用时会用哪个型号：名片上给她挑过的优先；没挑过，接过的沿用；
 * 都没有就挑一个资料齐的。名片和接入屏都问这里，两边说的才是同一个。
 */
export function joinModel(
  who: RosterId,
  ids: string[] | undefined,
  saved: Pick<ToolConnection, "state" | "modelId"> | undefined,
  picks: CardPicks,
): string {
  if (!ids?.length) return "";
  const chosen = chosenModel(who, ids, picks);
  if (chosen) return chosen;
  if (saved && saved.state !== "not_connected" && ids.includes(saved.modelId))
    return saved.modelId;
  return pickedModel(who, ids, picks);
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

import type { AccountModel, BillingGroup } from "../account/session";

type Node =
  | { kind: "number"; value: number }
  | { kind: "name" | "string"; value: string }
  | { kind: "binary"; op: string; left: Node; right: Node }
  | { kind: "call"; name: string; args: Node[] }
  | { kind: "conditional"; condition: Node; yes: Node; no: Node };
const variables = ["p", "c", "cr", "cc", "cc1h", "img", "ai", "ao", "img_o"];

// A bounded parser for display only. Never eval/new Function the server's rules.
// Unsupported expressions remain visible as rules; they never block setup.
function parse(source: string): Node {
  if (source.length > 8192) throw Error("too_long");
  source = source.replace(/^v1:/, "");
  const tokens =
    source.match(
      /"(?:[^"\\]|\\.)*"|(?:\d+(?:\.\d*)?|\.\d+)(?:[eE][+-]?\d+)?|[a-zA-Z_]\w*|&&|\|\||<=|>=|==|!=|[()+*\/,?:<>-]/g,
    ) ?? [];
  if (tokens.length > 1024) throw Error("too_many_tokens");
  let remainder = source;
  for (const token of tokens) {
    remainder = remainder.trimStart();
    if (!remainder.startsWith(token)) throw Error("invalid_token");
    remainder = remainder.slice(token.length);
  }
  if (remainder.trim()) throw Error("invalid_token");
  let index = 0;
  const take = (value: string) => tokens[index] === value && !!++index;
  const expect = (value: string) => {
    if (!take(value)) throw Error("invalid_syntax");
  };
  const precedence: Record<string, number> = {
    "||": 1,
    "&&": 2,
    "==": 3,
    "!=": 3,
    "<": 4,
    ">": 4,
    "<=": 4,
    ">=": 4,
    "+": 5,
    "-": 5,
    "*": 6,
    "/": 6,
  };
  function expression(minimum = 0, depth = 0): Node {
    if (depth > 48) throw Error("too_deep");
    const token = tokens[index++];
    let left: Node;
    if (token === "(") {
      left = expression(0, depth + 1);
      expect(")");
    } else if (token?.startsWith('"'))
      left = { kind: "string", value: JSON.parse(token) };
    else if (/^(?:\d|\.)/.test(token ?? "")) {
      const value = Number(token);
      if (!Number.isFinite(value) || value < 0) throw Error("invalid_number");
      left = { kind: "number", value };
    } else if (/^[a-zA-Z_]\w*$/.test(token ?? "")) {
      if (take("(")) {
        const args: Node[] = [];
        if (!take(")")) {
          do {
            args.push(expression(0, depth + 1));
          } while (take(","));
          expect(")");
        }
        left = { kind: "call", name: token, args };
      } else left = { kind: "name", value: token };
    } else throw Error("invalid_syntax");
    while ((precedence[tokens[index]] ?? -1) >= minimum) {
      const op = tokens[index++];
      left = {
        kind: "binary",
        op,
        left,
        right: expression(precedence[op] + 1, depth + 1),
      };
    }
    if (minimum === 0 && take("?")) {
      const yes = expression(0, depth + 1);
      expect(":");
      left = {
        kind: "conditional",
        condition: left,
        yes,
        no: expression(0, depth + 1),
      };
    }
    return left;
  }
  const root = expression();
  if (index !== tokens.length) throw Error("invalid_syntax");
  return root;
}

type Linear = Record<string, number>;
function linear(node: Node): Linear {
  if (node.kind === "number") return { constant: node.value };
  if (node.kind === "name" && variables.includes(node.value))
    return { [node.value]: 1 };
  if (node.kind !== "binary") throw Error("not_linear");
  const a = linear(node.left),
    b = linear(node.right);
  const scalar = (v: Linear) => Object.keys(v).every((k) => k === "constant");
  if (node.op === "+" || node.op === "-") {
    const result = { ...a };
    for (const [key, value] of Object.entries(b))
      result[key] = (result[key] ?? 0) + (node.op === "+" ? value : -value);
    return result;
  }
  if (node.op === "*" && (scalar(a) || scalar(b))) {
    const [factor, terms] = scalar(a)
      ? [a.constant ?? 0, b]
      : [b.constant ?? 0, a];
    return Object.fromEntries(
      Object.entries(terms).map(([k, n]) => [k, n * factor]),
    );
  }
  if (node.op === "/" && scalar(b) && b.constant > 0)
    return Object.fromEntries(
      Object.entries(a).map(([k, n]) => [k, n / b.constant]),
    );
  throw Error("not_linear");
}

function validCondition(node: Node): boolean {
  if (node.kind === "number" || node.kind === "string") return true;
  if (node.kind === "name") return [...variables, "len"].includes(node.value);
  if (node.kind === "binary")
    return validCondition(node.left) && validCondition(node.right);
  return (
    node.kind === "call" &&
    ["hour", "minute", "weekday", "month", "day"].includes(node.name) &&
    node.args.length === 1 &&
    node.args[0].kind === "string"
  );
}

export interface BillingTier {
  name: string;
  rates: Record<string, number>;
}
export function billingTiers(expression: string): BillingTier[] {
  try {
    const tiers: BillingTier[] = [];
    function visit(node: Node) {
      if (node.kind === "conditional") {
        if (!validCondition(node.condition))
          throw Error("unsupported_condition");
        visit(node.yes);
        visit(node.no);
        return;
      }
      let name = "基础计费";
      let cost: Node = node;
      if (
        node.kind === "call" &&
        node.name === "tier" &&
        node.args.length === 2 &&
        node.args[0].kind === "string"
      ) {
        name = node.args[0].value;
        cost = node.args[1];
      }
      const rates = linear(cost);
      if (
        (rates.constant ?? 0) !== 0 ||
        !Object.values(rates).every((n) => Number.isFinite(n) && n >= 0)
      )
        throw Error("unsupported_cost");
      tiers.push({ name, rates });
      if (tiers.length > 32) throw Error("too_many_tiers");
    }
    visit(parse(expression));
    return tiers;
  } catch {
    return [];
  }
}

export function chooseBillingGroup(
  model: AccountModel | undefined,
  previous: string,
): string {
  const groups = model?.billing?.groups ?? [];
  return (
    groups.find((g) => g.id === previous)?.id ??
    groups.find((g) => g.id === "default")?.id ??
    groups[0]?.id ??
    ""
  );
}

export function groupPrice(
  model: AccountModel,
  group: BillingGroup,
  fx: string,
): {
  rows: BillingTier[];
  currency: string;
  multiplier: number;
  unit: "tokens" | "request";
} {
  const exchange = Number(fx);
  const currency =
    fx && Number.isFinite(exchange) && exchange > 0 ? "CNY" : "USD";
  const multiplier = (currency === "CNY" ? exchange : 1) * (group.ratio ?? NaN);
  const billing = model.billing;
  let rows: BillingTier[] = [];
  if (billing) {
    if (model.billingMode === "tiered_expr")
      rows = billingTiers(billing.expression);
    else if (model.billingMode === "per_request" && billing.requestUsd !== null)
      rows = [{ name: "按次计费", rates: { request: billing.requestUsd } }];
    else if (
      model.billingMode === "ratio" &&
      billing.baseInputUsd !== null &&
      billing.baseOutputUsd !== null
    )
      rows = [
        {
          name: "按量计费",
          rates: {
            p: billing.baseInputUsd,
            c: billing.baseOutputUsd,
            ...(billing.cacheReadUsd == null
              ? {}
              : { cr: billing.cacheReadUsd }),
            ...(billing.cacheWriteUsd == null
              ? {}
              : { cc: billing.cacheWriteUsd }),
          },
        },
      ];
  }
  return {
    rows,
    currency,
    multiplier,
    unit: model.billingMode === "per_request" ? "request" : "tokens",
  };
}

export interface HundredMillionTokenEstimate {
  currency: "CNY";
  official: { minimum: number; maximum: number };
  yeschoy: { minimum: number; maximum: number };
  cacheFallback: boolean;
  tiered: boolean;
  savingPercent: number | null;
}

// Product's fixed comparison policy, not a live exchange-rate service.
// Domestic models keep their existing CNY pricing basis.
export function billingConversion(model: AccountModel, fx: string) {
  const id = model.id.toLowerCase().split("/").at(-1) ?? "";
  const foreign = /^(?:gpt-|chatgpt-|o[134](?:-|$)|claude-|gemini-)/.test(id);
  return foreign
    ? { reference: 6.75, site: 1 }
    : { reference: Number(fx), site: Number(fx) };
}

// Token-weighted average of the four user-supplied billing samples (2026-09-14).
// Input includes cache: 650,414 input - 645,888 cached = 4,526 new input;
// 1,096 output. Normalize the 651,510 total tokens to 100M, without rounding.
// This is the supplied sample average, not a server-wide measured average.
const SAMPLE_TOTAL_TOKENS = 650_414 + 1_096;
const NEW_INPUT_MILLIONS = (4_526 / SAMPLE_TOTAL_TOKENS) * 100;
const CACHE_READ_MILLIONS = (645_888 / SAMPLE_TOTAL_TOKENS) * 100;
const OUTPUT_MILLIONS = (1_096 / SAMPLE_TOTAL_TOKENS) * 100;

// 服务端未提供 USD 基准价时，用 CNY 每百万价套用同一工作负载公式。
// 缓存命中价无独立口径，按输入价计（cacheFallback=true 会在 UI 标注）。
function perMillionCnyEstimate(
  model: AccountModel,
): HundredMillionTokenEstimate | null {
  if (!model.pricingAvailable) return null;
  // 空串是「未提供价格」的惯例表示；Number("") === 0 会被误判为免费价。
  const inputRaw = model.officialInputCnyPerMillion.trim();
  const outputRaw = model.officialOutputCnyPerMillion.trim();
  if (!inputRaw || !outputRaw) return null;
  const input = Number(inputRaw);
  const output = Number(outputRaw);
  if (!Number.isFinite(input) || input < 0) return null;
  if (!Number.isFinite(output) || output < 0) return null;
  const official =
    NEW_INPUT_MILLIONS * input +
    CACHE_READ_MILLIONS * input +
    OUTPUT_MILLIONS * output;
  return {
    currency: "CNY",
    official: { minimum: official, maximum: official },
    yeschoy: { minimum: official, maximum: official },
    cacheFallback: true,
    tiered: false,
    // 无分组倍率上下文，savingPercent 由调用处按 group.ratio 覆盖。
    savingPercent: null,
  };
}

export function hundredMillionTokenEstimate(
  model: AccountModel,
  group: BillingGroup,
  fx: string,
): HundredMillionTokenEstimate | null {
  if (group.ratio === null || !Number.isFinite(group.ratio) || group.ratio < 0)
    return null;
  const { rows, unit } = groupPrice(model, group, fx);
  if (unit !== "tokens") return null;
  // 服务端只给 CNY 每百万价（无 USD 基准）时，用同一工作负载公式估算，
  // 避免「费用参考」整块降级为不可用。
  if (rows.length === 0) {
    const estimate = perMillionCnyEstimate(model);
    if (estimate) {
      const conversion = billingConversion(model, fx);
      if (conversion.reference !== conversion.site) {
        const originalRate = Number(fx);
        if (!Number.isFinite(originalRate) || originalRate <= 0) return null;
        const base = estimate.official.minimum / originalRate;
        estimate.official = {
          minimum: base * conversion.reference,
          maximum: base * conversion.reference,
        };
        estimate.yeschoy = {
          minimum: base * conversion.site * group.ratio,
          maximum: base * conversion.site * group.ratio,
        };
        estimate.savingPercent =
          estimate.official.minimum > 0
            ? Math.max(
                0,
                (1 - estimate.yeschoy.minimum / estimate.official.minimum) *
                  100,
              )
            : null;
        return estimate;
      }
      estimate.yeschoy = {
        minimum: estimate.official.minimum * group.ratio,
        maximum: estimate.official.maximum * group.ratio,
      };
      estimate.savingPercent =
        group.ratio < 1 ? Math.max(0, (1 - group.ratio) * 100) : null;
    }
    return estimate;
  }
  const conversion = billingConversion(model, fx);
  if (
    ![conversion.reference, conversion.site].every(
      (n) => Number.isFinite(n) && n > 0,
    )
  )
    return null;

  let cacheFallback = false;
  const official = rows.flatMap((row) => {
    const input = row.rates.p;
    const output = row.rates.c;
    if (
      !Number.isFinite(input) ||
      input < 0 ||
      !Number.isFinite(output) ||
      output < 0
    )
      return [];
    const cacheRead = row.rates.cr;
    const cache =
      Number.isFinite(cacheRead) && cacheRead >= 0 ? cacheRead : input;
    if (cache === input && cacheRead === undefined) cacheFallback = true;
    return [
      conversion.reference *
        (NEW_INPUT_MILLIONS * input +
          CACHE_READ_MILLIONS * cache +
          OUTPUT_MILLIONS * output),
    ];
  });
  if (official.length !== rows.length || official.length === 0) return null;
  const actual = official.map(
    (amount) =>
      (amount / conversion.reference) * conversion.site * group.ratio!,
  );
  return {
    currency: "CNY",
    official: {
      minimum: Math.min(...official),
      maximum: Math.max(...official),
    },
    yeschoy: {
      minimum: Math.min(...actual),
      maximum: Math.max(...actual),
    },
    cacheFallback,
    tiered: rows.length > 1,
    savingPercent: official.some((amount) => amount > 0)
      ? Math.max(
          0,
          (1 - (conversion.site * group.ratio) / conversion.reference) * 100,
        )
      : null,
  };
}

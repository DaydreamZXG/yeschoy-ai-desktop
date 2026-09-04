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
          rates: { p: billing.baseInputUsd, c: billing.baseOutputUsd },
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

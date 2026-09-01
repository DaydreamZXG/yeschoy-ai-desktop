import { z } from "zod";

// Internal display evidence, not a deployed NewAPI API or an authentication proof.
// Only a separately verified authenticated producer may supply this in production.
const text = z
  .string()
  .min(1)
  .max(256)
  .regex(/^[^\u0000-\u001f\u007f-\u009f\u202a-\u202e\u2066-\u2069]+$/u);
const integer = z.string().regex(/^(0|[1-9][0-9]{0,29})$/);
const timestamp = z.number().int().nonnegative().max(8_640_000_000_000_000);
export const tokenKinds = [
  "input",
  "output",
  "cache_read",
  "cache_write_5m",
  "cache_write_1h",
] as const;
export type TokenKind = (typeof tokenKinds)[number];
export const exclusionReasons = [
  "missing_usage",
  "model_unverified",
  "unsupported_billing",
  "pending_settlement",
  "missing_price_history",
  "unsupported_funding",
] as const;
export type ExclusionReason = (typeof exclusionReasons)[number];

const usageSchema = z
  .object({
    inputTotal: integer,
    input: integer,
    output: integer,
    cache_read: integer,
    cache_write_5m: integer,
    cache_write_1h: integer,
  })
  .strict();
const lineSchema = z
  .object({
    kind: z.enum(tokenKinds),
    quantity: integer,
    // Rates are supplied by the authoritative pricing producer, never a client table.
    pricePerMillionMicros: integer,
    amountMicros: integer,
  })
  .strict();
const receiptSchema = z
  .object({
    id: text,
    modelId: text,
    officialModelId: text,
    occurredAtMs: timestamp,
    usageSource: z.literal("upstream_reported"),
    modelMatch: z.literal("verified"),
    billingDimensionsComplete: z.literal(true),
    funding: z.literal("wallet"),
    settledNetCnyMicros: integer,
    officialCurrency: z.enum(["USD", "CNY"]),
    officialTotalMicros: integer,
    priceVersion: text,
    priceEffectiveAtMs: timestamp,
    sourceCheckedAtMs: timestamp,
    officialSourceUrl: z
      .string()
      .url()
      .max(2048)
      .refine((value) => {
        try {
          const url = new URL(value);
          return url.protocol === "https:" && !url.username && !url.password;
        } catch {
          return false;
        }
      }),
    fxVersion: text,
    tierLabel: text,
    usage: usageSchema,
    lines: z.array(lineSchema).min(1).max(5),
  })
  .strict();
const excludedSchema = z
  .object({
    id: text,
    modelId: text,
    occurredAtMs: timestamp,
    reason: z.enum(exclusionReasons),
    settledNetCnyMicros: integer.nullable(),
  })
  .strict();
export const comparisonEvidenceSchema = z
  .object({
    schemaVersion: z.literal(1),
    calculationVersion: z.literal("normalized-token-lines-v1"),
    accountId: text,
    periodStartMs: timestamp,
    periodEndMs: timestamp,
    generatedAtMs: timestamp,
    expiresAtMs: timestamp,
    comparisonFx: z.literal("6.75"),
    rows: z.array(receiptSchema).max(1000),
    excluded: z.array(excludedSchema).max(1000),
  })
  .strict();
export type ComparisonEvidence = z.infer<typeof comparisonEvidenceSchema>;
export interface ComparisonContext {
  accountId: string;
  periodStartMs: number;
  periodEndMs: number;
  nowMs: number;
}
export interface VerifiedComparison {
  status: "ready" | "partial" | "not_comparable";
  evidence: ComparisonEvidence;
  officialCnyMicros: bigint;
  actualCnyMicros: bigint;
  differenceCnyMicros: bigint;
  percentTenths: bigint | null;
  excludedKnownCnyMicros: bigint;
  excludedUnknownCharges: number;
}
export type ComparisonResult =
  | VerifiedComparison
  | { status: "unavailable" | "invalid" | "expired" };

function roundPositive(numerator: bigint, denominator: bigint): bigint {
  return (numerator + denominator / 2n) / denominator;
}

export function verifyComparison(
  raw: unknown,
  context?: ComparisonContext,
): ComparisonResult {
  if (raw === undefined || !context) return { status: "unavailable" };
  const parsed = comparisonEvidenceSchema.safeParse(raw);
  if (!parsed.success) return { status: "invalid" };
  const evidence = parsed.data;
  if (
    !Number.isSafeInteger(context.nowMs) ||
    context.nowMs < 0 ||
    evidence.accountId !== context.accountId ||
    evidence.periodStartMs !== context.periodStartMs ||
    evidence.periodEndMs !== context.periodEndMs ||
    evidence.periodStartMs >= evidence.periodEndMs ||
    evidence.generatedAtMs < evidence.periodStartMs ||
    evidence.generatedAtMs > context.nowMs ||
    evidence.expiresAtMs <= evidence.generatedAtMs
  )
    return { status: "invalid" };
  if (context.nowMs >= evidence.expiresAtMs) return { status: "expired" };
  const ids = new Set<string>();
  for (const row of [...evidence.rows, ...evidence.excluded]) {
    if (
      ids.has(row.id) ||
      row.occurredAtMs < evidence.periodStartMs ||
      row.occurredAtMs >= evidence.periodEndMs ||
      row.occurredAtMs > evidence.generatedAtMs
    )
      return { status: "invalid" };
    ids.add(row.id);
  }
  let officialCnyMicros = 0n;
  let actualCnyMicros = 0n;
  for (const row of evidence.rows) {
    if (
      row.priceEffectiveAtMs > row.occurredAtMs ||
      row.sourceCheckedAtMs > evidence.generatedAtMs
    )
      return { status: "invalid" };
    const usage = row.usage;
    if (
      BigInt(usage.inputTotal) !==
      BigInt(usage.input) +
        BigInt(usage.cache_read) +
        BigInt(usage.cache_write_5m) +
        BigInt(usage.cache_write_1h)
    )
      return { status: "invalid" };
    const kinds = new Set<TokenKind>();
    let officialTotal = 0n;
    for (const line of row.lines) {
      if (kinds.has(line.kind) || line.quantity !== usage[line.kind])
        return { status: "invalid" };
      kinds.add(line.kind);
      const amount = roundPositive(
        BigInt(line.quantity) * BigInt(line.pricePerMillionMicros),
        1_000_000n,
      );
      if (amount !== BigInt(line.amountMicros)) return { status: "invalid" };
      officialTotal += amount;
    }
    if (
      tokenKinds.some((kind) => BigInt(usage[kind]) > 0n && !kinds.has(kind)) ||
      officialTotal !== BigInt(row.officialTotalMicros)
    )
      return { status: "invalid" };
    officialCnyMicros +=
      row.officialCurrency === "USD"
        ? roundPositive(officialTotal * 675n, 100n)
        : officialTotal;
    actualCnyMicros += BigInt(row.settledNetCnyMicros);
  }
  let excludedKnownCnyMicros = 0n;
  let excludedUnknownCharges = 0;
  for (const row of evidence.excluded) {
    if (row.reason === "pending_settlement" && row.settledNetCnyMicros !== null)
      return { status: "invalid" };
    if (row.reason !== "pending_settlement" && row.settledNetCnyMicros === null)
      return { status: "invalid" };
    if (row.settledNetCnyMicros === null) excludedUnknownCharges += 1;
    else excludedKnownCnyMicros += BigInt(row.settledNetCnyMicros);
  }
  const differenceCnyMicros = officialCnyMicros - actualCnyMicros;
  const absoluteDifference =
    differenceCnyMicros < 0n ? -differenceCnyMicros : differenceCnyMicros;
  // Tiny official totals render as <¥0.01; omit a potentially misleading ratio.
  const percentTenths =
    officialCnyMicros >= 10_000n
      ? roundPositive(absoluteDifference * 1000n, officialCnyMicros) *
        (differenceCnyMicros < 0n ? -1n : 1n)
      : null;
  return {
    status:
      evidence.rows.length === 0
        ? "not_comparable"
        : evidence.excluded.length
          ? "partial"
          : "ready",
    evidence,
    officialCnyMicros,
    actualCnyMicros,
    differenceCnyMicros,
    percentTenths,
    excludedKnownCnyMicros,
    excludedUnknownCharges,
  };
}

export function formatCnyMicros(value: bigint, locale: string): string {
  const negative = value < 0n;
  const absolute = negative ? -value : value;
  if (absolute > 0n && absolute < 10_000n)
    return `${negative ? "−" : ""}<¥0.01`;
  const cents = roundPositive(absolute, 10_000n);
  return `${negative ? "−" : ""}¥${new Intl.NumberFormat(locale, { maximumFractionDigits: 0 }).format(cents / 100n)}.${(cents % 100n).toString().padStart(2, "0")}`;
}

export function formatPercentTenths(value: bigint | null): string | null {
  if (value === null) return null;
  const absolute = value < 0n ? -value : value;
  return `${absolute / 10n}.${absolute % 10n}%`;
}

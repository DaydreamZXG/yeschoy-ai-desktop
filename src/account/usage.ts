// PRD 6.6 用量账单：/api/log/self 分页明细（30 天窗口）的前端类型、
// 解码守卫与工具×模型聚合。金额换算由原生侧完成（价格以 NewAPI 为准），
// 前端只聚合展示，不猜任何能力。

export interface UsageRecord {
  /** 客户端管理的工具 ID；空串表示无法归因（如网页直用）。 */
  toolId: string;
  modelId: string;
  observedAtEpochMs: number;
  promptTokens: number;
  completionTokens: number;
  cacheTokens: number;
  /** 展示货币金额；换算参数缺失时为空串（不显示金额，不显示 0）。 */
  amount: string;
}

export interface UsageLogReport {
  status: "available" | "empty" | "unavailable";
  reasonCode: string;
  recordCount: number;
  scannedCount: number;
  windowDays: number;
  truncated: boolean;
  oldestAtEpochMs: number;
  newestAtEpochMs: number;
  records: UsageRecord[];
}

/** 聚合行：工具 × 模型（PRD 6.6）。 */
export interface UsageAggregateRow {
  toolId: string;
  modelId: string;
  requests: number;
  promptTokens: number;
  completionTokens: number;
  cacheTokens: number;
  /** 合计金额（元）；任一记录金额缺失则为 null（不显示，不猜）。 */
  amount: number | null;
}

const MAX_RECORDS = 500;

function object(value: unknown): value is Record<string, unknown> {
  return !!value && typeof value === "object" && !Array.isArray(value);
}

function safeText(
  value: unknown,
  maximum: number,
  empty = true,
): value is string {
  return (
    typeof value === "string" &&
    (empty || value.length > 0) &&
    Array.from(value).length <= maximum &&
    !/[\u0000-\u001f\u007f-\u009f\u202a-\u202e\u2066-\u2069]/u.test(value)
  );
}

function nonNegativeInteger(value: unknown): value is number {
  return (
    typeof value === "number" &&
    Number.isSafeInteger(value) &&
    value >= 0
  );
}

function decimalText(value: unknown): value is string {
  return (
    typeof value === "string" &&
    (value === "" || /^\d+(\.\d+)?$/.test(value))
  );
}

export function isUsageLogReport(value: unknown): value is UsageLogReport {
  if (
    !object(value) ||
    !["available", "empty", "unavailable"].includes(value.status as string) ||
    !safeText(value.reasonCode, 80, false) ||
    !nonNegativeInteger(value.recordCount) ||
    !nonNegativeInteger(value.scannedCount) ||
    !nonNegativeInteger(value.windowDays) ||
    typeof value.truncated !== "boolean" ||
    !nonNegativeInteger(value.oldestAtEpochMs) ||
    !nonNegativeInteger(value.newestAtEpochMs) ||
    !Array.isArray(value.records) ||
    value.records.length > MAX_RECORDS ||
    value.records.length !== value.recordCount
  ) {
    return false;
  }
  return value.records.every(
    (row) =>
      object(row) &&
      safeText(row.toolId, 64) &&
      safeText(row.modelId, 200, false) &&
      nonNegativeInteger(row.observedAtEpochMs) &&
      row.observedAtEpochMs > 0 &&
      nonNegativeInteger(row.promptTokens) &&
      nonNegativeInteger(row.completionTokens) &&
      nonNegativeInteger(row.cacheTokens) &&
      decimalText(row.amount),
  );
}

/** 工具 × 模型聚合（金额用 EPSILON 容差累加，缺失即整体置 null）。 */
export function aggregateUsage(
  records: UsageRecord[],
): UsageAggregateRow[] {
  const rows = new Map<string, UsageAggregateRow>();
  for (const record of records) {
    const key = `${record.toolId}\u0000${record.modelId}`;
    const row = rows.get(key) ?? {
      toolId: record.toolId,
      modelId: record.modelId,
      requests: 0,
      promptTokens: 0,
      completionTokens: 0,
      cacheTokens: 0,
      amount: 0,
    };
    row.requests += 1;
    row.promptTokens += record.promptTokens;
    row.completionTokens += record.completionTokens;
    row.cacheTokens += record.cacheTokens;
    if (row.amount !== null) {
      row.amount =
        record.amount === ""
          ? null
          : row.amount + Number(record.amount);
    }
    rows.set(key, row);
  }
  // 金额容差修整：浮点累加误差按 EPSILON 收敛（沿用 savings 口径）。
  for (const row of rows.values()) {
    if (row.amount !== null) {
      const tolerance = Number.EPSILON * Math.max(1, row.amount) * 8;
      const rounded = Math.round(row.amount);
      row.amount =
        Math.abs(row.amount - rounded) <= tolerance
          ? rounded
          : Number(row.amount.toFixed(6));
    }
  }
  return [...rows.values()].sort(
    (a, b) =>
      b.requests - a.requests ||
      b.promptTokens - a.promptTokens ||
      a.modelId.localeCompare(b.modelId),
  );
}

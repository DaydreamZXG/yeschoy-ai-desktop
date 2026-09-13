// 客户端自治（#23）：30/90 天会话时效的本地记账。
// 本地记账不精确（不感知服务端真实会话状态），但对用户提示够用；
// 提示永远不阻断主流程。
export const SESSION_WARN_AFTER_DAYS = 25;
export const SESSION_REAUTH_AFTER_DAYS = 80;
export const DAY_MS = 24 * 60 * 60 * 1000;

const STORAGE_KEY = "yeschoy.account.sessionAge.v1";

export interface SessionAgeRecord {
  authorizedAtEpochMs: number;
  lastUsedAtEpochMs: number;
}

export type SessionAgeLevel = "fresh" | "expiring" | "reauth";

interface AgeStorage {
  getItem(key: string): string | null;
  setItem(key: string, value: string): void;
  removeItem(key: string): void;
}

function safeStorage(): AgeStorage | null {
  try {
    return window.localStorage;
  } catch {
    return null;
  }
}

export function readSessionAgeRecord(
  storage: AgeStorage | null = safeStorage(),
): SessionAgeRecord | null {
  if (!storage) return null;
  let raw: string | null = null;
  try {
    raw = storage.getItem(STORAGE_KEY);
  } catch {
    return null;
  }
  if (!raw) return null;
  try {
    const value = JSON.parse(raw) as unknown;
    if (
      !!value &&
      typeof value === "object" &&
      !Array.isArray(value) &&
      Number.isSafeInteger((value as SessionAgeRecord).authorizedAtEpochMs) &&
      Number.isSafeInteger((value as SessionAgeRecord).lastUsedAtEpochMs) &&
      (value as SessionAgeRecord).authorizedAtEpochMs > 0 &&
      (value as SessionAgeRecord).lastUsedAtEpochMs >=
        (value as SessionAgeRecord).authorizedAtEpochMs
    )
      return value as SessionAgeRecord;
    return null;
  } catch {
    return null;
  }
}

export function writeSessionAgeRecord(
  record: SessionAgeRecord,
  storage: AgeStorage | null = safeStorage(),
): void {
  if (!storage) return;
  try {
    storage.setItem(STORAGE_KEY, JSON.stringify(record));
  } catch {
    // 写入失败（隐私模式/存储满）时静默降级：提示能力缺失但不影响主流程。
  }
}

/** signed_in 观测到达时调用：首次记录授权时间，之后只滚动最近使用时间。 */
export function noteSessionActivity(
  nowEpochMs: number = Date.now(),
  storage: AgeStorage | null = safeStorage(),
): void {
  const previous = readSessionAgeRecord(storage);
  writeSessionAgeRecord(
    previous
      ? { ...previous, lastUsedAtEpochMs: nowEpochMs }
      : { authorizedAtEpochMs: nowEpochMs, lastUsedAtEpochMs: nowEpochMs },
    storage,
  );
}

/** 会话确认消失（signed_out / session_expired）时清除记账。 */
export function clearSessionAgeRecord(
  storage: AgeStorage | null = safeStorage(),
): void {
  if (!storage) return;
  try {
    storage.removeItem(STORAGE_KEY);
  } catch {
    // 同上，静默降级。
  }
}

/** 时效分级：>25 天提示可能即将过期；>80 天引导重新授权。取两口径中更严重者。 */
export function sessionAgeLevel(
  record: SessionAgeRecord | null,
  nowEpochMs: number,
): SessionAgeLevel | null {
  if (!record) return null;
  const daysSinceLastUse = (nowEpochMs - record.lastUsedAtEpochMs) / DAY_MS;
  const daysSinceAuth = (nowEpochMs - record.authorizedAtEpochMs) / DAY_MS;
  if (
    daysSinceAuth > SESSION_REAUTH_AFTER_DAYS ||
    daysSinceLastUse > SESSION_REAUTH_AFTER_DAYS
  )
    return "reauth";
  if (
    daysSinceAuth > SESSION_WARN_AFTER_DAYS ||
    daysSinceLastUse > SESSION_WARN_AFTER_DAYS
  )
    return "expiring";
  return "fresh";
}

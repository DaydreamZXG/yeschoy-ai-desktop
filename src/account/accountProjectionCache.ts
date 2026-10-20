/**
 * 上次成功登录的账户快照（不含令牌）。开机先用它画出首页，再后台检查
 * 中转站；站点慢或挂了也不会空等一分钟才显示。
 */
import type { AccountProjection } from "./session";

const STORAGE_KEY = "yeschoy.account.lastSignedInProjection.v1";
const MAX_AGE_MS = 30 * 24 * 60 * 60 * 1000;

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

function looksLikeSignedIn(value: unknown): value is AccountProjection {
  if (!value || typeof value !== "object" || Array.isArray(value)) return false;
  const row = value as AccountProjection;
  return (
    row.status === "signed_in" &&
    typeof row.requestId === "string" &&
    typeof row.reasonCode === "string" &&
    !!row.account &&
    typeof row.account === "object"
  );
}

export function readCachedSignedInProjection(
  storage: AgeStorage | null = safeStorage(),
  nowEpochMs: number = Date.now(),
): AccountProjection | null {
  if (!storage) return null;
  let raw: string | null = null;
  try {
    raw = storage.getItem(STORAGE_KEY);
  } catch {
    return null;
  }
  if (!raw) return null;
  try {
    const parsed = JSON.parse(raw) as unknown;
    if (!looksLikeSignedIn(parsed)) return null;
    const age = nowEpochMs - (parsed.observedAtEpochMs || 0);
    if (age < 0 || age > MAX_AGE_MS) return null;
    return parsed;
  } catch {
    return null;
  }
}

export function writeCachedSignedInProjection(
  projection: AccountProjection,
  storage: AgeStorage | null = safeStorage(),
): void {
  if (projection.status !== "signed_in" || !storage) return;
  try {
    storage.setItem(STORAGE_KEY, JSON.stringify(projection));
  } catch {
    // 隐私模式或磁盘满：没有缓存也不挡主流程。
  }
}

export function clearCachedSignedInProjection(
  storage: AgeStorage | null = safeStorage(),
): void {
  if (!storage) return;
  try {
    storage.removeItem(STORAGE_KEY);
  } catch {
    // ignore
  }
}

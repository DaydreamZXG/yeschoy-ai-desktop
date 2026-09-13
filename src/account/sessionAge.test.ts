import { afterEach, describe, expect, it } from "vitest";
import {
  clearSessionAgeRecord,
  DAY_MS,
  noteSessionActivity,
  readSessionAgeRecord,
  SESSION_REAUTH_AFTER_DAYS,
  SESSION_WARN_AFTER_DAYS,
  sessionAgeLevel,
  writeSessionAgeRecord,
  type SessionAgeRecord,
} from "./sessionAge";

function memoryStorage(): Storage {
  const map = new Map<string, string>();
  return {
    get length() {
      return map.size;
    },
    clear: () => map.clear(),
    getItem: (key) => map.get(key) ?? null,
    key: (index) => [...map.keys()][index] ?? null,
    removeItem: (key) => void map.delete(key),
    setItem: (key, value) => void map.set(key, value),
  };
}

afterEach(() => {
  window.localStorage.clear();
});

describe("sessionAgeLevel 时效分级边界（#23）", () => {
  const now = 1_000_000_000_000;
  const record = (authDaysAgo: number, lastUseDaysAgo: number) =>
    ({
      authorizedAtEpochMs: now - authDaysAgo * DAY_MS,
      lastUsedAtEpochMs: now - lastUseDaysAgo * DAY_MS,
    }) satisfies SessionAgeRecord;

  it("无记录返回 null（不提示）", () => {
    expect(sessionAgeLevel(null, now)).toBeNull();
  });

  it.each([
    [24, "fresh"],
    [25, "fresh"],
    [26, "expiring"],
  ] as const)(
    "距上次使用（及授权）%i 天 → %s（>25 触发过期提示）",
    (days, expected) => {
      expect(sessionAgeLevel(record(days, days), now)).toBe(expected);
    },
  );

  it.each([
    [79, "expiring"],
    [80, "expiring"],
    [81, "reauth"],
  ] as const)(
    "距授权 %i 天 → %s（>80 触发重新授权引导）",
    (days, expected) => {
      expect(sessionAgeLevel(record(days, 1), now)).toBe(expected);
    },
  );

  it("两口径取更严重者：上次使用 81 天 → reauth（即使授权很新）", () => {
    expect(sessionAgeLevel(record(1, 81), now)).toBe("reauth");
  });

  it("阈值常量与 PRD 口径一致（25/80）", () => {
    expect(SESSION_WARN_AFTER_DAYS).toBe(25);
    expect(SESSION_REAUTH_AFTER_DAYS).toBe(80);
  });
});

describe("sessionAge 本地记账", () => {
  it("noteSessionActivity 首次记录授权时间，之后只滚动最近使用时间", () => {
    const storage = memoryStorage();
    const t0 = 1_700_000_000_000;
    noteSessionActivity(t0, storage);
    expect(readSessionAgeRecord(storage)).toEqual({
      authorizedAtEpochMs: t0,
      lastUsedAtEpochMs: t0,
    });
    noteSessionActivity(t0 + 5 * DAY_MS, storage);
    expect(readSessionAgeRecord(storage)).toEqual({
      authorizedAtEpochMs: t0,
      lastUsedAtEpochMs: t0 + 5 * DAY_MS,
    });
  });

  it("clearSessionAgeRecord 清除后读取为 null", () => {
    const storage = memoryStorage();
    noteSessionActivity(1_700_000_000_000, storage);
    clearSessionAgeRecord(storage);
    expect(readSessionAgeRecord(storage)).toBeNull();
  });

  it("readSessionAgeRecord 拒绝不合法数据（不伪造、不抛错）", () => {
    const storage = memoryStorage();
    for (const bad of [
      "not json",
      "{}",
      "[]",
      JSON.stringify({ authorizedAtEpochMs: 100, lastUsedAtEpochMs: 50 }),
      JSON.stringify({ authorizedAtEpochMs: -1, lastUsedAtEpochMs: -1 }),
      JSON.stringify({ authorizedAtEpochMs: 1.5, lastUsedAtEpochMs: 2 }),
    ]) {
      storage.setItem("yeschoy.account.sessionAge.v1", bad);
      expect(readSessionAgeRecord(storage)).toBeNull();
    }
  });

  it("storage 抛错时静默降级，不影响主流程", () => {
    const throwing: Storage = {
      ...memoryStorage(),
      getItem: () => {
        throw new Error("blocked");
      },
      setItem: () => {
        throw new Error("blocked");
      },
      removeItem: () => {
        throw new Error("blocked");
      },
    };
    expect(readSessionAgeRecord(throwing)).toBeNull();
    expect(() => {
      noteSessionActivity(1, throwing);
      clearSessionAgeRecord(throwing);
    }).not.toThrow();
  });

  it("writeSessionAgeRecord 写入的记录可读回", () => {
    const storage = memoryStorage();
    const record: SessionAgeRecord = {
      authorizedAtEpochMs: 100,
      lastUsedAtEpochMs: 200,
    };
    writeSessionAgeRecord(record, storage);
    expect(readSessionAgeRecord(storage)).toEqual(record);
  });
});

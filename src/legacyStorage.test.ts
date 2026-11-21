import { describe, expect, it } from "vitest";

import { adoptRenamedStorage } from "./legacyStorage";

function memory(initial: Record<string, string>, full = false) {
  const data = new Map(Object.entries(initial));
  return {
    data,
    getItem: (key: string) => data.get(key) ?? null,
    setItem: (key: string, value: string) => {
      if (full) throw new Error("QuotaExceededError");
      data.set(key, value);
    },
    removeItem: (key: string) => void data.delete(key),
  };
}

describe("改名前存的小东西搬到新名字下面", () => {
  it("旧值搬过去、旧名字删掉，再跑一次什么都不变", () => {
    const storage = memory({
      "yeschoy-language": "en",
      "yeschoy.announcements.seen.v1": "[1,2]",
      unrelated: "keep",
    });
    adoptRenamedStorage(storage);
    adoptRenamedStorage(storage);
    expect(Object.fromEntries(storage.data)).toEqual({
      "kaifan.language": "en",
      "kaifan.announcements.seen.v1": "[1,2]",
      unrelated: "keep",
    });
  });

  it("新名字下已经有值：以新的为准，不被旧值盖掉", () => {
    const storage = memory({
      "yeschoy-language": "en",
      "kaifan.language": "zh",
    });
    adoptRenamedStorage(storage);
    expect(Object.fromEntries(storage.data)).toEqual({
      "kaifan.language": "zh",
    });
  });

  it("写不进去：旧值原样留着，下次再搬", () => {
    const storage = memory({ "yeschoy-language": "en" }, true);
    adoptRenamedStorage(storage);
    expect(Object.fromEntries(storage.data)).toEqual({
      "yeschoy-language": "en",
    });
  });
});

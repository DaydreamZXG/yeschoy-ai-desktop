import { describe, expect, it } from "vitest";
import {
  markFirstActivationCelebrated,
  shouldCelebrateFirstActivation,
  type MilestoneStorage,
} from "./firstActivationMilestone";

function memoryStorage(initial: Record<string, string> = {}): MilestoneStorage & {
  data: Record<string, string>;
} {
  const data: Record<string, string> = { ...initial };
  return {
    data,
    getItem: (key) => (key in data ? data[key] : null),
    setItem: (key, value) => {
      data[key] = value;
    },
  };
}

function throwingStorage(): MilestoneStorage {
  return {
    getItem: () => {
      throw new Error("denied");
    },
    setItem: () => {
      throw new Error("denied");
    },
  };
}

describe("firstActivationMilestone", () => {
  it("首次未庆祝时应触发", () => {
    expect(shouldCelebrateFirstActivation(memoryStorage())).toBe(true);
  });

  it("已庆祝后不再触发", () => {
    const storage = memoryStorage();
    markFirstActivationCelebrated(storage);
    expect(shouldCelebrateFirstActivation(storage)).toBe(false);
  });

  it("标记幂等：多次标记仍只算已庆祝", () => {
    const storage = memoryStorage();
    markFirstActivationCelebrated(storage);
    markFirstActivationCelebrated(storage);
    expect(shouldCelebrateFirstActivation(storage)).toBe(false);
    expect(storage.data["yeschoy.firstActivationCelebrated.v1"]).toBe("1");
  });

  it("读取异常时静默降级为不触发", () => {
    expect(shouldCelebrateFirstActivation(throwingStorage())).toBe(false);
  });

  it("存储不可用时静默降级为不触发", () => {
    expect(shouldCelebrateFirstActivation(null)).toBe(false);
  });

  it("写入异常不抛出", () => {
    expect(() => markFirstActivationCelebrated(throwingStorage())).not.toThrow();
    expect(() => markFirstActivationCelebrated(null)).not.toThrow();
  });

  it("真实 localStorage 往返", () => {
    window.localStorage.removeItem("yeschoy.firstActivationCelebrated.v1");
    expect(shouldCelebrateFirstActivation()).toBe(true);
    markFirstActivationCelebrated();
    expect(shouldCelebrateFirstActivation()).toBe(false);
    window.localStorage.removeItem("yeschoy.firstActivationCelebrated.v1");
  });
});

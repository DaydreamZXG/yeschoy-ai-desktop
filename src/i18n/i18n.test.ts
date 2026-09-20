import { describe, expect, it, beforeEach } from "vitest";

import en from "./locales/en.json";
import zh from "./locales/zh.json";
import {
  LANGUAGE_KEY,
  SUPPORTED_LANGUAGES,
  readLanguage,
  storeLanguage,
} from "./language";

function flatten(value: unknown, prefix = ""): Map<string, string> {
  const out = new Map<string, string>();
  if (typeof value !== "object" || value === null) return out;
  for (const [key, child] of Object.entries(value)) {
    const path = prefix ? `${prefix}.${key}` : key;
    if (typeof child === "string") out.set(path, child);
    else for (const [k, v] of flatten(child, path)) out.set(k, v);
  }
  return out;
}

describe("已注册的语言必须是完整的", () => {
  // 这条是「英文补齐了没有」的可验证定义。文案会从 copy.ts 一批批迁进来，
  // 每迁一批都必须同时补英文——漏了这条测试当场红，而不是等到有人
  // 切到英文才发现半屏中文。
  it("en 与 zh 的键完全一致", () => {
    const zhKeys = [...flatten(zh).keys()].sort();
    const enKeys = [...flatten(en).keys()].sort();
    expect(enKeys).toEqual(zhKeys);
  });

  it("英文里没有漏译的中文", () => {
    const leftovers = [...flatten(en)].filter(([, text]) =>
      /[一-龥]/.test(text),
    );
    expect(leftovers).toEqual([]);
  });

  it("插值占位符两边一一对应", () => {
    // `{{count}}` 这类占位符漏掉不会报错，只会在界面上显示空白或原样的
    // `{{count}}`——两边对齐才能保证翻译时没把它写丢。
    const placeholders = (text: string) =>
      (text.match(/\{\{\s*\w+\s*\}\}/g) ?? []).sort();
    const enMap = flatten(en);
    for (const [key, text] of flatten(zh)) {
      expect({ key, tokens: placeholders(enMap.get(key) ?? "") }).toEqual({
        key,
        tokens: placeholders(text),
      });
    }
  });
});

describe("语言偏好", () => {
  beforeEach(() => localStorage.clear());

  it("默认中文", () => {
    expect(readLanguage()).toBe("zh");
  });

  it("只接受已注册的语言，其余回落到中文", () => {
    for (const value of SUPPORTED_LANGUAGES) {
      storeLanguage(value);
      expect(readLanguage()).toBe(value);
    }
    // 日文与繁中的译文还在仓库里但**没有注册**（它们只覆盖既有的 268 键，
    // 迁进来的那批没有译文）。存进去也不该被接受，否则用户会得到一个
    // 切过去大半界面仍是中文的开关。
    for (const value of ["ja", "zh-TW", "", "klingon"]) {
      localStorage.setItem(LANGUAGE_KEY, value);
      expect(readLanguage()).toBe("zh");
    }
  });

  it("存储不可用时不抛异常", () => {
    const original = Object.getOwnPropertyDescriptor(
      window,
      "localStorage",
    ) as PropertyDescriptor;
    Object.defineProperty(window, "localStorage", {
      configurable: true,
      get() {
        throw new Error("storage blocked");
      },
    });
    expect(() => readLanguage()).not.toThrow();
    expect(readLanguage()).toBe("zh");
    expect(() => storeLanguage("en")).not.toThrow();
    Object.defineProperty(window, "localStorage", original);
  });
});

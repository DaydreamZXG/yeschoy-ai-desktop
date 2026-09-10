import { describe, expect, it } from "vitest";
import { modelMatchesQuery } from "./profile";

describe("model search normalization does not change model identity", () => {
  it.each(["gpt6", "gpt 6", "GPT-6 Astra", "ＧＰＴ　６", "gpt—6", "astra gpt"])(
    "matches the reviewed ID or label with query %s",
    (query) => expect(modelMatchesQuery("gpt-6-astra", query)).toBe(true),
  );
  it.each(["deepseek v4", "DeepSeek_V4_Flash", "claude 4.6", "gpt 5.6 sol"])(
    "allows separators without requiring exact API punctuation: %s",
    (query) => {
      const id = query.toLowerCase().startsWith("deepseek")
        ? "deepseek-v4-flash"
        : query.startsWith("claude")
          ? "claude-sonnet-4-6"
          : "gpt-5.6-sol";
      expect(modelMatchesQuery(id, query)).toBe(true);
    },
  );
  it("requires every search term and handles empty or punctuation-only input", () => {
    expect(modelMatchesQuery("gpt-6-astra", "gpt nonexistent")).toBe(false);
    expect(modelMatchesQuery("gpt-6-astra", "gpt5")).toBe(false);
    expect(modelMatchesQuery("gpt-6-astra", "   ")).toBe(true);
    expect(modelMatchesQuery("gpt-6-astra", "---")).toBe(false);
    expect(modelMatchesQuery("gpt-5.6-sol", "gpt 6")).toBe(false);
    expect(modelMatchesQuery("gpt-5.6-sol", "gpt 5")).toBe(true);
    expect(modelMatchesQuery("gpt-5.6-sol", "gpt 5.6")).toBe(true);
  });
  it("does not infer the friendly identity of an unknown prefixed model", () => {
    expect(modelMatchesQuery("org/gpt-6-astra", "org gpt6")).toBe(true);
    expect(modelMatchesQuery("org/model-x", "GPT 6")).toBe(false);
    expect(modelMatchesQuery("org/自定义模型", "自定义")).toBe(true);
  });
});

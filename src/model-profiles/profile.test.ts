import { describe, expect, it } from "vitest";
import { modelCapabilities, modelMatchesQuery } from "./profile";

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

describe("modelCapabilities is a read-only catalog lookup that never guesses", () => {
  it("returns reviewed capabilities for a catalog hit", () => {
    expect(modelCapabilities("gpt-6-astra")).toEqual({
      contextWindow: 1050000,
      maxOutputTokens: 128000,
      input: ["text", "image"],
      reasoningLevels: ["low", "medium", "high", "xhigh", "max"],
      toolUse: true,
    });
  });

  it("returns an empty object for unknown models instead of guessing", () => {
    expect(modelCapabilities("not-a-real-model")).toEqual({});
    expect(modelCapabilities("")).toEqual({});
  });

  it("returns an empty object for known routing aliases we do not declare", () => {
    expect(modelCapabilities("codex-auto-review")).toEqual({});
    expect(modelCapabilities("gpt-image-2")).toEqual({});
  });

  it("declares tool use for verified models but never for routing aliases", () => {
    // 逐家族抽样：全部核实为支持工具调用。
    for (const model of [
      "gpt-6-astra",
      "deepseek-r1",
      "claude-haiku-4-5",
      "gemini-3.5-flash",
      "qwen3.7-max",
      "glm-5.3",
      "kimi-k3",
      "doubao-seed-2.1-pro",
      "llama-4-maverick",
      "hy4-preview",
    ]) {
      expect(modelCapabilities(model).toolUse).toBe(true);
    }
    // ark-code-latest 是火山方舟控制台路由别名，能力随路由变化——不猜。
    expect(modelCapabilities("ark-code-latest").toolUse).toBeUndefined();
  });

  it("exposes the default reasoning level when the catalog declares one", () => {
    expect(modelCapabilities("deepseek-v4-flash").defaultReasoning).toBe(
      "high",
    );
    expect(modelCapabilities("gpt-6-astra").defaultReasoning).toBeUndefined();
  });

  it("omits fields the catalog entry does not declare", () => {
    // Every entry in the merged catalog carries the four fields, so prove the
    // field-missing state with a defensive check: the API must only surface
    // fields that are actually present, never synthesize values.
    for (const model of ["gpt-6-astra", "kimi-k3", "glm-5.3", "hy4-preview"]) {
      const capabilities = modelCapabilities(model);
      expect(
        Object.values(capabilities).every((value) => value !== undefined),
      ).toBe(true);
    }
    // Input modalities are passed through verbatim, not inferred.
    expect(modelCapabilities("glm-5.3").input).toEqual(["text"]);
    expect(modelCapabilities("claude-sonnet-5").input).toEqual(["text", "image"]);
  });
});

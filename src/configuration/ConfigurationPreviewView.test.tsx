import { describe, expect, it } from "vitest";
import { modelSupportsTool } from "./modelCompatibility";

describe("Codex protocol compatibility", () => {
  it("accepts both direct Responses and automatically bridged Chat models", () => {
    expect(modelSupportsTool("codex_desktop", ["openai-response"])).toBe(true);
    expect(modelSupportsTool("codex_desktop", ["openai"])).toBe(true);
    expect(
      modelSupportsTool("codex_desktop", ["openai", "openai-response"]),
    ).toBe(true);
    expect(modelSupportsTool("codex_desktop", ["anthropic"])).toBe(false);
  });

  it("keeps the other tool protocol boundaries unchanged", () => {
    expect(modelSupportsTool("claude_desktop", ["anthropic"])).toBe(true);
    expect(modelSupportsTool("pi", ["openai"])).toBe(true);
    expect(modelSupportsTool("pi", ["openai-response"])).toBe(false);
  });
});

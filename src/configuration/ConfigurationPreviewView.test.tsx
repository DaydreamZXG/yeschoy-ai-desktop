import { describe, expect, it } from "vitest";
import { modelConnectionMode, modelSupportsTool } from "./modelCompatibility";
import { recoveryRetryMessage } from "./ConfigurationPreviewView";

describe("desktop protocol compatibility", () => {
  it("ru054 retry recovery treats restore as fallback instead of a required first step", () => {
    expect(recoveryRetryMessage("recovery_pending")).toContain("直接重试");
    expect(recoveryRetryMessage("recovery_pending")).toContain(
      "本次没有修改应用",
    );
    expect(recoveryRetryMessage("configuration_rollback_failed")).toContain(
      "自动恢复",
    );
    expect(recoveryRetryMessage("credential_restore_failed")).toContain(
      "系统钥匙串或凭据管理器",
    );
    expect(recoveryRetryMessage("other")).toBeUndefined();
  });

  it("accepts both direct Responses and automatically bridged Chat models", () => {
    expect(modelSupportsTool("codex_desktop", ["openai-response"])).toBe(true);
    expect(modelSupportsTool("codex_desktop", ["openai"])).toBe(true);
    expect(
      modelSupportsTool("codex_desktop", ["openai", "openai-response"]),
    ).toBe(true);
    expect(modelSupportsTool("codex_desktop", ["anthropic"])).toBe(false);
  });

  it("accepts native Anthropic and automatically bridged Chat for both Claude targets", () => {
    expect(modelSupportsTool("claude_desktop", ["anthropic"])).toBe(true);
    expect(modelSupportsTool("claude_desktop", ["openai"])).toBe(true);
    expect(modelSupportsTool("claude_code", ["anthropic", "openai"])).toBe(
      true,
    );
    expect(modelConnectionMode("claude_desktop", ["anthropic", "openai"])).toBe(
      "direct",
    );
    expect(modelConnectionMode("claude_code", ["openai"])).toBe("bridge");
    expect(modelSupportsTool("claude_code", ["openai-response"])).toBe(false);
  });

  it("keeps the non-Claude tool protocol boundaries unchanged", () => {
    expect(modelSupportsTool("pi", ["openai"])).toBe(true);
    expect(modelSupportsTool("pi", ["openai-response"])).toBe(false);
  });
});

import { describe, expect, it } from "vitest";
import {
  modelConnectionMode,
  modelSupportsTool,
  toolsSupportingModel,
} from "./modelCompatibility";
import {
  connectionLifecycleMode,
  connectionLifecycleNote,
  recoveryRetryMessage,
  runningAppHandoff,
} from "./ConfigurationPreviewView";

describe("desktop protocol compatibility", () => {
  it("ru068 gives every supported surface an honest lifecycle instead of applying the desktop restart rule to every agent", () => {
    expect(connectionLifecycleMode("codex_desktop")).toBe(
      "graceful_desktop_restart",
    );
    expect(connectionLifecycleMode("claude_desktop")).toBe(
      "graceful_desktop_restart",
    );
    for (const tool of ["claude_code", "pi"] as const) {
      expect(connectionLifecycleMode(tool)).toBe("new_terminal_session");
      expect(connectionLifecycleNote(tool, tool)).toContain(
        "不会关闭正在使用的命令行会话",
      );
    }
    expect(connectionLifecycleMode("dsh_web")).toBe("browser_launch");
    expect(connectionLifecycleNote("dsh_web", "DSH web")).toContain(
      "在浏览器中打开",
    );
    expect(connectionLifecycleMode("workbuddy")).toBe("hot_reload_desktop");
    expect(connectionLifecycleNote("workbuddy", "WorkBuddy")).toContain(
      "不需要关闭或重启",
    );
    const desktop = connectionLifecycleNote("codex_desktop", "Codex Desktop");
    expect(desktop).toContain("提醒你保存");
    expect(desktop).toContain("正常退出");
    expect(desktop).toContain("只剩后台进程");
    expect(desktop).toContain("安装路径对应的进程");
  });

  it("ru068 dismisses running-app consent before continuing and rejects stale consent", () => {
    expect(runningAppHandoff("same-selection", "same-selection")).toEqual({
      nextPromptContext: null,
      restart: true,
    });
    expect(runningAppHandoff("old-selection", "new-selection")).toEqual({
      nextPromptContext: null,
      restart: false,
    });
    expect(runningAppHandoff(null, "same-selection")).toEqual({
      nextPromptContext: null,
      restart: false,
    });
  });

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

  it("names the apps a Codex-incompatible model can still be used in", () => {
    // The relay's own pricing declares Chat-only channels, so DeepSeek-class
    // models cannot go into Codex. Dropping them from the picker told the user
    // nothing; this is what replaces the silence. A beginner does not know what
    // a protocol is, but does know what "用 Claude Code" means.
    const chatOnly = ["openai", "anthropic"];
    expect(toolsSupportingModel(chatOnly, "codex_desktop")).toEqual([
      "claude_code",
      "claude_desktop",
      "pi",
      "dsh_web",
      "workbuddy",
    ]);
    // The app being configured is never offered as somewhere else to go.
    expect(toolsSupportingModel(chatOnly, "pi")).not.toContain("pi");
    // A model nothing can run yields an empty list, so the copy falls back to
    // saying only that this app cannot use it rather than naming nowhere.
    expect(toolsSupportingModel([], "codex_desktop")).toEqual([]);
    expect(toolsSupportingModel(["openai-response"], "codex_desktop")).toEqual([
      "claude_code",
      "claude_desktop",
      "pi",
      "dsh_web",
      "workbuddy",
    ]);
  });

  it("accepts only verified Responses models for Codex", () => {
    expect(modelSupportsTool("codex_desktop", ["openai-response"])).toBe(true);
    expect(modelSupportsTool("codex_desktop", ["openai"])).toBe(false);
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
    expect(modelSupportsTool("claude_code", ["openai-response"])).toBe(true);
  });

  it("lets Chat Completions apps use models that only advertise Responses", () => {
    expect(modelSupportsTool("pi", ["openai"])).toBe(true);
    expect(modelSupportsTool("pi", ["openai-response"])).toBe(true);
    expect(modelSupportsTool("workbuddy", ["openai"])).toBe(true);
    expect(modelSupportsTool("workbuddy", ["openai-response"])).toBe(true);
    expect(modelSupportsTool("dsh_web", ["openai-response"])).toBe(true);
    expect(modelSupportsTool("workbuddy", ["anthropic"])).toBe(false);
  });
});

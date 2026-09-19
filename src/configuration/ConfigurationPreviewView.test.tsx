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

  it("greys a model only when it genuinely cannot hold a conversation", () => {
    // 线上 /api/pricing 里 supported_endpoint_types 是按**模型**汇总的，
    // 是所有渠道能力的并集，不分渠道。所以 openai 与 openai-response 的差别
    // 不代表能不能用 —— 按它置灰，灰掉的是能用的模型（WorkBuddy 上就发生过：
    // deepseek-v4-flash 明明能跑却是灰的）。
    for (const tool of ["workbuddy", "pi", "dsh_web"] as const) {
      expect(modelSupportsTool(tool, ["openai", "anthropic"])).toBe(true);
      expect(modelSupportsTool(tool, ["openai-response"])).toBe(true);
      // anthropic-only 不放行：今天没有这种模型，而中转会不会为它提供
      // chat 端点我们没有证据。按知道的事实判，不按猜测判。
      expect(modelSupportsTool(tool, ["anthropic"])).toBe(false);
      // 真不能用的：只会出图，和什么都没声明的。
      expect(modelSupportsTool(tool, ["image-generation"])).toBe(false);
      expect(modelSupportsTool(tool, [])).toBe(false);
    }
    // Claude 两端同样只在无法对话时置灰，但会区分直连和走本机桥。
    expect(modelConnectionMode("claude_code", ["anthropic"])).toBe("direct");
    expect(modelConnectionMode("claude_code", ["openai"])).toBe("bridge");
    expect(modelConnectionMode("claude_code", ["openai-response"])).toBe("bridge");
    expect(modelConnectionMode("claude_desktop", ["image-generation"])).toBeNull();
    // Codex 是唯一的例外：wire_api 只接受 responses，配置层面无解，
    // 所以这里必须看具体端点，能对话也不够。
    expect(modelSupportsTool("codex_desktop", ["openai", "anthropic"])).toBe(false);
    expect(modelSupportsTool("codex_desktop", ["openai-response"])).toBe(true);
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

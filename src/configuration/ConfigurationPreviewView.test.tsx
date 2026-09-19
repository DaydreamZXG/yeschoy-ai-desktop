import { describe, expect, it } from "vitest";
import { ACTIVATION_TOOL_IDS } from "./activation";
import { modelConnectionMode } from "./modelCompatibility";
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

  it("labels the connection without ever refusing a model", () => {
    // 闸门撤掉了（见 modelCompatibility.ts 顶部）。这里剩下的只有界面上
    // 那句「原生接口 / 自动兼容」，两个取值都代表能用。
    expect(modelConnectionMode("claude_code", ["anthropic"])).toBe("direct");
    expect(modelConnectionMode("claude_code", ["openai"])).toBe("bridge");
    expect(modelConnectionMode("codex_desktop", ["openai-response"])).toBe(
      "direct",
    );
    expect(modelConnectionMode("codex_desktop", ["anthropic"])).toBe("bridge");
    expect(modelConnectionMode("workbuddy", ["openai"])).toBe("direct");
    // 没有声明、空声明、甚至只会出图的，都不再被判死 —— 一律给个标签放行。
    for (const tool of ACTIVATION_TOOL_IDS) {
      expect(modelConnectionMode(tool, undefined)).toBeTruthy();
      expect(modelConnectionMode(tool, [])).toBeTruthy();
      expect(modelConnectionMode(tool, ["image-generation"])).toBeTruthy();
    }
  });
});

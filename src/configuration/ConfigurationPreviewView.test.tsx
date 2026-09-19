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
    // 这条规则来自 new-api 源码，不是从 supported_endpoint_types 推的：
    // 那个字段由 common/endpoint_type.go 按**渠道类型**算出，描述上游原生说
    // 哪种协议，而且在 relay/ 整条请求路径下一次都没被引用 —— 中转不拿它拦
    // 请求，每个 adaptor 都实现了四个入口格式的真转换。
    for (const tool of ["workbuddy", "pi", "dsh_web"] as const) {
      expect(modelSupportsTool(tool, ["openai", "anthropic"])).toBe(true);
      expect(modelSupportsTool(tool, ["openai-response"])).toBe(true);
      // Chat 打到 Anthropic 渠道，claude adaptor 的 ConvertOpenAIRequest 会
      // 转成 Messages。以前这里判 false，是没证据时的保守猜测，现在有证据了。
      expect(modelSupportsTool(tool, ["anthropic"])).toBe(true);
      expect(modelSupportsTool(tool, ["gemini"])).toBe(true);
      // 真不能用的：只会出图的，和什么都没声明的（线上是 codexpro/）。
      expect(modelSupportsTool(tool, ["image-generation"])).toBe(false);
      expect(modelSupportsTool(tool, [])).toBe(false);
    }
    // Claude 两端同样只在无法对话时置灰，但会区分直连和走本机桥。
    expect(modelConnectionMode("claude_code", ["anthropic"])).toBe("direct");
    expect(modelConnectionMode("claude_code", ["openai"])).toBe("bridge");
    expect(modelConnectionMode("claude_code", ["openai-response"])).toBe("bridge");
    expect(modelConnectionMode("claude_desktop", ["image-generation"])).toBeNull();
  });

  it("names the apps a Codex-incompatible model can still be used in", () => {
    // 接入页以前是把不兼容的模型直接过滤掉的，用户只会以为野菜没这个模型。
    // 现在灰着显示并点名去哪用。一个不懂协议的人不知道什么是 Responses，
    // 但知道「用 Claude Code」是什么意思。
    const responsesUnreachable = ["openai", "anthropic"];
    expect(toolsSupportingModel(responsesUnreachable, "codex_desktop")).toEqual([
      "claude_code",
      "claude_desktop",
      "pi",
      "dsh_web",
      "workbuddy",
    ]);
    // 正在配置的这个应用，永远不会被列进「去别处用」。
    expect(toolsSupportingModel(responsesUnreachable, "pi")).not.toContain("pi");
    // 哪都跑不了的模型给出空列表，文案退回到只说这个应用用不了。
    expect(toolsSupportingModel([], "codex_desktop")).toEqual([]);
    expect(toolsSupportingModel(["image-generation"], "workbuddy")).toEqual([]);
  });

  it("greys a model for Codex only when no conversion can carry Responses", () => {
    // 原生说 Responses 的，直通。
    expect(modelSupportsTool("codex_desktop", ["openai-response"])).toBe(true);
    expect(
      modelSupportsTool("codex_desktop", ["openai", "openai-response"]),
    ).toBe(true);
    // Gemini 渠道的 adaptor 把 Responses 转成 generateContent，有真实现。
    expect(modelSupportsTool("codex_desktop", ["gemini", "openai"])).toBe(true);
    // Anthropic 渠道不行：野菜部署的那版 claude adaptor 的
    // ConvertOpenAIResponsesRequest 是 `not implemented`。上游 main 后来补上
    // 了，但按部署的版本判 —— 线上 qwen3.8-max、mimo-v2.5 这类
    // [anthropic, openai] 模型在 Codex 里确实用不了。
    expect(modelSupportsTool("codex_desktop", ["anthropic"])).toBe(false);
    expect(modelSupportsTool("codex_desktop", ["openai", "anthropic"])).toBe(
      false,
    );
    // 光一个 openai 是唯一没有兜底的：openai adaptor 对 Responses 是原样透传，
    // 上游那个兼容网关有没有 /v1/responses 我们不知道。线上的三个 gemini-3.x
    // 就在这一格。
    expect(modelSupportsTool("codex_desktop", ["openai"])).toBe(false);
    // 不能对话的，Codex 一样灰。
    expect(modelSupportsTool("codex_desktop", ["image-generation"])).toBe(false);
    expect(modelSupportsTool("codex_desktop", [])).toBe(false);
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
  });
});

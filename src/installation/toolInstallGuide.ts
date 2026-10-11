import type { ActivationToolId } from "../configuration/activation";

/** Official install URLs — keep in sync with `catalog::guide` in Rust. */
export const TOOL_OFFICIAL_GUIDE_URL: Partial<
  Record<ActivationToolId, string>
> = {
  claude_code: "https://code.claude.com/docs/en/setup",
  claude_desktop: "https://claude.com/download",
  codex_desktop: "https://learn.chatgpt.com/docs/app",
  pi: "https://github.com/earendil-works/pi/tree/main/packages/coding-agent",
  dsh_web: "https://github.com/deepseek-ai/deepseek-harness",
  dsh_desktop: "https://www.deepseek.com/harness/",
  workbuddy: "https://www.workbuddy.cn/",
};

/** i18n keys under `installation.guidedSteps.<toolId>` (string arrays). */
export const GUIDED_INSTALL_TOOLS: ActivationToolId[] = [
  "claude_code",
  "pi",
  "dsh_web",
  "dsh_desktop",
  "workbuddy",
];

export function guidedInstallStepKey(tool: ActivationToolId): string | null {
  return GUIDED_INSTALL_TOOLS.includes(tool)
    ? `installation.guidedSteps.${tool}`
    : null;
}

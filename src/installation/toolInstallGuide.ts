import type { ActivationToolId } from "../configuration/activation";

/**
 * Official download / installer hubs — keep in sync with `catalog::guide_for`
 * in Rust. Prefer package download pages over documentation.
 * Platform-specific direct packages are resolved in Rust when help is opened.
 */
export const TOOL_OFFICIAL_GUIDE_URL: Partial<
  Record<ActivationToolId, string>
> = {
  claude_code: "https://claude.com/download",
  claude_desktop: "https://claude.com/download",
  codex_desktop: "https://chatgpt.com/codex",
  pi: "https://github.com/earendil-works/pi/releases/latest",
  opencode: "https://opencode.ai/download",
  dsh_web: "https://github.com/deepseek-ai/deepseek-harness",
  dsh_desktop: "https://www.deepseek.com/harness/",
  workbuddy: "https://www.workbuddy.cn/",
};

/** i18n keys under `installation.guidedSteps.<toolId>` (string arrays). */
export const GUIDED_INSTALL_TOOLS: ActivationToolId[] = [
  "claude_code",
  "pi",
  "opencode",
  "dsh_web",
  "dsh_desktop",
  "workbuddy",
];

export function guidedInstallStepKey(tool: ActivationToolId): string | null {
  return GUIDED_INSTALL_TOOLS.includes(tool)
    ? `installation.guidedSteps.${tool}`
    : null;
}

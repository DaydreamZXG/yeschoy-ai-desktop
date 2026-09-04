import type { ActivationToolId } from "./activation";

export function modelSupportsTool(
  toolId: ActivationToolId,
  endpoints: string[],
): boolean {
  if (toolId === "codex_desktop") {
    const codexCompatible =
      endpoints.includes("openai-response") || endpoints.includes("openai");
    return codexCompatible;
  }
  if (toolId === "claude_code" || toolId === "claude_desktop")
    return endpoints.includes("anthropic");
  return endpoints.includes("openai");
}

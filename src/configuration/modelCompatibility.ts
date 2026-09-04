import type { ActivationToolId } from "./activation";

export type ModelConnectionMode = "direct" | "bridge";

export function modelConnectionMode(
  toolId: ActivationToolId,
  endpoints: string[],
): ModelConnectionMode | null {
  if (toolId === "codex_desktop") {
    if (endpoints.includes("openai-response")) return "direct";
    if (endpoints.includes("openai")) return "bridge";
    return null;
  }
  if (toolId === "claude_code" || toolId === "claude_desktop") {
    if (endpoints.includes("anthropic")) return "direct";
    if (endpoints.includes("openai")) return "bridge";
    return null;
  }
  return endpoints.includes("openai") ? "direct" : null;
}

export function modelSupportsTool(
  toolId: ActivationToolId,
  endpoints: string[],
): boolean {
  return modelConnectionMode(toolId, endpoints) !== null;
}

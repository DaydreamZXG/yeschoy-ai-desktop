import { ACTIVATION_TOOL_IDS, type ActivationToolId } from "./activation";

export type ModelConnectionMode = "direct" | "bridge";

export function modelConnectionMode(
  toolId: ActivationToolId,
  endpoints: string[],
): ModelConnectionMode | null {
  if (toolId === "codex_desktop") {
    if (endpoints.includes("openai-response")) return "direct";
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

/**
 * 能用这个模型的其他应用。
 *
 * 之前接入页是把不兼容的模型**直接过滤掉**的：用户选了 Codex，DeepSeek 就
 * 从列表里消失，没有任何解释。对一个专门服务「不会配置的人」的产品，这是
 * 最糟的表现 —— 他不会想到是协议不匹配，只会以为野菜没有这个模型。
 *
 * 所以改成灰着显示 + 告诉他去哪用。这个函数算出「哪去」。
 */
export function toolsSupportingModel(
  endpoints: string[],
  except: ActivationToolId,
): ActivationToolId[] {
  return ACTIVATION_TOOL_IDS.filter(
    (toolId) => toolId !== except && modelSupportsTool(toolId, endpoints),
  );
}

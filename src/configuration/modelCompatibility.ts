import { ACTIVATION_TOOL_IDS, type ActivationToolId } from "./activation";

export type ModelConnectionMode = "direct" | "bridge";

/** OpenAI 形状的对话端点。
 *
 * `openai` 与 `openai-response` 的区别**不是**能不能用的区别：中转按**模型**
 * 汇总 supported_endpoint_types，它是所有渠道能力的并集，不分渠道，而同一个
 * origin 对已上架的模型仍然接受 /v1/chat/completions。按这个区别置灰，灰掉的
 * 是能用的模型 —— WorkBuddy 上就发生过，deepseek-v4-flash 明明能跑却是灰的。
 *
 * `image-generation` 和空声明不在其列，那两种是**真的**不能对话：
 * gpt-image-1.5 / gpt-image-2 / codexpro/。那才是该置灰的情况。
 *
 * `anthropic` 也刻意不在其列。今天没有任何模型只声明它，所以放行它买不到任何
 * 东西；而我们并没有证据说明中转会为一个纯 Anthropic 模型提供 chat 端点。
 * 没有证据就不放行 —— 这跟「只在真不能用时置灰」不矛盾，是同一条原则：
 * 按知道的事实判，不按猜测判。
 */
const CHAT_ENDPOINTS = ["openai", "openai-response"];

function speaksChat(endpoints: string[]): boolean {
  return endpoints.some((endpoint) => CHAT_ENDPOINTS.includes(endpoint));
}

export function modelConnectionMode(
  toolId: ActivationToolId,
  endpoints: string[],
): ModelConnectionMode | null {
  // Codex 是唯一需要精确闸门的：它只发 Responses，而官方 config 文档写明
  // `wire_api` 只接受 "responses"，配置层面没有退路。这是协议硬限制，
  // 不是渠道差异 —— 所以这里必须看具体端点，不能只看「能不能对话」。
  if (toolId === "codex_desktop") {
    return endpoints.includes("openai-response") ? "direct" : null;
  }
  // Claude 客户端说 Anthropic 协议：原生声明就直连，其余经本机桥转换。
  if (toolId === "claude_code" || toolId === "claude_desktop") {
    if (endpoints.includes("anthropic")) return "direct";
    return speaksChat(endpoints) ? "bridge" : null;
  }
  // WorkBuddy / Pi / DSH 说 Chat Completions，直连中转。
  return speaksChat(endpoints) ? "direct" : null;
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

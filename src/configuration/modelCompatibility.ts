import { ACTIVATION_TOOL_IDS, type ActivationToolId } from "./activation";

/**
 * `direct` = 应用说的协议就是这个模型上游原生说的协议，界面上叫「原生接口」。
 * `bridge` = 路上有一层转换，界面上叫「自动兼容」。
 *
 * 注意 `bridge` **不等于**「走本机回环桥」：本机桥只改写模型名和 1M 上下文，
 * 不做协议转换，协议转换一律在中转侧。Codex 从不经本机桥，但它用
 * Anthropic 渠道的模型时中转要转，所以那也是 `bridge`。
 */
export type ModelConnectionMode = "direct" | "bridge";

/**
 * 能不能用，取决于**中转会不会转**，不取决于模型声明了哪个端点。
 *
 * 这条规则以前是猜的，现在是查过 new-api 源码的（QuantumNous/new-api@main，
 * 就是野菜在跑的那套）：
 *
 * 1. `supported_endpoint_types` 是 `common/endpoint_type.go` 的
 *    `GetEndpointTypesByChannelType` 按**渠道类型**算出来的，描述的是上游渠道
 *    原生说哪种协议。Anthropic 渠道得到 `[anthropic, openai]`，Gemini 渠道得到
 *    `[gemini, openai]`，其余普通渠道一律 `[openai]`。
 * 2. 它在 `relay/` 整条请求路径下**一次都没被引用**，只有 `controller/model.go`
 *    和 `controller/model_meta.go` 拿去展示。中转不拿它拦请求。
 * 3. `relay/channel/adapter.go` 的 Adaptor 接口强制每个渠道都实现四个入口转换
 *    （`ConvertOpenAIRequest` / `ConvertClaudeRequest` / `ConvertGeminiRequest` /
 *    `ConvertOpenAIResponsesRequest`），而且都是真实现。OpenAI 渠道上那句
 *    「只有 claude 模型才让转」的守卫是被注释掉的
 *    （`relay/channel/openai/adaptor.go:63`）。
 *
 * 所以：**协议不是能不能用的理由**，中转会替我们转。真不能用的只有两种——
 * 只会出图的（`image-generation`）和什么都没声明的（`codexpro/`）。
 */
const CONVERSATIONAL_ENDPOINTS = [
  "openai",
  "openai-response",
  "anthropic",
  "gemini",
];

function canConverse(endpoints: string[]): boolean {
  return endpoints.some((endpoint) =>
    CONVERSATIONAL_ENDPOINTS.includes(endpoint),
  );
}

export function modelConnectionMode(
  toolId: ActivationToolId,
  endpoints: string[],
): ModelConnectionMode | null {
  if (!canConverse(endpoints)) return null;

  // Codex 只发 Responses（官方 config 文档写明 `wire_api` 只接受 "responses"，
  // 配置层面没有退路），而 Responses 是四个入口里唯一有**真空档**的：
  //
  //   - `openai-response`：上游原生就说 Responses，直通。
  //   - `anthropic`：claude adaptor 把 Responses 转成 Messages，并且
  //     `GetRequestURL` 恒定打 `/v1/messages`，转换是落地的。
  //   - `gemini`：gemini adaptor 同理，转成 generateContent。
  //   - 光一个 `openai`：openai adaptor 的 `ConvertOpenAIResponsesRequest`
  //     是**原样透传**，`GetRequestURL` 照样打上游的 `/v1/responses`
  //     —— 上游那个 OpenAI 兼容网关有没有这条路，中转不知道，我们也不知道。
  //
  // 所以 Codex 这里灰掉的不是「协议不匹配」，是「没有任何一段转换能兜底」。
  if (toolId === "codex_desktop") {
    if (endpoints.includes("openai-response")) return "direct";
    if (endpoints.includes("anthropic") || endpoints.includes("gemini")) {
      return "bridge";
    }
    return null;
  }

  // Claude 两端说 Anthropic 协议。上游原生说 Anthropic 的是直连；其余的照样
  // 发 `/v1/messages`，由中转转成上游格式，所以算一层转换。两种情况都会经过
  // 本机桥（桥要改写模型名和 1M 上下文），本机桥本身不碰协议。
  if (toolId === "claude_code" || toolId === "claude_desktop") {
    return endpoints.includes("anthropic") ? "direct" : "bridge";
  }

  // WorkBuddy / Pi / DSH 说 Chat Completions，直连中转。任何渠道类型的
  // adaptor 都实现了 `ConvertOpenAIRequest`，所以没有需要拦的情况。
  return "direct";
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

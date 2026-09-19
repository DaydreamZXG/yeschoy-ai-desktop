/**
 * 每个模型都能选。这里**不再**判断能不能用。
 *
 * 曾经这里是一道闸门，按 `supported_endpoint_types` 把「不兼容」的模型置灰。
 * 它被撤掉了，因为它一次也没判对过，而且每次判错都是同一个方向 —— 把我们
 * 不知道的事情当成不行：
 *
 *   1. 先是按 `openai` / `openai-response` 的差别置灰。这两个的区别根本
 *      不是能不能用，WorkBuddy 上明明能跑的模型被灰掉了。
 *   2. 改成「没有价目行就不能用」。`/api/pricing` 只覆盖它定过价的模型，
 *      账号可用列表里还有别的（deepseek-v4-flash），于是能跑的模型被挂上
 *      「哪个应用都用不了」这句最重的话。
 *   3. 再改成「端点列表为空就不能用」。中转查不到模型时返回的也是空切片，
 *      和「真的零端点」是同一个值，分不开。
 *
 * 根子上的问题是：**我们没有可靠的信号。** 中转不按这个字段拦请求（它在
 * `relay/` 全路径下零引用，只被两个展示用的 controller 读），每个渠道
 * adaptor 都实现了四种入口格式的互转，而中转上新模型永远比它补元数据快。
 * 在这种情况下继续猜，只会随着时间推移灰掉越来越多能用的模型。
 *
 * 所以现在：选择器列出账户的全部模型，一个不藏、一个不灰。真的不能用的
 * 模型，用户会从中转那边收到一条明确的报错 —— 那比我们凭一个不可靠的字段
 * 提前替他判死要诚实。
 *
 * ── 要重新开闸的话，下面是查证过的事实（针对 github.com/yeschoy/new-api，
 *    也就是野菜实际部署的那个 fork，不是上游 main，两者不一样）：
 *
 *    · `supported_endpoint_types` 由 `common/endpoint_type.go` 按**渠道类型**
 *      算出，描述上游原生说哪种协议，不是准入白名单。
 *    · `/v1/messages` 打到任何渠道都行：每个 adaptor 都实现了
 *      `ConvertClaudeRequest`，openai adaptor 里那句「只有 claude 模型才让转」
 *      的守卫是被注释掉的。
 *    · `/v1/chat/completions` 打到 Anthropic 渠道也行，claude adaptor 的
 *      `ConvertOpenAIRequest` 有真实现。
 *    · `/v1/responses`（Codex 唯一会发的格式）是唯一有真空档的：部署版
 *      claude adaptor 的 `ConvertOpenAIResponsesRequest` 是
 *      `return nil, errors.New("not implemented")`，gemini 的有真实现，
 *      openai 的是原样透传到上游的 `/v1/responses`。
 *
 *    也就是说，唯一有证据支撑的限制是「Codex + 纯 Anthropic 渠道的模型」，
 *    而且这条会随中转升级失效。真要做，得先有一个可靠的信号，或者干脆
 *    在接入时探一次。
 */

import type { ActivationToolId } from "./activation";

export type ModelConnectionMode = "direct" | "bridge";

/**
 * 界面上那句「原生接口 / 自动兼容」的来源。这是**说明**，不是闸门：两个取值
 * 都代表能用，区别只是路上有没有一层协议转换，而转换发生在中转侧，不在本机桥。
 */
export function modelConnectionMode(
  toolId: ActivationToolId,
  endpoints: string[] | undefined,
): ModelConnectionMode {
  if (toolId === "claude_code" || toolId === "claude_desktop") {
    return endpoints?.includes("anthropic") ? "direct" : "bridge";
  }
  if (toolId === "codex_desktop") {
    return endpoints?.includes("openai-response") ? "direct" : "bridge";
  }
  return endpoints?.includes("openai") ? "direct" : "bridge";
}

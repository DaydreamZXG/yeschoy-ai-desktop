import type { ToolConnection } from "../../configuration/connections";

/**
 * 预览页从「账户会话」与「连接列表」里读出来的几个判读。
 *
 * 为什么抽到这里：它们原本散在页面组件里，一半是 `?.` 链、一半是布尔组合，
 * 改一处条件要通读整页才敢确定别处没依赖；而且页面只测得到「渲染结果」，
 * 判读本身没有独立断言。规则放成纯函数，页面与 hook 都只引用它。
 *
 * 几条容易写错、这里刻意钉住的语义：
 *
 * - **连接状态不可知**不等于「未接入」。读取失败时页面必须说「暂时读不到」，
 *   而不是把用户的接入当成没发生 —— 后者会让备份提示、恢复按钮都跟着变。
 *   所以 `loading`、整表读取失败、以及这一条本身 `unavailable` 都算不可知。
 * - 单条 `unavailable` 而其余条目正常时，控制器会把整体标成局部失败
 *   （`connection_partial_unavailable`）。页面不能因此把所有工具都判成未知，
 *   只能对**那一条**保守。
 * - 已登录是「投影状态为 signed_in」，不是「拿得到投影」：投影在登录过期后
 *   仍会短暂保留最后一份。
 */
export interface ConnectionReading {
  loading: boolean;
  error: boolean;
  connections: readonly ToolConnection[];
  saved?: ToolConnection;
}

export function savedConnectionFor(
  connections: { connections?: readonly ToolConnection[] } | null | undefined,
  toolId: string,
): ToolConnection | undefined {
  return connections?.connections?.find((item) => item.toolId === toolId);
}

/**
 * 这条工具当前的连接状态是否**可判读**。
 *
 * 判读不了的原因有三种，界面应当把它们说成同一件事（「暂时读不到」）：
 * 正在读、整表读取失败且没有缓存、以及这一条自己 `unavailable`。
 */
export function connectionStateUnavailable(
  reading: ConnectionReading | null | undefined,
): boolean {
  if (!reading) return true;
  if (reading.loading) return true;
  if (reading.error && reading.connections.length === 0) return true;
  return reading.saved?.state === "unavailable";
}

/**
 * 会话是否已登录。
 *
 * `status` 是原生侧投影里的字段；只有 `signed_in` 算已登录，`signed_out`、
 * 过期等一律不算 —— 页面据此决定「接入」按钮是引导登录还是执行。
 */
export function isSignedIn(status: string | null | undefined): boolean {
  return status === "signed_in";
}

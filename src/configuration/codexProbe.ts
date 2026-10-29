/**
 * 「接入时探测 Codex 传输协议」这一个偏好项。
 *
 * 探测会对每个模型真的发一次最小生成请求（16 token）去问中转站点：这个模型
 * 走 Responses 直连还是需要桥成 Chat。它能让桥接路由更准，但**成功的那次会
 * 产生真实用量**，与产品对用户「不发收费的测试请求」的承诺冲突。所以默认
 * 关闭：不探测就停在直连（也就是今天的行为），代价是这类模型在 Codex 里用
 * 不了，而用户点开开关即可恢复。
 *
 * 存 localStorage 而不是走原生：它是纯渲染层偏好，接入请求里多一个布尔字段
 * 就够了，为它新增一条 IPC 命令或一个 Rust 配置状态不划算。与外观、语言同一
 * 种存法，见 `src/workbench/appearance.ts`。
 */
export const CODEX_PROBE_KEY = "yeschoy-codex-transport-probe";

export function readCodexProbeEnabled(): boolean {
  try {
    return localStorage.getItem(CODEX_PROBE_KEY) === "on";
  } catch {
    /* Storage is optional; a missing flag means the safe default. */
  }
  return false;
}

export function writeCodexProbeEnabled(enabled: boolean): void {
  try {
    if (enabled) localStorage.setItem(CODEX_PROBE_KEY, "on");
    else localStorage.removeItem(CODEX_PROBE_KEY);
  } catch {
    /* Session-only fallback; the default stays off. */
  }
}

/**
 * 一次接入实际要发的探测开关。
 *
 * 只对 Codex 有意义。其它工具不探测，开关开着也不该把一个无关字段
 * 塞进请求（原生侧 `deny_unknown_fields` 之前的语义虽然是「多收一个字段
 * 也认」，但请求越小越容易看出这条判断）。
 */
export function probeRequestedFor(toolId: string, enabled: boolean): boolean {
  return toolId === "codex_desktop" && enabled;
}

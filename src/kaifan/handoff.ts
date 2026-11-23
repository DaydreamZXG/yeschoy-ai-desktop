/**
 * 打开应用时的衔接：应用被打开的那一刻拉一块暗场，角色登场，应用的画面出现在
 * 暗场里的一块屏幕上，再放大成真的应用窗口。暗场是另一个窗口，由后端在真要
 * 打开应用的那一步拉起来；这里只负责事先说一声「由谁登场」。
 */
import { invoke } from "@tauri-apps/api/core";
import type { RosterId } from "./roster";
import { prefersCalm } from "./sound";

const OFF_KEY = "kaifan.handoff.off";

/** 只有 Windows 能把别的应用的实时画面放进来；别的系统上没有这一段。 */
export const handoffExists = () =>
  typeof navigator !== "undefined" && /Windows/i.test(navigator.userAgent);

export function handoffEnabled(): boolean {
  try {
    return localStorage.getItem(OFF_KEY) !== "1";
  } catch {
    return true;
  }
}

export function setHandoffEnabled(enabled: boolean) {
  try {
    if (enabled) localStorage.removeItem(OFF_KEY);
    else localStorage.setItem(OFF_KEY, "1");
  } catch {
    /* 记不住就只管这一次。 */
  }
}

/**
 * 包住一次会打开应用的操作（接入、单独打开）。登记失败、没有这一段、用户关了、
 * 系统要求少动效：都照原样做，只是没有衔接。
 */
export async function withHandoff<T>(
  toolId: string,
  who: RosterId,
  run: () => Promise<T>,
): Promise<T> {
  if (!handoffExists() || !handoffEnabled() || prefersCalm()) return run();
  await invoke("app_handoff_arm_v1", { request: { toolId, who } }).catch(
    () => undefined,
  );
  try {
    return await run();
  } finally {
    // 没用上的登记不能挂到下一次打开上。
    void invoke("app_handoff_disarm_v1").catch(() => undefined);
  }
}

/** 暗场那一页用：我画好了；用户要跳过。 */
export const handoffPainted = () =>
  invoke("app_handoff_ready_v1").catch(() => undefined);
export const handoffSkip = () =>
  invoke("app_handoff_end_v1").catch(() => undefined);

export interface HandoffScreen {
  /** 屏幕先出现在哪、再放大到哪：左、上、宽、高，CSS 像素。 */
  from: [number, number, number, number];
  to: [number, number, number, number];
  fadeMs: number;
  holdMs: number;
  growMs: number;
}

export const HANDOFF_SCREEN = "handoff://screen";
export const HANDOFF_LAND = "handoff://land";
export const HANDOFF_LEAVE = "handoff://leave";

const isBox = (value: unknown): value is HandoffScreen["from"] =>
  Array.isArray(value) &&
  value.length === 4 &&
  value.every((item) => typeof item === "number" && Number.isFinite(item));

const isMs = (value: unknown): value is number =>
  typeof value === "number" && value >= 0 && value <= 5000;

/** 后端发来的屏幕位置。形状不对就当没收到，暗场照常撤。 */
export function readHandoffScreen(payload: unknown): HandoffScreen | null {
  if (!payload || typeof payload !== "object") return null;
  const { from, to, fadeMs, holdMs, growMs } = payload as Record<
    string,
    unknown
  >;
  return isBox(from) &&
    isBox(to) &&
    isMs(fadeMs) &&
    isMs(holdMs) &&
    isMs(growMs)
    ? { from, to, fadeMs, holdMs, growMs }
    : null;
}

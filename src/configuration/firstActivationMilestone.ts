// 首次接入庆祝旗标（动效优化）：只在第一次接入成功时触发彩带/ticker，
// 之后的配置成功都走常规克制反馈。本地记账，失败静默降级。
const FIRST_ACTIVATION_KEY = "yeschoy.firstActivationCelebrated.v1";

export interface MilestoneStorage {
  getItem(key: string): string | null;
  setItem(key: string, value: string): void;
}

function safeStorage(): MilestoneStorage | null {
  try {
    return window.localStorage;
  } catch {
    return null;
  }
}

/** 是否还未庆祝过首次接入（首次渲染用，消费发生在成功时刻）。 */
export function shouldCelebrateFirstActivation(
  storage: MilestoneStorage | null = safeStorage(),
): boolean {
  if (!storage) return false;
  try {
    return storage.getItem(FIRST_ACTIVATION_KEY) !== "1";
  } catch {
    return false;
  }
}

/** 首次接入成功时调用：写入旗标，之后不再触发庆祝。 */
export function markFirstActivationCelebrated(
  storage: MilestoneStorage | null = safeStorage(),
): void {
  if (!storage) return;
  try {
    storage.setItem(FIRST_ACTIVATION_KEY, "1");
  } catch {
    // 静默降级：存储不可用时下次仍会触发，不影响功能。
  }
}

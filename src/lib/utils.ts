import { clsx, type ClassValue } from "clsx";

/**
 * 语义 class 的拼接器。
 *
 * 这里曾经用 tailwind-merge，但本仓库没有 Tailwind 管线、组件里也不再出现
 * 工具类；语义 class 之间没有「后一个覆盖前一个」的约定，直接 clsx 就好。
 */
export function cn(...inputs: ClassValue[]) {
  return clsx(inputs);
}

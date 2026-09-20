import { useMemo } from "react";
import { useTranslation } from "react-i18next";

import zhLocale from "../i18n/locales/zh.json";

/**
 * 接入流程的文案已经搬进 `i18n/locales/{zh,en}.json` 的 `configuration` 命名空间。
 *
 * 和 `workbench/copy.ts` 同一个做法：这里只保留原来的形状
 * （`const g = useConfigurationCopy(); g.modelChoice`），四个调用方一行不用改。
 *
 * 中文仍以 `zh.json` 为准，`configurationCopies.zh` 直接引它——
 * 另存一份必然漂移。
 */
const zh = zhLocale.configuration;
type Copy = { [K in keyof typeof zh]: string };

export const configurationCopies = { zh };
export type ConfigurationCopy = Copy;

export function useConfigurationCopy(): Copy {
  const { t, i18n } = useTranslation();
  // 依赖里带上 `i18n.language`：切语言时 `t` 的身份不一定变，
  // 只靠它做依赖文案不会跟着切。
  return useMemo(
    () =>
      Object.fromEntries(
        Object.keys(zh).map((key) => [key, t(`configuration.${key}`)]),
      ) as Copy,
    [t, i18n.language],
  );
}

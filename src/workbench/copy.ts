import { useMemo } from "react";
import { useTranslation } from "react-i18next";

import zhLocale from "../i18n/locales/zh.json";

/**
 * 界面文案已经搬进 `i18n/locales/{zh,en}.json` 的 `workbench` 命名空间。
 *
 * 这里保留同样的形状（`const c = useWorkbenchCopy(); c.home`），所以九个调用方
 * 一行都不用改——迁移的风险集中在这一个文件里，而不是摊到九处。
 *
 * 中文仍以 `zh.json` 为准：`workbenchCopies.zh` 直接引它，
 * 不再另存一份，否则两处必然漂移。
 */
const zh = zhLocale.workbench;
type Copy = { [K in keyof typeof zh]: string };

export const workbenchCopies = { zh };

export function useWorkbenchCopy(): Copy {
  const { t, i18n } = useTranslation();
  // 依赖里带上 `i18n.language`：切换语言时 `t` 的身份不一定变，
  // 只靠它做依赖的话文案不会跟着切。
  return useMemo(
    () =>
      Object.fromEntries(
        Object.keys(zh).map((key) => [key, t(`workbench.${key}`)]),
      ) as Copy,
    [t, i18n.language],
  );
}

/**
 * 界面语言偏好。
 *
 * 只存在这台电脑上，不随账号同步——换一台机器不会把语言带过去，
 * 这和外观偏好（`workbench/appearance.ts`）是同一个约定，
 * 所以这里刻意照着它写：同样的 try/catch、同样的「存储坏了也不挡 UI」。
 */
export const LANGUAGE_KEY = "yeschoy-language";

/** 已经**完整**翻译、可以真正切过去的语言。 */
export const SUPPORTED_LANGUAGES = ["zh", "en"] as const;
export type Language = (typeof SUPPORTED_LANGUAGES)[number];

/** 各语言的 `<html lang>`。读屏与断词都看它。 */
const HTML_LANG: Record<Language, string> = {
  zh: "zh-CN",
  en: "en",
};

export function readLanguage(): Language {
  try {
    const value = localStorage.getItem(LANGUAGE_KEY);
    if (SUPPORTED_LANGUAGES.includes(value as Language))
      return value as Language;
  } catch {
    /* 存储是可选的，永远不要因此挡住界面。 */
  }
  return "zh";
}

export function storeLanguage(value: Language): void {
  try {
    localStorage.setItem(LANGUAGE_KEY, value);
  } catch {
    /* 存不下就只在本次会话生效。 */
  }
}

export function applyHtmlLang(value: Language): void {
  if (typeof document !== "undefined") {
    document.documentElement.lang = HTML_LANG[value];
  }
}

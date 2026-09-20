import i18n from "i18next";
import { initReactI18next } from "react-i18next";

import en from "./locales/en.json";
import zh from "./locales/zh.json";
import { applyHtmlLang, readLanguage } from "./language";

/**
 * 只注册**已经完整翻译**的语言。
 *
 * `ja.json` / `zh-TW.json` 也在仓库里，各有 267 键、内容是齐的——但它们只覆盖
 * 这 268 个既有键，而接下来从 `copy.ts` 迁进来的那批不会有日/繁译文。
 * 注册进去就会得到一个「切过去大半界面还是中文」的开关，
 * 比没有这个开关更糟。等它们补齐再加。
 */
const language = readLanguage();

i18n.use(initReactI18next).init({
  resources: {
    zh: { translation: zh },
    en: { translation: en },
  },
  lng: language,
  // 回落到中文而不是键名：新迁进来的键在英文补上之前，
  // 用户看到的是一句中文，而不是 `yeschoySettings.languageTitle` 这种。
  fallbackLng: "zh",
  interpolation: {
    escapeValue: false,
  },
  debug: false,
});

applyHtmlLang(language);

export default i18n;

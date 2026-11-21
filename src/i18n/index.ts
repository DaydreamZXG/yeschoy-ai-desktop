import i18n from "i18next";
import { initReactI18next } from "react-i18next";

import en from "./locales/en.json";
import zh from "./locales/zh.json";
import {
  announceLanguageToNative,
  applyHtmlLang,
  readLanguage,
} from "./language";

/** 只注册翻译齐了的语言：切过去大半界面还是中文的开关，比没有更糟。 */
const language = readLanguage();

i18n.use(initReactI18next).init({
  resources: {
    zh: { translation: zh },
    en: { translation: en },
  },
  lng: language,
  // 回落到中文而不是键名：新迁进来的键在英文补上之前，
  // 用户看到的是一句中文，而不是 `kaifan.settings.language` 这种。
  fallbackLng: "zh",
  interpolation: {
    escapeValue: false,
  },
  debug: false,
});

applyHtmlLang(language);
announceLanguageToNative(language);

export default i18n;

import { useTranslation } from "react-i18next";
import { Languages } from "lucide-react";

import {
  LANGUAGE_NAME,
  SUPPORTED_LANGUAGES,
  applyHtmlLang,
  storeLanguage,
  type Language,
} from "./language";

/**
 * 界面语言切换器。和外观选择器同一个形状，因为它们是同一类偏好：
 * 只存在这台电脑上、马上生效、随时可以改回去。
 *
 * 不需要刷新页面：`changeLanguage` 会让所有 `useTranslation()` 的组件重渲染。
 * 少数走 i18next 单例 `t` 的模块级函数（`connectionLabel`、`installationStage`
 * 等）自己不重渲染，但它们全部由调了 `useTranslation()` 的组件渲染，
 * 父组件一重渲染就会重新调用它们。
 */
export function LanguagePicker() {
  const { t, i18n } = useTranslation();
  const current = (SUPPORTED_LANGUAGES as readonly string[]).includes(
    i18n.language,
  )
    ? (i18n.language as Language)
    : "zh";

  const change = (next: Language) => {
    if (next === current) return;
    // 先落盘再切：切换过程中若渲染抛错，下次打开至少还是用户选的那个。
    storeLanguage(next);
    applyHtmlLang(next);
    void i18n.changeLanguage(next);
  };

  return (
    <section className="settings-language">
      <h2>{t("yeschoySettings.languageTitle")}</h2>
      <div
        className="language-picker"
        role="group"
        aria-label={t("yeschoySettings.languageTitle")}
      >
        {SUPPORTED_LANGUAGES.map((id) => (
          <button
            type="button"
            key={id}
            lang={id}
            aria-pressed={current === id}
            onClick={() => change(id)}
          >
            <Languages aria-hidden="true" />
            <span>{LANGUAGE_NAME[id]}</span>
          </button>
        ))}
      </div>
      <p>{t("yeschoySettings.languageNote")}</p>
    </section>
  );
}

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

/**
 * 各语言自己的名字（endonym）。**这些永远不翻译。**
 *
 * 切换器要给「被困在一门看不懂的语言里」的人用 —— 那时候写「Chinese」帮不上忙，
 * 他要找的是「中文」这三个字。所以两个按钮在任何界面语言下都长一样。
 *
 * 也因此它们不放进语言文件：那里的每个键都是「同一句话的不同语言版本」，
 * 而这两个恰恰相反。放进去还会让「英文里没有漏译的中文」那条守卫红 ——
 * 它红得有道理，只是这里不是漏译。
 */
export const LANGUAGE_NAME: Record<Language, string> = {
  zh: "中文",
  en: "English",
};

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

/**
 * 把语言告诉原生侧（`src-tauri/src/ui_language.rs`）。
 *
 * 原生自己画的东西 —— Windows 关闭窗口时的 MessageBox、安装项的位置标签、
 * OAuth 回调页 —— 不经过 i18next，只能靠这一句知道该说哪种语言。
 * 启动时和每次切换都发一次；发不出去（比如在浏览器里预览）就当没这回事，
 * 渲染层的语言不受影响。
 */
export function announceLanguageToNative(value: Language): void {
  void import("@tauri-apps/api/core")
    .then(({ invoke }) => invoke("set_ui_language_v1", { language: value }))
    .catch(() => {
      /* 原生侧不在（浏览器预览）或命令不存在（旧版壳）都不是错误。 */
    });
}

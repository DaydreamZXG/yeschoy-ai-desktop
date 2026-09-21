//! 原生侧的界面语言。
//!
//! 语言偏好本来只活在渲染层的 `localStorage`（`src/i18n/language.ts`），原生
//! 侧一无所知 —— 于是 Windows 上关闭窗口弹的原生 MessageBox、安装项的位置
//! 标签、OAuth 回调页，英文用户看到的全是中文。这里给原生侧一份同样的答案：
//! 渲染层启动时和切换语言时各 `invoke` 一次，原生文案按它选。
//!
//! 不落盘：渲染层每次启动都会重新告知，原生侧只需要记住本次进程的值。
//! 渲染层还没挂上之前（极少数原生对话框会在那之前出现）默认中文，
//! 与渲染层自己的默认一致。

use std::sync::atomic::{AtomicU8, Ordering};

use serde::Deserialize;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Language {
    Zh,
    En,
}

impl Language {
    fn code(self) -> u8 {
        match self {
            Self::Zh => 0,
            Self::En => 1,
        }
    }

    fn from_code(code: u8) -> Self {
        match code {
            1 => Self::En,
            _ => Self::Zh,
        }
    }

    /// 按当前语言二选一。原生文案不多，一处一对字面量比一套 i18n 表更好维护。
    pub(crate) fn pick<'a>(self, zh: &'a str, en: &'a str) -> &'a str {
        match self {
            Self::Zh => zh,
            Self::En => en,
        }
    }
}

static CURRENT: AtomicU8 = AtomicU8::new(0);

pub(crate) fn current() -> Language {
    Language::from_code(CURRENT.load(Ordering::Relaxed))
}

pub(crate) fn set(language: Language) {
    CURRENT.store(language.code(), Ordering::Relaxed);
}

/// 只认得已经完整翻译的两种；别的值当作没说，保持现状。
#[tauri::command]
pub(crate) fn set_ui_language_v1(language: Language) -> bool {
    set(language);
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_to_chinese_and_follows_the_last_set_value() {
        // Tests share one process; restore whatever was there afterwards.
        let before = current();
        set(Language::Zh);
        assert_eq!(current(), Language::Zh);
        assert_eq!(current().pick("中文", "English"), "中文");
        set(Language::En);
        assert_eq!(current(), Language::En);
        assert_eq!(current().pick("中文", "English"), "English");
        set(before);
    }

    #[test]
    fn only_the_two_shipped_languages_deserialize() {
        assert_eq!(
            serde_json::from_str::<Language>("\"en\"").unwrap(),
            Language::En
        );
        assert_eq!(
            serde_json::from_str::<Language>("\"zh\"").unwrap(),
            Language::Zh
        );
        assert!(serde_json::from_str::<Language>("\"ja\"").is_err());
        assert!(serde_json::from_str::<Language>("\"EN\"").is_err());
    }
}

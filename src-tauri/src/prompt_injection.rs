//! 野菜特有提示词注入（预留，默认关闭）。
//!
//! 目标：给 GPT 类模型补上野菜的中转约束（工具调用格式、拒绝绕行、
//! 成本意识等），解决「换个中转站就变笨」的观感问题。这一版只把插槽
//! 和防护做好，**不改变任何线上请求**：
//!
//! 1. 开关与正文都由构建期环境变量决定，默认关闭。官方包在确认了
//!    prompt 内容、灰度顺序和用户可见说明之后再打开；社区包保持关闭。
//! 2. 只影响走本机桥转发到站点 `/v1/chat/completions` 的请求。桥是助手
//!    自己起的进程，改请求体不需要在用户机器上留任何配置。
//! 3. 只把 `instructions` / `instructions` 字段追加在 `messages` 最前面，
//!    不重排、不删除用户内容，幂等：同一段提示词不会追加两次。
//! 4. 只作用于 `gpt-*` / `o<n>*` 模型。DeepSeek、Claude、Gemini 等原生
//!    模型有自己的系统提示词习惯，套用野菜提示词只会互相打架。
//!
//! 打开方式（官方构建）：
//! `YESCHOY_PROMPT_INJECTION=1 YESCHOY_PROMPT_INJECTION_TEXT="<提示词>"`
//! 正文里有 `{{model}}` 占位符时替换为请求里的模型 ID。
use serde_json::{json, Value};

/// 构建期读取一次。`option_env!` 让未设置时得到 `None`。
/// 两个变量名由 `build.rs` 声明 `rerun-if-env-changed`。
const ENABLED: Option<&str> = option_env!("YESCHOY_PROMPT_INJECTION");
const TEXT: Option<&str> = option_env!("YESCHOY_PROMPT_INJECTION_TEXT");

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum InjectionOutcome {
    /// 总开关关闭（默认）。
    Disabled,
    /// 开关打开但没有正文，或正文是空白：视为配置错误，绝不发出。
    NoTemplate,
    /// 不是需要注入的模型家族。
    NotTargeted,
    /// 已经追加过，幂等跳过。
    AlreadyPresent,
    Appended,
}

impl InjectionOutcome {
    /// 请求体是否被改写过。其余结果都表示「原样转发」。
    pub(crate) fn is_appended(self) -> bool {
        self == InjectionOutcome::Appended
    }
}

/// 当前是否启用注入。渲染层「高级信息」只展示，不允许运行时切换。
pub(crate) fn enabled() -> bool {
    matches!(
        ENABLED.map(str::trim),
        Some("1") | Some("true") | Some("on")
    )
}

/// 只接管 GPT 家族。`gpt-5.6-sol`、`o3-mini`、`chatgpt-4o` 命中；
/// `gpt-image-1` 是图像模型，没有对话消息，自然跳过。
pub(crate) fn is_target_model(id: &str) -> bool {
    let bare = id
        .rsplit_once('/')
        .map(|(_, tail)| tail)
        .unwrap_or(id)
        .to_ascii_lowercase();
    // 部分站点会在 ID 后加计费分组后缀，只取家族前缀判断即可。
    if bare.starts_with("gpt-image") || bare.starts_with("dall-e") {
        return false;
    }
    bare.starts_with("gpt-") || bare.starts_with("chatgpt-") || starts_with_o_series(&bare)
}

fn starts_with_o_series(id: &str) -> bool {
    let Some(rest) = id.strip_prefix('o') else {
        return false;
    };
    // `o1`、`o3-mini`、`o4` 是推理模型；`opencode` 之类不是。
    let mut chars = rest.chars();
    matches!(chars.next(), Some(digit) if digit.is_ascii_digit())
        && chars.next().is_none_or(|c| c == '-' || c == '.')
}

/// 把野菜提示词追加到 Chat Completions 请求体最前面。
///
/// 返回 `(是否修改, 结果说明)`。请求体不是对象、没有 `messages` 数组或
/// `messages` 为空时原样返回：宁可少注入，也不替用户凭空造一条消息。
pub(crate) fn apply_to_chat_body(body: &mut Value, model: &str) -> InjectionOutcome {
    let config = Config {
        enabled: enabled(),
        template: TEXT,
    };
    apply_with_config(body, model, &config)
}

/// 构建期配置。测试通过这个结构覆盖所有分支，生产路径只走 [`apply_to_chat_body`]。
struct Config {
    enabled: bool,
    template: Option<&'static str>,
}

fn apply_with_config(body: &mut Value, model: &str, config: &Config) -> InjectionOutcome {
    if !config.enabled {
        return InjectionOutcome::Disabled;
    }
    let Some(text) = config.template.map(|text| text.replace("{{model}}", model)) else {
        return InjectionOutcome::NoTemplate;
    };
    let text = text.trim();
    if text.is_empty() {
        return InjectionOutcome::NoTemplate;
    }
    if !is_target_model(model) {
        return InjectionOutcome::NotTargeted;
    }
    let Some(messages) = body.get_mut("messages").and_then(Value::as_array_mut) else {
        return InjectionOutcome::NotTargeted;
    };
    if messages.is_empty() {
        return InjectionOutcome::NotTargeted;
    }
    // 幂等：桥可能被同一个客户端重试，重复追加会让模型看到两遍。
    let already = messages.iter().any(|message| {
        message.get("role").and_then(Value::as_str) == Some("system")
            && message.get("content").and_then(Value::as_str) == Some(text)
    });
    if already {
        return InjectionOutcome::AlreadyPresent;
    }
    messages.insert(0, json!({"role": "system", "content": text}));
    InjectionOutcome::Appended
}

#[cfg(test)]
mod tests {
    use super::*;

    fn on(template: &'static str) -> Config {
        Config {
            enabled: true,
            template: Some(template),
        }
    }

    fn chat() -> Value {
        json!({"model": "gpt-5.6-sol", "messages": [{"role": "user", "content": "hi"}]})
    }

    #[test]
    fn ships_disabled_by_default() {
        // 仓库与 CI 构建都不设置环境变量；这条守住「预留」的边界。
        if std::env::var_os("YESCHOY_PROMPT_INJECTION").is_none() {
            assert!(!enabled());
            let mut body = chat();
            assert_eq!(
                apply_to_chat_body(&mut body, "gpt-5.6-sol"),
                InjectionOutcome::Disabled
            );
            assert_eq!(body["messages"].as_array().unwrap().len(), 1);
        }
    }

    #[test]
    fn only_gpt_family_models_are_targeted() {
        for id in [
            "gpt-5.6-sol",
            "gpt-6.1-sol",
            "openai/gpt-5.4-mini",
            "chatgpt-4o-latest",
            "o1",
            "o3-mini",
            "o4",
        ] {
            assert!(is_target_model(id), "{id} should be targeted");
        }
        for id in [
            "deepseek-v4.1-flash",
            "claude-sonnet-5",
            "gemini-3-pro",
            "opencode",
            "glm-5.3",
            "gpt-image-1",
            "omni-1",
        ] {
            assert!(!is_target_model(id), "{id} must not be targeted");
        }
    }

    #[test]
    fn appends_one_system_message_at_the_front_and_is_idempotent() {
        let mut body = chat();
        let config = on("野菜规则");
        assert_eq!(
            apply_with_config(&mut body, "gpt-5.6-sol", &config),
            InjectionOutcome::Appended
        );
        let messages = body["messages"].as_array().unwrap();
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0]["role"], "system");
        assert_eq!(messages[0]["content"], "野菜规则");
        assert_eq!(messages[1]["content"], "hi");
        assert_eq!(
            apply_with_config(&mut body, "gpt-5.6-sol", &config),
            InjectionOutcome::AlreadyPresent
        );
        assert_eq!(body["messages"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn substitutes_the_model_id_placeholder() {
        let mut body = chat();
        assert_eq!(
            apply_with_config(&mut body, "gpt-6.1-sol", &on("serving {{model}}")),
            InjectionOutcome::Appended
        );
        assert_eq!(body["messages"][0]["content"], "serving gpt-6.1-sol");
    }

    #[test]
    fn leaves_other_models_and_odd_bodies_untouched() {
        let mut body = chat();
        assert_eq!(
            apply_with_config(&mut body, "deepseek-v4.1-flash", &on("x")),
            InjectionOutcome::NotTargeted
        );
        assert_eq!(body["messages"].as_array().unwrap().len(), 1);

        let mut empty = json!({"model": "gpt-5.6-sol", "messages": []});
        assert_eq!(
            apply_with_config(&mut empty, "gpt-5.6-sol", &on("x")),
            InjectionOutcome::NotTargeted
        );
        assert_eq!(empty["messages"].as_array().unwrap().len(), 0);

        let mut shapeless = json!({"model": "gpt-5.6-sol"});
        assert_eq!(
            apply_with_config(&mut shapeless, "gpt-5.6-sol", &on("x")),
            InjectionOutcome::NotTargeted
        );
        assert!(shapeless.get("messages").is_none());
    }

    #[test]
    fn blank_template_or_switched_off_sends_nothing() {
        let mut body = chat();
        assert_eq!(
            apply_with_config(&mut body, "gpt-5.6-sol", &on("   \n ")),
            InjectionOutcome::NoTemplate
        );
        assert_eq!(
            apply_with_config(
                &mut body,
                "gpt-5.6-sol",
                &Config {
                    enabled: false,
                    template: Some("rules")
                }
            ),
            InjectionOutcome::Disabled
        );
        assert_eq!(
            apply_with_config(
                &mut body,
                "gpt-5.6-sol",
                &Config {
                    enabled: true,
                    template: None
                }
            ),
            InjectionOutcome::NoTemplate
        );
        assert_eq!(body["messages"].as_array().unwrap().len(), 1);
    }
}

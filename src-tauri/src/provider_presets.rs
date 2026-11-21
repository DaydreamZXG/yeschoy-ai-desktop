//! 内置的云厂商：一家一行数据，不为哪一家单独写逻辑。
//!
//! 中转站的规矩是「网址后面接 `/v1`」；云厂商各有各的路径，而且给 Claude 的
//! 和给 Codex、OpenCode 的常常是两个地址。凭据里存的仍然只是站点的网址
//! （`https://域名`），到真正要发请求、要往应用里写地址的那一刻，才在这里
//! 查出完整地址。表里没有的网址一律按中转站的规矩来，所以老的连接不受影响。
use serde::Serialize;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Billing {
    /// 包月套餐。厂商多半只许在编程工具里用，界面要把这件事说在前面。
    Plan,
    /// 按量付费的普通接口。
    PayAsYouGo,
}

/// 连接时拿什么确认这把密钥是对的。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum KeyCheck {
    /// 模型名单要密钥才给看：读得到就是对的。
    Models,
    /// 模型名单不要密钥也给看，另有一个只认密钥的地址。
    Path(&'static str),
    /// 模型名单不要密钥也给看，也没有不花钱的办法核对：照实说「没法提前核对」。
    Unverifiable,
}

#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Preset {
    pub(crate) id: &'static str,
    /// 同一家的国内站和海外站共用一个 `family`，界面上是一张卡片里的切换。
    pub(crate) family: &'static str,
    pub(crate) region: Option<&'static str>,
    pub(crate) name: &'static str,
    pub(crate) origin: &'static str,
    #[serde(skip)]
    openai_path: &'static str,
    #[serde(skip)]
    anthropic_path: Option<&'static str>,
    pub(crate) billing: Billing,
    pub(crate) key_check: KeyCheck,
    /// 站点自己就有 Codex 要的 Responses 接口。没有的由本机桥转成 Chat。
    #[serde(skip)]
    pub(crate) responses: bool,
    /// 只有这些应用能接；空表示都能接。
    pub(crate) only_tools: &'static [&'static str],
    pub(crate) key_page: &'static str,
    /// 连接屏默认就露出来；不常用的收在「还有几家」里。
    pub(crate) common: bool,
}

const fn preset(
    id: &'static str,
    family: &'static str,
    region: Option<&'static str>,
    name: &'static str,
    origin: &'static str,
    openai_path: &'static str,
    anthropic_path: Option<&'static str>,
    billing: Billing,
    key_page: &'static str,
) -> Preset {
    Preset {
        id,
        family,
        region,
        name,
        origin,
        openai_path,
        anthropic_path,
        billing,
        key_check: KeyCheck::Models,
        responses: false,
        only_tools: &[],
        key_page,
        // 默认露在外面；不常用的在表里单独标，收进「还有几家」。
        common: true,
    }
}

/// 地址逐条对过官方文档，并用一把假密钥请求过（2026-11）：都回 401，
/// 也就是地址在、只是密钥不对。加一家只加一行。
pub(crate) const PRESETS: &[Preset] = &[
    preset(
        "zhipu_coding",
        "zhipu_coding",
        None,
        "智谱 GLM Coding Plan",
        "https://open.bigmodel.cn",
        "/api/coding/paas/v4",
        Some("/api/anthropic"),
        Billing::Plan,
        "https://bigmodel.cn/usercenter/proj-mgmt/apikeys",
    ),
    preset(
        "kimi_code_cn",
        "kimi_code",
        Some("cn"),
        "Kimi Code",
        "https://api.kimi.com",
        "/coding/v1",
        Some("/coding"),
        Billing::Plan,
        "https://www.kimi.com/code/console",
    ),
    preset(
        "kimi_code_global",
        "kimi_code",
        Some("global"),
        "Kimi Code",
        "https://api.kimi.ai",
        "/coding/v1",
        Some("/coding"),
        Billing::Plan,
        "https://www.kimi.ai/code/console",
    ),
    Preset {
        common: false,
        ..preset(
            "volcengine_coding",
            "volcengine_coding",
            None,
            "火山方舟 Coding Plan",
            "https://ark.cn-beijing.volces.com",
            "/api/coding/v3",
            Some("/api/coding"),
            Billing::Plan,
            "https://console.volcengine.com/ark/region:ark+cn-beijing/apiKey",
        )
    },
    Preset {
        key_check: KeyCheck::Unverifiable,
        common: false,
        ..preset(
            "bailian_coding",
            "bailian_coding",
            None,
            "阿里云百炼 Coding Plan",
            "https://coding.dashscope.aliyuncs.com",
            "/v1",
            Some("/apps/anthropic"),
            Billing::Plan,
            "https://bailian.console.aliyun.com/",
        )
    },
    Preset {
        key_check: KeyCheck::Unverifiable,
        ..preset(
            "ollama_cloud",
            "ollama_cloud",
            None,
            "Ollama Cloud",
            "https://ollama.com",
            "/v1",
            Some(""),
            Billing::Plan,
            "https://ollama.com/settings/keys",
        )
    },
    Preset {
        // 它的对话接口要求带 OpenCode 自己的会话标记，别的应用发过去会被拒。
        only_tools: &["opencode"],
        ..preset(
            "opencode_go",
            "opencode_go",
            None,
            "OpenCode Go",
            "https://opencode.ai",
            "/inference/go/v1",
            Some("/inference/go"),
            Billing::Plan,
            "https://opencode.ai/auth",
        )
    },
    Preset {
        key_check: KeyCheck::Path("/api/v1/key"),
        responses: true,
        ..preset(
            "openrouter",
            "openrouter",
            None,
            "OpenRouter",
            "https://openrouter.ai",
            "/api/v1",
            Some("/api"),
            Billing::PayAsYouGo,
            "https://openrouter.ai/settings/keys",
        )
    },
    Preset {
        common: false,
        ..preset(
            "deepseek",
            "deepseek",
            None,
            "DeepSeek",
            "https://api.deepseek.com",
            "/v1",
            Some("/anthropic"),
            Billing::PayAsYouGo,
            "https://platform.deepseek.com/api_keys",
        )
    },
    Preset {
        common: false,
        ..preset(
            "moonshot_cn",
            "moonshot",
            Some("cn"),
            "Kimi 开放平台",
            "https://api.moonshot.cn",
            "/v1",
            Some("/anthropic"),
            Billing::PayAsYouGo,
            "https://platform.moonshot.cn/console/api-keys",
        )
    },
    Preset {
        common: false,
        ..preset(
            "moonshot_global",
            "moonshot",
            Some("global"),
            "Kimi 开放平台",
            "https://api.moonshot.ai",
            "/v1",
            Some("/anthropic"),
            Billing::PayAsYouGo,
            "https://platform.moonshot.ai/console/api-keys",
        )
    },
    Preset {
        responses: true,
        ..preset(
            "openai",
            "openai",
            None,
            "OpenAI",
            "https://api.openai.com",
            "/v1",
            None,
            Billing::PayAsYouGo,
            "https://platform.openai.com/api-keys",
        )
    },
];

pub(crate) fn for_origin(origin: &str) -> Option<&'static Preset> {
    let origin = origin.trim_end_matches('/');
    PRESETS.iter().find(|preset| preset.origin == origin)
}

/// `/chat/completions`、`/responses`、`/models` 接在它后面。
pub(crate) fn openai_base(origin: &str) -> String {
    let origin = origin.trim_end_matches('/');
    match for_origin(origin) {
        Some(preset) => format!("{origin}{}", preset.openai_path),
        None => format!("{origin}/v1"),
    }
}

/// `/v1/messages` 接在它后面，也就是应用里 `ANTHROPIC_BASE_URL` 该填的值。
/// 这一家不说 Anthropic 格式时是 `None`：Claude 只能过桥转成 Chat。
pub(crate) fn anthropic_base(origin: &str) -> Option<String> {
    let origin = origin.trim_end_matches('/');
    match for_origin(origin) {
        Some(preset) => preset.anthropic_path.map(|path| format!("{origin}{path}")),
        None => Some(origin.to_owned()),
    }
}

/// 这一家能不能接这个应用。表里没有的站点不在这里判断。
pub(crate) fn allows_tool(origin: &str, tool_id: &str) -> bool {
    for_origin(origin)
        .is_none_or(|preset| preset.only_tools.is_empty() || preset.only_tools.contains(&tool_id))
}

/// 连接屏「云厂商」那一组要画的全部内容。
#[tauri::command]
pub fn provider_presets_v1() -> &'static [Preset] {
    PRESETS
}

/// 在浏览器里打开这一家拿密钥的网页。界面只能说「哪一家」，网址只从表里取：
/// 界面不能让客户端打开任意网址。
#[tauri::command]
pub fn provider_open_key_page_v1(id: String) -> Result<(), String> {
    let preset = PRESETS
        .iter()
        .find(|preset| preset.id == id)
        .ok_or("unknown_provider")?;
    crate::account_v2::open_system_browser(preset.key_page)
        .map_err(|()| "browser_unavailable".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::station_connect::connect_origin_is_acceptable;

    #[test]
    fn every_preset_is_keyed_by_an_origin_the_rest_of_the_client_already_accepts() {
        let mut origins = Vec::new();
        let mut ids = Vec::new();
        for preset in PRESETS {
            assert_eq!(
                connect_origin_is_acceptable(preset.origin).as_deref(),
                Some(preset.origin),
                "{}",
                preset.id
            );
            assert!(!origins.contains(&preset.origin), "{}", preset.id);
            assert!(!ids.contains(&preset.id), "{}", preset.id);
            origins.push(preset.origin);
            ids.push(preset.id);
            assert!(preset.key_page.starts_with("https://"), "{}", preset.id);
            assert!(preset.openai_path.starts_with('/'), "{}", preset.id);
            assert!(!preset.openai_path.ends_with('/'), "{}", preset.id);
            if let Some(path) = preset.anthropic_path {
                assert!(path.is_empty() || path.starts_with('/'), "{}", preset.id);
                assert!(!path.ends_with('/'), "{}", preset.id);
            }
        }
    }

    #[test]
    fn a_vendor_gets_its_own_paths_and_any_other_station_keeps_the_relay_convention() {
        assert_eq!(
            openai_base("https://open.bigmodel.cn"),
            "https://open.bigmodel.cn/api/coding/paas/v4"
        );
        assert_eq!(
            anthropic_base("https://open.bigmodel.cn").as_deref(),
            Some("https://open.bigmodel.cn/api/anthropic")
        );
        assert_eq!(openai_base("https://ollama.com/"), "https://ollama.com/v1");
        assert_eq!(
            anthropic_base("https://ollama.com").as_deref(),
            Some("https://ollama.com")
        );
        assert_eq!(anthropic_base("https://api.openai.com"), None);
        assert_eq!(
            openai_base("https://relay.example.com"),
            "https://relay.example.com/v1"
        );
        assert_eq!(
            anthropic_base("https://relay.example.com").as_deref(),
            Some("https://relay.example.com")
        );
    }

    #[test]
    fn opencode_go_is_offered_to_opencode_only() {
        assert!(allows_tool("https://opencode.ai", "opencode"));
        assert!(!allows_tool("https://opencode.ai", "claude_code"));
        assert!(allows_tool("https://api.deepseek.com", "claude_code"));
        assert!(allows_tool("https://relay.example.com", "codex_desktop"));
    }
}

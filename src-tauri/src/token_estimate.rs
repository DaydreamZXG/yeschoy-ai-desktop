//! 本地估算一次 Anthropic 请求有多少 input token。
//!
//! 为什么要本地算：Claude Desktop 靠 `POST /v1/messages/count_tokens` 量对话
//! 多大，再决定何时自动压缩。中转站没有这个端点 —— 不带凭据探过，它对
//! `/v1/messages/count_tokens` 和对一个随手编的假路径返回的是**逐字相同**的
//! `404 Invalid URL`，而 `/v1/messages` 返回 `401`。所以转发没有意义，
//! 数只能我们自己出。
//!
//! 这里给的是**估算**，不是分词。Claude 的分词器不公开，而这座桥后面真正跑的
//! 常常是 GPT / DeepSeek / Kimi / Gemini，各家分词器又各不相同 —— 没有任何一个
//! 数能对所有路由都准。所以目标不是精确，是「量级对、并且宁可略高」：
//!
//! * 高估 → Desktop 早一点压缩，代价是偶尔多丢一点上下文；
//! * 低估 → 压缩太晚而撑爆，退回到今天的行为（上游报上下文过长，
//!   `claude_bridge::context_too_long_error` 把它翻成标准错误，Desktop 提示压缩）。
//!
//! 用户的痛点是**根本不压缩**，所以偏向高估。[`SAFETY_MARGIN_PERCENT`] 就是这份
//! 余量，见那里的校准说明。

use serde_json::{Map, Value};

/// 每条消息的固定开销：role 标记与消息分隔。
const PER_MESSAGE_TOKENS: u64 = 4;
/// 每个工具定义的固定开销：包裹 schema 的那层结构。
const PER_TOOL_TOKENS: u64 = 8;
/// 一张图/一份文档的估值。Anthropic 的公式是 `宽 × 高 / 750`，典型的
/// 1092×1092 约 1590。请求里拿不到尺寸（要解码 base64 才知道），所以取典型值。
const PER_IMAGE_TOKENS: u64 = 1600;
/// 空请求也返回正数。返回 0 等于告诉客户端「这段对话是空的」，
/// 那会让它永远认为还有余量。
const MINIMUM_TOKENS: u64 = 1;

/// 往上偏的余量，百分数。110 = 高估 10%。
///
/// **这是估的，不是测的。** 校准的料现成就有，不必新写代码：
/// 每次 count_tokens 我们都把估值记进日志（`claude_bridge count_tokens … tokens=`），
/// 而上游报上下文溢出时，`claude_bridge::parse_context_overflow` 会解出**权威的**
/// token 数。两边对同一段对话一比，就知道这个系数偏了多少。
const SAFETY_MARGIN_PERCENT: u64 = 110;

// 字符类别权重，单位是 1/100 token。用定点整数而不是浮点：这样单测能断言
// 精确值，不会因平台浮点差异假红。
//
// 依据是各家 BPE 的普遍表现，不是某一家的实测：拉丁文字大约 4 个字符一个
// token，汉字/假名/谚文大约一个字一个 token，其余（西里尔、阿拉伯、emoji…）
// 介于两者之间。
const CJK_CENTI_TOKENS: u64 = 100;
const ASCII_CENTI_TOKENS: u64 = 25;
const OTHER_CENTI_TOKENS: u64 = 50;
/// `true` / `false` / `null` 这类字面量按四个字符算。
const LITERAL_CENTI_TOKENS: u64 = ASCII_CENTI_TOKENS * 4;

const CENTI: u64 = 100;

/// 一次请求的估算结果，连同调用方要记进日志的形状。
///
/// 形状一并带出来，是为了让日志不必再走一遍 JSON —— 也为了下一步能回答
/// 「Desktop 一次打 20 个 count_tokens，到底在数什么」：`messages` 和 `tools`
/// 这两个数会说话。
pub(crate) struct Estimate {
    pub(crate) tokens: u64,
    pub(crate) messages: usize,
    pub(crate) tools: usize,
    pub(crate) has_system: bool,
}

/// 估算一个 Anthropic Messages 形状的请求体。
///
/// 只认 `system` / `messages` / `tools` 三处 —— `model`、`max_tokens` 这些
/// 参数不进提示词，不该计入。
pub(crate) fn estimate_request(request: &Value) -> Estimate {
    let system = request.get("system").filter(|value| !value.is_null());
    let messages = request.get("messages").and_then(Value::as_array);
    let tools = request.get("tools").and_then(Value::as_array);

    let mut centi = system.map_or(0, value_centi_tokens);
    for message in messages.into_iter().flatten() {
        centi = centi
            .saturating_add(value_centi_tokens(message))
            .saturating_add(PER_MESSAGE_TOKENS * CENTI);
    }
    for tool in tools.into_iter().flatten() {
        centi = centi
            .saturating_add(value_centi_tokens(tool))
            .saturating_add(PER_TOOL_TOKENS * CENTI);
    }

    // 余量和「厘 → token」一步算完，所以 1.10 只乘一次；向上取整，
    // 因为这份余量的意义就是宁可多报。
    let tokens = centi
        .saturating_mul(SAFETY_MARGIN_PERCENT)
        .div_ceil(CENTI * CENTI);
    Estimate {
        tokens: tokens.max(MINIMUM_TOKENS),
        messages: messages.map_or(0, Vec::len),
        tools: tools.map_or(0, Vec::len),
        has_system: system.is_some(),
    }
}

/// 递归估一段 JSON。
///
/// 对象的**键**也计入：工具 schema 是原样序列化后进提示词的，键在那里是真
/// 消耗 token 的。内容块因此会被略微多算（`{"type":"text"}` 里的 `type`），
/// 这个方向与整体的高估偏好一致，所以不做特例。
///
/// 不认识的块类型按其 JSON 文本估，而不是跳过 —— 跳过会低估，而低估正是我们
/// 要避免的那个方向。
fn value_centi_tokens(value: &Value) -> u64 {
    match value {
        Value::String(text) => text_centi_tokens(text),
        Value::Number(number) => text_centi_tokens(&number.to_string()),
        Value::Bool(_) | Value::Null => LITERAL_CENTI_TOKENS,
        Value::Array(items) => items
            .iter()
            .fold(0, |sum, item| sum.saturating_add(value_centi_tokens(item))),
        Value::Object(fields) => {
            // 二进制附件必须在这里截住。一张 1MB 的图在 JSON 里是一串 base64，
            // 照字符算会变成二十几万 token —— 那不是高估，那是荒谬，会让
            // Desktop 每轮都压缩。
            if let Some(attachment) = attachment_centi_tokens(fields) {
                return attachment;
            }
            fields.iter().fold(0, |sum, (key, value)| {
                sum.saturating_add(text_centi_tokens(key))
                    .saturating_add(value_centi_tokens(value))
            })
        }
    }
}

/// 认出附件的 source 对象，给它定价。
///
/// **纯文本附件不走这里。** Anthropic 的文本文档长这样：
/// `{"type": "text", "media_type": "text/plain", "data": "<全文>"}` ——
/// 那个 `data` 就是正文本身。一个 3MB 的 .txt 拖进对话，按「一张图」计价会
/// 少算二十多万 token，而少算正是我们最要避免的方向。所以这里返回 `None`，
/// 让它照字数走正常的文本估算。
fn attachment_centi_tokens(fields: &Map<String, Value>) -> Option<u64> {
    let source_type = fields.get("type").and_then(Value::as_str);
    let media_type = fields.get("media_type").and_then(Value::as_str);
    if source_type == Some("text") || media_type.is_some_and(|kind| kind.starts_with("text/")) {
        return None;
    }
    // URL 形式的图字符很少，照算会把一张图当成几十个 token。
    if source_type == Some("url") && fields.contains_key("url") {
        return Some(PER_IMAGE_TOKENS * CENTI);
    }
    let data = fields.get("data").and_then(Value::as_str)?;
    let media_type = media_type?;
    if media_type.starts_with("image/") {
        return Some(PER_IMAGE_TOKENS * CENTI);
    }
    // PDF 之类的二进制：按解码后的体积走，而不是给个固定值 —— 一份 200 页的
    // PDF 和一张缩略图不该同价。base64 每 4 个字符还原 3 字节。
    //
    // 每 8 字节一个 token 是**粗糙的经验值**：Anthropic 对 PDF 按页计价
    // （每页约一千五到三千 token），而一页通常是几 KB 到几十 KB 的 PDF 字节。
    // 它注定不准，取它是因为它至少随体积变化，而固定值连方向都没有。
    let bytes = (data.len() as u64 / 4).saturating_mul(3);
    Some((bytes / 8).max(PER_IMAGE_TOKENS).saturating_mul(CENTI))
}

fn text_centi_tokens(text: &str) -> u64 {
    text.chars()
        .fold(0, |sum, ch| sum.saturating_add(char_centi_tokens(ch)))
}

fn char_centi_tokens(ch: char) -> u64 {
    if ch.is_ascii() {
        ASCII_CENTI_TOKENS
    } else if is_ideographic(ch) {
        CJK_CENTI_TOKENS
    } else {
        OTHER_CENTI_TOKENS
    }
}

/// 一个字大约就是一个 token 的那些文字：汉字、假名、谚文，以及跟它们混排的
/// 全角标点。
fn is_ideographic(ch: char) -> bool {
    matches!(ch as u32,
        0x1100..=0x11FF   // 谚文字母
        | 0x2E80..=0x2EFF // 康熙部首补充
        | 0x3000..=0x303F // 中日韩符号与标点
        | 0x3040..=0x30FF // 平假名、片假名
        | 0x3100..=0x312F // 注音符号
        | 0x3130..=0x318F // 谚文兼容字母
        | 0x31F0..=0x31FF // 片假名音标扩展
        | 0x3400..=0x4DBF // 中日韩扩展 A
        | 0x4E00..=0x9FFF // 中日韩统一表意文字
        | 0xAC00..=0xD7AF // 谚文音节
        | 0xF900..=0xFAFF // 中日韩兼容表意文字
        | 0xFF00..=0xFFEF // 全角与半角形式
        | 0x20000..=0x2FA1F // 中日韩扩展 B 及以后
    )
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    fn tokens(request: &serde_json::Value) -> u64 {
        super::estimate_request(request).tokens
    }

    /// 钉死英文的算法。改了权重或余量，这里就该红 —— 它是本模块唯一的口径。
    #[test]
    fn a_short_english_turn_costs_what_the_weights_say() {
        // "role"+"user"+"content"+"hello" = 4+4+7+5 个 ASCII = 500 厘，
        // 加每条消息 400 厘 = 900 厘 = 9 token，乘 1.1 向上取整 = 10。
        assert_eq!(
            tokens(&json!({"messages": [{"role": "user", "content": "hello"}]})),
            10
        );
    }

    /// 同样长度的中文比英文贵得多 —— 这正是「一个数配所有语言」不成立的地方。
    #[test]
    fn the_same_turn_in_chinese_costs_more_than_in_english() {
        // 四个汉字各 100 厘，其余键值同上：775 + 400 = 1175 厘，乘 1.1 = 13。
        assert_eq!(
            tokens(&json!({"messages": [{"role": "user", "content": "你好世界"}]})),
            13
        );
    }

    /// 余量只乘一次。乘两次的话这里会是 121。
    #[test]
    fn the_margin_is_applied_once_not_twice() {
        let system = "a".repeat(400); // 400 × 25 厘 = 10000 厘 = 100 token
        assert_eq!(tokens(&json!({ "system": system })), 110);
    }

    /// 一张 base64 图不能按字符算。1MB 的 base64 会变成二十几万 token，
    /// Desktop 会以为每一轮都撑爆，于是每一轮都压缩。
    #[test]
    fn a_base64_image_costs_the_same_however_large_the_blob_is() {
        let image = |bytes: usize| {
            json!({"messages": [{"role": "user", "content": [{
                "type": "image",
                "source": {
                    "type": "base64",
                    "media_type": "image/png",
                    "data": "A".repeat(bytes),
                },
            }]}]})
        };
        let small = tokens(&image(64));
        assert_eq!(small, tokens(&image(1_000_000)));
        // 而且确实按一张图计了价，不是被当成几十个字符忽略过去。
        assert!(small > super::PER_IMAGE_TOKENS, "small={small}");
    }

    /// 拖一个 .txt 进对话，它的 `data` 就是正文 —— 必须照字数算。
    ///
    /// 按「一张图」计价的话，三百万字符会变成 1600 token，少算二十多万。
    /// 这正好是「验证自动压缩」这件事本身会踩的坑：用一个大文本文件把窗口
    /// 填满，结果我们报了 1600，什么也测不出来。
    #[test]
    fn a_text_attachment_is_counted_by_its_words_not_as_one_image() {
        let document = |chars: usize| {
            json!({"messages": [{"role": "user", "content": [{
                "type": "document",
                "source": {
                    "type": "text",
                    "media_type": "text/plain",
                    "data": "word ".repeat(chars / 5),
                },
            }]}]})
        };
        let small = tokens(&document(5_000));
        let large = tokens(&document(500_000));
        // 一百倍的正文，token 数就该是一百倍上下 —— 不是同一个固定值。
        assert!(large > small * 50, "small={small} large={large}");
        // 而且确实按 ASCII 的分量算：500000 字符 × 0.25 ≈ 125000 token。
        assert!(
            (120_000..150_000).contains(&large),
            "large={large} 不在合理区间"
        );
    }

    /// 二进制附件按体积走，而不是一个固定值 —— 两百页的 PDF 和一张缩略图
    /// 不该同价。
    #[test]
    fn a_binary_attachment_scales_with_its_size() {
        let pdf = |bytes: usize| {
            json!({"messages": [{"role": "user", "content": [{
                "type": "document",
                "source": {
                    "type": "base64",
                    "media_type": "application/pdf",
                    "data": "A".repeat(bytes),
                },
            }]}]})
        };
        assert!(tokens(&pdf(4_000_000)) > tokens(&pdf(40_000)));
        // 但小附件不会掉到图片的地板价以下。
        assert!(tokens(&pdf(64)) >= super::PER_IMAGE_TOKENS);
    }

    /// URL 形式的图字符很少，照算会便宜得离谱 —— 它跟 base64 一张价。
    #[test]
    fn a_url_image_is_priced_as_an_image_not_as_its_link() {
        let request = json!({"messages": [{"role": "user", "content": [{
            "type": "image",
            "source": {"type": "url", "url": "https://example.test/a.png"},
        }]}]});
        assert!(tokens(&request) > super::PER_IMAGE_TOKENS);
    }

    /// 对话只会变长。任何一版估算都不能在加了一轮之后报出更小的数 ——
    /// 那会让 Desktop 把「该压缩了」判断成「还早」。
    #[test]
    fn adding_a_turn_never_lowers_the_count() {
        let mut messages = vec![];
        let mut previous = 0;
        for turn in 0..12 {
            messages.push(json!({
                "role": if turn % 2 == 0 { "user" } else { "assistant" },
                "content": format!("第 {turn} 轮，说点什么 with some English too"),
            }));
            let current = tokens(&json!({ "messages": messages }));
            assert!(current > previous, "turn {turn}: {current} <= {previous}");
            previous = current;
        }
    }

    /// 工具定义是真进提示词的，schema 的键也算钱。
    #[test]
    fn tool_definitions_are_counted_schema_and_all() {
        let bare = json!({"messages": [{"role": "user", "content": "hi"}]});
        let armed = json!({
            "messages": [{"role": "user", "content": "hi"}],
            "tools": [{
                "name": "read_file",
                "description": "Read a file from disk and return its contents.",
                "input_schema": {
                    "type": "object",
                    "properties": {"path": {"type": "string", "description": "Absolute path"}},
                    "required": ["path"],
                },
            }],
        });
        assert!(tokens(&armed) > tokens(&bare) + super::PER_TOOL_TOKENS);
    }

    /// 空请求也要是正数。报 0 等于说「这段对话是空的」。
    #[test]
    fn an_empty_request_still_counts_as_something() {
        assert_eq!(tokens(&json!({})), super::MINIMUM_TOKENS);
        assert_eq!(tokens(&json!({"messages": []})), super::MINIMUM_TOKENS);
    }

    /// `content` 既可以是字符串也可以是块数组，两种都得走通。
    /// 这个端点一次对话要被打二十次，任何一种形状 panic 都是二十次 500。
    #[test]
    fn neither_shape_of_content_can_panic() {
        let shapes = [
            json!({"messages": [{"role": "user", "content": "plain string"}]}),
            json!({"messages": [{"role": "user", "content": [{"type": "text", "text": "block"}]}]}),
            json!({"messages": [{"role": "user", "content": []}]}),
            json!({"messages": [{"role": "user"}]}),
            json!({"messages": [{"content": {"unexpected": "object"}}]}),
            json!({"messages": "not an array", "tools": 7, "system": null}),
            json!({"messages": [{"role": "user", "content": [
                {"type": "thinking", "thinking": "hmm"},
                {"type": "tool_use", "id": "t1", "name": "read", "input": {"path": "/tmp"}},
                {"type": "tool_result", "tool_use_id": "t1", "content": "ok"},
                {"type": "something_new_anthropic_added", "payload": {"nested": [1, true, null]}},
            ]}]}),
        ];
        for shape in shapes {
            assert!(tokens(&shape) >= super::MINIMUM_TOKENS, "{shape}");
        }
    }

    /// 形状是随估算一起带出来的，日志靠它 —— 下一轮要用这两个数回答
    /// 「一次打二十个 count_tokens，到底在数什么」。
    #[test]
    fn the_shape_reported_alongside_the_count_matches_the_request() {
        let estimate = super::estimate_request(&json!({
            "system": "be brief",
            "messages": [{"role": "user", "content": "hi"}, {"role": "assistant", "content": "yo"}],
            "tools": [{"name": "a"}, {"name": "b"}, {"name": "c"}],
        }));
        assert_eq!(estimate.messages, 2);
        assert_eq!(estimate.tools, 3);
        assert!(estimate.has_system);

        // `"system": null` 是「没有系统提示词」，不是「有一个空的」。
        let absent = super::estimate_request(&json!({"messages": [], "system": null}));
        assert!(!absent.has_system);
    }

    /// 参数不进提示词，不该计入。`max_tokens` 变大不代表对话变长。
    #[test]
    fn request_parameters_are_not_mistaken_for_prompt() {
        let lean = json!({"messages": [{"role": "user", "content": "hi"}]});
        let padded = json!({
            "messages": [{"role": "user", "content": "hi"}],
            "model": "claude-sonnet-5-v0123456789abcdef",
            "max_tokens": 64000,
            "temperature": 1,
            "stream": true,
        });
        assert_eq!(tokens(&lean), tokens(&padded));
    }
}

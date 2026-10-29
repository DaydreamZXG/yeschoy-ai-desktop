//! 在 JSONC（JSON with comments）原文上做最小编辑，而不是整份重排。
//!
//! 为什么需要它：适配器一直用 `json5` 解析、`serde_json::to_vec_pretty`
//! 写回。语义没错，但写回的是「我们重新排版过的版本」——用户写在
//! `models.json` / `opencode.jsonc` 里的每一行注释、每一处自定义缩进、每一个
//! 尾随逗号，都会在接入那一刻消失。而产品承诺的是「只动我们自己的那一块」，
//! 注释也是用户的东西。
//!
//! 做法分两层：
//!
//! 1. `Scanner`：只在**词法**层扫描一遍原文，记下每个值的位置和成员关系。
//!    它不解释语义 —— 调用方已经用 `json5` 确认过原文可解析，这里只负责回答
//!    「值在原文的哪儿」。
//! 2. `apply`：把编辑映射回原文的位置，只替换那几处；没被点名的字节原样保留。
//!    新成员沿用最后一个成员的缩进，并把值里的换行按同样的宽度重新缩进。
//!
//! 失败时一律返回 `None`，由调用方退回整份重排：宁可丢注释，也不能写坏
//! 一份用户配置，更不能因为括号对不上而改错位置。

/// 原文里一个值的起止范围（字节偏移，左闭右开）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Span {
    start: usize,
    end: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Segment<'a> {
    Key(&'a str),
    /// 数组下标。适配器写的都是对象成员，保留它是因为 `resolve` 要能处理
    /// 数组路径（否则一段路径错位就会静默改到别处），并由测试覆盖。
    #[allow(dead_code)]
    Index(usize),
}

/// 一次编辑。
pub(crate) enum Edit<'a> {
    /// 把 `path` 指向的值换成 `text`；值不存在时，在父容器里新增同名成员。
    Set {
        path: Vec<Segment<'a>>,
        text: String,
    },
    /// 删掉 `path` 指向的成员。适配器目前只增不减，保留并测试它，是因为
    /// 恢复路径迟早需要它，而隔很久再补的删除逻辑最容易留尾逗号。
    #[allow(dead_code)]
    Remove { path: Vec<Segment<'a>> },
}

struct Node {
    span: Span,
    /// 对象成员：(键名, 值节点下标)。重复键保留最后一次出现。
    members: Vec<(String, usize)>,
    /// 数组元素：值节点下标。
    items: Vec<usize>,
}

/// 容器里成员的排版，用来让新成员和已有成员对齐。
struct Layout {
    indent: String,
    unit: String,
}

impl Default for Layout {
    fn default() -> Self {
        Self {
            indent: String::new(),
            unit: "  ".to_owned(),
        }
    }
}

struct Scanner<'s> {
    source: &'s str,
    bytes: &'s [u8],
    at: usize,
    nodes: Vec<Node>,
    root: Option<usize>,
}

impl<'s> Scanner<'s> {
    fn new(source: &'s str) -> Self {
        Self {
            source,
            bytes: source.as_bytes(),
            at: 0,
            nodes: Vec::new(),
            root: None,
        }
    }

    fn scan_document(mut self) -> Option<Self> {
        self.skip_trivia()?;
        let root = self.value()?;
        self.skip_trivia()?;
        if self.at != self.bytes.len() {
            return None;
        }
        self.root = Some(root);
        Some(self)
    }

    fn skip_trivia(&mut self) -> Option<()> {
        loop {
            while self.at < self.bytes.len() && self.bytes[self.at].is_ascii_whitespace() {
                self.at += 1;
            }
            if self.source[self.at..].starts_with("//") {
                match self.source[self.at..].find('\n') {
                    Some(offset) => self.at += offset + 1,
                    None => self.at = self.bytes.len(),
                }
                continue;
            }
            if self.source[self.at..].starts_with("/*") {
                self.at += self.source[self.at + 2..].find("*/")? + 4;
                continue;
            }
            return Some(());
        }
    }

    fn push(&mut self, span: Span, members: Vec<(String, usize)>, items: Vec<usize>) -> usize {
        self.nodes.push(Node {
            span,
            members,
            items,
        });
        self.nodes.len() - 1
    }

    /// 解析一个值。**先跳过空白与注释再记起点**：值的位置必须从值的第一个
    /// 字符开始，否则前面的缩进会被算进值里，删除成员时就会删掉它的值、
    /// 把缩进留在原地。
    fn value(&mut self) -> Option<usize> {
        self.skip_trivia()?;
        let start = self.at;
        match *self.bytes.get(start)? {
            b'{' => {
                self.at = start + 1;
                let mut members: Vec<(String, usize)> = Vec::new();
                loop {
                    self.skip_trivia()?;
                    if self.bytes.get(self.at) == Some(&b'}') {
                        self.at += 1;
                        break;
                    }
                    let key = self.key()?;
                    self.skip_trivia()?;
                    if self.bytes.get(self.at) != Some(&b':') {
                        return None;
                    }
                    self.at += 1;
                    let child = self.value()?;
                    // 重复键：后出现的覆盖先出现的，与 JSON 语义一致。
                    members.retain(|(existing, _)| existing != &key);
                    members.push((key, child));
                    self.skip_trivia()?;
                    match self.bytes.get(self.at) {
                        Some(b',') => self.at += 1,
                        Some(b'}') => {
                            self.at += 1;
                            break;
                        }
                        _ => return None,
                    }
                }
                Some(self.push(
                    Span {
                        start,
                        end: self.at,
                    },
                    members,
                    Vec::new(),
                ))
            }
            b'[' => {
                self.at = start + 1;
                let mut items = Vec::new();
                loop {
                    self.skip_trivia()?;
                    if self.bytes.get(self.at) == Some(&b']') {
                        self.at += 1;
                        break;
                    }
                    items.push(self.value()?);
                    self.skip_trivia()?;
                    match self.bytes.get(self.at) {
                        Some(b',') => self.at += 1,
                        Some(b']') => {
                            self.at += 1;
                            break;
                        }
                        _ => return None,
                    }
                }
                Some(self.push(
                    Span {
                        start,
                        end: self.at,
                    },
                    Vec::new(),
                    items,
                ))
            }
            b'"' | b'\'' => {
                self.string()?;
                Some(self.push(
                    Span {
                        start,
                        end: self.at,
                    },
                    Vec::new(),
                    Vec::new(),
                ))
            }
            _ => {
                // 字面量：读到分隔符为止。合法性由调用方的 `json5` 负责。
                while self.at < self.bytes.len()
                    && !matches!(
                        self.bytes[self.at],
                        b',' | b'}' | b']' | b'/' | b'\n' | b'\r' | b' ' | b'\t'
                    )
                {
                    self.at += 1;
                }
                if self.at == start {
                    return None;
                }
                Some(self.push(
                    Span {
                        start,
                        end: self.at,
                    },
                    Vec::new(),
                    Vec::new(),
                ))
            }
        }
    }

    /// 读一个对象键：带引号的字符串，或 JSON5 允许的无引号标识符。
    fn key(&mut self) -> Option<String> {
        if matches!(self.bytes.get(self.at), Some(b'"' | b'\'')) {
            return self.string();
        }
        let start = self.at;
        while self.at < self.bytes.len()
            && (self.bytes[self.at].is_ascii_alphanumeric()
                || matches!(self.bytes[self.at], b'_' | b'$')
                || self.bytes[self.at] >= 0x80)
        {
            self.at += 1;
        }
        (self.at > start).then(|| self.source[start..self.at].to_owned())
    }

    /// 读一个字符串字面量并返回其内容（用于对象键）。
    fn string(&mut self) -> Option<String> {
        let quote = *self.bytes.get(self.at)?;
        if !matches!(quote, b'"' | b'\'') {
            return None;
        }
        self.at += 1;
        let mut out = String::new();
        loop {
            let byte = *self.bytes.get(self.at)?;
            if byte == quote {
                self.at += 1;
                return Some(out);
            }
            if byte != b'\\' {
                let ch = self.source[self.at..].chars().next()?;
                out.push(ch);
                self.at += ch.len_utf8();
                continue;
            }
            self.at += 1;
            let escape = *self.bytes.get(self.at)?;
            self.at += 1;
            match escape {
                b'n' => out.push('\n'),
                b't' => out.push('\t'),
                b'r' => out.push('\r'),
                b'b' => out.push('\u{8}'),
                b'f' => out.push('\u{c}'),
                b'0' => out.push('\0'),
                b'u' => {
                    let code =
                        u32::from_str_radix(self.source.get(self.at..self.at + 4)?, 16).ok()?;
                    self.at += 4;
                    let ch = if (0xD800..0xDC00).contains(&code) {
                        let low = self.source.get(self.at..self.at + 6)?;
                        let low = u32::from_str_radix(low.strip_prefix("\\u")?, 16).ok()?;
                        self.at += 6;
                        char::from_u32(0x1_0000 + ((code - 0xD800) << 10) + (low - 0xDC00))?
                    } else {
                        char::from_u32(code)?
                    };
                    out.push(ch);
                }
                other => out.push(other as char),
            }
        }
    }

    fn resolve(&self, path: &[Segment<'_>]) -> Option<usize> {
        let mut node = self.root?;
        for segment in path {
            node = match segment {
                Segment::Key(key) => self.nodes[node]
                    .members
                    .iter()
                    .rev()
                    .find(|(name, _)| name == key)
                    .map(|(_, child)| *child)?,
                Segment::Index(index) => *self.nodes[node].items.get(*index)?,
            };
        }
        Some(node)
    }

    /// 某一行的缩进（从 `at` 所在行开头算）。
    fn line_indent(&self, at: usize) -> String {
        let line_start = self.source[..at].rfind('\n').map_or(0, |i| i + 1);
        self.source[line_start..at]
            .chars()
            .take_while(|c| *c == ' ' || *c == '\t')
            .collect()
    }

    /// 容器里成员的缩进与一级宽度：取最后一个成员所在行。
    fn layout_of(&self, parent: usize) -> Layout {
        let node = &self.nodes[parent];
        let last = node
            .members
            .last()
            .map(|(_, value)| *value)
            .or_else(|| node.items.last().copied());
        let Some(last) = last else {
            let close = self.line_indent(node.span.end - 1).len();
            let _ = close;
            return Layout::default();
        };
        let start = self.nodes[last].span.start;
        let line_start = self.source[..start].rfind('\n').map_or(0, |i| i + 1);
        let indent = self.line_indent(start);
        // 内联容器（`{"a": {"b": 1}}`）没有独立缩进，按两空格一级。
        if indent.is_empty() && self.source[line_start..start].contains(['{', '[']) {
            return Layout::default();
        }
        let parent_indent = self.line_indent(node.span.start);
        let unit = indent
            .strip_prefix(parent_indent.as_str())
            .filter(|unit| !unit.is_empty())
            .unwrap_or("  ")
            .to_owned();
        Layout { indent, unit }
    }

    /// 按容器排版重新缩进一段值文本。第一行接在 `"key": ` 后面，不缩进；
    /// 之后每一行按嵌套深度缩进，收尾括号少一级。单行的值原样返回。
    fn render(&self, text: &str, layout: &Layout) -> String {
        let mut out = String::new();
        let mut depth = 0usize;
        let mut in_string = false;
        let mut escaped = false;
        for (index, line) in text.split('\n').enumerate() {
            let trimmed = line.trim();
            if index > 0 {
                out.push('\n');
                if !trimmed.is_empty() {
                    let levels = if matches!(trimmed.chars().next(), Some('}' | ']')) {
                        depth.saturating_sub(1)
                    } else {
                        depth
                    };
                    for _ in 0..levels {
                        out.push_str(&layout.unit);
                    }
                    out.push_str(&layout.indent);
                }
            }
            for ch in trimmed.chars() {
                match ch {
                    '"' if !escaped => in_string = !in_string,
                    '{' | '[' if !in_string => depth += 1,
                    '}' | ']' if !in_string => depth = depth.saturating_sub(1),
                    _ => {}
                }
                escaped = ch == '\\' && !escaped;
                if ch != '\\' {
                    escaped = false;
                }
                out.push(ch);
            }
        }
        out
    }

    /// 在对象 `parent` 里写入成员 `key`（已存在则替换）。
    fn set_member(&self, parent: usize, key: &str, text: &str) -> Option<(Span, String)> {
        let node = &self.nodes[parent];
        if let Some((_, existing)) = node.members.iter().rev().find(|(name, _)| name == key) {
            let layout = self.layout_of(parent);
            let span = self.nodes[*existing].span;
            return Some((span, self.render(text, &layout)));
        }
        let close = node.span.end - 1;
        if self.bytes.get(close) != Some(&b'}') {
            return None;
        }
        let layout = self.layout_of(parent);
        let value_layout = Layout {
            indent: String::new(),
            unit: layout.unit.clone(),
        };
        let member = format!("\"{key}\": {}", self.render(text, &value_layout));
        let close_indent = self.line_indent(close);
        let Some(last) = node.members.last().map(|(_, child)| *child) else {
            // 空对象：没有兄弟可以对齐，按括号所在行的缩进再加一级。
            let indent = format!("{close_indent}{}", layout.unit);
            return Some((
                Span {
                    start: node.span.start + 1,
                    end: close,
                },
                format!("\n{indent}{member}\n{close_indent}"),
            ));
        };
        // 范围从最后一个成员的末尾一直到右括号。其中若是「逗号 + 空白 +
        // 注释」，注释属于那个成员，原样保留并把新成员插在它后面。
        let from = self.nodes[last].span.end;
        let between = &self.source[from..close];
        let (_, comments) = scan_between(between)?;
        // 行尾注释：注释前面只有逗号与空白、且没有换行。
        let same_line_comment = between
            .find("//")
            .is_some_and(|at| !between[..at].contains('\n') && between[..at].trim().len() <= 1);
        let mut trailing = String::new();
        for (index, line) in comments
            .lines()
            .filter(|l| !l.trim().is_empty())
            .enumerate()
        {
            if index == 0 && same_line_comment {
                // 紧跟在值同一行的行尾注释留在原地。
                trailing.push(' ');
                trailing.push_str(line.trim());
            } else {
                trailing.push('\n');
                trailing.push_str(&layout.indent);
                trailing.push_str(line.trim());
            }
        }
        // 内联容器（`{ local: {...} }`）没有可借用的换行结构：往里塞一个多行
        // 成员会把右括号挤到值后面。这类容器交给调用方整份重排 —— 丢注释
        // 好过写坏文件。
        if !self.source[self.nodes[parent].span.start..close].contains('\n') {
            return None;
        }
        // 从最后一个成员的末尾开始替换，逗号由这里重新写：原文可能已有逗号
        // 也可能没有（尾随逗号），两种都收敛成恰好一个。
        let rendered = format!(",{trailing}\n{}{member}\n{close_indent}", layout.indent);
        Some((
            Span {
                start: from,
                end: close,
            },
            rendered,
        ))
    }

    /// 在对象 `parent` 里一次写入多个成员（保持给定顺序）。
    ///
    /// 换行容器的做法：保留原有成员那段文本（含注释）原样，跟着补一个逗号，
    /// 再按成员缩进写出新成员，最后对齐右括号。内联容器（整行 `{ a: 1 }`）
    /// 没有可借用的换行结构，交给调用方整份重排。
    fn set_members(&self, parent: usize, members: &[(String, String)]) -> Option<(Span, String)> {
        if members.len() == 1 {
            return self.set_member(parent, &members[0].0, &members[0].1);
        }
        let node = &self.nodes[parent];
        let close = node.span.end - 1;
        if self.bytes.get(close) != Some(&b'}') || members.is_empty() {
            return None;
        }
        let open = node.span.start;
        if !self.source[open..close].contains('\n') {
            return None;
        }
        let layout = self.layout_of(parent);
        let close_indent = self.line_indent(close);
        let value_layout = Layout {
            indent: String::new(),
            unit: layout.unit.clone(),
        };
        let rendered = members
            .iter()
            .map(|(key, text)| format!("\"{key}\": {}", self.render(text, &value_layout)))
            .collect::<Vec<_>>()
            .join(&format!(",\n{}", layout.indent));
        let Some(last) = node.members.last().map(|(_, child)| *child) else {
            return Some((
                Span {
                    start: open + 1,
                    end: close,
                },
                {
                    let indent = format!("{close_indent}{}", layout.unit);
                    format!("\n{indent}{rendered}\n{close_indent}")
                },
            ));
        };
        // 保留最后一个成员之后的注释（它属于那个成员），只重写逗号结构。
        let from = self.nodes[last].span.end;
        let between = &self.source[from..close];
        let (_, comments) = scan_between(between)?;
        let same_line_comment = between
            .find("//")
            .is_some_and(|at| !between[..at].contains('\n') && between[..at].trim().len() <= 1);
        let mut trailing = String::new();
        for (index, line) in comments
            .lines()
            .filter(|l| !l.trim().is_empty())
            .enumerate()
        {
            if index == 0 && same_line_comment {
                trailing.push(' ');
                trailing.push_str(line.trim());
            } else {
                trailing.push('\n');
                trailing.push_str(&layout.indent);
                trailing.push_str(line.trim());
            }
        }
        Some((
            Span {
                start: from,
                end: close,
            },
            format!(",{trailing}\n{}{rendered}\n{close_indent}", layout.indent),
        ))
    }

    /// 删除对象成员 `key` 时该切掉的范围。
    ///
    /// 只吞掉**它自己后面**的逗号：第一个成员被删掉后，后一个成员自然成为
    /// 第一个，语法依然成立；去删前一个成员的逗号会误伤别人。
    ///
    /// 「整行删除」的判据是**行首到键名之间只有空白**，不是「值独占一行」：
    /// 值的起点前面永远有键名和冒号，拿它去比永远是 false，结果是只删掉值、
    /// 把 `"key": ` 和缩进留在原地。
    fn removal_span(&self, parent: usize, key: &str) -> Option<Span> {
        let node = &self.nodes[parent];
        let (_, child) = node.members.iter().find(|(name, _)| name == key)?;
        let value = self.nodes[*child].span;
        // 键名（含引号）在值前面，同一个成员。
        let key_start = self.source[..value.start]
            .rfind(&format!("\"{key}\""))
            .unwrap_or(value.start);
        let line_start = self.source[..key_start].rfind('\n').map_or(0, |i| i + 1);
        // 「整行删除」的判据是行首到键名之间只有空白。用值的起点去判永远是
        // false —— 值前面总有键名和冒号。
        if !self.source[line_start..key_start].trim().is_empty() {
            // 成员和别的东西挤在同一行（`{"a": 1, "b": 2}`）：只切成员本身和
            // 它后面紧跟的逗号，不留尾逗号问题（后面还有成员）。
            let mut end = value.end;
            if let Some(comma) = self.source[end..].find(',') {
                if self.source[end..end + comma].trim().is_empty() {
                    end += comma + 1;
                }
            }
            return Some(Span {
                start: key_start,
                end,
            });
        }
        // 整行删除：吞掉这一行里成员后面的逗号和行尾注释，连同换行。
        let mut end = value.end;
        // 切到这一行的行尾，**不**包含换行：换行属于下一行，留着它 `}` 才会
        // 继续独占一行而不贴到上一个成员后面。
        let line_end = self.source[end..]
            .find('\n')
            .map(|offset| end + offset)
            .unwrap_or(self.source.len());
        let remainder = self.source[end..line_end].trim();
        let only_comma_and_comment = remainder.is_empty()
            || remainder == ","
            || (remainder.starts_with(',') && remainder[1..].trim().starts_with("//"))
            || remainder.starts_with("//");
        if only_comma_and_comment {
            // 后面还有成员：把换行也吞掉，免得留一个空行。
            // 后面就是右括号：只到行尾，换行留着让 `}` 独占一行。
            let has_later = {
                let after = &self.source[line_end..];
                after.trim_start().starts_with('"') || after.trim_start().starts_with('\'')
            };
            end = if has_later && line_end < self.source.len() {
                line_end + 1
            } else {
                line_end
            };
        }
        let mut start = line_start;
        // 删的是最后一个成员（这一行没有逗号）：前一个成员的逗号也要去掉，
        // 否则留下 `{"a": 1,}` —— JSON5 认，很多只吃 JSON 的工具不认。
        if !remainder.starts_with(',') {
            let prefix = &self.source[..line_start];
            if let Some(comma_at) = prefix.rfind(',') {
                if prefix[comma_at + 1..].trim().is_empty() {
                    start = comma_at;
                }
            }
        }
        Some(Span { start, end })
    }
}

/// 检查两个兄弟节点之间的文本是不是只有逗号、空白和注释。
/// 返回「有没有逗号」和「只含注释的部分」（重新对齐缩进用）。
fn scan_between(between: &str) -> Option<(bool, String)> {
    let bytes = between.as_bytes();
    let mut at = 0;
    let mut comma = false;
    let mut comments = String::new();
    while at < bytes.len() {
        match bytes[at] {
            b' ' | b'\t' | b'\r' | b'\n' => at += 1,
            b',' if !comma => {
                comma = true;
                at += 1;
            }
            b'/' if between[at..].starts_with("//") => {
                let end = at + between[at..].find('\n').unwrap_or(between.len() - at);
                comments.push_str(&between[at..end]);
                comments.push('\n');
                at = end;
            }
            b'/' if between[at..].starts_with("/*") => {
                let end = at + between[at + 2..].find("*/")? + 4;
                comments.push_str(&between[at..end]);
                comments.push('\n');
                at = end;
            }
            _ => return None,
        }
    }
    Some((comma, comments))
}

/// 找到路径上最深的、原文里确实存在的祖先，以及它下面缺的那几段。
fn deepest_existing<'a>(
    scanner: &Scanner<'_>,
    path: &[Segment<'a>],
) -> Option<(Vec<Segment<'a>>, Vec<&'a str>)> {
    for cut in (0..=path.len()).rev() {
        if scanner.resolve(&path[..cut]).is_some() {
            let missing = path[cut..]
                .iter()
                .map(|segment| match segment {
                    Segment::Key(key) => Some(*key),
                    Segment::Index(_) => None,
                })
                .collect::<Option<Vec<_>>>()?;
            return Some((path[..cut].to_vec(), missing));
        }
    }
    None
}

/// 把若干编辑应用到原文。任何一处对不上就整次放弃。
///
/// 同一个对象里新增多个成员时，它们共享同一个插入点。分两次插入会得到两个
/// 落在同一位置的范围（互相重叠而放弃），所以这里按父容器归组、一次写全。
pub(crate) fn apply(source: &str, edits: &[Edit<'_>]) -> Option<String> {
    if edits.is_empty() {
        return Some(source.to_owned());
    }
    let scanner = Scanner::new(source).scan_document()?;
    let mut replacements: Vec<(Span, String)> = Vec::new();
    // 新增成员按父容器归组：父容器节点 → [(键, 值文本)]。
    let mut insertions: Vec<(usize, Vec<(String, String)>)> = Vec::new();
    for edit in edits {
        match edit {
            Edit::Set { path, text } => {
                let Some((Segment::Key(key), rest)) = path.split_last() else {
                    return None;
                };
                // 路径中间缺一层（`provider.yeschoy` 里的 `provider` 还不存在）
                // 时不能直接放弃：那会让整份配置回退重排。先在最近的已有祖先
                // 下补出中间层，再落到这一次编辑上。
                let Some(parent) = scanner.resolve(rest) else {
                    let (ancestor_path, missing) = deepest_existing(&scanner, rest)?;
                    let ancestor = scanner.resolve(&ancestor_path)?;
                    // 缺的每一段都要变成一层对象；叶子段（`key`）装的是值本身。
                    let mut nested = text.clone();
                    let mut layers: Vec<&str> = vec![key];
                    layers.extend(missing[1..].iter().rev().copied());
                    for name in layers {
                        nested = format!("{{\"{name}\": {nested}}}");
                    }
                    let top = missing[0].to_string();
                    match insertions.iter_mut().find(|(node, _)| *node == ancestor) {
                        Some((_, members)) => members.push((top, nested)),
                        None => insertions.push((ancestor, vec![(top, nested)])),
                    }
                    continue;
                };
                let exists = scanner.nodes[parent]
                    .members
                    .iter()
                    .any(|(name, _)| name == key);
                if exists {
                    let r = scanner.set_member(parent, key, text);
                    let (span, rendered) = r?;
                    replacements.push((span, rendered));
                } else {
                    match insertions.iter_mut().find(|(node, _)| *node == parent) {
                        Some((_, members)) => members.push((key.to_string(), text.clone())),
                        None => insertions.push((parent, vec![(key.to_string(), text.clone())])),
                    }
                }
            }
            Edit::Remove { path } => {
                let Some((Segment::Key(key), rest)) = path.split_last() else {
                    continue;
                };
                let Some(parent) = scanner.resolve(rest) else {
                    continue;
                };
                if let Some(span) = scanner.removal_span(parent, key) {
                    replacements.push((span, String::new()));
                }
            }
        }
    }
    for (parent, members) in insertions {
        replacements.push(scanner.set_members(parent, &members)?);
    }
    replacements.sort_by_key(|(span, _)| span.start);
    for pair in replacements.windows(2) {
        if pair[0].0.end > pair[1].0.start {
            return None;
        }
    }
    let mut out = String::with_capacity(source.len() + 256);
    let mut cursor = 0;
    for (span, text) in replacements {
        out.push_str(&source[cursor..span.start]);
        out.push_str(&text);
        cursor = span.end;
    }
    out.push_str(&source[cursor..]);
    Some(out)
}

/// 便捷入口：把若干 `(路径, 新值)` 落到原文上。
///
/// 值没变的路径会被跳过（原文一个字节都不动）；有变化的路径做最小编辑；
/// 任何一处对不上就返回 `None`，调用方退回整份重排。
pub(crate) fn apply_values(
    source: &str,
    existing: &serde_json::Value,
    values: &[(Vec<Segment<'_>>, serde_json::Value)],
) -> Option<String> {
    let mut edits = Vec::new();
    for (path, value) in values {
        if resolve_value(existing, path).is_some_and(|current| current == value) {
            continue;
        }
        edits.push(Edit::Set {
            path: path.clone(),
            text: serde_json::to_string_pretty(value).ok()?,
        });
    }
    apply(source, &edits)
}

fn resolve_value<'a>(
    mut value: &'a serde_json::Value,
    path: &[Segment<'_>],
) -> Option<&'a serde_json::Value> {
    for segment in path {
        value = match segment {
            Segment::Key(key) => value.get(*key)?,
            Segment::Index(index) => value.get(*index)?,
        };
    }
    Some(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set<'a>(path: &[Segment<'a>], text: &str) -> Edit<'a> {
        Edit::Set {
            path: path.to_vec(),
            text: text.to_owned(),
        }
    }

    fn key(name: &str) -> Segment<'_> {
        Segment::Key(name)
    }

    const COMMENTED: &str = r#"{
  // 用户自己的分组
  "defaultProvider": "anthropic",
  "theme": "dark", // 行尾注释
  /* 块注释 */
  "nested": { "keep": [1, 2, 3], },
}
"#;

    #[test]
    fn untouched_bytes_survive_including_comments_and_trailing_commas() {
        let out = apply(COMMENTED, &[set(&[key("defaultProvider")], "\"yeschoy\"")]).unwrap();
        assert_eq!(out, COMMENTED.replace("\"anthropic\"", "\"yeschoy\""));
    }

    #[test]
    fn a_new_member_follows_the_existing_indentation() {
        let source = "{\n    \"a\": 1,\n    \"b\": 2\n}\n";
        let out = apply(source, &[set(&[key("defaultProvider")], "\"yeschoy\"")]).unwrap();
        assert_eq!(
            out,
            "{\n    \"a\": 1,\n    \"b\": 2,\n    \"defaultProvider\": \"yeschoy\"\n}\n"
        );
    }

    #[test]
    fn a_new_member_lands_after_the_comment_that_belongs_to_the_last_one() {
        let source = "{\n  \"a\": 1 // keep me\n}\n";
        let out = apply(source, &[set(&[key("b")], "2")]).unwrap();
        assert_eq!(out, "{\n  \"a\": 1, // keep me\n  \"b\": 2\n}\n");
    }

    #[test]
    fn a_new_member_in_an_empty_object_opens_the_braces() {
        assert_eq!(
            apply("{}", &[set(&[key("a")], "1")]).unwrap(),
            "{\n  \"a\": 1\n}"
        );
    }

    #[test]
    fn a_multiline_value_is_reindented_to_fit_the_container() {
        let source = "{\n  \"a\": 0,\n  \"b\": 2\n}\n";
        let out = apply(source, &[set(&[key("a")], "{\n\"x\": 1,\n\"y\": [2]\n}")]).unwrap();
        assert_eq!(
            out,
            "{\n  \"a\": {\n    \"x\": 1,\n    \"y\": [2]\n  },\n  \"b\": 2\n}\n"
        );
    }

    #[test]
    fn nested_object_paths_are_located() {
        let source = "{\n  'providers': {\n    'a': {\n      'list': [10, 20]\n    }\n  },\n}\n";
        let out = apply(
            source,
            &[set(&[key("providers"), key("a"), key("list")], "[10, 21]")],
        )
        .unwrap();
        assert_eq!(
            out,
            "{\n  'providers': {\n    'a': {\n      'list': [10, 21]\n    }\n  },\n}\n"
        );
        let out = apply(
            source,
            &[set(&[key("providers"), key("a"), key("model")], "\"m\"")],
        )
        .unwrap();
        assert!(out.contains("\n      \"model\": \"m\"\n    }"), "{out}");
    }

    #[test]
    fn a_missing_intermediate_object_is_created() {
        let src =
            "{\n  // keep\n  theme: 'x',\n  provider: {\n    ollama: { npm: 'n' },\n  },\n}\n";
        let original: serde_json::Value = json5::from_str(src).unwrap();
        let out = apply_values(
            src,
            &original,
            &[(
                vec![key("provider"), key("yeschoy")],
                serde_json::json!({"npm": "x"}),
            )],
        );
        let out = out.expect("should create provider.yeschoy");
        let v: serde_json::Value = json5::from_str(&out).unwrap();
        assert_eq!(v["provider"]["yeschoy"]["npm"], "x", "{out}");
        assert!(out.contains("// keep"), "{out}");
    }

    #[test]
    fn inserting_a_nested_object_replaces_the_value_not_its_parent() {
        // 单行根对象属于「内联容器」，最小编辑会主动让位（见 `set_member`）。
        let src = "{\n  \"theme\": \"keep\"\n}\n";
        let original: serde_json::Value = json5::from_str(src).unwrap();
        let out = apply_values(
            src,
            &original,
            &[(
                vec![key("provider")],
                serde_json::json!({"yeschoy": {"npm": "x"}}),
            )],
        )
        .unwrap();
        let v: serde_json::Value = json5::from_str(&out).unwrap();
        assert_eq!(v["provider"]["yeschoy"]["npm"], "x", "{out}");
    }

    #[test]
    fn several_new_members_in_one_object_are_written_together() {
        let source = "{\n  // keep\n  \"thinkingLevel\": \"low\",\n}\n";
        let out = apply(
            source,
            &[
                set(&[key("defaultProvider")], "\"yeschoy\""),
                set(&[key("defaultModel")], "\"model-a\""),
            ],
        )
        .unwrap();
        assert_eq!(
            out,
            "{\n  // keep\n  \"thinkingLevel\": \"low\",\n  \"defaultProvider\": \"yeschoy\",\n  \"defaultModel\": \"model-a\"\n}\n"
        );
        json5::from_str::<serde_json::Value>(&out).expect("parseable");
    }

    #[test]
    fn comments_between_tokens_are_skipped() {
        let source = "{\n  // before\n  \"a\" /* mid */ : /* after */ 1, // tail\n  \"b\": 2\n}\n";
        let out = apply(source, &[set(&[key("b")], "3")]).unwrap();
        assert_eq!(
            out,
            "{\n  // before\n  \"a\" /* mid */ : /* after */ 1, // tail\n  \"b\": 3\n}\n"
        );
    }

    #[test]
    fn duplicate_keys_resolve_to_the_last_one_like_json_does() {
        assert_eq!(
            apply("{\"a\": 1, \"a\": 2}", &[set(&[key("a")], "9")]).unwrap(),
            "{\"a\": 1, \"a\": 9}"
        );
    }

    #[test]
    fn removal_takes_the_comma_with_it() {
        let source = "{\n  \"a\": 1,\n  \"b\": 2,\n  \"c\": 3\n}\n";
        fn remove(name: &str) -> Edit<'_> {
            Edit::Remove {
                path: vec![key(name)],
            }
        }
        assert_eq!(
            apply(source, &[remove("b")]).unwrap(),
            "{\n  \"a\": 1,\n  \"c\": 3\n}\n"
        );
        assert_eq!(
            apply(source, &[remove("a")]).unwrap(),
            "{\n  \"b\": 2,\n  \"c\": 3\n}\n"
        );
        assert_eq!(
            apply(source, &[remove("c")]).unwrap(),
            "{\n  \"a\": 1,\n  \"b\": 2\n}\n"
        );
    }

    #[test]
    fn a_missing_path_is_created_rather_than_abandoned() {
        // 中间层不存在就补出来：否则 `provider.yeschoy` 这种首次写入会让
        // 整份配置回退重排，注释跟着丢。
        let out = apply("{\"a\": {}}", &[set(&[key("a"), key("b"), key("c")], "1")]).unwrap();
        let value: serde_json::Value = json5::from_str(&out).unwrap();
        assert_eq!(value["a"]["b"]["c"], 1, "{out}");
        // 数组下标不是对象键：没法「补出来」，只能放弃。
        assert!(apply("{\"a\": 1}", &[set(&[Segment::Index(0)], "2")]).is_none());
    }

    #[test]
    fn malformed_input_gives_up() {
        for source in ["{", "{\"a\" 1}", "[1, 2", "{} extra", "/* unterminated"] {
            assert!(
                apply(source, &[set(&[key("a")], "1")]).is_none(),
                "{source}"
            );
        }
    }

    #[test]
    fn overlapping_edits_are_refused() {
        let edits = [
            set(&[key("a"), key("b")], "2"),
            set(&[key("a")], "{\"b\": 2}"),
        ];
        assert!(apply("{\"a\": {\"b\": 1}}", &edits).is_none());
    }

    #[test]
    fn escaped_keys_and_unicode_are_understood() {
        assert_eq!(
            apply(
                "{\"a\\u002fb\": 1, \"emoji\": \"\\ud83d\\ude00\"}",
                &[set(&[key("a/b")], "2")]
            )
            .unwrap(),
            "{\"a\\u002fb\": 2, \"emoji\": \"\\ud83d\\ude00\"}"
        );
    }

    #[test]
    fn no_edits_is_the_identity() {
        assert_eq!(apply(COMMENTED, &[]).unwrap(), COMMENTED);
    }

    #[test]
    fn unchanged_values_are_not_rewritten_at_all() {
        let original: serde_json::Value = json5::from_str(COMMENTED).unwrap();
        let values = [
            (vec![key("defaultProvider")], serde_json::json!("anthropic")),
            (vec![key("theme")], serde_json::json!("dark")),
        ];
        assert_eq!(
            apply_values(COMMENTED, &original, &values).unwrap(),
            COMMENTED
        );
    }
}

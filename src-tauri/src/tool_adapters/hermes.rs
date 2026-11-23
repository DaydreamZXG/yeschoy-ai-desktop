//! Hermes Agent：写它的 `config.yaml`，只动顶层的 `model:` 这一块。
//!
//! Hermes 用哪一家、哪个型号，全看 `model:`。接自定义地址的写法来自它的官方
//! 文档（`provider: custom` 加 `base_url`、`api_key`、`default`），用
//! `hermes chat -q` 对着本机假上游实测过（0.0.0 @15806b32）。
//!
//! 这份文件是用户手写的，里面常有注释，Hermes 自己改它时也保留注释。所以
//! 这里按文本换：找到顶格的 `model:` 那一块整块换掉，别处一个字都不动。换完
//! 再按 YAML 读一遍核对（别的键没变、`model` 是我们要的）；核对不过（少见的
//! 写法）才退回整份重排。改回原样同理，见 [`restore`]。
//!
//! 密钥：`api_key` 写的是**本地令牌**，请求先到 `hermes_bridge`，由桥从系统
//! 钥匙串取出真正的站点密钥再转发。站点密钥不落盘，也不进 Hermes 的 `.env`。
use std::{
    ffi::OsStr,
    ops::Range,
    path::{Path, PathBuf},
};

use serde_yaml::{Mapping, Value};

use crate::{
    hermes_bridge::PROXY_BASE,
    tool_adapters::{
        common::{self, ConfigFailure, FileTransaction},
        AdapterFailure,
    },
};

/// Hermes 的 `.env` 里设了这个变量，它就不看 `model.base_url`，请求去的是这个
/// 变量指的地址。只认 `.env` 里的：写在 shell 环境里的那个 Hermes 不理
/// （0.0.0 @15806b32 两种都实测过）。
const OVERRIDING_VARIABLE: &str = "CUSTOM_BASE_URL";
/// Hermes 的家挪到了哪里。和 Codex 的 `CODEX_HOME` 一样，跟着它走。
pub(crate) const HOME_VARIABLE: &str = "HERMES_HOME";

pub(crate) struct Prepared {
    transaction: FileTransaction,
    path: PathBuf,
    model: String,
    local_token: String,
}

fn config_error(error: ConfigFailure) -> AdapterFailure {
    match error {
        ConfigFailure::ExternalChange => AdapterFailure::ExternalOverride,
        ConfigFailure::Read => AdapterFailure::ConfigurationFailed("configuration_read_failed"),
        ConfigFailure::Parse => AdapterFailure::ConfigurationFailed("configuration_parse_failed"),
        ConfigFailure::Write => AdapterFailure::ConfigurationFailed("configuration_write_failed"),
        ConfigFailure::Readback => {
            AdapterFailure::ConfigurationFailed("configuration_readback_failed")
        }
        ConfigFailure::Rollback => {
            AdapterFailure::ConfigurationFailed("configuration_rollback_failed")
        }
    }
}

/// 没挪过的话 Hermes 的家在哪：Windows 上是 `%LOCALAPPDATA%\hermes`，别处是
/// `~/.hermes`（`hermes_constants._platform_default_hermes_home`）。
fn default_root(home: &Path, local_app_data: Option<&OsStr>) -> PathBuf {
    if cfg!(target_os = "windows") {
        local_app_data
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join("AppData").join("Local"))
            .join("hermes")
    } else {
        home.join(".hermes")
    }
}

/// 直接敲 `hermes` 时它读的是哪个文件夹。
///
/// 照 Hermes 自己的顺序（`hermes_cli/main.py::_apply_profile_override`）：
/// `HERMES_HOME` 指着某个 profile（上一级叫 `profiles`）就用它；否则它是根，
/// 根下面的 `active_profile` 里写着名字（`hermes profile use` 写的）就用
/// `profiles/<名字>`，没写或写的是 `default` 就用根。
fn config_dir_in(root: PathBuf) -> Result<PathBuf, AdapterFailure> {
    if !root.is_absolute()
        || root
            .components()
            .any(|part| matches!(part, std::path::Component::ParentDir))
    {
        return Err(AdapterFailure::ExternalOverride);
    }
    if root
        .parent()
        .and_then(Path::file_name)
        .is_some_and(|name| name == "profiles")
    {
        return Ok(root);
    }
    let sticky = std::fs::read_to_string(root.join("active_profile")).unwrap_or_default();
    let name = sticky.trim_start_matches('\u{feff}').trim();
    if name.is_empty() || name == "default" {
        return Ok(root);
    }
    // 名字是 Hermes 写的，只会是字母、数字、`-`、`_`。不是这个样子就不猜。
    if !name
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return Err(AdapterFailure::ExternalOverride);
    }
    Ok(root.join("profiles").join(name))
}

pub(crate) fn config_dir(home: &Path) -> Result<PathBuf, AdapterFailure> {
    // 走登录 shell 问：`HERMES_HOME` 一般写在 rc 文件里，从 Dock 启动的程序看不到。
    let root = match crate::shell_environment::var_os(HOME_VARIABLE) {
        Some(value) => PathBuf::from(value),
        None => default_root(home, std::env::var_os("LOCALAPPDATA").as_deref()),
    };
    config_dir_in(root)
}

pub(crate) fn config_path(home: &Path) -> Result<PathBuf, AdapterFailure> {
    Ok(config_dir(home)?.join("config.yaml"))
}

/// Hermes 的 `.env` 里有没有一行把 `CUSTOM_BASE_URL` 设成了非空。
fn env_file_overrides(text: &str) -> bool {
    text.lines().any(|line| {
        let line = line.trim_start();
        let line = line.strip_prefix("export ").unwrap_or(line).trim_start();
        line.strip_prefix(OVERRIDING_VARIABLE)
            .and_then(|rest| rest.trim_start().strip_prefix('='))
            .is_some_and(|value| !value.trim().trim_matches(['"', '\'']).is_empty())
    })
}

// ── 按文本找顶层的 `model:` 块 ────────────────────────────────────────────

/// 顶格的 `model:` 这一块在文本里占哪一段：从这一行起，到下一个顶格写着东西
/// 的行之前；块尾的空行不算在内。找不到（没有，或者写法不寻常）就是 `None`。
fn model_block(text: &str) -> Option<Range<usize>> {
    let mut offset = 0;
    let mut start = None;
    let mut end = 0;
    for line in text.split_inclusive('\n') {
        let here = offset;
        offset += line.len();
        let content = line.trim_end_matches(['\n', '\r']);
        match start {
            None => {
                let opens = content
                    .strip_prefix("model:")
                    .is_some_and(|rest| rest.is_empty() || rest.starts_with([' ', '\t']));
                if opens {
                    start = Some(here);
                    end = offset;
                }
            }
            Some(_) => {
                if content.trim().is_empty() {
                    continue;
                }
                if !content.starts_with([' ', '\t']) {
                    break;
                }
                end = offset;
            }
        }
    }
    start.map(|start| start..end)
}

/// `model:` 块里除了 `dropped` 以外的项，原文照抄，连同它们上面的注释；缩进
/// 统一换成 `indent`。块的写法认不出来就是 `None`。
fn extras(block: &str, indent: &str, dropped: &Mapping) -> Option<String> {
    let mut kept = String::new();
    // 还没归属的空行和注释：跟着下一项走。
    let mut pending = String::new();
    let mut theirs: Option<&str> = None;
    let mut keeping = false;
    for line in block.split_inclusive('\n').skip(1) {
        let content = line.trim_end_matches(['\n', '\r']);
        let body = content.trim_start_matches(' ');
        if body.is_empty() || body.starts_with('#') {
            pending.push_str(line);
            continue;
        }
        let width = content.len() - body.len();
        let own = *theirs.get_or_insert(&content[..width]);
        if width < own.len() {
            return None;
        }
        if width == own.len() {
            let key = body.split_once(':')?.0.trim().trim_matches(['"', '\'']);
            keeping = !dropped.contains_key(key);
        }
        if keeping {
            for line in std::mem::take(&mut pending)
                .split_inclusive('\n')
                .chain([line])
            {
                kept.push_str(indent);
                kept.push_str(
                    line.strip_prefix(own)
                        .unwrap_or(line.trim_start_matches(' ')),
                );
            }
        } else {
            pending.clear();
        }
    }
    if !kept.is_empty() && !kept.ends_with('\n') {
        kept.push_str(line_ending(block));
    }
    Some(kept)
}

/// `model` 这张表里除了 `dropped` 以外的项。
fn extra_values(model: Option<&Value>, dropped: &Mapping) -> Mapping {
    model
        .and_then(Value::as_mapping)
        .into_iter()
        .flatten()
        .filter(|(key, _)| !dropped.contains_key(*key))
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect()
}

fn line_ending(text: &str) -> &'static str {
    if text.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    }
}

fn ours(model: &str, local_token: &str) -> Mapping {
    let mut block = Mapping::new();
    block.insert("provider".into(), "custom".into());
    block.insert("default".into(), model.into());
    block.insert("base_url".into(), PROXY_BASE.into());
    block.insert("api_key".into(), local_token.into());
    block
}

/// 我们那一块的文本。型号一律加引号：`5.3` 这样的名字不加会被读成数字。
fn block_text(model: &str, local_token: &str, newline: &str) -> Result<String, ()> {
    let quoted = serde_json::to_string(model).map_err(|_| ())?;
    Ok([
        "model:".to_owned(),
        "  provider: custom".to_owned(),
        format!("  default: {quoted}"),
        format!("  base_url: {PROXY_BASE}"),
        format!("  api_key: {local_token}"),
    ]
    .map(|line| line + newline)
    .concat())
}

/// 整份按 YAML 读成一张表。空文件、只有注释的文件算空表。
fn parse(bytes: Option<&[u8]>) -> Result<Mapping, ()> {
    let text = std::str::from_utf8(bytes.unwrap_or_default()).map_err(|_| ())?;
    match serde_yaml::from_str::<Value>(text).map_err(|_| ())? {
        Value::Mapping(mapping) => Ok(mapping),
        Value::Null => Ok(Mapping::new()),
        _ => Err(()),
    }
}

/// `edited` 和 `original` 除了 `model` 以外是不是一模一样，且 `model` 正好是 `expected`。
fn only_model_differs(original: &Mapping, edited: &[u8], expected: Option<&Value>) -> bool {
    let Ok(mut edited) = parse(Some(edited)) else {
        return false;
    };
    let mut original = original.clone();
    original.remove("model");
    edited.remove("model").as_ref() == expected && edited == original
}

/// 把 `text` 里的 `model:` 块换成 `replacement`；原来没有就接在文件末尾。
fn splice(text: &str, replacement: &str) -> String {
    match model_block(text) {
        Some(span) => [&text[..span.start], replacement, &text[span.end..]].concat(),
        None if text.is_empty() || text.ends_with('\n') => [text, replacement].concat(),
        None => [text, line_ending(text), replacement].concat(),
    }
}

pub(crate) fn render(
    existing: Option<&[u8]>,
    model: &str,
    local_token: &str,
) -> Result<Vec<u8>, ()> {
    let original = parse(existing)?;
    let text = std::str::from_utf8(existing.unwrap_or_default()).map_err(|_| ())?;
    let mut wanted = ours(model, local_token);
    let mut block = block_text(model, local_token, line_ending(text))?;
    // 已经接着我们、只是重新接一次：用户后来加在这一块里的项留着。型号换了
    // 就不留 `context_length`——那是给上一个型号定的，Hermes 自己换型号时也会
    // 清掉它。原来接的是别家，那一块整块换掉：别家的 `api_mode` 之类留下来
    // 反而连不上。
    if model_points_at_bridge(&original) {
        let mut dropped = wanted.clone();
        if original["model"]["default"].as_str() != Some(model) {
            dropped.insert("context_length".into(), Value::Null);
        }
        let added = extra_values(original.get("model"), &dropped);
        block += &model_block(text)
            .and_then(|span| extras(&text[span], "  ", &dropped))
            .unwrap_or_default();
        wanted.extend(added);
    }
    let wanted = Value::Mapping(wanted);
    let edited = splice(text, &block).into_bytes();
    if only_model_differs(&original, &edited, Some(&wanted)) {
        return Ok(edited);
    }
    // 写法不寻常，按文本换不准：整份重排。注释会丢，内容不会；原来那份留在
    // 接入记录里，改回原样时一字不差地还回去。
    let mut whole = original;
    whole.insert("model".into(), wanted);
    serde_yaml::to_string(&whole)
        .map(String::into_bytes)
        .map_err(|_| ())
}

/// `model:` 现在是不是还指着本地桥。
fn model_points_at_bridge(document: &Mapping) -> bool {
    document.get("model").is_some_and(|model| {
        model.get("provider").and_then(Value::as_str) == Some("custom")
            && model.get("base_url").and_then(Value::as_str) == Some(PROXY_BASE)
    })
}

pub(crate) fn points_at_our_bridge(bytes: &[u8]) -> bool {
    parse(Some(bytes)).is_ok_and(|document| model_points_at_bridge(&document))
}

/// 接好之后用户（或 Hermes）改了别处，`model:` 那一块还是我们写下的样子：
/// 不算「被改过」。不是 Hermes 的文件就是 `None`。
pub(crate) fn still_ours(path: &Path, ours: &[u8], now: &[u8]) -> Option<bool> {
    if !is_config(path) || !points_at_our_bridge(ours) {
        return None;
    }
    let written = parse(Some(ours)).ok()?;
    Some(parse(Some(now)).is_ok_and(|now| now.get("model") == written.get("model")))
}

fn is_config(path: &Path) -> bool {
    path.extension()
        .is_some_and(|extension| extension == "yaml" || extension == "yml")
}

/// 改回原样，按文本做，注释留着。不是 Hermes 的文件、或者按文本做不准，就是
/// `None`，交给通用的那条路。
///
/// 进到这里时文件既不是接入前的样子，也不是我们写下的样子：用户（或 Hermes）
/// 后来又改过。`model:` 还指着本地桥，就把接入前的那一块换回去，别处的改动
/// 原样留着；`model:` 已经被换成别的，那是用户的决定，整份不动。
///
/// 返回 `(该写回的内容, 是否留下了用户后来的改动)`；内容是 `None` 表示删掉文件。
pub(crate) fn restore(
    path: &Path,
    before: Option<&[u8]>,
    after: &[u8],
    current: &[u8],
) -> Option<(Option<Vec<u8>>, bool)> {
    if !is_config(path) || !points_at_our_bridge(after) {
        return None;
    }
    let now = parse(Some(current)).ok()?;
    if !model_points_at_bridge(&now) {
        return Some((Some(current.to_vec()), true));
    }
    let original = parse(before).ok()?;
    let before_text = std::str::from_utf8(before.unwrap_or_default()).ok()?;
    let current_text = std::str::from_utf8(current).ok()?;
    let replacement = match (original.get("model"), model_block(before_text)) {
        (Some(_), Some(span)) => {
            // 接入前那一块若是文件最后一行又没有换行，补上，免得和后面的粘在一起。
            let block = &before_text[span];
            if block.ends_with('\n') {
                block.to_owned()
            } else {
                [block, line_ending(current_text)].concat()
            }
        }
        (None, _) => String::new(),
        (Some(_), None) => return None,
    };
    let span = model_block(current_text)?;
    let restored = [
        &current_text[..span.start],
        replacement.as_str(),
        &current_text[span.end..],
    ]
    .concat();
    if !only_model_differs(&now, restored.as_bytes(), original.get("model")) {
        return None;
    }
    if before.is_none() && restored.trim().is_empty() {
        return Some((None, false));
    }
    // 还回去的和接入前不一样：多出来的是用户后来在别处改的（注释也算），留下了，
    // 要照实说。末尾补的那个换行不算。
    let kept = restored.trim_end() != before_text.trim_end();
    Some((Some(restored.into_bytes()), kept))
}

pub(crate) fn prepare(
    home: &Path,
    model: &str,
    local_token: Option<&str>,
) -> Result<Prepared, AdapterFailure> {
    common::validate_catalog(model, std::slice::from_ref(&model.to_owned()))?;
    let directory = config_dir(home)?;
    // `.env` 里另指了地址，Hermes 用的就是它，不是我们写的。
    let overridden =
        std::fs::read_to_string(directory.join(".env")).is_ok_and(|text| env_file_overrides(&text));
    if overridden {
        return Err(AdapterFailure::ExternalOverride);
    }
    let local_token = local_token
        .filter(|value| value.starts_with("ycg-") && value.len() == 68)
        .ok_or(AdapterFailure::SecureStorageUnavailable)?
        .to_owned();
    let path = directory.join("config.yaml");
    let before = common::snapshot(&path)
        .map_err(|_| AdapterFailure::ConfigurationFailed("configuration_read_failed"))?;
    let after = render(before.as_deref(), model, &local_token)
        .map_err(|_| AdapterFailure::ConfigurationFailed("configuration_parse_failed"))?;
    let transaction =
        FileTransaction::stage_with_snapshot(path.clone(), before, after).map_err(config_error)?;
    Ok(Prepared {
        transaction,
        path,
        model: model.to_owned(),
        local_token,
    })
}

/// 重启客户端后，Hermes 若仍指着我们这座桥，就把桥重新起起来。
/// 只看 Hermes 自己的设置：用户已经换走了，就不占端口。
pub(crate) async fn resume_if_configured() {
    let Some(credential) = configured_credential() else {
        return;
    };
    let bridge = crate::hermes_bridge::global();
    match bridge.start(credential).await {
        Ok(_) => {}
        Err(failure) if crate::local_bridge::is_port_busy(&failure) => {
            crate::local_bridge::retry_while_port_busy(
                bridge.clone(),
                "hermes_bridge",
                configured_credential,
            );
        }
        Err(error) => log::warn!("hermes_bridge stage=resume_start_failed reason={error:?}"),
    }
}

/// 存着的凭据，前提是它的设置还指向本机桥。
fn configured_credential() -> Option<crate::tool_credentials::ToolCredential> {
    let credential = crate::tool_credentials::load("hermes").ok()?;
    let home = super::user_home()?;
    let path = config_path(&home).ok()?;
    let bytes = std::fs::read(path).ok()?;
    points_at_our_bridge(&bytes).then_some(credential)
}

impl Prepared {
    pub(crate) fn changes(&self) -> &[common::FileChange] {
        self.transaction.changes()
    }

    pub(crate) fn commit(&mut self) -> Result<(), AdapterFailure> {
        self.transaction.commit().map_err(config_error)?;
        self.validate_existing()
    }

    /// 写完读回和日后「还是不是我们写的那样」用同一条：只看那四项。用户在这
    /// 一块里另加的（比如 `context_length`）不算改坏。
    pub(crate) fn validate_existing(&self) -> Result<(), AdapterFailure> {
        let failed = AdapterFailure::ConfigurationFailed("configuration_readback_failed");
        let bytes = common::snapshot(&self.path)
            .map_err(|_| failed)?
            .ok_or(failed)?;
        let document = parse(Some(&bytes)).map_err(|_| failed)?;
        let model = document.get("model").ok_or(failed)?;
        let correct = ours(&self.model, &self.local_token)
            .iter()
            .all(|(key, value)| model.get(key) == Some(value));
        if correct {
            Ok(())
        } else {
            Err(failed)
        }
    }

    pub(crate) fn rollback(&mut self) -> Result<(), AdapterFailure> {
        self.transaction.rollback().map_err(config_error)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn token() -> String {
        format!("ycg-{}", "a".repeat(64))
    }

    fn text(bytes: &[u8]) -> &str {
        std::str::from_utf8(bytes).unwrap()
    }

    const THEIRS: &str = "\
# 我的 Hermes 设置
model:
  provider: openrouter
  default: anthropic/claude-opus-4   # 平时用这个
  # base_url: https://example.com/v1

# 终端
terminal:
  backend: local   # 不用 docker
";

    #[test]
    fn a_missing_file_gets_just_our_block() {
        let bytes = render(None, "glm-5.3", &token()).unwrap();
        assert_eq!(
            text(&bytes),
            format!(
                "model:\n  provider: custom\n  default: \"glm-5.3\"\n  base_url: http://127.0.0.1:15734/hermes/v1\n  api_key: {}\n",
                token()
            )
        );
        assert!(points_at_our_bridge(&bytes));
    }

    #[test]
    fn only_the_model_block_changes_and_every_comment_elsewhere_survives() {
        let bytes = render(Some(THEIRS.as_bytes()), "glm-5.3", &token()).unwrap();
        let written = text(&bytes);
        assert!(written.starts_with("# 我的 Hermes 设置\nmodel:\n  provider: custom\n"));
        assert!(written.ends_with("\n\n# 终端\nterminal:\n  backend: local   # 不用 docker\n"));
        assert!(!written.contains("openrouter"));
        assert!(points_at_our_bridge(&bytes));
    }

    #[test]
    fn a_model_name_that_looks_like_a_number_stays_text() {
        let bytes = render(None, "5.3", &token()).unwrap();
        let document = parse(Some(&bytes)).unwrap();
        assert_eq!(document["model"]["default"], Value::from("5.3"));
    }

    #[test]
    fn a_file_without_a_model_block_gets_one_at_the_end() {
        for existing in [
            "terminal:\n  backend: local\n",
            "terminal:\n  backend: local",
        ] {
            let bytes = render(Some(existing.as_bytes()), "glm-5.3", &token()).unwrap();
            assert!(text(&bytes).starts_with("terminal:\n  backend: local\nmodel:\n"));
            assert!(points_at_our_bridge(&bytes));
        }
    }

    #[test]
    fn the_old_one_line_form_and_windows_line_endings_are_handled() {
        let bytes = render(
            Some(b"model: anthropic/claude-opus-4\r\ntoolsets:\r\n  - web\r\n"),
            "glm-5.3",
            &token(),
        )
        .unwrap();
        let written = text(&bytes);
        assert!(written.starts_with("model:\r\n  provider: custom\r\n"));
        assert!(written.ends_with("toolsets:\r\n  - web\r\n"));
        assert!(!written.replace("\r\n", "").contains('\n'));
        assert!(points_at_our_bridge(&bytes));
    }

    #[test]
    fn an_unusual_layout_falls_back_to_rewriting_the_whole_file() {
        // 花括号写法：按文本找不到顶格的 `model:` 块之外的边界，核对会不过。
        let existing = b"{model: {provider: openrouter, default: x}, terminal: {backend: local}}\n";
        let bytes = render(Some(existing), "glm-5.3", &token()).unwrap();
        let document = parse(Some(&bytes)).unwrap();
        assert!(points_at_our_bridge(&bytes));
        assert_eq!(document["terminal"]["backend"], Value::from("local"));
        assert_eq!(document.len(), 2);
    }

    #[test]
    fn a_file_that_is_not_a_yaml_table_is_refused() {
        assert!(render(Some(b"- just\n- a list\n"), "glm-5.3", &token()).is_err());
        assert!(render(Some(b"model: [unclosed\n"), "glm-5.3", &token()).is_err());
        assert!(render(Some(&[0xff, 0xfe]), "glm-5.3", &token()).is_err());
    }

    fn restored(before: Option<&str>, current: &str) -> Option<(Option<String>, bool)> {
        let after = render(before.map(str::as_bytes), "glm-5.3", &token()).unwrap();
        restore(
            Path::new("/home/u/.hermes/config.yaml"),
            before.map(str::as_bytes),
            &after,
            current.as_bytes(),
        )
        .map(|(bytes, kept)| (bytes.map(|bytes| String::from_utf8(bytes).unwrap()), kept))
    }

    #[test]
    fn restoring_puts_back_the_original_block_and_keeps_later_edits_elsewhere() {
        let after = render(Some(THEIRS.as_bytes()), "glm-5.3", &token()).unwrap();
        // 接入之后用户改了别处，还加了一行注释。
        let current = text(&after).replace(
            "  backend: local   # 不用 docker\n",
            "  backend: docker   # 换成 docker 了\n# 新加的\ncompression:\n  enabled: true\n",
        );
        let (bytes, kept) = restored(Some(THEIRS), &current).unwrap();
        assert!(kept, "别处后来的改动留下了，要说出来");
        assert_eq!(
            bytes.unwrap(),
            THEIRS.replace(
                "  backend: local   # 不用 docker\n",
                "  backend: docker   # 换成 docker 了\n# 新加的\ncompression:\n  enabled: true\n",
            )
        );
    }

    #[test]
    fn restoring_removes_our_block_when_there_was_none_before() {
        let before = "terminal:\n  backend: local\n";
        let after = render(Some(before.as_bytes()), "glm-5.3", &token()).unwrap();
        let current = format!("# 后来加的\n{}", text(&after));
        let (bytes, kept) = restored(Some(before), &current).unwrap();
        assert!(kept);
        assert_eq!(bytes.unwrap(), format!("# 后来加的\n{before}"));

        // 只是我们那一块里多了一项、别处没动：还回去的就是原样。
        let current = format!("{}  context_length: 200000\n", text(&after));
        assert_eq!(
            restored(Some(before), &current),
            Some((Some(before.into()), false))
        );

        // 文件是我们建的，后来只是在我们那一块里多了一项：拿走之后什么都不剩，删掉。
        let after = render(None, "glm-5.3", &token()).unwrap();
        let current = format!("{}  context_length: 200000\n", text(&after));
        assert_eq!(restored(None, &current), Some((None, false)));

        // 文件是我们建的，用户后来加了别的：只拿走我们那一块。
        let current = format!("{}toolsets:\n  - web\n", text(&after));
        assert_eq!(
            restored(None, &current),
            Some((Some("toolsets:\n  - web\n".into()), true))
        );
    }

    /// 接好之后用户在我们那一块里加了几项，再点一次接入：加的项和上面的注释
    /// 留着。换了型号，`context_length` 是给上一个型号定的，不留。
    #[test]
    fn reconnecting_keeps_what_the_user_added_to_our_block() {
        let first = render(Some(THEIRS.as_bytes()), "glm-5.3", &token()).unwrap();
        let added =
            "  # 探测不到，手填\n  context_length: 200000\n  default_headers:\n    x-team: a\n";
        let current = text(&first).replace("\n\n# 终端", &format!("\n{added}\n# 终端"));
        assert_ne!(current, text(&first));

        let same = render(Some(current.as_bytes()), "glm-5.3", &token()).unwrap();
        assert_eq!(text(&same), current);

        let other = render(Some(current.as_bytes()), "kimi-k3", &token()).unwrap();
        assert_eq!(
            text(&other),
            current
                .replace("\"glm-5.3\"", "\"kimi-k3\"")
                .replace("  # 探测不到，手填\n  context_length: 200000\n", "")
        );

        // 改回原样时整块换回接入前的样子：加在我们那一块里的项跟着这次接入走。
        assert_eq!(
            restored(Some(THEIRS), &current),
            Some((Some(THEIRS.into()), false))
        );
    }

    #[test]
    fn a_model_the_user_switched_away_from_us_is_left_alone() {
        let current = "model:\n  provider: openrouter\n  default: x\n";
        assert_eq!(
            restored(Some(THEIRS), current),
            Some((Some(current.into()), true))
        );
    }

    #[test]
    fn restore_only_speaks_for_files_this_adapter_wrote() {
        // 别的应用的 YAML：不归这里管。
        assert!(restore(
            Path::new("/home/u/.dsh/settings.yaml"),
            Some(b"model: old\n"),
            b"model: new\n",
            b"model: new\nuser: true\n",
        )
        .is_none());
        // 是我们写的，但现在的写法按文本换不准：交给通用的那条路。
        let after = render(Some(THEIRS.as_bytes()), "glm-5.3", &token()).unwrap();
        let flow = format!(
            "{{model: {{provider: custom, base_url: \"{PROXY_BASE}\"}}, terminal: {{backend: local}}}}\n"
        );
        assert!(restore(
            Path::new("/home/u/.hermes/config.yaml"),
            Some(THEIRS.as_bytes()),
            &after,
            flow.as_bytes(),
        )
        .is_none());
    }

    #[test]
    fn the_folder_follows_the_profile_hermes_would_use() {
        let root = common::temporary_working_directory("hermes-home").unwrap();
        assert_eq!(config_dir_in(root.clone()).unwrap(), root);

        std::fs::write(root.join("active_profile"), "work\n").unwrap();
        assert_eq!(
            config_dir_in(root.clone()).unwrap(),
            root.join("profiles").join("work")
        );
        std::fs::write(root.join("active_profile"), "default\n").unwrap();
        assert_eq!(config_dir_in(root.clone()).unwrap(), root);

        // `HERMES_HOME` 直接指着某个 profile：就是它，不再看 `active_profile`。
        let profile = root.join("profiles").join("coder");
        std::fs::create_dir_all(&profile).unwrap();
        std::fs::write(profile.join("active_profile"), "work\n").unwrap();
        assert_eq!(config_dir_in(profile.clone()).unwrap(), profile);

        std::fs::write(root.join("active_profile"), "../elsewhere\n").unwrap();
        assert_eq!(
            config_dir_in(root.clone()),
            Err(AdapterFailure::ExternalOverride)
        );
        assert_eq!(
            config_dir_in(PathBuf::from("relative/home")),
            Err(AdapterFailure::ExternalOverride)
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn the_default_folder_is_where_hermes_puts_it_on_this_system() {
        let home = Path::new(if cfg!(windows) {
            r"C:\Users\u"
        } else {
            "/home/u"
        });
        let found = default_root(home, Some(OsStr::new(r"C:\Users\u\AppData\Local")));
        if cfg!(target_os = "windows") {
            assert_eq!(found, Path::new(r"C:\Users\u\AppData\Local\hermes"));
            assert_eq!(
                default_root(home, None),
                Path::new(r"C:\Users\u\AppData\Local\hermes")
            );
        } else {
            assert_eq!(found, Path::new("/home/u/.hermes"));
        }
    }

    #[test]
    fn an_address_set_in_hermes_env_file_counts_as_an_override() {
        assert!(env_file_overrides("CUSTOM_BASE_URL=http://x/v1\n"));
        assert!(env_file_overrides(
            "A=1\nexport CUSTOM_BASE_URL = \"http://x/v1\"\n"
        ));
        assert!(!env_file_overrides("# CUSTOM_BASE_URL=http://x/v1\n"));
        assert!(!env_file_overrides(
            "CUSTOM_BASE_URL=\nCUSTOM_BASE_URL=\"\"\n"
        ));
        assert!(!env_file_overrides(
            "MY_CUSTOM_BASE_URL=http://x/v1\nCUSTOM_BASE_URL_OLD=y\n"
        ));
    }

    #[test]
    fn readback_checks_our_four_fields_and_tolerates_extras() {
        let home = common::temporary_working_directory("hermes-readback").unwrap();
        let path = home.join("config.yaml");
        let after = render(Some(THEIRS.as_bytes()), "glm-5.3", &token()).unwrap();
        let prepared = Prepared {
            transaction: FileTransaction::stage_with_snapshot(path.clone(), None, after.clone())
                .unwrap(),
            path: path.clone(),
            model: "glm-5.3".into(),
            local_token: token(),
        };
        assert!(prepared.validate_existing().is_err());
        std::fs::write(&path, &after).unwrap();
        assert!(prepared.validate_existing().is_ok());
        let extra = text(&after).replace("  api_key:", "  context_length: 200000\n  api_key:");
        std::fs::write(&path, extra).unwrap();
        assert!(prepared.validate_existing().is_ok());
        let other = text(&after).replace("glm-5.3", "glm-4");
        std::fs::write(&path, other).unwrap();
        assert!(prepared.validate_existing().is_err());
        std::fs::remove_dir_all(home).unwrap();
    }
}

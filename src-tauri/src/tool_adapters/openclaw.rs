//! OpenClaw：写它的 `openclaw.json`（默认在 `~/.openclaw/`）。
//!
//! 只动我们自己的那几处：`models.providers.yeschoy`、`agents.defaults.model`
//! 里的主型号，以及（用户设了型号白名单时）白名单里的一项 `yeschoy/*`。别的
//! 厂商、频道、网关设置一律原样保留。恢复由连接恢复记录按「我们改了什么就
//! 改回什么」处理，用户后来自己改的留着。
//!
//! 这份文件是 JSON5，用户可以写注释；OpenClaw 自己每次保存都会把它重排成
//! 标准 JSON、去掉注释（官方文档 Write safety 一节），我们写的时候也一样。
//!
//! OpenClaw 对这份文件是严格校验的：多一个它不认识的键，整份配置作废、拒绝
//! 运行。所以只写每个版本都有的那几个键（2026.11.6 实测通过）。
//!
//! 密钥：`apiKey` 写的是**本地令牌**，请求先到 `openclaw_bridge`，由桥从系统
//! 钥匙串取出真正的站点密钥再转发。站点密钥不落盘。
use std::{
    ffi::OsString,
    path::{Component, Path, PathBuf},
};

use serde_json::{json, Map, Value};

use crate::{
    openclaw_bridge::PROXY_BASE,
    tool_adapters::{
        common::{self, ConfigFailure, FileTransaction},
        AdapterFailure,
    },
};

pub(crate) const PROVIDER_ID: &str = "yeschoy";
/// 白名单里代表「我们这一家的所有型号」的那一项。
pub(crate) const ALLOW_ENTRY: &str = "yeschoy/*";

/// OpenClaw 用来决定配置文件在哪的四个变量，先后顺序见 `config_path_from`。
/// 测试拿它核对：登录 shell 的探测有没有把这四个都问到。
#[cfg(test)]
pub(crate) const PATH_VARIABLES: [&str; 4] = [
    "OPENCLAW_CONFIG_PATH",
    "OPENCLAW_STATE_DIR",
    "OPENCLAW_HOME",
    "OPENCLAW_PROFILE",
];

const PARSE_FAILED: AdapterFailure =
    AdapterFailure::ConfigurationFailed("configuration_parse_failed");

pub(crate) struct Prepared {
    transaction: FileTransaction,
    path: PathBuf,
    model: String,
    model_ids: Vec<String>,
    local_token: String,
}

fn config_error(error: ConfigFailure) -> AdapterFailure {
    match error {
        ConfigFailure::ExternalChange => AdapterFailure::ExternalOverride,
        ConfigFailure::Read => AdapterFailure::ConfigurationFailed("configuration_read_failed"),
        ConfigFailure::Parse => PARSE_FAILED,
        ConfigFailure::Write => AdapterFailure::ConfigurationFailed("configuration_write_failed"),
        ConfigFailure::Readback => {
            AdapterFailure::ConfigurationFailed("configuration_readback_failed")
        }
        ConfigFailure::Rollback => {
            AdapterFailure::ConfigurationFailed("configuration_rollback_failed")
        }
    }
}

/// 和 OpenClaw 自己找配置文件的顺序一致（2026.11.6 用 `openclaw config file`
/// 逐种实测）：`OPENCLAW_CONFIG_PATH` 直接就是文件；否则 `OPENCLAW_STATE_DIR`
/// 下的 `openclaw.json`；否则家目录（`OPENCLAW_HOME` 可以换掉它）下的
/// `.openclaw`，选了具名 profile 时是 `.openclaw-<名字>`。空串等于没设。
fn config_path_from(
    home: &Path,
    variable: impl Fn(&str) -> Option<OsString>,
) -> Result<PathBuf, AdapterFailure> {
    let set = |name: &str| {
        variable(name)
            .and_then(|value| value.into_string().ok())
            .map(|value| value.trim().to_owned())
            .filter(|value| !value.is_empty())
    };
    let path = if let Some(path) = set("OPENCLAW_CONFIG_PATH") {
        PathBuf::from(path)
    } else if let Some(directory) = set("OPENCLAW_STATE_DIR") {
        PathBuf::from(directory).join("openclaw.json")
    } else {
        let directory = match set("OPENCLAW_PROFILE") {
            Some(profile) if profile != "default" => {
                let plain = profile
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'));
                if !plain {
                    return Err(AdapterFailure::ExternalOverride);
                }
                format!(".openclaw-{profile}")
            }
            _ => ".openclaw".to_owned(),
        };
        set("OPENCLAW_HOME")
            .map_or_else(|| home.to_owned(), PathBuf::from)
            .join(directory)
            .join("openclaw.json")
    };
    // 写了 `~`、相对路径之类要照 OpenClaw 的规则展开的：不猜，猜错就写到别的
    // 文件里去了。
    if !path.is_absolute()
        || path
            .components()
            .any(|part| matches!(part, Component::ParentDir))
    {
        return Err(AdapterFailure::ExternalOverride);
    }
    Ok(path)
}

pub(crate) fn config_path(home: &Path) -> Result<PathBuf, AdapterFailure> {
    // 走登录 shell 问：这些变量一般写在 rc 文件里，从 Dock 启动的程序看不到。
    config_path_from(home, crate::shell_environment::var_os)
}

pub(crate) fn config_dir(home: &Path) -> Result<PathBuf, AdapterFailure> {
    config_path(home)?
        .parent()
        .map(Path::to_owned)
        .ok_or(AdapterFailure::ExternalOverride)
}

fn parse_document(bytes: Option<&[u8]>) -> Result<Value, ()> {
    match bytes {
        Some(bytes) if !bytes.iter().all(u8::is_ascii_whitespace) => {
            let source = std::str::from_utf8(bytes).map_err(|_| ())?;
            let value: Value = json5::from_str(source).map_err(|_| ())?;
            value.is_object().then_some(value).ok_or(())
        }
        _ => Ok(Value::Object(Map::new())),
    }
}

pub(crate) fn primary(model: &Value) -> Option<&str> {
    model.as_str().or_else(|| model["primary"].as_str())
}

pub(crate) fn is_ours(reference: &str) -> bool {
    reference
        .strip_prefix(PROVIDER_ID)
        .is_some_and(|rest| rest.starts_with('/'))
}

/// 主 agent 实际用的型号：它自己另外定了（`agents.entries.main.model`）就是
/// 那个，这时它不看 `agents.defaults`（2026.11.6 实测）。
fn effective_model(root: &Value) -> &Value {
    let own = &root["agents"]["entries"]["main"]["model"];
    if own.is_null() {
        &root["agents"]["defaults"]["model"]
    } else {
        own
    }
}

/// 我们要写的那几段是用 `$include` 从别的文件拼进来的，写了也不算数。
fn included(root: &Value) -> bool {
    [
        root,
        &root["models"],
        &root["models"]["providers"],
        &root["agents"],
        &root["agents"]["defaults"],
        &root["agents"]["defaults"]["model"],
        &root["agents"]["defaults"]["modelPolicy"],
        &root["agents"]["entries"],
        &root["agents"]["entries"]["main"],
    ]
    .iter()
    .any(|section| section.get("$include").is_some())
}

fn child<'a>(
    object: &'a mut Map<String, Value>,
    key: &str,
) -> Result<&'a mut Map<String, Value>, AdapterFailure> {
    object
        .entry(key)
        .or_insert_with(|| Value::Object(Map::new()))
        .as_object_mut()
        .ok_or(PARSE_FAILED)
}

pub(crate) fn render(
    existing: Option<&[u8]>,
    model: &str,
    model_ids: &[String],
    local_token: &str,
) -> Result<Vec<u8>, AdapterFailure> {
    let mut root = parse_document(existing).map_err(|()| PARSE_FAILED)?;
    let reference = format!("{PROVIDER_ID}/{model}");
    // 主 agent 自己定的型号不是我们的东西，不替用户改。它和这次选的不是同一个
    // 时，接上去实际用的也不是选的那个（定的是我们名下已经不在的型号时还会
    // 直接报错），所以不接。
    let pinned = &root["agents"]["entries"]["main"]["model"];
    if included(&root) || (!pinned.is_null() && primary(pinned) != Some(reference.as_str())) {
        return Err(AdapterFailure::ExternalOverride);
    }
    let object = root.as_object_mut().ok_or(PARSE_FAILED)?;

    let models: Vec<Value> = model_ids
        .iter()
        .map(|id| json!({ "id": id, "name": id }))
        .collect();
    child(child(object, "models")?, "providers")?.insert(
        PROVIDER_ID.into(),
        json!({
            "baseUrl": PROXY_BASE,
            "apiKey": local_token,
            "api": "openai-completions",
            "models": models,
        }),
    );

    let defaults = child(child(object, "agents")?, "defaults")?;
    match defaults.get_mut("model") {
        // `{ primary, fallbacks }`：只换主型号，备用的那串是用户的。
        Some(Value::Object(chosen)) => {
            chosen.insert("primary".into(), reference.clone().into());
        }
        Some(Value::String(_)) | None => {
            defaults.insert("model".into(), reference.clone().into());
        }
        Some(_) => return Err(PARSE_FAILED),
    }
    // 用户设了白名单而里面没有我们：主型号会被换成名单里的第一个（实测）。
    if let Some(allow) = defaults
        .get_mut("modelPolicy")
        .and_then(|policy| policy.get_mut("allow"))
        .and_then(Value::as_array_mut)
    {
        let listed = allow
            .iter()
            .filter_map(Value::as_str)
            .any(|entry| entry == ALLOW_ENTRY || entry == reference);
        if !allow.is_empty() && !listed {
            allow.push(ALLOW_ENTRY.into());
        }
    }

    let mut bytes = serde_json::to_vec_pretty(&root).map_err(|_| PARSE_FAILED)?;
    bytes.push(b'\n');
    Ok(bytes)
}

pub(crate) fn prepare_catalog(
    home: &Path,
    model: &str,
    model_ids: &[String],
    local_token: Option<&str>,
) -> Result<Prepared, AdapterFailure> {
    prepare_at(config_path(home)?, model, model_ids, local_token)
}

fn prepare_at(
    path: PathBuf,
    model: &str,
    model_ids: &[String],
    local_token: Option<&str>,
) -> Result<Prepared, AdapterFailure> {
    common::validate_catalog(model, model_ids)?;
    let local_token = local_token
        .filter(|value| value.starts_with("ycg-") && value.len() == 68)
        .ok_or(AdapterFailure::SecureStorageUnavailable)?
        .to_owned();
    // OpenClaw 自己也不往符号链接上写（Nix 之类把配置做成只读链接的装法）。
    if std::fs::symlink_metadata(&path).is_ok_and(|meta| meta.file_type().is_symlink()) {
        return Err(AdapterFailure::ExternalOverride);
    }
    let before = common::snapshot(&path)
        .map_err(|_| AdapterFailure::ConfigurationFailed("configuration_read_failed"))?;
    let after = render(before.as_deref(), model, model_ids, &local_token)?;
    let transaction =
        FileTransaction::stage_with_snapshot(path.clone(), before, after).map_err(config_error)?;
    Ok(Prepared {
        transaction,
        path,
        model: model.to_owned(),
        model_ids: model_ids.to_vec(),
        local_token,
    })
}

/// `now` 里有、而我们从没写过的那些键（用户或 OpenClaw 之后加的），`restored`
/// 里没了就补回去。返回有没有补。
fn keep_added(ours: &Value, now: &Value, restored: &mut Value) -> bool {
    let (Some(ours), Some(now)) = (ours.as_object(), now.as_object()) else {
        return false;
    };
    let mut kept = false;
    for (key, current) in now {
        match ours.get(key) {
            None => {
                if let Some(result) = restored.as_object_mut() {
                    if !result.contains_key(key) {
                        result.insert(key.clone(), current.clone());
                        kept = true;
                    }
                }
            }
            Some(written) => {
                let mut inner = restored.get(key).cloned().unwrap_or(Value::Null);
                let missing = inner.is_null();
                if missing {
                    inner = Value::Object(Map::new());
                }
                if inner.is_object() && keep_added(written, current, &mut inner) {
                    if let Some(result) = restored.as_object_mut() {
                        result.insert(key.clone(), inner);
                        kept = true;
                    }
                }
            }
        }
    }
    kept
}

/// `holder.model` 还指着我们那一家：换回接入前的那个，原来没有就拿掉。
fn put_back_model(holder: &mut Map<String, Value>, previous: &Value) {
    if !holder.get("model").and_then(primary).is_some_and(is_ours) {
        return;
    }
    let earlier = primary(previous).map(str::to_owned);
    match holder.get_mut("model") {
        Some(Value::Object(chosen)) => {
            match earlier {
                Some(reference) => chosen.insert("primary".into(), reference.into()),
                None => chosen.remove("primary"),
            };
            if chosen.is_empty() {
                holder.remove("model");
            }
        }
        _ if previous.is_null() => {
            holder.remove("model");
        }
        _ => {
            holder.insert("model".into(), previous.clone());
        }
    }
}

/// `path` 上的那一段收尾之后空了、接入前又没有它：拿掉，不留空壳。
fn drop_if_emptied(before: &Value, restored: &mut Value, path: &[&str]) {
    let Some((last, parents)) = path.split_last() else {
        return;
    };
    let was_there = path
        .iter()
        .try_fold(before, |value, key| value.get(key))
        .is_some();
    let Some(parent) = parents
        .iter()
        .try_fold(restored, |value, key| value.get_mut(key))
        .and_then(Value::as_object_mut)
    else {
        return;
    };
    let emptied = parent
        .get(*last)
        .and_then(Value::as_object)
        .is_some_and(Map::is_empty);
    if emptied && !was_there {
        parent.remove(*last);
    }
}

/// 改回原样时，按「我们改了什么就改回什么」合并完之后收个尾：
/// * 接入时新建的段（原来没有 `models`、`agents`）会被整段拿走，用户之后加在
///   里面的别家厂商、别的设置不是我们的，补回去；
/// * 我们那一家已经拿走了，可用户之后在 OpenClaw 里挑过我们的另一个型号，
///   主型号就还指着一家不存在的厂商，OpenClaw 会起不来对话。这种就换回接入前
///   的那个；白名单里我们加的那一项、`openclaw models set` 顺手记下的型号别名
///   也一并拿走。
///
/// 别的都不动。
pub(crate) fn settle_restored(
    before: &Value,
    ours: &Value,
    now: &Value,
    restored: &mut Value,
    preserved: &mut bool,
) {
    let provider = |root: &Value| root["models"]["providers"].get(PROVIDER_ID).cloned();
    let is_this_file = provider(ours).is_some_and(|entry| entry["baseUrl"] == PROXY_BASE);
    if !is_this_file {
        return;
    }
    // 我们那一家整条都是我们的：用户往里加的请求头之类跟着它一起走，不单独
    // 留下半条厂商。
    let mut theirs = now.clone();
    if let Some(providers) = theirs
        .get_mut("models")
        .and_then(|models| models.get_mut("providers"))
        .and_then(Value::as_object_mut)
    {
        providers.remove(PROVIDER_ID);
    }
    *preserved |= keep_added(ours, &theirs, restored);
    // 用户改过我们那一家里面的东西（比如删了一个型号），通用的合并会把它当成
    // 用户的留下；可密钥已经收走，留下的是一家用不了的厂商。接入前没有它、
    // 它也还指着本机转发，就整条拿走。改去指别处的已经是用户自己的，不碰。
    let stranded = provider(before).is_none()
        && provider(restored).is_some_and(|entry| entry["baseUrl"] == PROXY_BASE);
    if stranded {
        if let Some(providers) = restored
            .get_mut("models")
            .and_then(|models| models.get_mut("providers"))
            .and_then(Value::as_object_mut)
        {
            providers.remove(PROVIDER_ID);
        }
        for path in [&["models", "providers"][..], &["models"]] {
            drop_if_emptied(before, restored, path);
        }
    }
    if provider(restored).is_some() {
        return;
    }
    // 主 agent 自己定的型号排在 `agents.defaults` 前面，一样不能指着空厂商。
    if let Some(main) = restored
        .get_mut("agents")
        .and_then(|agents| agents.get_mut("entries"))
        .and_then(|entries| entries.get_mut("main"))
        .and_then(Value::as_object_mut)
    {
        put_back_model(main, &before["agents"]["entries"]["main"]["model"]);
    }
    for path in [
        &["agents", "entries", "main"][..],
        &["agents", "entries"],
        &["agents"],
    ] {
        drop_if_emptied(before, restored, path);
    }
    let Some(defaults) = restored
        .get_mut("agents")
        .and_then(|agents| agents.get_mut("defaults"))
        .and_then(Value::as_object_mut)
    else {
        return;
    };
    put_back_model(defaults, &before["agents"]["defaults"]["model"]);
    let listed_before = before["agents"]["defaults"]["modelPolicy"]["allow"]
        .as_array()
        .is_some_and(|allow| allow.iter().any(|entry| entry == ALLOW_ENTRY));
    if !listed_before {
        if let Some(allow) = defaults
            .get_mut("modelPolicy")
            .and_then(|policy| policy.get_mut("allow"))
            .and_then(Value::as_array_mut)
        {
            allow.retain(|entry| entry != ALLOW_ENTRY);
        }
    }
    // `agents.defaults.models` 是型号的别名表，键是 `厂商/型号`。
    let known_before = |reference: &str| {
        before["agents"]["defaults"]["models"]
            .get(reference)
            .is_some()
    };
    if let Some(aliases) = defaults.get_mut("models").and_then(Value::as_object_mut) {
        aliases.retain(|reference, _| !is_ours(reference) || known_before(reference));
        if aliases.is_empty() && before["agents"]["defaults"].get("models").is_none() {
            defaults.remove("models");
        }
    }
    if defaults.is_empty() {
        if let Some(agents) = restored.get_mut("agents").and_then(Value::as_object_mut) {
            agents.remove("defaults");
            if agents.is_empty() && before.get("agents").is_none() {
                if let Some(root) = restored.as_object_mut() {
                    root.remove("agents");
                }
            }
        }
    }
}

/// 重启 YesChoy 后，OpenClaw 若仍指着我们这座桥，就把桥重新起起来。
/// 只看 OpenClaw 自己的配置：用户已经换走了，就不占端口。
pub(crate) async fn resume_if_configured() {
    let Some(credential) = configured_credential() else {
        return;
    };
    let bridge = crate::openclaw_bridge::global();
    match bridge.start(credential).await {
        Ok(_) => {}
        Err(failure) if crate::local_bridge::is_port_busy(&failure) => {
            crate::local_bridge::retry_while_port_busy(
                bridge.clone(),
                "openclaw_bridge",
                configured_credential,
            );
        }
        Err(error) => log::warn!("openclaw_bridge stage=resume_start_failed reason={error:?}"),
    }
}

/// 存着的凭据，前提是它的设置还指向本机桥。
fn configured_credential() -> Option<crate::tool_credentials::ToolCredential> {
    let credential = crate::tool_credentials::load("openclaw").ok()?;
    let home = super::user_home()?;
    let path = config_path(&home).ok()?;
    let bytes = std::fs::read(path).ok()?;
    points_at_our_bridge(&bytes).then_some(credential)
}

/// 主型号选的是不是我们这一家，且这一家还指着本地桥。
pub(crate) fn points_at_our_bridge(bytes: &[u8]) -> bool {
    parse_document(Some(bytes)).is_ok_and(|value| {
        primary(effective_model(&value)).is_some_and(is_ours)
            && value["models"]["providers"][PROVIDER_ID]["baseUrl"].as_str() == Some(PROXY_BASE)
    })
}

fn allows(root: &Value, reference: &str) -> bool {
    root["agents"]["defaults"]["modelPolicy"]["allow"]
        .as_array()
        .is_none_or(|allow| {
            allow.is_empty()
                || allow
                    .iter()
                    .filter_map(Value::as_str)
                    .any(|entry| entry == ALLOW_ENTRY || entry == reference)
        })
}

/// 接好之后文件和我们写下的不再一字不差，但我们那一家还原样在、主型号还是
/// 接入时选的那个、白名单也没把它挡住：OpenClaw 自己会重写这份文件，用户也
/// 会改别处。这都不算「被改过」。不是我们写的文件就是 `None`。
pub(crate) fn still_ours(ours: &[u8], now: &[u8]) -> Option<bool> {
    let ours = parse_document(Some(ours)).ok()?;
    let written = ours["models"]["providers"].get(PROVIDER_ID)?;
    if written["baseUrl"].as_str() != Some(PROXY_BASE) {
        return None;
    }
    let chosen = primary(effective_model(&ours)).filter(|reference| is_ours(reference))?;
    Some(parse_document(Some(now)).is_ok_and(|now| {
        // 后来改成从别的文件拼进来的：那边写了什么这里看不到，不敢说还接着。
        !included(&now)
            && now["models"]["providers"].get(PROVIDER_ID) == Some(written)
            && primary(effective_model(&now)) == Some(chosen)
            && allows(&now, chosen)
    }))
}

impl Prepared {
    pub(crate) fn changes(&self) -> &[common::FileChange] {
        self.transaction.changes()
    }

    pub(crate) fn commit(&mut self) -> Result<(), AdapterFailure> {
        self.transaction.commit().map_err(config_error)?;
        self.validate_readback(true)
    }

    pub(crate) fn validate_existing(&self) -> Result<(), AdapterFailure> {
        self.validate_readback(false)
    }

    fn validate_readback(&self, strict_default: bool) -> Result<(), AdapterFailure> {
        let failed = AdapterFailure::ConfigurationFailed("configuration_readback_failed");
        let bytes = common::snapshot(&self.path)
            .map_err(|_| failed)?
            .ok_or(failed)?;
        let value = parse_document(Some(&bytes)).map_err(|()| failed)?;
        let provider = &value["models"]["providers"][PROVIDER_ID];
        let mut actual: Vec<&str> = provider["models"]
            .as_array()
            .map(|models| {
                models
                    .iter()
                    .filter_map(|model| model["id"].as_str())
                    .collect()
            })
            .unwrap_or_default();
        actual.sort_unstable();
        let mut expected: Vec<&str> = self.model_ids.iter().map(String::as_str).collect();
        expected.sort_unstable();
        let defaults = &value["agents"]["defaults"];
        let default = primary(effective_model(&value))
            .and_then(|reference| reference.strip_prefix(PROVIDER_ID))
            .and_then(|rest| rest.strip_prefix('/'));
        let allowed = defaults["modelPolicy"]["allow"]
            .as_array()
            .is_none_or(|allow| {
                allow.is_empty()
                    || allow.iter().filter_map(Value::as_str).any(|entry| {
                        entry == ALLOW_ENTRY || entry.strip_prefix("yeschoy/") == default
                    })
            });
        let correct = !included(&value)
            && provider["baseUrl"].as_str() == Some(PROXY_BASE)
            && provider["apiKey"].as_str() == Some(self.local_token.as_str())
            && provider["api"].as_str() == Some("openai-completions")
            && actual == expected
            && allowed
            && common::default_matches(default, &self.model, &self.model_ids, strict_default);
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

    fn ids() -> Vec<String> {
        vec!["glm-5.3".into(), "kimi-k3".into()]
    }

    fn rendered(before: &str) -> Value {
        let bytes = render(Some(before.as_bytes()), "glm-5.3", &ids(), &token()).unwrap();
        serde_json::from_slice(&bytes).unwrap()
    }

    /// 测试里的路径按 unix 的样子写；Windows 上 `/srv` 不算绝对路径，补上盘符。
    fn rooted(path: &str) -> String {
        if cfg!(windows) && path.starts_with('/') {
            format!("C:{path}")
        } else {
            path.to_owned()
        }
    }

    fn path_with(pairs: &[(&str, &str)]) -> Result<PathBuf, AdapterFailure> {
        config_path_from(Path::new(&rooted("/home/u")), |name| {
            pairs
                .iter()
                .find(|(key, _)| *key == name)
                .map(|(_, value)| OsString::from(rooted(value)))
        })
    }

    /// 每一行都和真的 `openclaw config file`（2026.11.6）对过。
    #[test]
    fn the_file_is_where_openclaw_itself_looks() {
        let cases: [(&[(&str, &str)], &str); 8] = [
            (&[], "/home/u/.openclaw/openclaw.json"),
            (
                &[("OPENCLAW_PROFILE", "work")],
                "/home/u/.openclaw-work/openclaw.json",
            ),
            (
                &[("OPENCLAW_PROFILE", "default")],
                "/home/u/.openclaw/openclaw.json",
            ),
            (
                &[("OPENCLAW_HOME", "/srv/oc")],
                "/srv/oc/.openclaw/openclaw.json",
            ),
            (
                &[("OPENCLAW_HOME", "/srv/oc"), ("OPENCLAW_PROFILE", "work")],
                "/srv/oc/.openclaw-work/openclaw.json",
            ),
            (
                &[
                    ("OPENCLAW_STATE_DIR", "/srv/state"),
                    ("OPENCLAW_HOME", "/srv/oc"),
                    ("OPENCLAW_PROFILE", "work"),
                ],
                "/srv/state/openclaw.json",
            ),
            (
                &[
                    ("OPENCLAW_CONFIG_PATH", "/srv/cfg/my.json5"),
                    ("OPENCLAW_STATE_DIR", "/srv/state"),
                ],
                "/srv/cfg/my.json5",
            ),
            (
                &[("OPENCLAW_STATE_DIR", " ")],
                "/home/u/.openclaw/openclaw.json",
            ),
        ];
        for (variables, expected) in cases {
            assert_eq!(
                path_with(variables).unwrap(),
                PathBuf::from(rooted(expected)),
                "{variables:?}"
            );
        }
        for name in PATH_VARIABLES {
            assert!(
                cases
                    .iter()
                    .any(|(set, _)| set.iter().any(|(key, _)| *key == name)),
                "{name}"
            );
        }
    }

    #[test]
    fn a_path_we_would_have_to_guess_is_refused() {
        for variables in [
            [("OPENCLAW_HOME", "~/elsewhere")],
            [("OPENCLAW_CONFIG_PATH", "undefined")],
            [("OPENCLAW_STATE_DIR", "/srv/../etc")],
            [("OPENCLAW_PROFILE", "Bad Name")],
            [("OPENCLAW_PROFILE", "../x")],
        ] {
            assert_eq!(
                path_with(&variables),
                Err(AdapterFailure::ExternalOverride),
                "{variables:?}"
            );
        }
    }

    #[test]
    fn merge_keeps_user_settings_and_writes_no_site_key() {
        let value = rendered(
            r#"{
              // 我自己的设置
              gateway: { port: 18789 },
              channels: { telegram: { enabled: true } },
              models: { mode: 'merge', providers: { ollama: { baseUrl: 'http://127.0.0.1:11434', models: [] } } },
              agents: { defaults: { workspace: '~/claw', model: { primary: 'ollama/qwen', fallbacks: ['ollama/llama'] } },
                        entries: { helper: { model: 'ollama/llama' } } },
            }"#,
        );
        assert_eq!(value["gateway"]["port"], 18789);
        assert_eq!(value["channels"]["telegram"]["enabled"], true);
        assert_eq!(value["models"]["mode"], "merge");
        assert_eq!(
            value["models"]["providers"]["ollama"]["baseUrl"],
            "http://127.0.0.1:11434"
        );
        assert_eq!(value["agents"]["defaults"]["workspace"], "~/claw");
        assert_eq!(
            value["agents"]["entries"]["helper"]["model"],
            "ollama/llama"
        );
        // 对象形式：只换主型号，备用的那串还是用户的。
        assert_eq!(
            value["agents"]["defaults"]["model"],
            json!({ "primary": "yeschoy/glm-5.3", "fallbacks": ["ollama/llama"] })
        );
        // 严格校验下能过的最小一组键：多写一个（比如 `name`）整份配置就作废。
        assert_eq!(
            value["models"]["providers"][PROVIDER_ID],
            json!({
                "baseUrl": PROXY_BASE,
                "apiKey": token(),
                "api": "openai-completions",
                "models": [
                    { "id": "glm-5.3", "name": "glm-5.3" },
                    { "id": "kimi-k3", "name": "kimi-k3" },
                ],
            })
        );
        assert!(!value.to_string().contains("sk-"));
    }

    #[test]
    fn an_empty_or_missing_file_gets_the_string_form() {
        for before in [None, Some(&b""[..]), Some(&b"  \n"[..]), Some(&b"{}"[..])] {
            let bytes = render(before, "glm-5.3", &ids(), &token()).unwrap();
            let value: Value = serde_json::from_slice(&bytes).unwrap();
            assert_eq!(value["agents"]["defaults"]["model"], "yeschoy/glm-5.3");
            assert!(value["agents"]["defaults"].get("modelPolicy").is_none());
            assert!(points_at_our_bridge(&bytes));
        }
        let value = rendered(r#"{ agents: { defaults: { model: "openai/gpt-5.4" } } }"#);
        assert_eq!(value["agents"]["defaults"]["model"], "yeschoy/glm-5.3");
    }

    /// 实测：白名单里没有我们时，OpenClaw 会把主型号换成名单里的第一个。
    #[test]
    fn a_model_allowlist_gains_our_entry_once() {
        let policy = |allow: &str| {
            rendered(&format!(
                "{{ agents: {{ defaults: {{ modelPolicy: {{ allow: {allow} }} }} }} }}"
            ))["agents"]["defaults"]["modelPolicy"]["allow"]
                .clone()
        };
        assert_eq!(policy(r#"["openai/*"]"#), json!(["openai/*", ALLOW_ENTRY]));
        assert_eq!(
            policy(r#"["openai/*", "yeschoy/*"]"#),
            json!(["openai/*", ALLOW_ENTRY])
        );
        assert_eq!(policy(r#"["yeschoy/glm-5.3"]"#), json!(["yeschoy/glm-5.3"]));
        // 空名单 = 什么都许，不用加。
        assert_eq!(policy("[]"), json!([]));
    }

    #[test]
    fn settings_owned_by_something_else_are_refused() {
        for before in [
            // 主 agent 自己另定了型号：它不看 `agents.defaults`。
            r#"{ agents: { entries: { main: { model: "openai/gpt-5.4" } } } }"#,
            r#"{ agents: { entries: { main: { model: { primary: "openai/gpt-5.4" } } } } }"#,
            // 这几段是从别的文件拼进来的。
            r#"{ $include: "./base.json5" }"#,
            r#"{ models: { $include: "./models.json5" } }"#,
            r#"{ models: { providers: { $include: "./providers.json5" } } }"#,
            r#"{ agents: { $include: "./agents.json5" } }"#,
            r#"{ agents: { defaults: { $include: "./defaults.json5" } } }"#,
        ] {
            assert_eq!(
                render(Some(before.as_bytes()), "glm-5.3", &ids(), &token()),
                Err(AdapterFailure::ExternalOverride),
                "{before}"
            );
        }
        // 主 agent 定的是我们名下的另一个型号：接上去它用的还是那个，这次的
        // 名单里没有它时还会直接报错。同样不接。
        assert_eq!(
            render(
                Some(br#"{ agents: { entries: { main: { model: "yeschoy/kimi-k3" } } } }"#),
                "glm-5.3",
                &["glm-5.3".to_owned()],
                &token()
            ),
            Err(AdapterFailure::ExternalOverride)
        );
        // 定的正好是这次选的：不拦，也不动它。
        let value = rendered(r#"{ agents: { entries: { main: { model: "yeschoy/glm-5.3" } } } }"#);
        assert_eq!(
            value["agents"]["entries"]["main"]["model"],
            "yeschoy/glm-5.3"
        );
    }

    #[test]
    fn a_main_agent_model_added_later_counts_as_the_one_in_use() {
        let after = String::from_utf8(render(None, "glm-5.3", &ids(), &token()).unwrap()).unwrap();
        let pinned = |reference: &str| {
            let mut value: Value = serde_json::from_str(&after).unwrap();
            value["agents"]["entries"] = json!({ "main": { "model": reference } });
            serde_json::to_vec(&value).unwrap()
        };
        assert!(points_at_our_bridge(after.as_bytes()));
        assert!(points_at_our_bridge(&pinned("yeschoy/deepseek-v4")));
        assert!(!points_at_our_bridge(&pinned("openai/gpt-5.4")));
    }

    #[test]
    fn a_broken_or_odd_file_is_not_overwritten() {
        for before in [
            "{ not json",
            "[]",
            r#"{ models: [] }"#,
            r#"{ agents: { defaults: { model: 3 } } }"#,
        ] {
            assert_eq!(
                render(Some(before.as_bytes()), "glm-5.3", &ids(), &token()),
                Err(PARSE_FAILED),
                "{before}"
            );
        }
    }

    #[test]
    fn the_bridge_resumes_only_while_openclaw_still_uses_it() {
        let ours = render(None, "glm-5.3", &ids(), &token()).unwrap();
        assert!(points_at_our_bridge(&ours));
        let mut value: Value = serde_json::from_slice(&ours).unwrap();
        value["agents"]["defaults"]["model"] = "openai/gpt-5.4".into();
        assert!(!points_at_our_bridge(value.to_string().as_bytes()));
        value["agents"]["defaults"]["model"] = json!({ "primary": "yeschoy/kimi-k3" });
        assert!(points_at_our_bridge(value.to_string().as_bytes()));
        // 名字以 `yeschoy` 开头的别家厂商不算我们。
        value["agents"]["defaults"]["model"] = "yeschoyx/glm-5.3".into();
        assert!(!points_at_our_bridge(value.to_string().as_bytes()));
        value["agents"]["defaults"]["model"] = "yeschoy/glm-5.3".into();
        value["models"]["providers"][PROVIDER_ID]["baseUrl"] = "https://example.com/v1".into();
        assert!(!points_at_our_bridge(value.to_string().as_bytes()));
        assert!(!points_at_our_bridge(b"{ not json"));
    }

    #[test]
    fn apply_then_rollback_restores_original_bytes() {
        let home = common::temporary_working_directory("openclaw-apply").unwrap();
        let path = home.join(".openclaw").join("openclaw.json");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let original = b"{\n  // mine\n  gateway: { port: 18789 },\n}\n";
        std::fs::write(&path, original).unwrap();

        let mut prepared = prepare_at(path.clone(), "glm-5.3", &ids(), Some(&token())).unwrap();
        prepared.commit().unwrap();
        prepared.validate_existing().unwrap();
        let written = std::fs::read(&path).unwrap();
        assert!(points_at_our_bridge(&written));
        // OpenClaw 读得了 JSON5，但我们写出去的是它自己保存时的那种标准 JSON。
        serde_json::from_slice::<Value>(&written).unwrap();

        prepared.rollback().unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), original);
        let _ = std::fs::remove_dir_all(home);
    }

    #[test]
    fn readback_notices_a_changed_file() {
        let home = common::temporary_working_directory("openclaw-readback").unwrap();
        let path = home.join("openclaw.json");
        let mut prepared = prepare_at(path.clone(), "glm-5.3", &ids(), Some(&token())).unwrap();
        prepared.commit().unwrap();

        let mut value: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        // 用户在 OpenClaw 里换成我们名单里的另一个型号：仍算接着。
        value["agents"]["defaults"]["model"] = "yeschoy/kimi-k3".into();
        std::fs::write(&path, value.to_string()).unwrap();
        prepared.validate_existing().unwrap();
        // 白名单把我们挡在外面了：不算。
        value["agents"]["defaults"]["modelPolicy"] = json!({ "allow": ["openai/*"] });
        std::fs::write(&path, value.to_string()).unwrap();
        assert!(prepared.validate_existing().is_err());
        value["agents"]["defaults"]["modelPolicy"] = json!({ "allow": ["openai/*", "yeschoy/*"] });
        // 主 agent 另定了型号：不算。
        value["agents"]["entries"] = json!({ "main": { "model": "openai/gpt-5.4" } });
        std::fs::write(&path, value.to_string()).unwrap();
        assert!(prepared.validate_existing().is_err());
        let _ = std::fs::remove_dir_all(home);
    }

    #[test]
    fn a_site_key_or_a_symlinked_file_is_refused() {
        let home = common::temporary_working_directory("openclaw-refuse").unwrap();
        let path = home.join("openclaw.json");
        assert!(matches!(
            prepare_at(path.clone(), "glm-5.3", &ids(), Some("sk-real-site-key")),
            Err(AdapterFailure::SecureStorageUnavailable)
        ));
        #[cfg(unix)]
        {
            let target = home.join("managed.json");
            std::fs::write(&target, b"{}").unwrap();
            std::os::unix::fs::symlink(&target, &path).unwrap();
            assert!(matches!(
                prepare_at(path, "glm-5.3", &ids(), Some(&token())),
                Err(AdapterFailure::ExternalOverride)
            ));
            assert_eq!(std::fs::read(&target).unwrap(), b"{}");
        }
        let _ = std::fs::remove_dir_all(home);
    }
}

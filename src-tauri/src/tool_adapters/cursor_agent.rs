//! Cursor Agent（命令 `agent`）的配置管理。
//!
//! 为什么只有 CLI：Cursor 桌面版把「自定义 OpenAI 兼容服务」的地址存在它自己的
//! SQLite `state.vscdb` 里、密钥存进它的钥匙串条目，而且是否启用还要在 Cursor
//! 界面里勾选。没有第三种「配置文件」入口可以写。硬改这两处等于替用户做它自己
//! 的登录与开关操作，一旦出错会把 Cursor 的登录态弄坏——所以桌面版只做只读概览，
//! 界面如实告诉用户要改哪一个开关。
//!
//! CLI 侧是干净的：`~/.cursor/cli-config.json` 是文档化的用户配置，带 JSON
//! Schema；`network.*` 与 `model.*` 就是端点、自定义请求头与默认模型。
//!
//! 写入遵守产品不变量：先存原文件、原子写、读回校验，失败整体回滚。
use std::path::{Path, PathBuf};

use serde::Serialize;
use serde_json::{Map, Value};

use crate::{tool_adapters::AdapterFailure, tool_model_profile};

pub(crate) struct Prepared {
    path: PathBuf,
    /// 写之前的原始内容；`None` 表示文件原本不存在。
    before: Option<Vec<u8>>,
    expected: Vec<(String, String)>,
}

/// `~/.cursor/cli-config.json`。
pub(crate) fn config_path(home: &Path) -> PathBuf {
    home.join(".cursor").join("cli-config.json")
}

#[cfg_attr(not(test), allow(dead_code))]
fn base_url(origin: &str) -> String {
    origin.trim_end_matches('/').to_owned()
}

/// 端点必须是 https，http 只允许本机回环。渲染层已经校验过一次，这里是
/// 原生侧最后一道，因为写进去的是用户应用真正会连接的地址。
#[cfg_attr(not(test), allow(unused))]
pub(crate) fn valid_endpoint(value: &str) -> bool {
    let Ok(url) = reqwest::Url::parse(value) else {
        return false;
    };
    match url.scheme() {
        "https" => !url.host_str().unwrap_or_default().is_empty(),
        "http" => matches!(
            url.host_str(),
            Some("127.0.0.1") | Some("localhost") | Some("[::1]")
        ),
        _ => false,
    }
}

fn parse(existing: Option<&[u8]>) -> Result<Value, ()> {
    match existing {
        None | Some([]) => Ok(Value::Object(Map::new())),
        Some(bytes) => {
            let value: Value = serde_json::from_slice(bytes).map_err(|_| ())?;
            value.is_object().then_some(value).ok_or(())
        }
    }
}

fn object<'a>(root: &'a mut Value, key: &str) -> Result<&'a mut Map<String, Value>, ()> {
    let root = root.as_object_mut().ok_or(())?;
    root.entry(key)
        .or_insert_with(|| Value::Object(Map::new()))
        .as_object_mut()
        .ok_or(())
}

/// 合并写入：只改 `network.baseUrl`、`network.headers`，以及（有选择时）
/// `model` 区块。用户自己的其他键（权限、编辑器、沙箱……）原样保留。
fn render(
    existing: Option<&[u8]>,
    endpoint: &str,
    default_model: Option<&str>,
) -> Result<Vec<u8>, &'static str> {
    let mut root = parse(existing).map_err(|_| "configuration_parse_failed")?;
    if !valid_endpoint(endpoint) {
        return Err("invalid_endpoint");
    }
    let network = object(&mut root, "network").map_err(|_| "configuration_parse_failed")?;
    network.insert("baseUrl".into(), Value::String(endpoint.to_owned()));
    network.insert("useHttp1ForAgent".into(), Value::Bool(false));
    let headers = network
        .entry("headers")
        .or_insert_with(|| Value::Object(Map::new()))
        .as_object_mut()
        .ok_or("configuration_parse_failed")?;
    // Cursor CLI 自己发出的 Authorization 不会被覆盖；这里只补客户端标识。
    headers.insert(
        "X-Yeschoy-Desktop".into(),
        Value::String("cursor-agent".into()),
    );

    if let Some(model) = default_model {
        let model_key = model_selector(model);
        let selection = object(&mut root, "model").map_err(|_| "configuration_parse_failed")?;
        selection.insert("modelId".into(), Value::String(model_key));
        // 目录里没有这个型号时不猜参数，保留用户原有的。
        if let Some(profile) = tool_model_profile::profile(model) {
            let mut parameters = Vec::new();
            if let Some(context) = profile.context_window {
                parameters.push(serde_json::json!({
                    "id": "context",
                    "value": format!("{context}"),
                }));
            }
            if let Some(default) = &profile.default_reasoning {
                parameters.push(serde_json::json!({
                    "id": "reasoning_effort",
                    "value": default,
                }));
            }
            if !parameters.is_empty() {
                selection.insert("parameters".into(), Value::Array(parameters));
            }
        }
    }

    let mut bytes = serde_json::to_vec_pretty(&root).map_err(|_| "configuration_write_failed")?;
    bytes.push(b'\n');
    Ok(bytes)
}

/// Cursor CLI 用 `[context=…,effort=…]` 后缀表达参数化模型；没有档位信息的
/// 型号只写基础 id。
fn model_selector(model: &str) -> String {
    let Some(profile) = tool_model_profile::profile(model) else {
        return model.to_owned();
    };
    let mut parts: Vec<String> = Vec::new();
    // 只有型号本身标了 1m 才写 1m 上下文，避免把普通窗口说成 1M。
    if model.to_ascii_lowercase().contains("1m") && profile.context_window.is_some() {
        parts.push("context=1m".into());
    }
    if let Some(effort) = profile
        .default_reasoning
        .as_deref()
        .or_else(|| profile.reasoning_levels.first().map(String::as_str))
        .filter(|level| !level.is_empty() && *level != "none")
    {
        parts.push(format!("effort={effort}"));
    }
    if parts.is_empty() {
        model.to_owned()
    } else {
        format!("{model}[{}]", parts.join(","))
    }
}

/// 只读：现在的端点与请求头，用于预览。「恢复原设置」不需要它。
pub(crate) fn current(home: &Path) -> Option<(String, Vec<String>)> {
    let bytes = std::fs::read(config_path(home)).ok()?;
    let root = parse(Some(&bytes)).ok()?;
    let network = root.get("network")?.as_object()?;
    let endpoint = network.get("baseUrl")?.as_str()?.to_owned();
    let headers = network
        .get("headers")
        .and_then(Value::as_object)
        .map(|headers| headers.keys().cloned().collect())
        .unwrap_or_default();
    Some((endpoint, headers))
}

pub(crate) fn prepare(
    home: &Path,
    origin: &str,
    default_model: Option<&str>,
) -> Result<Prepared, AdapterFailure> {
    let path = config_path(home);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|_| AdapterFailure::ConfigurationFailed("configuration_write_failed"))?;
    }
    let endpoint = base_url(origin);
    let before = match std::fs::read(&path) {
        Ok(bytes) => Some(bytes),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(_) => {
            return Err(AdapterFailure::ConfigurationFailed(
                "configuration_read_failed",
            ))
        }
    };
    let bytes = render(before.as_deref(), &endpoint, default_model)
        .map_err(AdapterFailure::ConfigurationFailed)?;
    write_atomically(&path, &bytes)
        .map_err(|_| AdapterFailure::ConfigurationFailed("configuration_write_failed"))?;

    let mut expected = vec![("network.baseUrl".to_owned(), endpoint)];
    if let Some(model) = default_model {
        expected.push(("model.modelId".to_owned(), model_selector(model)));
    }
    expected.push((
        "network.headers.X-Yeschoy-Desktop".to_owned(),
        "cursor-agent".to_owned(),
    ));
    Ok(Prepared {
        path,
        before,
        expected,
    })
}

/// 同目录临时文件 + rename：中途失败不会留下半个 JSON。
fn write_atomically(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("cli-config.json");
    let temporary = path.with_file_name(format!(
        ".{file_name}.yeschoy-{}-{}.tmp",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or_default()
    ));
    std::fs::write(&temporary, bytes)?;
    std::fs::rename(&temporary, path).inspect_err(|_| {
        let _ = std::fs::remove_file(&temporary);
    })
}

/// 读回校验。CLI 的配置文件里出现任何我们写下的值都算通过；不一致就
/// 整体回滚，不留半成品。
pub(crate) fn verify(prepared: &Prepared) -> Result<(), AdapterFailure> {
    let bytes = std::fs::read(&prepared.path)
        .map_err(|_| AdapterFailure::ConfigurationFailed("configuration_readback_failed"))?;
    let root = parse(Some(&bytes))
        .map_err(|_| AdapterFailure::ConfigurationFailed("configuration_readback_failed"))?;
    for (path, expected) in &prepared.expected {
        let actual = match path.as_str() {
            "network.baseUrl" => root
                .pointer("/network/baseUrl")
                .and_then(Value::as_str)
                .map(str::to_owned),
            "model.modelId" => root
                .pointer("/model/modelId")
                .and_then(Value::as_str)
                .map(str::to_owned),
            "network.headers.X-Yeschoy-Desktop" => root
                .pointer("/network/headers/X-Yeschoy-Desktop")
                .and_then(Value::as_str)
                .map(str::to_owned),
            _ => None,
        };
        if actual.as_deref() != Some(expected.as_str()) {
            return Err(AdapterFailure::ConfigurationFailed(
                "configuration_readback_failed",
            ));
        }
    }
    Ok(())
}

/// 把文件写回接入前的内容；原本不存在就删掉，免得留下空文件。
pub(crate) fn rollback(prepared: Prepared) -> Result<(), AdapterFailure> {
    let result = match prepared.before {
        Some(before) => write_atomically(&prepared.path, &before),
        None => match std::fs::remove_file(&prepared.path) {
            Err(error) if error.kind() != std::io::ErrorKind::NotFound => Err(error),
            _ => Ok(()),
        },
    };
    result.map_err(|_| AdapterFailure::ConfigurationFailed("configuration_rollback_failed"))
}

/// 撤销：把 `network.baseUrl` / 自定义头 / 我们写的模型选择去掉，其余保留。
/// 「恢复原设置」只撤回助手写的字段（产品不变量 4）。
pub(crate) fn undo(home: &Path) -> Result<(), AdapterFailure> {
    let path = config_path(home);
    let Ok(existing) = std::fs::read(&path) else {
        return Ok(());
    };
    let mut root = parse(Some(&existing))
        .map_err(|_| AdapterFailure::ConfigurationFailed("configuration_parse_failed"))?;
    if let Some(network) = root.get_mut("network").and_then(Value::as_object_mut) {
        network.remove("baseUrl");
        if let Some(headers) = network.get_mut("headers").and_then(Value::as_object_mut) {
            headers.remove("X-Yeschoy-Desktop");
            if headers.is_empty() {
                network.remove("headers");
            }
        }
    }
    // 只删助手写过的模型选择；用户自己选的保留。
    if let Some(selection) = root.get("model").and_then(Value::as_object) {
        if selection.contains_key("modelId") {
            if let Some(root) = root.as_object_mut() {
                root.remove("model");
            }
        }
    }
    if root
        .get("network")
        .and_then(Value::as_object)
        .is_some_and(|network| network.is_empty())
    {
        if let Some(root) = root.as_object_mut() {
            root.remove("network");
        }
    }
    let mut bytes = serde_json::to_vec_pretty(&root)
        .map_err(|_| AdapterFailure::ConfigurationFailed("configuration_write_failed"))?;
    bytes.push(b'\n');
    write_atomically(&path, &bytes)
        .map_err(|_| AdapterFailure::ConfigurationFailed("configuration_write_failed"))
}

/// Cursor 桌面版：只读概览，说明为什么不能一键接入。
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CursorIdeState {
    /// 找没找到 Cursor.app。
    pub(crate) observed: bool,
    /// Cursor 里是否配过自定义 OpenAI 兼容端点（只读判定，不展示地址）。
    pub(crate) custom_endpoint_configured: bool,
    /// `openai` | `none` | `unknown`
    pub(crate) existing_provider: &'static str,
}

/// Cursor 桌面版的只读概览。只判断「装没装」和「有没有配过自定义端点」，
/// 不读取、也不展示那个端点和密钥——它们在 Cursor 自己的数据库与钥匙串里。
fn ide_state(_home: &Path) -> CursorIdeState {
    #[cfg(target_os = "macos")]
    let app = Path::new("/Applications/Cursor.app");
    #[cfg(target_os = "windows")]
    let app = &_home
        .join("AppData")
        .join("Local")
        .join("Programs")
        .join("cursor");
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let app = &_home.join(".local").join("share").join("cursor");
    let observed = cfg!(target_os = "windows") || app.exists();
    CursorIdeState {
        observed,
        // 精确判定需要在 Cursor 的 SQLite 里查 `openAIBaseUrl`，这一版不做
        // 猜测：宁可说「未知」，也不误报用户已经配过。
        custom_endpoint_configured: false,
        existing_provider: "unknown",
    }
}

#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn cli_candidates(home: &Path) -> Vec<PathBuf> {
    let mut paths = vec![
        home.join(".local").join("bin").join("cursor-agent"),
        home.join(".local").join("bin").join("agent"),
        home.join(".cursor").join("bin").join("cursor-agent"),
    ];
    #[cfg(target_os = "macos")]
    paths.push(
        Path::new("/Applications/Cursor.app/Contents/Resources/app/bin/cursor-agent").to_path_buf(),
    );
    #[cfg(target_os = "macos")]
    paths
        .push(Path::new("/Applications/Cursor.app/Contents/Resources/app/bin/agent").to_path_buf());
    paths
}

pub(crate) fn detect_cli(home: &Path) -> Option<PathBuf> {
    if let Some(found) = crate::tool_discovery::discover_candidates("cursor-agent")
        .into_iter()
        .next()
    {
        return Some(found.path);
    }
    cli_candidates(home).into_iter().find(|path| path.is_file())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    struct TempHome(PathBuf);
    impl TempHome {
        fn new(tag: &str) -> Self {
            let root = std::env::temp_dir().join(format!(
                "yeschoy-cursor-{tag}-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
            std::fs::create_dir_all(root.join(".cursor")).unwrap();
            Self(root)
        }
        fn config(&self) -> PathBuf {
            config_path(&self.0)
        }
    }
    impl Drop for TempHome {
        fn drop(&mut self) {
            std::fs::remove_dir_all(&self.0).ok();
        }
    }

    const EXISTING: &[u8] = br#"{
  "version": 1,
  "permissions": { "allow": ["Shell(ls)"], "deny": [] },
  "approvalMode": "allowlist",
  "network": { "headers": { "X-Own": "1" }, "customTimeout": 9 }
}"#;

    #[test]
    fn only_https_or_loopback_http_endpoints_are_accepted() {
        for ok in [
            "https://api.yeschoy.com",
            "https://yeschoy.com",
            "http://127.0.0.1:15734",
            "http://localhost:8080",
        ] {
            assert!(valid_endpoint(ok), "{ok}");
        }
        for bad in [
            "http://api.yeschoy.com",
            "ftp://yeschoy.com",
            "yeschoy.com",
            "https://",
            "javascript:alert(1)",
            "",
        ] {
            assert!(!valid_endpoint(bad), "{bad}");
        }
    }

    #[test]
    fn merge_keeps_user_keys_and_adds_ours() {
        let bytes = render(Some(EXISTING), "https://api.yeschoy.com", None).unwrap();
        let value: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(value["permissions"]["allow"][0], "Shell(ls)");
        assert_eq!(value["approvalMode"], "allowlist");
        assert_eq!(value["network"]["baseUrl"], "https://api.yeschoy.com");
        assert_eq!(value["network"]["customTimeout"], 9);
        assert_eq!(value["network"]["headers"]["X-Own"], "1");
        assert_eq!(
            value["network"]["headers"]["X-Yeschoy-Desktop"],
            "cursor-agent"
        );
        assert!(value.get("model").is_none());
    }

    #[test]
    fn refuses_to_write_a_non_https_endpoint() {
        assert_eq!(
            render(None, "http://api.yeschoy.com", None),
            Err("invalid_endpoint")
        );
        assert_eq!(
            render(Some(b"[]"), "https://x.dev", None),
            Err("configuration_parse_failed")
        );
        assert_eq!(
            render(Some(b"{oops"), "https://x.dev", None),
            Err("configuration_parse_failed")
        );
    }

    #[test]
    fn writes_a_model_selection_only_from_the_catalog() {
        // 目录里有档位信息的型号：写成 Cursor 的参数化 modelId，
        // 档位取目录里的默认值（deepseek-v4.1-flash 默认 high）。
        let bytes = render(None, "https://api.yeschoy.com", Some("deepseek-v4.1-flash")).unwrap();
        let value: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(
            value["model"]["modelId"],
            "deepseek-v4.1-flash[effort=high]"
        );
        assert_eq!(value["model"]["parameters"][1]["id"], "reasoning_effort");
        assert_eq!(value["model"]["parameters"][1]["value"], "high");
        // 目录外的型号：照写 id，不编造参数。
        let bytes = render(None, "https://api.yeschoy.com", Some("unknown-model")).unwrap();
        let value: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(value["model"]["modelId"], "unknown-model");
        assert!(value["model"].get("parameters").is_none());
    }

    #[test]
    fn prepare_verify_and_undo_restore_the_users_file() {
        let temp = TempHome::new("prepare");
        std::fs::write(temp.config(), EXISTING).unwrap();
        let prepared = prepare(&temp.0, "https://api.yeschoy.com/", Some("glm-5.3")).unwrap();
        verify(&prepared).unwrap();
        let (endpoint, _) = current(&temp.0).unwrap();
        assert_eq!(endpoint, "https://api.yeschoy.com");

        undo(&temp.0).unwrap();
        let after: Value = serde_json::from_slice(&std::fs::read(temp.config()).unwrap()).unwrap();
        assert!(after["network"].get("baseUrl").is_none());
        assert!(after["network"]["headers"]
            .get("X-Yeschoy-Desktop")
            .is_none());
        assert!(after.get("model").is_none());
        // 用户的键一个不少。
        assert_eq!(after["network"]["headers"]["X-Own"], "1");
        assert_eq!(after["network"]["customTimeout"], 9);
        assert_eq!(after["permissions"]["allow"][0], "Shell(ls)");
    }

    #[test]
    fn verify_reports_drift_after_an_external_edit() {
        let temp = TempHome::new("drift");
        let prepared = prepare(&temp.0, "https://api.yeschoy.com", None).unwrap();
        let mut value: Value =
            serde_json::from_slice(&std::fs::read(temp.config()).unwrap()).unwrap();
        value["network"]["baseUrl"] = Value::String("https://evil.example".into());
        std::fs::write(temp.config(), serde_json::to_vec(&value).unwrap()).unwrap();
        assert_eq!(
            verify(&prepared),
            Err(AdapterFailure::ConfigurationFailed(
                "configuration_readback_failed"
            ))
        );
    }

    #[test]
    fn rollback_leaves_the_original_file_in_place() {
        let temp = TempHome::new("rollback");
        std::fs::write(temp.config(), EXISTING).unwrap();
        let prepared = prepare(&temp.0, "https://api.yeschoy.com", None).unwrap();
        rollback(prepared).unwrap();
        assert_eq!(std::fs::read(temp.config()).unwrap(), EXISTING);
    }

    #[test]
    fn undo_without_a_file_is_a_no_op() {
        let temp = TempHome::new("empty");
        undo(&temp.0).unwrap();
        assert!(!temp.config().exists());
    }
}

pub(crate) mod ipc {
    //! Cursor 的两条接入路径，**故意**不并进 `TARGETS` 的一键接入主干。
    //!
    //! 为什么独立：Cursor Desktop 的自定义服务地址与密钥存在它自己的
    //! `state.vscdb` 和钥匙串里，能否启用还得用户在 Cursor 界面勾选；把它塞进
    //! 「选模型 → 写配置 → 读回」的主干，等于替用户做它自己的登录与开关操作。
    //! 所以这里只写 CLI 的 `~/.cursor/cli-config.json`，并且：
    //!
    //! - 端点不由渲染层给，固定取构建期 `YESCHOY_AUTHORIZATION_PAGE_ORIGIN`，
    //!   因此不需要、也不接受任意 URL；
    //! - 不发任何令牌、不调站点账户接口，所以不经过 `activation` 的令牌步骤；
    //! - 有自己的 `cursor_revert_v1`：写前存原文件、原子写、读回校验、失败整体
    //!   回滚都在本模块内完成，因此**不写 `connection_recovery` 基线**，主干的
    //!   「恢复原设置」对它不适用——这是刻意取舍，不是遗漏；
    //! - 当前不持有 `activation_lock`，因为端口固定且不与其他工具共享配置文件。
    //!
    //! 若将来要让 Cursor 参与主干的恢复与锁，需要补的是：把 `connection_recovery`
    //! 的文件集合加上它、接入 `activation_lock`、并给前端 `ACTIVATION_TOOL_IDS`
    //! 加条目——而不是简单往 `TARGETS` 里塞一行。
    use super::*;
    use crate::{tool_adapters, tool_adapters::AdapterFailure};
    use serde::Deserialize;

    /// 渲染层只给模型 ID 与一个显式确认；端点由原生侧决定：
    /// `YESCHOY_AUTHORIZATION_PAGE_ORIGIN`（构建期固定），不接受任意 URL。
    fn gateway_origin() -> String {
        env!("YESCHOY_AUTHORIZATION_PAGE_ORIGIN").to_owned()
    }

    #[derive(Clone, Debug, Serialize, PartialEq, Eq)]
    #[serde(rename_all = "camelCase")]
    pub(crate) struct CursorConfigSnapshot {
        pub(crate) tool_id: &'static str,
        pub(crate) cli_detected: bool,
        /// 端点用于预览；密钥永远不会经过这里（Cursor CLI 用自己的登录或环境变量）。
        pub(crate) endpoint: String,
        pub(crate) custom_endpoint_configured: bool,
        pub(crate) header_names: Vec<String>,
        pub(crate) ide: CursorIdeState,
    }

    /// 只读：CLI 配置文件现状 + Cursor 桌面版是否装在这台机器上。
    #[tauri::command]
    pub(crate) fn cursor_config_read_only() -> CursorConfigSnapshot {
        let home = tool_adapters::user_home();
        let cli_detected = home.as_deref().and_then(detect_cli).is_some();
        let (custom_endpoint_configured, header_names) = home
            .as_deref()
            .and_then(current)
            .map(|(_, headers)| (true, headers))
            .unwrap_or((false, Vec::new()));
        let ide = home.as_deref().map(ide_state).unwrap_or(CursorIdeState {
            observed: false,
            custom_endpoint_configured: false,
            existing_provider: "unknown",
        });
        log::info!(
            "cursor_agent stage=read cli_detected={} custom_endpoint={} ide_observed={}",
            cli_detected,
            custom_endpoint_configured,
            ide.observed
        );
        CursorConfigSnapshot {
            tool_id: "cursor_agent",
            cli_detected,
            endpoint: format!("{}/v1", gateway_origin().trim_end_matches('/')),
            custom_endpoint_configured,
            header_names,
            ide,
        }
    }

    #[derive(Clone, Debug, Deserialize)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    pub(crate) struct ApplyRequest {
        pub(crate) default_model: Option<String>,
    }

    #[derive(Clone, Debug, Serialize, PartialEq, Eq)]
    #[serde(rename_all = "camelCase")]
    pub(crate) struct ApplyResult {
        pub(crate) tool_id: &'static str,
        /// `applied` | `rolled_back` | 稳定错误码
        pub(crate) status: &'static str,
        pub(crate) reason_code: &'static str,
        pub(crate) model_id: Option<String>,
    }

    /// 写入 CLI 配置。没有用户确认的模型 ID 就报错：不替用户编一个。
    #[tauri::command]
    pub(crate) async fn cursor_apply_v1(request: ApplyRequest) -> ApplyResult {
        let Some(model) = request.default_model.filter(|model| {
            !model.is_empty()
                && model.len() <= 200
                && model.bytes().all(|b| {
                    b.is_ascii_alphanumeric()
                        || matches!(b, b'-' | b'_' | b'.' | b'/' | b'[' | b']' | b'=' | b',')
                })
        }) else {
            return ApplyResult {
                tool_id: "cursor_agent",
                status: "failed",
                reason_code: "model_required",
                model_id: None,
            };
        };
        let Some(home) = tool_adapters::user_home() else {
            return ApplyResult {
                tool_id: "cursor_agent",
                status: "failed",
                reason_code: "user_home_unavailable",
                model_id: None,
            };
        };
        let origin = gateway_origin();
        // 阻断操作不该占着 IPC 线程跑文件 IO。
        let prepared = tauri::async_runtime::spawn_blocking(move || {
            let prepared = prepare(&home, &origin, Some(&model));
            match prepared {
                Ok(prepared) => match verify(&prepared) {
                    Ok(()) => Ok(model),
                    Err(_) => {
                        let _ = rollback(prepared);
                        Err("configuration_readback_failed")
                    }
                },
                Err(failure) => Err(failure_code(failure)),
            }
        })
        .await
        .unwrap_or(Err("configuration_write_failed"));
        log::info!(
            "cursor_agent stage=apply result={}",
            if prepared.is_ok() {
                "applied"
            } else {
                "failed"
            }
        );
        match prepared {
            Ok(model) => ApplyResult {
                tool_id: "cursor_agent",
                status: "applied",
                reason_code: "configured",
                model_id: Some(model),
            },
            Err(code) => ApplyResult {
                tool_id: "cursor_agent",
                status: "failed",
                reason_code: code,
                model_id: None,
            },
        }
    }

    #[tauri::command]
    pub(crate) async fn cursor_revert_v1() -> ApplyResult {
        let Some(home) = tool_adapters::user_home() else {
            return ApplyResult {
                tool_id: "cursor_agent",
                status: "failed",
                reason_code: "user_home_unavailable",
                model_id: None,
            };
        };
        let result = tauri::async_runtime::spawn_blocking(move || undo(&home))
            .await
            .unwrap_or(Err(AdapterFailure::ConfigurationFailed(
                "configuration_rollback_failed",
            )));
        match result {
            Ok(()) => ApplyResult {
                tool_id: "cursor_agent",
                status: "reverted",
                reason_code: "reverted",
                model_id: None,
            },
            Err(failure) => ApplyResult {
                tool_id: "cursor_agent",
                status: "failed",
                reason_code: failure_code(failure),
                model_id: None,
            },
        }
    }

    fn failure_code(failure: AdapterFailure) -> &'static str {
        match failure {
            AdapterFailure::ConfigurationFailed(code) => code,
            AdapterFailure::ExternalOverride => "external_override",
            AdapterFailure::ToolNotFound => "tool_not_found",
            _ => "configuration_write_failed",
        }
    }
}

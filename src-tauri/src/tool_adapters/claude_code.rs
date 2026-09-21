use std::path::Path;

use serde_json::{json, Map, Value};

use crate::{
    claude_bridge::{self, ClaudeTransport},
    local_bridge::LocalBridgeRuntime,
    tool_adapters::{
        common::{self, ConfigFailure, FileTransaction},
        AdapterFailure,
    },
    tool_credentials::{self, ToolCredential},
};

const PROXY_ADDRESS: &str = "127.0.0.1:15728";
const PROXY_BASE: &str = "http://127.0.0.1:15728/claude-code";

#[derive(Clone)]
pub(crate) struct ClaudeCodeRuntimeState {
    runtime: LocalBridgeRuntime,
}

impl Default for ClaudeCodeRuntimeState {
    fn default() -> Self {
        Self {
            runtime: claude_bridge::claude_runtime(PROXY_ADDRESS, "/claude-code"),
        }
    }
}

pub(crate) struct Prepared {
    transaction: FileTransaction,
    path: std::path::PathBuf,
    origin: String,
    model: String,
    model_ids: Vec<String>,
    modern: bool,
    helper: String,
}

fn config_error(error: ConfigFailure) -> AdapterFailure {
    match error {
        ConfigFailure::ExternalChange => AdapterFailure::ExternalOverride,
        ConfigFailure::Parse => AdapterFailure::ConfigurationFailed("configuration_parse_failed"),
        ConfigFailure::Read => AdapterFailure::ConfigurationFailed("configuration_read_failed"),
        ConfigFailure::Write => AdapterFailure::ConfigurationFailed("configuration_write_failed"),
        ConfigFailure::Readback => {
            AdapterFailure::ConfigurationFailed("configuration_readback_failed")
        }
        ConfigFailure::Rollback => {
            AdapterFailure::ConfigurationFailed("configuration_rollback_failed")
        }
    }
}

/// Claude Code 的自动压缩窗口，从目录里那个模型的真实上下文窗口来。
///
/// 不写这一项的话，Claude Code 不知道它连的是哪个模型的窗口 —— `ANTHROPIC_MODEL`
/// 是 `glm-5.3`、`gemini-3.7-flash` 这种它内置表里没有的名字。结果就是对话涨过
/// 窗口也不会自动压缩，一直涨到中转那边报上下文超限，用户看到的是一条报错而不是
/// 一次压缩。
///
/// `autoCompactWindow` 是 Claude Code settings.json 的顶层项，schema 写明
/// 最小 100000、最大 1000000，所以：
///
///   · 超过 1000000 的（目录里多数 1M 模型其实是 1048576）夹到上限 —— 早压一点
///     是安全的，晚压不是。
///   · 小于 100000 的不写。schema 表达不了，硬夹到 100000 会让它压得太晚，
///     那比不写更糟。
///   · 目录里没有这个模型的，也不写 —— 不猜。
fn auto_compact_window(model: &str) -> Option<u64> {
    const MIN: u64 = 100_000;
    const MAX: u64 = 1_000_000;
    let (bare, _) = crate::tool_model_profile::split_one_m_context_marker(model);
    let window = crate::tool_model_profile::capability_profile(bare)?.context_window?;
    (window >= MIN).then(|| window.min(MAX))
}

fn render(existing: Option<&[u8]>, origin: &str, model: &str, helper: &str) -> Result<Vec<u8>, ()> {
    let mut root = match existing {
        Some(bytes) if !bytes.is_empty() => {
            serde_json::from_slice::<Value>(bytes).map_err(|_| ())?
        }
        _ => Value::Object(Map::new()),
    };
    let object = root.as_object_mut().ok_or(())?;
    object.insert("apiKeyHelper".into(), helper.into());
    match auto_compact_window(model) {
        Some(window) => {
            object.insert("autoCompactWindow".into(), window.into());
        }
        // 目录没说的就不留一个过期的数在那儿：上一个模型的窗口套在新模型上，
        // 比没有更糟。
        None => {
            object.remove("autoCompactWindow");
        }
    }
    let environment = object
        .entry("env")
        .or_insert_with(|| Value::Object(Map::new()))
        .as_object_mut()
        .ok_or(())?;
    // These two literal-secret fields were owned by the 0.3 adapter. Removing
    // them is required so Claude Code cannot bypass apiKeyHelper precedence.
    environment.remove("ANTHROPIC_AUTH_TOKEN");
    environment.remove("ANTHROPIC_API_KEY");
    environment.insert("ANTHROPIC_BASE_URL".into(), origin.into());
    for key in [
        "ANTHROPIC_MODEL",
        "ANTHROPIC_DEFAULT_HAIKU_MODEL",
        "ANTHROPIC_DEFAULT_SONNET_MODEL",
        "ANTHROPIC_DEFAULT_OPUS_MODEL",
    ] {
        environment.insert(key.into(), model.into());
    }
    let mut bytes = serde_json::to_vec_pretty(&root).map_err(|_| ())?;
    bytes.push(b'\n');
    Ok(bytes)
}

fn render_catalog(
    existing: Option<&[u8]>,
    origin: &str,
    model: &str,
    helper: &str,
    model_ids: &[String],
) -> Result<Vec<u8>, ()> {
    let mut value: Value =
        serde_json::from_slice(&render(existing, origin, model, helper)?).map_err(|_| ())?;
    let env = value["env"].as_object_mut().ok_or(())?;
    // Persist the actual default in `model`, which native /model may update.
    // Never label unrelated upstream models as Claude's built-in families.
    for key in [
        "ANTHROPIC_MODEL",
        "ANTHROPIC_DEFAULT_HAIKU_MODEL",
        "ANTHROPIC_DEFAULT_SONNET_MODEL",
        "ANTHROPIC_DEFAULT_OPUS_MODEL",
    ] {
        env.remove(key);
    }
    value["model"] = json!(model);
    value["availableModels"] = json!(model_ids);
    // `behavesAs` controls Claude Code's client-side prompt/capability handling;
    // the exact model ID is still sent through the bridge. Known models receive
    // the closest native effort family, while unregistered IDs use a conservative
    // compatibility fallback so they remain selectable.
    value["modelPicker"] = json!({"replaceBuiltInOptions":true,"options":model_ids.iter().map(|id| json!({
        "model":id,
        "label":crate::tool_model_profile::display_name(id),
        "description":id,
        "behavesAs":crate::tool_model_profile::claude_code_behaves_as(id)
    })).collect::<Vec<_>>()});
    // `autoCompactWindow` is one number, and `/model` can move off the model it
    // was written for. This map is per model, so switching keeps the right
    // window. Read out of the installed Claude Desktop bundle, which parses
    // `~/.claude/settings.json` with
    //
    //     autoCompactWindow: O().optional(),
    //     contextWindowByModel: Kn(D(), O()).optional(),
    //
    // and looks a model up as `t[name] ?? t[name.replace(/\[.*\]$/, "")]` --
    // the fallback strips a trailing `[1m]`, so one entry per bare id covers
    // both spellings. Claude Code's own schema does not list the key but
    // accepts unknown ones (`additionalProperties: {}`), so writing it is inert
    // there and useful to the surfaces that do read it.
    let windows: serde_json::Map<String, Value> = model_ids
        .iter()
        .filter_map(|id| {
            let window = crate::tool_model_profile::capability_profile(id)?.context_window?;
            Some((id.clone(), json!(window)))
        })
        .collect();
    let root = value.as_object_mut().ok_or(())?;
    if windows.is_empty() {
        // Same rule as `autoCompactWindow`: an entry left over from a previous
        // model set is worse than none.
        root.remove("contextWindowByModel");
    } else {
        root.insert("contextWindowByModel".into(), Value::Object(windows));
    }
    let mut bytes = serde_json::to_vec_pretty(&value).map_err(|_| ())?;
    bytes.push(b'\n');
    Ok(bytes)
}

pub(crate) fn prepare(
    home: &Path,
    origin: &str,
    model: &str,
    transport: ClaudeTransport,
    existing_local_token: Option<&str>,
) -> Result<Prepared, AdapterFailure> {
    prepare_inner(
        home,
        origin,
        model,
        transport,
        existing_local_token,
        &[model.to_owned()],
        false,
    )
}

pub(crate) fn prepare_catalog(
    home: &Path,
    origin: &str,
    model: &str,
    transport: ClaudeTransport,
    existing_local_token: Option<&str>,
    model_ids: &[String],
) -> Result<Prepared, AdapterFailure> {
    crate::tool_adapters::common::validate_catalog(model, model_ids)?;
    prepare_inner(
        home,
        origin,
        model,
        transport,
        existing_local_token,
        model_ids,
        true,
    )
}

fn prepare_inner(
    home: &Path,
    origin: &str,
    model: &str,
    _transport: ClaudeTransport,
    _existing_local_token: Option<&str>,
    model_ids: &[String],
    modern: bool,
) -> Result<Prepared, AdapterFailure> {
    let path = home.join(".claude").join("settings.json");
    let before = common::snapshot(&path)
        .map_err(|_| AdapterFailure::ConfigurationFailed("configuration_read_failed"))?;
    let helper = if modern {
        tool_credentials::shell_gateway_helper_command("claude_code")
    } else {
        tool_credentials::shell_helper_command("claude_code")
    }
    .map_err(|_| AdapterFailure::SecureStorageUnavailable)?;
    // The canonical credential owner generates/stores the local token. Claude
    // Code reads it through apiKeyHelper, so no token belongs in Prepared or
    // the settings file. Catalog connections use a small local pass-through to
    // normalize Claude Code's `[1m]` picker marker before it reaches the relay.
    // Legacy single-model preparation remains direct for recovery compatibility.
    let configured_origin = if modern { PROXY_BASE } else { origin };
    let after = if modern {
        render_catalog(
            before.as_deref(),
            configured_origin,
            model,
            &helper,
            model_ids,
        )
    } else {
        render(before.as_deref(), configured_origin, model, &helper)
    }
    .map_err(|_| AdapterFailure::ConfigurationFailed("configuration_parse_failed"))?;
    let transaction =
        FileTransaction::stage_with_snapshot(path.clone(), before, after).map_err(config_error)?;
    Ok(Prepared {
        transaction,
        path,
        origin: configured_origin.to_owned(),
        model: model.to_owned(),
        model_ids: model_ids.to_vec(),
        modern,
        helper,
    })
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
        let bytes = common::snapshot(&self.path)
            .map_err(|_| AdapterFailure::ConfigurationFailed("configuration_readback_failed"))?
            .ok_or(AdapterFailure::ConfigurationFailed(
                "configuration_readback_failed",
            ))?;
        let value: Value = serde_json::from_slice(&bytes)
            .map_err(|_| AdapterFailure::ConfigurationFailed("configuration_readback_failed"))?;
        let environment = value.get("env").and_then(Value::as_object);
        let correct = value.get("apiKeyHelper").and_then(Value::as_str) == Some(&self.helper)
            && environment
                .and_then(|env| env.get("ANTHROPIC_BASE_URL"))
                .and_then(Value::as_str)
                == Some(&self.origin)
            && environment.is_some_and(|env| {
                !env.contains_key("ANTHROPIC_AUTH_TOKEN")
                    && !env.contains_key("ANTHROPIC_API_KEY")
                    && (self.modern
                        || [
                            "ANTHROPIC_MODEL",
                            "ANTHROPIC_DEFAULT_HAIKU_MODEL",
                            "ANTHROPIC_DEFAULT_SONNET_MODEL",
                            "ANTHROPIC_DEFAULT_OPUS_MODEL",
                        ]
                        .into_iter()
                        .all(|key| env.get(key).and_then(Value::as_str) == Some(&self.model)))
            })
            && (!self.modern
                || (crate::tool_adapters::common::default_matches(
                    value["model"].as_str(),
                    &self.model,
                    &self.model_ids,
                    strict_default,
                ) && crate::tool_adapters::common::catalog_matches(
                    &value["availableModels"],
                    None,
                    &self.model_ids,
                ) && crate::tool_adapters::common::catalog_matches(
                    &value["modelPicker"]["options"],
                    Some("model"),
                    &self.model_ids,
                ) && value["modelPicker"]["options"]
                    .as_array()
                    .is_some_and(|models| {
                        models.iter().all(|model| {
                            model["model"].as_str().is_some_and(|id| {
                                // The label is cosmetic. Read-only inspection
                                // only asks that one is there: a catalog
                                // refresh that renames or drops a model must
                                // not turn a working connection into
                                // `settings_changed` when the user changed
                                // nothing. Write-then-read-back still checks
                                // the exact label we just wrote.
                                model["label"].as_str().is_some_and(|label| {
                                    !label.is_empty()
                                        && (!strict_default
                                            || label == crate::tool_model_profile::display_name(id))
                                }) && model["description"].as_str() == Some(id)
                                    && model["behavesAs"].as_str()
                                        == Some(crate::tool_model_profile::claude_code_behaves_as(
                                            id,
                                        ))
                            })
                        })
                    })
                    && value["modelPicker"]["replaceBuiltInOptions"].as_bool() == Some(true)
                    && environment.is_some_and(|env| {
                        [
                            "ANTHROPIC_MODEL",
                            "ANTHROPIC_DEFAULT_HAIKU_MODEL",
                            "ANTHROPIC_DEFAULT_SONNET_MODEL",
                            "ANTHROPIC_DEFAULT_OPUS_MODEL",
                        ]
                        .iter()
                        .all(|key| !env.contains_key(*key))
                    })));
        if correct {
            Ok(())
        } else {
            Err(AdapterFailure::ConfigurationFailed(
                "configuration_readback_failed",
            ))
        }
    }

    pub(crate) fn rollback(&mut self) -> Result<(), AdapterFailure> {
        self.transaction.rollback().map_err(config_error)
    }
}

impl ClaudeCodeRuntimeState {
    pub(crate) async fn start(&self, credential: ToolCredential) -> Result<(), AdapterFailure> {
        self.runtime.start(credential).await.map(|_| ())
    }

    pub(crate) async fn stop(&self) {
        self.runtime.stop().await;
    }
}

pub(crate) async fn resume_if_configured(state: ClaudeCodeRuntimeState) {
    let Ok(credential) = tool_credentials::load("claude_code") else {
        return;
    };
    // Only current catalog credentials carry the local capability token and
    // are written to the pass-through endpoint.
    if !credential.has_model_set() {
        return;
    }
    let Some(home) = super::user_home() else {
        return;
    };
    let path = home.join(".claude").join("settings.json");
    let active = common::snapshot(&path)
        .ok()
        .flatten()
        .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
        .and_then(|value| {
            value["env"]["ANTHROPIC_BASE_URL"]
                .as_str()
                .map(|origin| origin == PROXY_BASE)
        })
        .unwrap_or(false);
    if active {
        let _ = state.start(credential).await;
    }
}

#[cfg(test)]
mod tests {
    /// 线上模型把推理档位钉进 id（`gemini-3.7-flash-high`），目录里存的是基础
    /// 型号（`gemini-3.7-flash`）。渲染层早就会拆这个后缀，Rust 侧一直没有，
    /// 所以这类模型拿不到 `autoCompactWindow` —— Claude Code 永不压缩，对话
    /// 一路涨到中转报上下文超限，用户看到的是报错而不是一次压缩。
    #[test]
    fn an_effort_pinned_id_still_gets_its_context_window() {
        // 基础型号本来就有窗口。
        assert_eq!(auto_compact_window("gemini-3.7-flash"), Some(1_000_000));
        // 钉死档位之后必须还是同一个窗口。
        for id in [
            "gemini-3.7-flash-high",
            "gemini-3.8-flash-high",
            "gemini-3.1-pro-low",
        ] {
            assert_eq!(
                auto_compact_window(id),
                Some(1_000_000),
                "{id} 应当拿到和基础型号一样的窗口"
            );
        }
    }

    /// 后缀顺序有讲究，且只有声明过档位的那几族才拆 —— 否则任何以 `-low`
    /// 结尾的模型名都会被砍掉一截，配到别的型号上去。
    #[test]
    fn only_effort_suffixed_families_are_split() {
        // `-xhigh` 不能被 `-high` 先吃掉。
        assert_eq!(auto_compact_window("gpt-6-astra-xhigh"), Some(1_000_000));
        // 不在那几族里的名字原样查，查不到就是查不到，不做截断。
        assert_eq!(auto_compact_window("qwen3.8-max"), Some(1_000_000));
        assert_eq!(auto_compact_window("mystery-model-low"), None);
    }

    use super::*;

    #[test]
    fn ru042_claude_code_catalog_uses_real_picker_ids_and_member_defaults() {
        let home = common::temporary_working_directory("claude-code-model-set").unwrap();
        let ids = vec!["model-a".into(), "org/model-b".into()];
        let token = format!("ycg-{}", "a".repeat(64));
        let mut prepared = prepare_catalog(
            &home,
            "https://yeschoy.com",
            "model-a",
            ClaudeTransport::DirectAnthropic,
            Some(&token),
            &ids,
        )
        .unwrap();
        prepared.commit().unwrap();
        assert!(!std::fs::read_to_string(&prepared.path)
            .unwrap()
            .contains(&token));
        let mut settings: Value =
            serde_json::from_slice(&std::fs::read(&prepared.path).unwrap()).unwrap();
        assert_eq!(settings["env"]["ANTHROPIC_BASE_URL"], PROXY_BASE);
        assert!(settings["apiKeyHelper"]
            .as_str()
            .is_some_and(|helper| helper.contains("gateway-credential-helper claude_code")));
        assert!(settings["env"]
            .get("ANTHROPIC_DEFAULT_SONNET_MODEL")
            .is_none());
        assert_eq!(
            settings["modelPicker"]["options"][1],
            json!({
                "model":"org/model-b",
                "label":"org/model-b",
                "description":"org/model-b",
                "behavesAs":crate::tool_model_profile::CLAUDE_BEHAVES_AS
            })
        );
        settings["model"] = "org/model-b".into();
        std::fs::write(&prepared.path, serde_json::to_vec(&settings).unwrap()).unwrap();
        assert!(prepared.validate_existing().is_ok());
        assert!(prepared.validate_readback(true).is_err());
        settings["modelPicker"]["options"][1]
            .as_object_mut()
            .unwrap()
            .remove("behavesAs");
        std::fs::write(&prepared.path, serde_json::to_vec(&settings).unwrap()).unwrap();
        assert!(prepared.validate_existing().is_err());
        settings["modelPicker"]["options"][1]["behavesAs"] =
            crate::tool_model_profile::CLAUDE_BEHAVES_AS.into();
        settings["model"] = "not-enrolled".into();
        std::fs::write(&prepared.path, serde_json::to_vec(&settings).unwrap()).unwrap();
        assert!(prepared.validate_existing().is_err());
        std::fs::remove_dir_all(home).unwrap();
    }

    /// 标签只是给人看的。目录刷新把某个模型改了名（或干脆删了），用户什么都
    /// 没动，「打开」却回 `settings_changed` —— 只读校验不能拿今天的显示名去
    /// 要求昨天写下的文件。写完回读那一路仍然严格：刚写的就该是刚写的。
    #[test]
    fn a_renamed_catalog_entry_does_not_make_an_untouched_connection_look_changed() {
        let home = common::temporary_working_directory("claude-code-label-drift").unwrap();
        let ids = vec!["model-a".into(), "org/model-b".into()];
        let token = format!("ycg-{}", "a".repeat(64));
        let mut prepared = prepare_catalog(
            &home,
            "https://yeschoy.com",
            "model-a",
            ClaudeTransport::DirectAnthropic,
            Some(&token),
            &ids,
        )
        .unwrap();
        prepared.commit().unwrap();
        let mut settings: Value =
            serde_json::from_slice(&std::fs::read(&prepared.path).unwrap()).unwrap();
        settings["modelPicker"]["options"][1]["label"] = "Model B (renamed upstream)".into();
        std::fs::write(&prepared.path, serde_json::to_vec(&settings).unwrap()).unwrap();
        assert!(
            prepared.validate_existing().is_ok(),
            "目录改名不是用户改了设置"
        );
        assert!(
            prepared.validate_readback(true).is_err(),
            "刚写完回读仍要精确"
        );
        // 但标签必须在：空标签会让选择器里出现一个没名字的项。
        settings["modelPicker"]["options"][1]["label"] = "".into();
        std::fs::write(&prepared.path, serde_json::to_vec(&settings).unwrap()).unwrap();
        assert!(prepared.validate_existing().is_err());
        std::fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn ru054_claude_code_unknown_models_declare_behavior_without_changing_ids() {
        ru042_claude_code_catalog_uses_real_picker_ids_and_member_defaults();
    }

    #[test]
    fn ru056_claude_code_catalog_projects_each_models_native_effort_family() {
        let bytes = render_catalog(
            None,
            "https://yeschoy.com",
            "deepseek-v4-flash",
            "synthetic-helper",
            &[
                "deepseek-v4-flash".into(),
                "gpt-6-astra".into(),
                "gpt-5.6-sol".into(),
                "future-model".into(),
            ],
        )
        .unwrap();
        let settings: Value = serde_json::from_slice(&bytes).unwrap();
        let options = settings["modelPicker"]["options"].as_array().unwrap();
        let behaviors = options
            .iter()
            .map(|option| {
                (
                    option["model"].as_str().unwrap(),
                    option["behavesAs"].as_str().unwrap(),
                )
            })
            .collect::<std::collections::HashMap<_, _>>();
        assert_eq!(behaviors["deepseek-v4-flash"], "claude-sonnet-4-6");
        assert_eq!(behaviors["gpt-6-astra"], "claude-opus-5");
        assert_eq!(behaviors["gpt-5.6-sol"], "claude-sonnet-5");
        assert_eq!(
            behaviors["future-model"],
            crate::tool_model_profile::CLAUDE_BEHAVES_AS
        );
    }

    #[test]
    fn claude_existing_validation_is_read_only_and_checks_all_owned_model_fields() {
        let directory = common::temporary_working_directory("claude-readonly").unwrap();
        let path = directory.join("settings.json");
        let origin = "https://yeschoy.com";
        let model = "fixture-model";
        let helper = "synthetic-helper credential-helper claude_code";
        let bytes = render(
            Some(br#"{"permissions":{"allow":["Read"]}}"#),
            origin,
            model,
            helper,
        )
        .unwrap();
        std::fs::write(&path, &bytes).unwrap();
        let prepared = Prepared {
            transaction: FileTransaction::stage_with_snapshot(
                path.clone(),
                Some(bytes.clone()),
                bytes.clone(),
            )
            .unwrap(),
            path: path.clone(),
            origin: origin.into(),
            model: model.into(),
            model_ids: vec![model.into()],
            modern: false,
            helper: helper.into(),
        };
        assert!(prepared.validate_existing().is_ok());
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
        for key in [
            "ANTHROPIC_MODEL",
            "ANTHROPIC_DEFAULT_HAIKU_MODEL",
            "ANTHROPIC_DEFAULT_SONNET_MODEL",
            "ANTHROPIC_DEFAULT_OPUS_MODEL",
            "ANTHROPIC_API_KEY",
        ] {
            let mut value: Value = serde_json::from_slice(&bytes).unwrap();
            value["env"][key] = "external-value".into();
            let changed = serde_json::to_vec(&value).unwrap();
            std::fs::write(&path, &changed).unwrap();
            assert!(prepared.validate_existing().is_err(), "{key}");
            assert_eq!(std::fs::read(&path).unwrap(), changed);
        }
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn render_uses_helper_and_preserves_unrelated_settings() {
        let before = br#"{"permissions":{"allow":["Read"]},"env":{"KEEP":"yes","ANTHROPIC_AUTH_TOKEN":"sk-old"}}"#;
        let bytes = render(
            Some(before),
            "https://yeschoy.com",
            "glm-5.3",
            "'/Applications/野菜 API.app/client' credential-helper claude_code",
        )
        .unwrap();
        let value: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(value["permissions"]["allow"][0], "Read");
        assert_eq!(value["env"]["KEEP"], "yes");
        assert_eq!(value["env"]["ANTHROPIC_MODEL"], "glm-5.3");
        assert!(value["env"].get("ANTHROPIC_AUTH_TOKEN").is_none());
        assert!(value.to_string().find("sk-old").is_none());
    }

    #[test]
    fn direct_transport_writes_the_relay_origin_and_a_helper_command() {
        let settings: Value = serde_json::from_slice(
            &render(
                None,
                "https://yeschoy.com",
                "fixture-model",
                "synthetic-helper credential-helper claude_code",
            )
            .unwrap(),
        )
        .unwrap();
        assert_eq!(settings["env"]["ANTHROPIC_BASE_URL"], "https://yeschoy.com");
        assert_eq!(
            settings["apiKeyHelper"],
            "synthetic-helper credential-helper claude_code"
        );
        assert!(settings["env"].get("ANTHROPIC_API_KEY").is_none());
    }

    #[test]
    fn the_auto_compact_window_follows_the_model_and_stays_inside_the_schema() {
        let window = |model: &str| {
            let bytes = render(None, "https://yeschoy.com", model, "helper").unwrap();
            let value: Value = serde_json::from_slice(&bytes).unwrap();
            value.get("autoCompactWindow").and_then(Value::as_u64)
        };
        // 目录里 1048576 的模型夹到 schema 上限 1000000：早压一点是安全的。
        assert_eq!(window("glm-5.3"), Some(1_000_000));
        assert_eq!(window("kimi-k3"), Some(1_000_000));
        assert_eq!(window("gemini-3.7-flash"), Some(1_000_000));
        // 正好 1000000 的原样写。
        assert_eq!(window("deepseek-v4.1-flash"), Some(1_000_000));
        // 目录里没有的模型不猜，也不写。
        assert_eq!(window("something-not-in-the-catalog"), None);
        // 上一个模型留下的值不能套在新模型上。
        let stale = br#"{"autoCompactWindow":1000000}"#;
        let bytes = render(
            Some(stale),
            "https://yeschoy.com",
            "something-not-in-the-catalog",
            "helper",
        )
        .unwrap();
        let value: Value = serde_json::from_slice(&bytes).unwrap();
        assert!(value.get("autoCompactWindow").is_none());
        // 用户原有的其他设置不受影响。
        let bytes = render(
            Some(br#"{"permissions":{"allow":["Read"]},"autoCompactEnabled":false}"#),
            "https://yeschoy.com",
            "glm-5.3",
            "helper",
        )
        .unwrap();
        let value: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(value["permissions"]["allow"][0], "Read");
        // 开关是用户的选择，我们只给窗口大小，不替他打开或关掉。
        assert_eq!(value["autoCompactEnabled"], Value::Bool(false));
    }

    #[test]
    fn the_catalog_carries_a_window_for_every_model_the_user_can_switch_to() {
        let ids: Vec<String> = ["glm-5.3", "deepseek-v4.1-flash", "not-in-catalog"]
            .iter()
            .map(|s| (*s).to_string())
            .collect();
        let bytes = render_catalog(None, "https://yeschoy.com", "glm-5.3", "helper", &ids).unwrap();
        let value: Value = serde_json::from_slice(&bytes).unwrap();
        let map = value["contextWindowByModel"].as_object().unwrap();
        // 每个能被 /model 切到的模型都要有自己的窗口 —— 单个
        // autoCompactWindow 在切走之后就不对了。
        assert_eq!(map["glm-5.3"], 1_048_576);
        assert_eq!(map["deepseek-v4.1-flash"], 1_000_000);
        // 目录里没有的不猜。
        assert!(!map.contains_key("not-in-catalog"));
        // 这里不夹到 100000..1000000：那是 autoCompactWindow 的 schema 约束，
        // 这张表写的是模型真实的窗口。
        assert_eq!(value["autoCompactWindow"], 1_000_000);

        // 一个模型都查不到时，不要留下上一轮的表。
        let stale = br#"{"contextWindowByModel":{"old-model":123}}"#;
        let bytes = render_catalog(
            Some(stale),
            "https://yeschoy.com",
            "not-in-catalog",
            "helper",
            &["not-in-catalog".to_string()],
        )
        .unwrap();
        let value: Value = serde_json::from_slice(&bytes).unwrap();
        assert!(value.get("contextWindowByModel").is_none());
    }
}

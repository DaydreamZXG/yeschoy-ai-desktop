//! OpenCode：写 `~/.config/opencode/opencode.json`（已有 `opencode.jsonc` 时写它）。
//!
//! 只动我们自己的那一块：`provider.yeschoy` 和顶层 `model`。用户自己的其他
//! provider、主题、快捷键等一律原样保留。恢复时整份文件按接入前的字节还原
//! （由 `FileTransaction` / 连接恢复记录负责）。
//!
//! 密钥：`apiKey` 写的是**本地令牌**，请求先到 `opencode_bridge`，
//! 由桥从系统钥匙串取出真正的中转站 key 再转发。中转站 key 不落盘。
use std::path::{Path, PathBuf};

use serde_json::{Map, Value};

use crate::{
    opencode_bridge::PROXY_BASE,
    tool_adapters::{
        common::{self, ConfigFailure, FileTransaction},
        jsonc, AdapterFailure,
    },
};

pub(crate) const PROVIDER_ID: &str = "yeschoy";

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

/// OpenCode 读取的全局配置目录。
pub(crate) fn config_dir(home: &Path) -> PathBuf {
    home.join(".config").join("opencode")
}

/// 已有 `opencode.jsonc` 就写它（OpenCode 两个都认），否则写 `opencode.json`。
pub(crate) fn config_path(home: &Path) -> PathBuf {
    let directory = config_dir(home);
    let jsonc = directory.join("opencode.jsonc");
    let json = directory.join("opencode.json");
    if jsonc.is_file() && !json.exists() {
        jsonc
    } else {
        json
    }
}

fn parse_document(bytes: Option<&[u8]>) -> Result<Value, ()> {
    match bytes {
        Some(bytes) if !bytes.is_empty() => {
            let source = std::str::from_utf8(bytes).map_err(|_| ())?;
            let value: Value = json5::from_str(source).map_err(|_| ())?;
            value.is_object().then_some(value).ok_or(())
        }
        _ => Ok(Value::Object(Map::new())),
    }
}

pub(crate) fn render(
    existing: Option<&[u8]>,
    model: &str,
    model_ids: &[String],
    local_token: &str,
) -> Result<Vec<u8>, ()> {
    let mut root = parse_document(existing)?;
    let original = root.clone();
    let object = root.as_object_mut().ok_or(())?;
    object
        .entry("$schema")
        .or_insert_with(|| "https://opencode.ai/config.json".into());
    let providers = object
        .entry("provider")
        .or_insert_with(|| Value::Object(Map::new()))
        .as_object_mut()
        .ok_or(())?;
    let mut models = Map::new();
    for id in model_ids {
        models.insert(id.clone(), serde_json::json!({ "name": id }));
    }
    let provider_value = serde_json::json!({
        "npm": "@ai-sdk/openai-compatible",
        "name": "野菜API",
        "options": { "baseURL": PROXY_BASE, "apiKey": local_token },
        "models": models,
    });
    providers.insert(PROVIDER_ID.into(), provider_value.clone());
    object.insert("model".into(), format!("{PROVIDER_ID}/{model}").into());
    // 先尝试在原文上做最小编辑：`opencode.jsonc` 是用户可能写满注释的文件，
    // 整份重排会把它们全抹掉。做不到（结构超出预期）再退回重排，两种结果
    // 语义一致，由 `activation_keeps_jsonc_comments` 守着。
    if let Some(source) = existing.and_then(|bytes| std::str::from_utf8(bytes).ok()) {
        // 只改我们自己的两块：`provider.yeschoy` 与顶层 `model`。写成
        // `[provider]` 会整棵覆盖，把用户的 ollama 等 provider 一起抹掉。
        // `$schema` 只在原文没有时才补，`apply_values` 对未变化的路径不动原文。
        let values = [
            (
                vec![jsonc::Segment::Key("$schema")],
                Value::from("https://opencode.ai/config.json"),
            ),
            (
                vec![
                    jsonc::Segment::Key("provider"),
                    jsonc::Segment::Key(PROVIDER_ID),
                ],
                provider_value.clone(),
            ),
            (
                vec![jsonc::Segment::Key("model")],
                Value::from(format!("{PROVIDER_ID}/{model}")),
            ),
        ];
        if let Some(edited) = jsonc::apply_values(source, &original, &values) {
            return Ok(edited.into_bytes());
        }
    }
    let mut bytes = serde_json::to_vec_pretty(&root).map_err(|_| ())?;
    bytes.push(b'\n');
    Ok(bytes)
}

pub(crate) fn prepare_catalog(
    home: &Path,
    model: &str,
    model_ids: &[String],
    local_token: Option<&str>,
) -> Result<Prepared, AdapterFailure> {
    common::validate_catalog(model, model_ids)?;
    // 用户用 OPENCODE_CONFIG 指到了别处：我们写的那份它不会读。
    if crate::shell_environment::is_set_anywhere("OPENCODE_CONFIG") {
        return Err(AdapterFailure::ExternalOverride);
    }
    let local_token = local_token
        .filter(|value| value.starts_with("ycg-") && value.len() == 68)
        .ok_or(AdapterFailure::SecureStorageUnavailable)?
        .to_owned();
    let path = config_path(home);
    let before = common::snapshot(&path)
        .map_err(|_| AdapterFailure::ConfigurationFailed("configuration_read_failed"))?;
    let after = render(before.as_deref(), model, model_ids, &local_token)
        .map_err(|_| AdapterFailure::ConfigurationFailed("configuration_parse_failed"))?;
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

/// 重启桌面助手后，OpenCode 若仍指着我们这座桥，就把桥重新起起来。
/// 只看 OpenCode 自己的配置：用户已经换走了，就不占端口。
pub(crate) async fn resume_if_configured() {
    let Ok(credential) = crate::tool_credentials::load("opencode") else {
        return;
    };
    let Some(home) = super::user_home() else {
        return;
    };
    let Ok(bytes) = std::fs::read(config_path(&home)) else {
        return;
    };
    if points_at_our_bridge(&bytes) {
        if let Err(error) = crate::opencode_bridge::global().start(credential).await {
            log::warn!("opencode_bridge stage=resume_start_failed reason={error:?}");
        }
    }
}

/// 当前选中的模型是不是我们的 provider，且它还指着本地桥。
pub(crate) fn points_at_our_bridge(bytes: &[u8]) -> bool {
    parse_document(Some(bytes)).is_ok_and(|value| {
        value["model"]
            .as_str()
            .is_some_and(|model| model.starts_with(&format!("{PROVIDER_ID}/")))
            && value["provider"][PROVIDER_ID]["options"]["baseURL"].as_str() == Some(PROXY_BASE)
    })
}

impl Prepared {
    pub(crate) fn changes(&self) -> &[common::FileChange] {
        self.transaction.changes()
    }

    pub(crate) fn commit(&mut self) -> Result<(), AdapterFailure> {
        crate::application::ports::configuration::commit_with_plan(self)
    }

    pub(crate) fn validate_existing(&self) -> Result<(), AdapterFailure> {
        self.validate_readback(false)
    }

    fn validate_readback(&self, strict_default: bool) -> Result<(), AdapterFailure> {
        let failed = AdapterFailure::ConfigurationFailed("configuration_readback_failed");
        let bytes = common::snapshot(&self.path)
            .map_err(|_| failed)?
            .ok_or(failed)?;
        let value = parse_document(Some(&bytes)).map_err(|_| failed)?;
        let provider = &value["provider"][PROVIDER_ID];
        let listed: Vec<String> = provider["models"]
            .as_object()
            .map(|models| models.keys().cloned().collect())
            .unwrap_or_default();
        let mut expected = self.model_ids.clone();
        expected.sort();
        let mut actual = listed.clone();
        actual.sort();
        let default = value["model"]
            .as_str()
            .and_then(|model| model.strip_prefix(&format!("{PROVIDER_ID}/")));
        let correct = provider["options"]["baseURL"].as_str() == Some(PROXY_BASE)
            && provider["options"]["apiKey"].as_str() == Some(self.local_token.as_str())
            && provider["npm"].as_str() == Some("@ai-sdk/openai-compatible")
            && actual == expected
            && common::default_matches(default, &self.model, &self.model_ids, strict_default);
        if correct {
            Ok(())
        } else {
            Err(failed)
        }
    }

    pub(crate) fn rollback(&mut self) -> Result<(), AdapterFailure> {
        crate::application::ports::configuration::rollback_with_plan(self)
    }
}

impl crate::application::ports::configuration::ConfigurationPlan for Prepared {
    fn transaction(&mut self) -> &mut dyn crate::application::ports::configuration::ConfigCommit {
        &mut self.transaction
    }

    fn verify_written(&self) -> Result<(), AdapterFailure> {
        self.validate_readback(true)
    }
}

/// Read-back capability handed to the shared commit helper. It borrows the plan
/// immutably while the transaction is borrowed mutably by the caller, so the
/// commit ordering is shared without duplicating a read-back wrapper.
struct PlanReadback<'a>(&'a Prepared);

impl crate::application::ports::configuration::WrittenPlan for PlanReadback<'_> {
    fn verify(&self) -> Result<(), AdapterFailure> {
        self.0.validate_readback(true)
    }
}
impl crate::application::ports::configuration::SharedCommit for Prepared {
    fn commit_with_plan(&mut self) -> Result<(), AdapterFailure> {
        self.transaction
            .commit()
            .map_err(crate::domain::error::AdapterFailure::from)?;
        self.validate_readback(true)
    }
}

impl crate::application::ports::configuration::SharedRollback for Prepared {
    fn rollback_with_plan(&mut self) -> Result<(), AdapterFailure> {
        self.transaction
            .rollback()
            .map_err(crate::domain::error::AdapterFailure::from)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn token() -> String {
        format!("ycg-{}", "a".repeat(64))
    }

    #[test]
    fn merge_keeps_user_settings_and_writes_no_relay_key() {
        let before = br#"{
          // user settings
          theme: 'tokyonight',
          provider: { ollama: { npm: '@ai-sdk/openai-compatible', options: { baseURL: 'http://127.0.0.1:11434/v1' } } },
        }"#;
        let bytes = render(
            Some(before),
            "glm-5.3",
            &["glm-5.3".into(), "kimi-k3".into()],
            &token(),
        )
        .unwrap();
        let value: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(value["theme"], "tokyonight");
        assert_eq!(
            value["provider"]["ollama"]["options"]["baseURL"],
            "http://127.0.0.1:11434/v1"
        );
        assert_eq!(
            value["provider"]["yeschoy"]["options"]["baseURL"],
            PROXY_BASE
        );
        assert_eq!(value["model"], "yeschoy/glm-5.3");
        assert!(value["provider"]["yeschoy"]["models"]["kimi-k3"].is_object());
        let text = String::from_utf8(bytes).unwrap();
        assert!(!text.contains("sk-"), "relay key must never be written");
    }

    #[test]
    fn apply_then_rollback_restores_original_bytes() {
        let home = common::temporary_working_directory("opencode-apply").unwrap();
        let path = config_dir(&home).join("opencode.json");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let original = b"{\"theme\":\"keep\"}";
        std::fs::write(&path, original).unwrap();
        let ids = vec!["model-a".to_string(), "供应商/model-b".to_string()];
        let mut prepared = prepare_catalog(&home, "model-a", &ids, Some(&token())).unwrap();
        let r = prepared.commit();
        r.unwrap();
        assert!(prepared.validate_existing().is_ok());
        prepared.rollback().unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), original);
        std::fs::remove_dir_all(home).unwrap();
    }

    /// `opencode.jsonc` 里用户写的注释必须原样保留：我们只拥有
    /// `provider.yeschoy` 与顶层 `model`。
    #[test]
    fn activation_keeps_jsonc_comments() {
        let home = common::temporary_working_directory("opencode-comments").unwrap();
        std::fs::create_dir_all(config_dir(&home)).unwrap();
        let path = config_dir(&home).join("opencode.jsonc");
        let before = "{\n  // \u{81ea}\u{5df1}\u{7684}\u{4e3b}\u{9898}\n  theme: 'tokyonight',\n  provider: {\n    // \u{522b}\u{52a8} ollama\n    ollama: { npm: '@ai-sdk/openai-compatible' },\n  },\n}\n";
        std::fs::write(&path, before).unwrap();
        let mut prepared =
            prepare_catalog(&home, "model-a", &["model-a".into()], Some(&token())).unwrap();
        prepared.commit().unwrap();
        let after = std::fs::read_to_string(&path).unwrap();
        assert!(
            after.contains("\u{81ea}\u{5df1}\u{7684}\u{4e3b}\u{9898}"),
            "{after}"
        );
        assert!(after.contains("\u{522b}\u{52a8} ollama"), "{after}");
        assert!(after.contains(RPROVIDER), "{after}");
        let value: Value = json5::from_str(&after).unwrap();
        assert_eq!(value["theme"], "tokyonight");
        assert_eq!(value["model"], "yeschoy/model-a");
        std::fs::remove_dir_all(home).unwrap();
    }

    const RPROVIDER: &str = "yeschoy";

    #[test]
    fn resume_only_when_still_selected() {
        let ours = render(None, "m", &["m".into()], &token()).unwrap();
        assert!(points_at_our_bridge(&ours));
        let mut switched: Value = serde_json::from_slice(&ours).unwrap();
        switched["model"] = "anthropic/claude".into();
        assert!(!points_at_our_bridge(
            &serde_json::to_vec(&switched).unwrap()
        ));
    }

    #[test]
    fn missing_local_token_is_refused() {
        let home = common::temporary_working_directory("opencode-token").unwrap();
        assert!(matches!(
            prepare_catalog(&home, "m", &["m".into()], None),
            Err(AdapterFailure::SecureStorageUnavailable)
        ));
        std::fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn prefers_existing_jsonc() {
        let home = common::temporary_working_directory("opencode-jsonc").unwrap();
        std::fs::create_dir_all(config_dir(&home)).unwrap();
        std::fs::write(config_dir(&home).join("opencode.jsonc"), b"{}").unwrap();
        assert!(config_path(&home).ends_with("opencode.jsonc"));
        std::fs::remove_dir_all(home).unwrap();
    }
}

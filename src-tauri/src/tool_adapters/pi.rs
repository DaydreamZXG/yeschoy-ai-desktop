use std::path::{Path, PathBuf};

#[cfg(test)]
use serde_json::json;
use serde_json::{Map, Value};

use crate::{
    tool_adapters::{
        common::{self, ConfigFailure, FileTransaction},
        jsonc, AdapterFailure,
    },
    tool_credentials,
};

pub(crate) struct Prepared {
    transaction: FileTransaction,
    models_path: PathBuf,
    settings_path: PathBuf,
    origin: String,
    model: String,
    model_ids: Vec<String>,
    helper: String,
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

fn pretty(value: &Value) -> Result<Vec<u8>, ()> {
    let mut bytes = serde_json::to_vec_pretty(value).map_err(|_| ())?;
    bytes.push(b'\n');
    Ok(bytes)
}

#[cfg(test)]
fn render_models(
    existing: Option<&[u8]>,
    origin: &str,
    model: &str,
    helper: &str,
) -> Result<Vec<u8>, ()> {
    render_model_catalog(existing, origin, &[model.to_owned()], helper)
}

/// 先尝试在原文上做最小编辑（保留用户注释与排版），做不到再整份重排。
///
/// 两种写法必须产出同一份语义；这是 `render_model_catalog_keeps_user_comments`
/// 这类格式测试在守的东西。任何解析/定位失败都静默回退 —— 写坏配置的代价
/// 远大于丢一行注释。
fn minimal_or_pretty(
    existing: Option<&[u8]>,
    original: &Value,
    values: &[(Vec<jsonc::Segment<'static>>, Value)],
    fallback: &Value,
) -> Result<Vec<u8>, ()> {
    if let Some(source) = existing.and_then(|bytes| std::str::from_utf8(bytes).ok()) {
        if let Some(edited) = jsonc::apply_values(source, original, values) {
            return Ok(edited.into_bytes());
        }
    }
    pretty(fallback)
}

const fn jsonc_key(key: &'static str) -> jsonc::Segment<'static> {
    jsonc::Segment::Key(key)
}

fn render_model_catalog(
    existing: Option<&[u8]>,
    origin: &str,
    model_ids: &[String],
    helper: &str,
) -> Result<Vec<u8>, ()> {
    let mut root = parse_document(existing)?;
    let original = root.clone();
    let root = root.as_object_mut().ok_or(())?;
    let providers = root
        .entry("providers")
        .or_insert_with(|| Value::Object(Map::new()))
        .as_object_mut()
        .ok_or(())?;
    let provider = providers
        .entry("yeschoy")
        .or_insert_with(|| Value::Object(Map::new()))
        .as_object_mut()
        .ok_or(())?;
    provider.insert(
        "baseUrl".into(),
        format!("{}/v1", origin.trim_end_matches('/')).into(),
    );
    provider.insert("api".into(), "openai-completions".into());
    provider.insert("apiKey".into(), format!("!{helper}").into());
    provider.insert("authHeader".into(), true.into());
    let models = crate::tool_model_profile::native_chat_catalog(
        provider.get("models"),
        model_ids,
        crate::tool_model_profile::ModelConsumer::Pi,
    );
    provider.insert("models".into(), models);
    minimal_or_pretty(
        existing,
        &original,
        &[(
            vec![jsonc_key("providers"), jsonc_key("yeschoy")],
            Value::Object(provider.clone()),
        )],
        &Value::Object(root.clone()),
    )
}

fn render_settings(existing: Option<&[u8]>, model: &str) -> Result<Vec<u8>, ()> {
    let mut root = parse_document(existing)?;
    let original = root.clone();
    let root = root.as_object_mut().ok_or(())?;
    root.insert("defaultProvider".into(), "yeschoy".into());
    root.insert("defaultModel".into(), model.into());
    minimal_or_pretty(
        existing,
        &original,
        &[
            (vec![jsonc_key("defaultProvider")], Value::from("yeschoy")),
            (vec![jsonc_key("defaultModel")], Value::from(model)),
        ],
        &Value::Object(root.clone()),
    )
}

pub(crate) fn prepare(home: &Path, origin: &str, model: &str) -> Result<Prepared, AdapterFailure> {
    prepare_inner(home, origin, model, &[model.to_owned()])
}

pub(crate) fn prepare_catalog(
    home: &Path,
    origin: &str,
    model: &str,
    model_ids: &[String],
) -> Result<Prepared, AdapterFailure> {
    crate::tool_adapters::common::validate_catalog(model, model_ids)?;
    prepare_inner(home, origin, model, model_ids)
}

fn prepare_inner(
    home: &Path,
    origin: &str,
    model: &str,
    model_ids: &[String],
) -> Result<Prepared, AdapterFailure> {
    // Read through the login shell: an export in `.zshrc` is invisible to a
    // Dock-launched app, so this guard used to pass and the adapter wrote
    // ~/.pi/agent while Pi itself read the overridden directory.
    if crate::shell_environment::is_set_anywhere("PI_CODING_AGENT_DIR") {
        return Err(AdapterFailure::ExternalOverride);
    }
    let directory = home.join(".pi").join("agent");
    let models_path = directory.join("models.json");
    let settings_path = directory.join("settings.json");
    let models_before = common::snapshot(&models_path)
        .map_err(|_| AdapterFailure::ConfigurationFailed("configuration_read_failed"))?;
    let settings_before = common::snapshot(&settings_path)
        .map_err(|_| AdapterFailure::ConfigurationFailed("configuration_read_failed"))?;
    let helper = tool_credentials::shell_helper_command("pi")
        .map_err(|_| AdapterFailure::SecureStorageUnavailable)?;
    let models_after =
        render_model_catalog(models_before.as_deref(), origin, model_ids, &helper)
            .map_err(|_| AdapterFailure::ConfigurationFailed("configuration_parse_failed"))?;
    let settings_after = render_settings(settings_before.as_deref(), model)
        .map_err(|_| AdapterFailure::ConfigurationFailed("configuration_parse_failed"))?;
    let mut transaction =
        FileTransaction::stage_with_snapshot(models_path.clone(), models_before, models_after)
            .map_err(config_error)?;
    transaction
        .push_with_snapshot(settings_path.clone(), settings_before, settings_after)
        .map_err(config_error)?;
    Ok(Prepared {
        transaction,
        models_path,
        settings_path,
        origin: origin.to_owned(),
        model: model.to_owned(),
        model_ids: model_ids.to_vec(),
        helper,
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
        let models = common::snapshot(&self.models_path)
            .map_err(|_| AdapterFailure::ConfigurationFailed("configuration_readback_failed"))?
            .ok_or(AdapterFailure::ConfigurationFailed(
                "configuration_readback_failed",
            ))?;
        let settings = common::snapshot(&self.settings_path)
            .map_err(|_| AdapterFailure::ConfigurationFailed("configuration_readback_failed"))?
            .ok_or(AdapterFailure::ConfigurationFailed(
                "configuration_readback_failed",
            ))?;
        // 读回必须和读入同一种语法：Pi 的配置允许注释与尾随逗号，而我们
        // 现在**保留**它们（见 `minimal_or_pretty`）。用严格 JSON 解析会把
        // 自己刚写好的、完全正确的文件判成读回失败。
        let parse = |bytes: &[u8]| {
            let source = std::str::from_utf8(bytes).ok()?;
            json5::from_str::<Value>(source).ok()
        };
        let models = parse(&models).ok_or(AdapterFailure::ConfigurationFailed(
            "configuration_readback_failed",
        ))?;
        let settings = parse(&settings).ok_or(AdapterFailure::ConfigurationFailed(
            "configuration_readback_failed",
        ))?;
        let provider = &models["providers"]["yeschoy"];
        let correct = provider["baseUrl"].as_str()
            == Some(format!("{}/v1", self.origin.trim_end_matches('/')).as_str())
            && provider["api"].as_str() == Some("openai-completions")
            && provider["apiKey"].as_str() == Some(format!("!{}", self.helper).as_str())
            && provider["authHeader"].as_bool() == Some(true)
            && crate::tool_adapters::common::catalog_matches(
                &provider["models"],
                Some("id"),
                &self.model_ids,
            )
            && settings["defaultProvider"].as_str() == Some("yeschoy")
            && crate::tool_adapters::common::default_matches(
                settings["defaultModel"].as_str(),
                &self.model,
                &self.model_ids,
                strict_default,
            );
        if correct {
            Ok(())
        } else {
            Err(AdapterFailure::ConfigurationFailed(
                "configuration_readback_failed",
            ))
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

    /// 用户在 `models.json` / `settings.json` 里写的注释、排版、尾随逗号，
    /// 接入时一个字节都不该被我们抹掉 —— 我们只拥有 `providers.yeschoy`
    /// 和两个顶层键。
    #[test]
    fn activation_keeps_the_users_comments_and_layout() {
        let home = common::temporary_working_directory("pi-comments").unwrap();
        let models_path = home.join(".pi/agent/models.json");
        let settings_path = home.join(".pi/agent/settings.json");
        std::fs::create_dir_all(models_path.parent().unwrap()).unwrap();
        let models = r#"{
  // 我自己的中转，别删
  "providers": {
    "other": { "api": "openai", "apiKey": "synthetic-other" } // 行尾注释
  },
}
"#;
        let settings = r#"{
  // 思考档位：低
  "thinkingLevel": "low",
}
"#;
        std::fs::write(&models_path, models).unwrap();
        std::fs::write(&settings_path, settings).unwrap();
        let mut prepared = prepare_catalog(
            &home,
            "https://yeschoy.com",
            "model-a",
            &["model-a".to_owned()],
        )
        .unwrap();
        prepared.commit().unwrap();

        let written_models = std::fs::read_to_string(&models_path).unwrap();
        let written_settings = std::fs::read_to_string(&settings_path).unwrap();
        for kept in [
            "// 我自己的中转，别删",
            "// 行尾注释",
            "\"other\": { \"api\": \"openai\", \"apiKey\": \"synthetic-other\" }",
        ] {
            assert!(
                written_models.contains(kept),
                "lost {kept:?} in\n{written_models}"
            );
        }
        assert!(
            written_settings.contains("// 思考档位：低"),
            "{written_settings}"
        );
        // 语义仍然正确：我们那份 provider 与默认模型都写进去了。
        let parsed: Value = json5::from_str(&written_models).unwrap();
        assert_eq!(parsed["providers"]["yeschoy"]["api"], "openai-completions");
        let parsed: Value = json5::from_str(&written_settings).unwrap();
        assert_eq!(parsed["defaultProvider"], "yeschoy");
        assert_eq!(parsed["thinkingLevel"], "low");
        assert!(written_models.contains("// "));
    }

    #[test]
    fn ru043_pi_reapply_preserves_caps_and_restores_original_bytes() {
        let home = common::temporary_working_directory("pi-profile-reapply").unwrap();
        let path = home.join(".pi/agent/models.json");
        let settings_path = home.join(".pi/agent/settings.json");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let original = br#"{"providers":{"yeschoy":{"models":[{"id":"gpt-6-astra","api":"old-api","baseUrl":"https://old.invalid/v1","headers":{"Authorization":"synthetic-old-key"},"maxTokens":4096,"contextWindow":65536,"compat":{"supportsDeveloperRole":false}}]},"other":{"apiKey":"synthetic-other"}}}"#;
        let settings = br#"{"thinkingLevel":"low","futureSetting":true}"#;
        std::fs::write(&path, original).unwrap();
        std::fs::write(&settings_path, settings).unwrap();
        let mut prepared = prepare_catalog(
            &home,
            "https://yeschoy.com",
            "gpt-6-astra",
            &["gpt-6-astra".into()],
        )
        .unwrap();
        prepared.commit().unwrap();
        let value: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        let model = &value["providers"]["yeschoy"]["models"][0];
        assert_eq!(model["maxTokens"], 4096);
        assert_eq!(model["contextWindow"], 65536);
        assert_eq!(model["thinkingLevelMap"]["max"], "max");
        assert_eq!(model["compat"]["supportsDeveloperRole"], false);
        for key in ["api", "baseUrl", "headers"] {
            assert!(model.get(key).is_none());
        }
        assert_eq!(
            value["providers"]["yeschoy"]["baseUrl"],
            format!("{}/v1", prepared.origin)
        );
        assert_eq!(value["providers"]["other"]["apiKey"], "synthetic-other");
        let saved: Value = serde_json::from_slice(&std::fs::read(&settings_path).unwrap()).unwrap();
        assert_eq!(saved["thinkingLevel"], "low");
        prepared.rollback().unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), original);
        assert_eq!(std::fs::read(&settings_path).unwrap(), settings);
        std::fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn ru042_pi_catalog_keeps_real_ids_and_accepts_registered_native_default() {
        let home = common::temporary_working_directory("pi-model-set").unwrap();
        let ids = vec!["model-a".into(), "供应商/model-b".into()];
        let mut prepared = prepare_catalog(&home, "https://yeschoy.com", "model-a", &ids).unwrap();
        prepared.commit().unwrap();
        let models: Value =
            serde_json::from_slice(&std::fs::read(&prepared.models_path).unwrap()).unwrap();
        assert_eq!(
            models["providers"]["yeschoy"]["baseUrl"],
            "https://yeschoy.com/v1"
        );
        assert_eq!(
            models["providers"]["yeschoy"]["models"][1]["id"],
            "供应商/model-b"
        );
        let mut settings: Value =
            serde_json::from_slice(&std::fs::read(&prepared.settings_path).unwrap()).unwrap();
        settings["defaultModel"] = json!("供应商/model-b");
        std::fs::write(
            &prepared.settings_path,
            serde_json::to_vec(&settings).unwrap(),
        )
        .unwrap();
        assert!(prepared.validate_existing().is_ok());
        assert!(prepared.validate_readback(true).is_err());
        settings["defaultModel"] = json!("not-enrolled");
        std::fs::write(
            &prepared.settings_path,
            serde_json::to_vec(&settings).unwrap(),
        )
        .unwrap();
        assert!(prepared.validate_existing().is_err());
        std::fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn pi_existing_validation_is_read_only_and_checks_both_owned_files() {
        let directory = common::temporary_working_directory("pi-readonly").unwrap();
        let models_path = directory.join("models.json");
        let settings_path = directory.join("settings.json");
        let origin = "https://yeschoy.com";
        let model = "fixture-model";
        let helper = "synthetic-helper credential-helper pi";
        let models = render_models(None, origin, model, helper).unwrap();
        let settings = render_settings(Some(br#"{"theme":"keep"}"#), model).unwrap();
        std::fs::write(&models_path, &models).unwrap();
        std::fs::write(&settings_path, &settings).unwrap();
        let prepared = Prepared {
            transaction: FileTransaction::stage_with_snapshot(
                models_path.clone(),
                Some(models.clone()),
                models.clone(),
            )
            .unwrap(),
            models_path: models_path.clone(),
            settings_path: settings_path.clone(),
            origin: origin.into(),
            model: model.into(),
            model_ids: vec![model.into()],
            helper: helper.into(),
        };
        assert!(prepared.validate_existing().is_ok());
        assert_eq!(std::fs::read(&models_path).unwrap(), models);
        assert_eq!(std::fs::read(&settings_path).unwrap(), settings);
        for (path, bytes) in [(&models_path, models), (&settings_path, settings)] {
            let changed = String::from_utf8(bytes.clone())
                .unwrap()
                .replace(model, "different-model");
            std::fs::write(path, &changed).unwrap();
            assert!(prepared.validate_existing().is_err());
            assert_eq!(std::fs::read_to_string(path).unwrap(), changed);
            std::fs::write(path, bytes).unwrap();
        }
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn provider_merge_keeps_other_providers_and_has_no_literal_key() {
        let before = br#"{
          // existing user provider
          providers: { local: { baseUrl: 'http://127.0.0.1:11434/v1' } },
          future: true,
        }"#;
        let bytes = render_models(
            Some(before),
            "https://yeschoy.com",
            "glm-5.3",
            "'/Applications/野菜 API.app/client' credential-helper pi",
        )
        .unwrap();
        let value: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(value["future"], true);
        assert_eq!(
            value["providers"]["local"]["baseUrl"],
            "http://127.0.0.1:11434/v1"
        );
        assert_eq!(value["providers"]["yeschoy"]["models"][0]["id"], "glm-5.3");
        assert!(!String::from_utf8(bytes).unwrap().contains("sk-secret"));
    }
}

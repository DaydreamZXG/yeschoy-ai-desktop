//! Cline：写 `~/.cline/data/settings/providers.json`。
//!
//! Cline 的命令行和编辑器扩展读的是这同一份文件（官方文档：`~/.cline/` 对
//! IDE、CLI、SDK 全局生效）。只动我们自己的那一块：`providers` 里
//! `openai-compatible` 这一项，以及顶层的 `lastUsedProvider`。用户在 Cline 里
//! 配过的其他厂商原样留着，改回原样时再把选中的那家还回去。
//!
//! 密钥：`apiKey` 写的是**本地令牌**，请求先到 `cline_bridge`，由桥从系统
//! 钥匙串取出真正的站点密钥再转发。站点密钥不落盘。
//!
//! Cline 每跑一次都会把自己正在用的那一项的 `updatedAt` 改成当时的时间，所以
//! 这份文件不会和我们写下的一字不差。认不认得是自己的，只看 `settings` 里那
//! 四个字段。
use std::path::{Path, PathBuf};

use serde_json::{Map, Value};

use crate::{
    cline_bridge::PROXY_BASE,
    tool_adapters::{
        common::{self, ConfigFailure, FileTransaction},
        AdapterFailure,
    },
};

/// Cline 给「OpenAI Compatible」这一家起的名字，不是我们起的。
pub(crate) const PROVIDER_ID: &str = "openai-compatible";

/// 编辑器扩展的目录名前缀（发布者 `saoudrizwan`，扩展名 `claude-dev`）。
const EXTENSION_PREFIX: &str = "saoudrizwan.claude-dev-";

/// 这几个环境变量任何一个设了，Cline 读的就不是 `~/.cline` 下面那份。
pub(crate) const RELOCATING_VARIABLES: [&str; 3] = [
    "CLINE_PROVIDER_SETTINGS_PATH",
    "CLINE_DATA_DIR",
    "CLINE_DIR",
];

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

pub(crate) fn config_path(home: &Path) -> PathBuf {
    home.join(".cline")
        .join("data")
        .join("settings")
        .join("providers.json")
}

/// 是不是 Cline 的那份厂商设置。连接恢复按路径认它，好忽略 `updatedAt`。
pub(crate) fn is_provider_settings(path: &Path) -> bool {
    let mut parts = path.components().rev().map(|part| part.as_os_str());
    parts.next().is_some_and(|name| name == "providers.json")
        && parts.next().is_some_and(|name| name == "settings")
        && parts.next().is_some_and(|name| name == "data")
        && parts.next().is_some_and(|name| name == ".cline")
}

/// 改回原样时，第一次接入建出来的 `providers` 会被整个拿走。用户之后自己在
/// Cline 里加的别家不是我们的，留下；留下的文件补上 `version` 和 `providers`，
/// 这两个少一个 Cline 就当整份文件是坏的。
pub(crate) fn keep_what_is_not_ours(
    path: &Path,
    before: &Value,
    ours: &Value,
    now: &Value,
    restored: &mut Value,
    preserved: &mut bool,
) {
    if !is_provider_settings(path) || before["providers"].is_object() {
        return;
    }
    let Some(result) = restored.as_object_mut() else {
        return;
    };
    if !result.contains_key("providers") {
        let theirs: Map<String, Value> = now["providers"]
            .as_object()
            .into_iter()
            .flatten()
            .filter(|(id, _)| ours["providers"].get(id.as_str()).is_none())
            .map(|(id, entry)| (id.clone(), entry.clone()))
            .collect();
        if !theirs.is_empty() {
            *preserved = true;
            result.insert("providers".into(), Value::Object(theirs));
        }
    }
    if result.is_empty() {
        return;
    }
    result
        .entry("providers")
        .or_insert_with(|| Value::Object(Map::new()));
    if let Some(version) = now.get("version") {
        result.entry("version").or_insert_with(|| version.clone());
    }
}

/// 各家编辑器放扩展的目录里，Cline 扩展所在的文件夹。
///
/// 只装了扩展、没装命令行的人也算装了 Cline：两边读同一份设置。
pub(crate) fn extension_directories(home: &Path) -> Vec<PathBuf> {
    const EDITORS: [&str; 7] = [
        ".vscode",
        ".vscode-insiders",
        ".vscode-oss",
        ".cursor",
        ".windsurf",
        ".antigravity",
        ".trae",
    ];
    let mut found = Vec::new();
    for editor in EDITORS {
        let Ok(entries) = std::fs::read_dir(home.join(editor).join("extensions")) else {
            continue;
        };
        // 升级后旧版本的文件夹会留一阵子。每个编辑器只报一个，取名字最大的。
        let newest = entries
            .flatten()
            .filter(|entry| {
                entry
                    .file_name()
                    .to_str()
                    .is_some_and(|name| name.starts_with(EXTENSION_PREFIX))
                    && entry.path().is_dir()
            })
            .map(|entry| entry.path())
            .max();
        found.extend(newest);
    }
    found
}

fn parse_document(bytes: Option<&[u8]>) -> Result<Value, ()> {
    match bytes {
        Some(bytes) if !bytes.iter().all(u8::is_ascii_whitespace) => {
            let value: Value = serde_json::from_slice(bytes).map_err(|_| ())?;
            value.is_object().then_some(value).ok_or(())
        }
        _ => Ok(Value::Object(Map::new())),
    }
}

fn settings(model: &str, local_token: &str) -> Value {
    serde_json::json!({
        "provider": PROVIDER_ID,
        "apiKey": local_token,
        "model": model,
        "baseUrl": PROXY_BASE,
    })
}

pub(crate) fn render(
    existing: Option<&[u8]>,
    model: &str,
    local_token: &str,
    updated_at: &str,
) -> Result<Vec<u8>, ()> {
    let mut root = parse_document(existing)?;
    let object = root.as_object_mut().ok_or(())?;
    object.entry("version").or_insert_with(|| 1.into());
    // Cline 读到没有 `modes` 的文件会自己补一个空的；先补上，改回原样时才
    // 不会多出一处「你后来改过」。
    object
        .entry("modes")
        .or_insert_with(|| Value::Object(Map::new()));
    object.insert("lastUsedProvider".into(), PROVIDER_ID.into());
    let providers = object
        .entry("providers")
        .or_insert_with(|| Value::Object(Map::new()))
        .as_object_mut()
        .ok_or(())?;
    // `updatedAt` 和 `tokenSource` 少一个，Cline 就当这一项是坏的整项丢掉
    // （3.0.75 实测），所以两个都写。
    providers.insert(
        PROVIDER_ID.into(),
        serde_json::json!({
            "settings": settings(model, local_token),
            "updatedAt": updated_at,
            "tokenSource": "manual",
        }),
    );
    let mut bytes = serde_json::to_vec_pretty(&root).map_err(|_| ())?;
    bytes.push(b'\n');
    Ok(bytes)
}

/// `2026-01-02T03:04:05.678Z`，Cline 自己写 `updatedAt` 用的就是这个样子。
fn iso8601(since_epoch: std::time::Duration) -> String {
    let seconds = since_epoch.as_secs();
    // 天数换年月日：Howard Hinnant 的 `civil_from_days`。
    let days = (seconds / 86_400) as i64 + 719_468;
    let era = days.div_euclid(146_097);
    let day_of_era = days.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let shifted_month = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * shifted_month + 2) / 5 + 1;
    let month = if shifted_month < 10 {
        shifted_month + 3
    } else {
        shifted_month - 9
    };
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    let of_day = seconds % 86_400;
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}.{:03}Z",
        of_day / 3_600,
        of_day % 3_600 / 60,
        of_day % 60,
        since_epoch.subsec_millis()
    )
}

fn now_iso8601() -> String {
    iso8601(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default(),
    )
}

pub(crate) fn prepare(
    home: &Path,
    model: &str,
    local_token: Option<&str>,
) -> Result<Prepared, AdapterFailure> {
    common::validate_catalog(model, std::slice::from_ref(&model.to_owned()))?;
    // 用户把 Cline 的设置挪到了别处：我们写的那份它不会读。
    if RELOCATING_VARIABLES
        .iter()
        .any(|key| crate::shell_environment::is_set_anywhere(key))
    {
        return Err(AdapterFailure::ExternalOverride);
    }
    let local_token = local_token
        .filter(|value| value.starts_with("ycg-") && value.len() == 68)
        .ok_or(AdapterFailure::SecureStorageUnavailable)?
        .to_owned();
    let path = config_path(home);
    let before = common::snapshot(&path)
        .map_err(|_| AdapterFailure::ConfigurationFailed("configuration_read_failed"))?;
    let after = render(before.as_deref(), model, &local_token, &now_iso8601())
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

/// 重启客户端后，Cline 若仍指着我们这座桥，就把桥重新起起来。
/// 只看 Cline 自己的设置：用户已经换走了，就不占端口。
pub(crate) async fn resume_if_configured() {
    let Ok(credential) = crate::tool_credentials::load("cline") else {
        return;
    };
    let Some(home) = super::user_home() else {
        return;
    };
    let Ok(bytes) = std::fs::read(config_path(&home)) else {
        return;
    };
    if points_at_our_bridge(&bytes) {
        if let Err(error) = crate::cline_bridge::global().start(credential).await {
            log::warn!("cline_bridge stage=resume_start_failed reason={error:?}");
        }
    }
}

/// 现在选中的是不是「OpenAI Compatible」，且它还指着本地桥。
pub(crate) fn points_at_our_bridge(bytes: &[u8]) -> bool {
    parse_document(Some(bytes)).is_ok_and(|value| {
        value["lastUsedProvider"].as_str() == Some(PROVIDER_ID)
            && value["providers"][PROVIDER_ID]["settings"]["baseUrl"].as_str() == Some(PROXY_BASE)
    })
}

/// 改回原样之前：Cline 只是把 `updatedAt` 改成了它上次运行的时间，别的没动，
/// 那就当它没改过。不这样做，三方合并会把这个时间当成「用户后来改的」留下，
/// 剩一项只有时间、没有设置的空壳。
pub(crate) fn forget_touch(path: &Path, ours: &Value, now: &mut Value) {
    if !is_provider_settings(path) {
        return;
    }
    let written = &ours["providers"][PROVIDER_ID];
    let Some(current) = now
        .get_mut("providers")
        .and_then(|providers| providers.get_mut(PROVIDER_ID))
        .and_then(Value::as_object_mut)
    else {
        return;
    };
    if written["settings"].is_object()
        && current.get("settings") == Some(&written["settings"])
        && current.get("updatedAt").is_some_and(Value::is_string)
    {
        current.insert("updatedAt".into(), written["updatedAt"].clone());
    }
}

impl Prepared {
    pub(crate) fn changes(&self) -> &[common::FileChange] {
        self.transaction.changes()
    }

    pub(crate) fn commit(&mut self) -> Result<(), AdapterFailure> {
        self.transaction.commit().map_err(config_error)?;
        self.validate_existing()
    }

    /// 写完读回和日后「还是不是我们写的那样」用同一条：看的是那四个字段和
    /// 选中的是哪一家，不比字节（Cline 会改 `updatedAt`）。
    pub(crate) fn validate_existing(&self) -> Result<(), AdapterFailure> {
        let failed = AdapterFailure::ConfigurationFailed("configuration_readback_failed");
        let bytes = common::snapshot(&self.path)
            .map_err(|_| failed)?
            .ok_or(failed)?;
        let value = parse_document(Some(&bytes)).map_err(|_| failed)?;
        let correct = value["lastUsedProvider"].as_str() == Some(PROVIDER_ID)
            && value["providers"][PROVIDER_ID]["settings"]
                == settings(&self.model, &self.local_token);
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

    const STAMP: &str = "2026-01-02T03:04:05.678Z";

    fn token() -> String {
        format!("ycg-{}", "a".repeat(64))
    }

    #[test]
    fn a_missing_file_gets_every_field_cline_insists_on() {
        let bytes = render(None, "glm-5.3", &token(), STAMP).unwrap();
        let value: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(
            value,
            serde_json::json!({
                "version": 1,
                "lastUsedProvider": "openai-compatible",
                "modes": {},
                "providers": {
                    "openai-compatible": {
                        "settings": {
                            "provider": "openai-compatible",
                            "apiKey": token(),
                            "model": "glm-5.3",
                            "baseUrl": "http://127.0.0.1:15733/cline/v1",
                        },
                        "updatedAt": STAMP,
                        "tokenSource": "manual",
                    }
                }
            })
        );
        assert!(bytes.ends_with(b"}\n"));
    }

    #[test]
    fn timestamps_look_like_the_ones_cline_writes() {
        use std::time::Duration;
        assert_eq!(iso8601(Duration::ZERO), "1970-01-01T00:00:00.000Z");
        assert_eq!(
            iso8601(Duration::from_millis(951_782_400_000)),
            "2000-02-29T00:00:00.000Z"
        );
        assert_eq!(
            iso8601(Duration::from_millis(1_795_276_267_346)),
            "2026-11-21T15:51:07.346Z"
        );
        assert_eq!(
            iso8601(Duration::from_millis(4_102_444_799_999)),
            "2099-12-31T23:59:59.999Z"
        );
    }

    #[test]
    fn merge_keeps_the_other_providers_and_writes_no_relay_key() {
        let before = br#"{
          "version": 1,
          "lastUsedProvider": "anthropic",
          "modes": {"plan": {"provider": "anthropic"}},
          "providers": {
            "anthropic": {"settings": {"provider": "anthropic", "apiKey": "user-own-key"}, "updatedAt": "2026-01-01T00:00:00.000Z", "tokenSource": "manual"},
            "openai-compatible": {"settings": {"provider": "openai-compatible", "apiKey": "old", "model": "old", "baseUrl": "https://old.example/v1"}, "updatedAt": "2026-01-01T00:00:00.000Z", "tokenSource": "manual"}
          },
          "somethingNew": true
        }"#;
        let bytes = render(Some(before), "glm-5.3", &token(), STAMP).unwrap();
        let value: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(value["lastUsedProvider"], PROVIDER_ID);
        assert_eq!(
            value["providers"]["anthropic"]["settings"]["apiKey"],
            "user-own-key"
        );
        assert_eq!(value["modes"]["plan"]["provider"], "anthropic");
        assert_eq!(value["somethingNew"], true);
        let ours = &value["providers"][PROVIDER_ID];
        assert_eq!(ours["settings"]["baseUrl"], PROXY_BASE);
        assert_eq!(ours["settings"]["model"], "glm-5.3");
        assert_eq!(ours["settings"]["apiKey"], token());
        assert_eq!(ours["tokenSource"], "manual");
        assert!(!String::from_utf8(bytes).unwrap().contains("sk-"));
    }

    #[test]
    fn a_file_that_is_not_an_object_is_refused_rather_than_replaced() {
        assert!(render(Some(b"[1, 2]"), "m", &token(), STAMP).is_err());
        assert!(render(Some(b"{ not json"), "m", &token(), STAMP).is_err());
        assert!(render(Some(br#"{"providers": []}"#), "m", &token(), STAMP).is_err());
    }

    #[test]
    fn apply_survives_cline_touching_the_timestamp_then_rolls_back_exactly() {
        let home = common::temporary_working_directory("cline-apply").unwrap();
        let path = config_path(&home);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let original = br#"{"version":1,"lastUsedProvider":"cline","modes":{},"providers":{"cline":{"settings":{"provider":"cline"},"updatedAt":"2026-01-01T00:00:00.000Z","tokenSource":"manual"}}}"#;
        std::fs::write(&path, original).unwrap();

        let mut prepared = prepare(&home, "glm-5.3", Some(&token())).unwrap();
        prepared.commit().unwrap();
        let written = std::fs::read(&path).unwrap();
        assert!(points_at_our_bridge(&written));

        // Cline 跑了一次：只有 `updatedAt` 变了。
        let mut touched: Value = serde_json::from_slice(&written).unwrap();
        touched["providers"][PROVIDER_ID]["updatedAt"] = "2030-01-01T00:00:00.000Z".into();
        std::fs::write(&path, serde_json::to_vec_pretty(&touched).unwrap()).unwrap();
        assert!(prepared.validate_existing().is_ok());

        // 用户在 Cline 里换了别家：不再是我们写的那样。
        let mut switched = touched.clone();
        switched["lastUsedProvider"] = "cline".into();
        std::fs::write(&path, serde_json::to_vec_pretty(&switched).unwrap()).unwrap();
        assert!(prepared.validate_existing().is_err());
        assert!(!points_at_our_bridge(&std::fs::read(&path).unwrap()));

        std::fs::write(&path, &written).unwrap();
        prepared.rollback().unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), original);
        let _ = std::fs::remove_dir_all(home);
    }

    #[test]
    fn a_bad_local_token_never_reaches_the_file() {
        let home = common::temporary_working_directory("cline-token").unwrap();
        for bad in [None, Some("sk-real-looking-key"), Some("ycg-short")] {
            assert!(matches!(
                prepare(&home, "glm-5.3", bad),
                Err(AdapterFailure::SecureStorageUnavailable)
            ));
        }
        assert!(!config_path(&home).exists());
        let _ = std::fs::remove_dir_all(home);
    }

    #[test]
    fn forget_touch_only_forgives_the_timestamp() {
        let path = Path::new("/home/someone/.cline/data/settings/providers.json");
        let ours: Value =
            serde_json::from_slice(&render(None, "glm-5.3", &token(), STAMP).unwrap()).unwrap();

        let mut touched = ours.clone();
        touched["providers"][PROVIDER_ID]["updatedAt"] = "2030-01-01T00:00:00.000Z".into();
        forget_touch(path, &ours, &mut touched);
        assert_eq!(touched, ours);

        // 用户改了型号：这是他的改动，时间也原样留着。
        let mut edited = ours.clone();
        edited["providers"][PROVIDER_ID]["settings"]["model"] = "other".into();
        edited["providers"][PROVIDER_ID]["updatedAt"] = "2030-01-01T00:00:00.000Z".into();
        let expected = edited.clone();
        forget_touch(path, &ours, &mut edited);
        assert_eq!(edited, expected);

        // 别的文件一概不碰。
        let mut elsewhere = expected.clone();
        elsewhere["providers"][PROVIDER_ID]["settings"] =
            ours["providers"][PROVIDER_ID]["settings"].clone();
        let unchanged = elsewhere.clone();
        forget_touch(
            Path::new("/home/someone/.config/opencode/opencode.json"),
            &ours,
            &mut elsewhere,
        );
        assert_eq!(elsewhere, unchanged);
    }

    #[test]
    fn finds_the_extension_in_each_editor_and_reports_one_per_editor() {
        let home = common::temporary_working_directory("cline-extension").unwrap();
        let code = home.join(".vscode").join("extensions");
        let cursor = home.join(".cursor").join("extensions");
        for directory in [
            code.join("saoudrizwan.claude-dev-3.0.1"),
            code.join("saoudrizwan.claude-dev-3.0.9"),
            code.join("someone.else-1.0.0"),
            cursor.join("saoudrizwan.claude-dev-3.0.9-universal"),
        ] {
            std::fs::create_dir_all(directory).unwrap();
        }
        // 同名的文件不是扩展。
        std::fs::create_dir_all(home.join(".windsurf").join("extensions")).unwrap();
        std::fs::write(
            home.join(".windsurf")
                .join("extensions")
                .join("saoudrizwan.claude-dev-3.0.9"),
            b"",
        )
        .unwrap();
        assert_eq!(
            extension_directories(&home),
            [
                code.join("saoudrizwan.claude-dev-3.0.9"),
                cursor.join("saoudrizwan.claude-dev-3.0.9-universal"),
            ]
        );
        assert!(extension_directories(&home.join("nowhere")).is_empty());
        let _ = std::fs::remove_dir_all(home);
    }

    #[test]
    fn recognises_only_clines_provider_settings_path() {
        assert!(is_provider_settings(&config_path(Path::new("/h"))));
        assert!(!is_provider_settings(Path::new(
            "/h/.cline/data/settings/global-settings.json"
        )));
        assert!(!is_provider_settings(Path::new(
            "/h/other/data/settings/providers.json"
        )));
    }
}

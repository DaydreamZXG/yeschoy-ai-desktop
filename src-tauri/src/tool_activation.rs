use std::{
    env,
    fs::{self, File, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use reqwest::{Method, Url};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use toml_edit::{value, DocumentMut, Item, Table};

use crate::{
    account_v2::{
        native_account_json, native_session_access, AccountV2State, NativeSessionFailure,
    },
    connectivity_core::request_id_is_valid,
};

const MAX_CONFIG_BYTES: u64 = 2 * 1024 * 1024;
const TOKEN_PAGE_SIZE: &str = "100";

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ToolActivationRequest {
    request_id: String,
    line_id: String,
    tool_id: String,
    model_id: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolActivationProjection {
    request_id: String,
    schema_version: u8,
    status: &'static str,
    tool_id: String,
    model_id: String,
    observed_at_epoch_ms: u64,
    reason_code: &'static str,
}

impl ToolActivationProjection {
    fn new(
        request: &ToolActivationRequest,
        status: &'static str,
        reason_code: &'static str,
    ) -> Self {
        Self {
            request_id: request.request_id.clone(),
            schema_version: 1,
            status,
            tool_id: request.tool_id.clone(),
            model_id: request.model_id.clone(),
            observed_at_epoch_ms: now_epoch_ms(),
            reason_code,
        }
    }
}

#[derive(Clone, Copy)]
enum ActivationFailure {
    SignedOut,
    UnsupportedModel,
    ServerUnavailable,
    ConfigurationFailed(&'static str),
}

impl ActivationFailure {
    fn projection(self, request: &ToolActivationRequest) -> ToolActivationProjection {
        match self {
            Self::SignedOut => ToolActivationProjection::new(request, "signed_out", "signed_out"),
            Self::UnsupportedModel => {
                ToolActivationProjection::new(request, "unsupported_model", "model_not_available")
            }
            Self::ServerUnavailable => {
                ToolActivationProjection::new(request, "server_unavailable", "server_unavailable")
            }
            Self::ConfigurationFailed(reason) => {
                ToolActivationProjection::new(request, "configuration_failed", reason)
            }
        }
    }
}

struct TokenLease {
    id: u64,
    key: String,
    created: bool,
}

fn now_epoch_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u64::MAX as u128) as u64
}

fn bounded_plain_text(value: &str, maximum: usize) -> bool {
    !value.is_empty()
        && value.chars().count() <= maximum
        && !value.chars().any(|character| {
            character.is_control()
                || matches!(character, '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
        })
}

fn request_is_valid(request: &ToolActivationRequest) -> bool {
    request_id_is_valid(&request.request_id)
        && matches!(
            request.line_id.as_str(),
            "mainland_optimized" | "global_accelerated"
        )
        && matches!(request.tool_id.as_str(), "claude_desktop" | "codex_desktop")
        && bounded_plain_text(&request.model_id, 200)
}

fn data(value: &Value) -> Option<&Value> {
    let object = value.as_object()?;
    (object.get("success")?.as_bool()? == true)
        .then(|| object.get("data"))
        .flatten()
}

fn server_success(status: u16, value: &Value) -> bool {
    (200..300).contains(&status)
        && value
            .as_object()
            .and_then(|object| object.get("success"))
            .and_then(Value::as_bool)
            == Some(true)
}

async fn validate_model(
    origin: &str,
    access_token: &str,
    model_id: &str,
    tool_id: &str,
) -> Result<(), ActivationFailure> {
    let (status, value) = native_account_json(
        Method::GET,
        &format!("{origin}/api/user/models"),
        access_token,
        None,
    )
    .await
    .map_err(|_| ActivationFailure::ServerUnavailable)?;
    if matches!(status, 401 | 403) {
        return Err(ActivationFailure::SignedOut);
    }
    if !server_success(status, &value) {
        return Err(ActivationFailure::ServerUnavailable);
    }
    let available = data(&value)
        .and_then(Value::as_array)
        .is_some_and(|models| models.iter().any(|model| model.as_str() == Some(model_id)));
    if !available {
        return Err(ActivationFailure::UnsupportedModel);
    }
    let (pricing_status, pricing) = native_account_json(
        Method::GET,
        &format!("{origin}/api/pricing"),
        access_token,
        None,
    )
    .await
    .map_err(|_| ActivationFailure::ServerUnavailable)?;
    if matches!(pricing_status, 401 | 403) {
        return Err(ActivationFailure::SignedOut);
    }
    if !server_success(pricing_status, &pricing) {
        return Err(ActivationFailure::ServerUnavailable);
    }
    if model_supports_tool(&pricing, model_id, tool_id) {
        Ok(())
    } else {
        Err(ActivationFailure::UnsupportedModel)
    }
}

fn model_supports_tool(pricing: &Value, model_id: &str, tool_id: &str) -> bool {
    let required_endpoint = match tool_id {
        "claude_desktop" => "anthropic",
        "codex_desktop" => "openai-response",
        _ => return false,
    };
    data(pricing)
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_object)
        .find(|row| row.get("model_name").and_then(Value::as_str) == Some(model_id))
        .and_then(|row| row.get("supported_endpoint_types"))
        .and_then(Value::as_array)
        .is_some_and(|endpoints| {
            endpoints
                .iter()
                .any(|endpoint| endpoint.as_str() == Some(required_endpoint))
        })
}

fn token_name(tool_id: &str) -> &'static str {
    match tool_id {
        "claude_desktop" => "野菜API Claude Desktop",
        _ => "野菜API Codex",
    }
}

fn token_search_url(origin: &str, name: &str) -> Result<String, ActivationFailure> {
    let mut url = Url::parse(&format!("{origin}/api/token/search"))
        .map_err(|_| ActivationFailure::ServerUnavailable)?;
    url.query_pairs_mut()
        .append_pair("keyword", name)
        .append_pair("p", "1")
        .append_pair("size", TOKEN_PAGE_SIZE);
    Ok(url.to_string())
}

async fn find_token_id(
    origin: &str,
    access_token: &str,
    name: &str,
) -> Result<Option<u64>, ActivationFailure> {
    let url = token_search_url(origin, name)?;
    let (status, value) = native_account_json(Method::GET, &url, access_token, None)
        .await
        .map_err(|_| ActivationFailure::ServerUnavailable)?;
    if matches!(status, 401 | 403) {
        return Err(ActivationFailure::SignedOut);
    }
    if !server_success(status, &value) {
        return Err(ActivationFailure::ServerUnavailable);
    }
    let id = data(&value)
        .and_then(Value::as_object)
        .and_then(|page| page.get("items"))
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_object)
        .filter(|token| token.get("name").and_then(Value::as_str) == Some(name))
        .filter_map(|token| token.get("id").and_then(Value::as_u64))
        .max();
    Ok(id)
}

fn token_request(name: &str, id: Option<u64>) -> Value {
    let mut body = json!({
        "name": name,
        "remain_quota": 0,
        "expired_time": -1,
        "unlimited_quota": true,
        "model_limits_enabled": false,
        "model_limits": "",
        "allow_ips": "",
        "group": "",
        "auto_groups": [],
        "cross_group_retry": false
    });
    if let Some(id) = id {
        body.as_object_mut()
            .expect("token request is an object")
            .insert("id".into(), id.into());
    }
    body
}

fn normalize_api_key(value: &str) -> Option<String> {
    let value = value.trim();
    if value.len() < 16
        || value.len() > 256
        || value
            .chars()
            .any(|character| character.is_control() || character.is_whitespace())
    {
        return None;
    }
    Some(if value.starts_with("sk-") {
        value.to_owned()
    } else {
        format!("sk-{value}")
    })
}

async fn fetch_token_key(
    origin: &str,
    access_token: &str,
    id: u64,
) -> Result<String, ActivationFailure> {
    let (status, value) = native_account_json(
        Method::POST,
        &format!("{origin}/api/token/{id}/key"),
        access_token,
        None,
    )
    .await
    .map_err(|_| ActivationFailure::ServerUnavailable)?;
    if matches!(status, 401 | 403) {
        return Err(ActivationFailure::SignedOut);
    }
    if !server_success(status, &value) {
        return Err(ActivationFailure::ServerUnavailable);
    }
    data(&value)
        .and_then(Value::as_object)
        .and_then(|value| value.get("key"))
        .and_then(Value::as_str)
        .and_then(normalize_api_key)
        .ok_or(ActivationFailure::ServerUnavailable)
}

async fn acquire_token(
    origin: &str,
    access_token: &str,
    tool_id: &str,
) -> Result<TokenLease, ActivationFailure> {
    let name = token_name(tool_id);
    if let Some(id) = find_token_id(origin, access_token, name).await? {
        let (status, value) = native_account_json(
            Method::PUT,
            &format!("{origin}/api/token/"),
            access_token,
            Some(token_request(name, Some(id))),
        )
        .await
        .map_err(|_| ActivationFailure::ServerUnavailable)?;
        if matches!(status, 401 | 403) {
            return Err(ActivationFailure::SignedOut);
        }
        if !server_success(status, &value) {
            return Err(ActivationFailure::ServerUnavailable);
        }
        return Ok(TokenLease {
            id,
            key: fetch_token_key(origin, access_token, id).await?,
            created: false,
        });
    }

    let (status, value) = native_account_json(
        Method::POST,
        &format!("{origin}/api/token/"),
        access_token,
        Some(token_request(name, None)),
    )
    .await
    .map_err(|_| ActivationFailure::ServerUnavailable)?;
    if matches!(status, 401 | 403) {
        return Err(ActivationFailure::SignedOut);
    }
    if !server_success(status, &value) {
        return Err(ActivationFailure::ServerUnavailable);
    }
    let key = data(&value)
        .and_then(Value::as_object)
        .and_then(|value| value.get("key"))
        .and_then(Value::as_str)
        .and_then(normalize_api_key)
        .ok_or(ActivationFailure::ServerUnavailable)?;
    let id = find_token_id(origin, access_token, name)
        .await?
        .ok_or(ActivationFailure::ServerUnavailable)?;
    Ok(TokenLease {
        id,
        key,
        created: true,
    })
}

async fn delete_created_token(origin: &str, access_token: &str, lease: &TokenLease) {
    if lease.created {
        let _ = native_account_json(
            Method::DELETE,
            &format!("{origin}/api/token/{}", lease.id),
            access_token,
            None,
        )
        .await;
    }
}

fn user_home() -> Option<PathBuf> {
    #[cfg(target_os = "windows")]
    {
        env::var_os("USERPROFILE").map(PathBuf::from).or_else(|| {
            let drive = env::var_os("HOMEDRIVE")?;
            let path = env::var_os("HOMEPATH")?;
            let mut home = PathBuf::from(drive);
            home.push(path);
            Some(home)
        })
    }
    #[cfg(not(target_os = "windows"))]
    {
        env::var_os("HOME").map(PathBuf::from)
    }
}

fn ensure_safe_parent(path: &Path) -> io::Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "missing parent"))?;
    if parent.exists() {
        let metadata = fs::symlink_metadata(parent)?;
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "unsafe parent"));
        }
    } else {
        fs::create_dir(parent)?;
    }
    if path.exists() {
        let metadata = fs::symlink_metadata(path)?;
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "unsafe target"));
        }
        if metadata.len() > MAX_CONFIG_BYTES {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "oversized target",
            ));
        }
    }
    Ok(())
}

fn read_snapshot(path: &Path) -> io::Result<Option<Vec<u8>>> {
    ensure_safe_parent(path)?;
    if !path.exists() {
        return Ok(None);
    }
    fs::read(path).map(Some)
}

fn create_temp_file(path: &Path) -> io::Result<(PathBuf, File)> {
    let parent = path.parent().expect("validated target parent");
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("config");
    for attempt in 0..16u8 {
        let temp = parent.join(format!(
            ".{file_name}.yeschoy-{}-{}-{attempt}.tmp",
            std::process::id(),
            now_epoch_ms()
        ));
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        match options.open(&temp) {
            Ok(file) => return Ok((temp, file)),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    }
    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "temporary file collision",
    ))
}

#[cfg(target_os = "windows")]
fn replace_file(temp: &Path, target: &Path) -> io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{
        MoveFileExW, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH,
    };
    let mut from = temp
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect::<Vec<_>>();
    let mut to = target
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect::<Vec<_>>();
    let success = unsafe {
        MoveFileExW(
            from.as_mut_ptr(),
            to.as_mut_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    };
    if success == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

#[cfg(not(target_os = "windows"))]
fn replace_file(temp: &Path, target: &Path) -> io::Result<()> {
    fs::rename(temp, target)
}

fn atomic_write(path: &Path, bytes: &[u8]) -> io::Result<()> {
    ensure_safe_parent(path)?;
    if bytes.len() as u64 > MAX_CONFIG_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "oversized content",
        ));
    }
    let existing_permissions = fs::metadata(path)
        .ok()
        .map(|metadata| metadata.permissions());
    let (temp, mut file) = create_temp_file(path)?;
    let result = (|| {
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        if let Some(permissions) = existing_permissions {
            fs::set_permissions(&temp, permissions)?;
        }
        replace_file(&temp, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temp);
    }
    result
}

fn restore_snapshot(path: &Path, snapshot: Option<&[u8]>) -> io::Result<()> {
    match snapshot {
        Some(bytes) => atomic_write(path, bytes),
        None if path.exists() => fs::remove_file(path),
        None => Ok(()),
    }
}

fn claude_settings(
    existing: Option<&[u8]>,
    origin: &str,
    key: &str,
    model: &str,
) -> Result<Vec<u8>, ()> {
    let mut root = match existing {
        Some(bytes) if !bytes.is_empty() => {
            serde_json::from_slice::<Value>(bytes).map_err(|_| ())?
        }
        _ => Value::Object(Map::new()),
    };
    let root = root.as_object_mut().ok_or(())?;
    if !root.contains_key("env") {
        root.insert("env".into(), Value::Object(Map::new()));
    }
    let env = root
        .get_mut("env")
        .and_then(Value::as_object_mut)
        .ok_or(())?;
    for field in [
        "ANTHROPIC_MODEL",
        "ANTHROPIC_DEFAULT_HAIKU_MODEL",
        "ANTHROPIC_DEFAULT_SONNET_MODEL",
        "ANTHROPIC_DEFAULT_OPUS_MODEL",
    ] {
        env.insert(field.into(), model.into());
    }
    env.insert("ANTHROPIC_BASE_URL".into(), origin.into());
    env.insert("ANTHROPIC_AUTH_TOKEN".into(), key.into());
    let mut bytes = serde_json::to_vec_pretty(&root).map_err(|_| ())?;
    bytes.push(b'\n');
    Ok(bytes)
}

fn codex_config(existing: Option<&[u8]>, origin: &str, model: &str) -> Result<Vec<u8>, ()> {
    let source = existing
        .map(std::str::from_utf8)
        .transpose()
        .map_err(|_| ())?
        .unwrap_or("");
    let mut document = if source.trim().is_empty() {
        DocumentMut::new()
    } else {
        source.parse::<DocumentMut>().map_err(|_| ())?
    };
    document["model_provider"] = value("yeschoy");
    document["model"] = value(model);
    if document.get("model_providers").is_none() {
        let mut table = Table::new();
        table.set_implicit(true);
        document.insert("model_providers", Item::Table(table));
    }
    let providers = document
        .get_mut("model_providers")
        .and_then(Item::as_table_like_mut)
        .ok_or(())?;
    if providers.get("yeschoy").is_none() {
        providers.insert("yeschoy", Item::Table(Table::new()));
    }
    let provider = providers
        .get_mut("yeschoy")
        .and_then(Item::as_table_like_mut)
        .ok_or(())?;
    provider.insert("name", value("野菜API"));
    provider.insert(
        "base_url",
        value(format!("{}/v1", origin.trim_end_matches('/'))),
    );
    provider.insert("wire_api", value("responses"));
    provider.insert("requires_openai_auth", value(true));
    Ok(document.to_string().into_bytes())
}

fn codex_auth(existing: Option<&[u8]>, key: &str) -> Result<Vec<u8>, ()> {
    let mut root = match existing {
        Some(bytes) if !bytes.is_empty() => {
            serde_json::from_slice::<Value>(bytes).map_err(|_| ())?
        }
        _ => Value::Object(Map::new()),
    };
    root.as_object_mut()
        .ok_or(())?
        .insert("OPENAI_API_KEY".into(), key.into());
    let mut bytes = serde_json::to_vec_pretty(&root).map_err(|_| ())?;
    bytes.push(b'\n');
    Ok(bytes)
}

fn configure_claude(
    home: &Path,
    origin: &str,
    key: &str,
    model: &str,
) -> Result<(), ActivationFailure> {
    let path = home.join(".claude").join("settings.json");
    let before = read_snapshot(&path)
        .map_err(|_| ActivationFailure::ConfigurationFailed("configuration_read_failed"))?;
    let after = claude_settings(before.as_deref(), origin, key, model)
        .map_err(|_| ActivationFailure::ConfigurationFailed("configuration_parse_failed"))?;
    atomic_write(&path, &after)
        .map_err(|_| ActivationFailure::ConfigurationFailed("configuration_write_failed"))
}

fn configure_codex(
    home: &Path,
    origin: &str,
    key: &str,
    model: &str,
) -> Result<(), ActivationFailure> {
    let directory = home.join(".codex");
    let config_path = directory.join("config.toml");
    let auth_path = directory.join("auth.json");
    let config_before = read_snapshot(&config_path)
        .map_err(|_| ActivationFailure::ConfigurationFailed("configuration_read_failed"))?;
    let auth_before = read_snapshot(&auth_path)
        .map_err(|_| ActivationFailure::ConfigurationFailed("configuration_read_failed"))?;
    let config_after = codex_config(config_before.as_deref(), origin, model)
        .map_err(|_| ActivationFailure::ConfigurationFailed("configuration_parse_failed"))?;
    let auth_after = codex_auth(auth_before.as_deref(), key)
        .map_err(|_| ActivationFailure::ConfigurationFailed("configuration_parse_failed"))?;
    atomic_write(&config_path, &config_after)
        .map_err(|_| ActivationFailure::ConfigurationFailed("configuration_write_failed"))?;
    if atomic_write(&auth_path, &auth_after).is_err() {
        let _ = restore_snapshot(&config_path, config_before.as_deref());
        return Err(ActivationFailure::ConfigurationFailed(
            "configuration_write_failed",
        ));
    }
    Ok(())
}

fn configure_local_tool(
    tool_id: &str,
    origin: &str,
    key: &str,
    model: &str,
) -> Result<(), ActivationFailure> {
    let home = user_home()
        .filter(|path| path.is_absolute() && path.is_dir())
        .ok_or(ActivationFailure::ConfigurationFailed("home_unavailable"))?;
    match tool_id {
        "claude_desktop" => configure_claude(&home, origin, key, model),
        "codex_desktop" => configure_codex(&home, origin, key, model),
        _ => Err(ActivationFailure::ConfigurationFailed("invalid_request")),
    }
}

#[tauri::command]
pub async fn configure_desktop_tool_v1(
    state: tauri::State<'_, AccountV2State>,
    request: ToolActivationRequest,
) -> Result<ToolActivationProjection, String> {
    if !request_is_valid(&request) {
        return Err("invalid_tool_activation_request".into());
    }
    let (origin, access_token) = match native_session_access(&state, &request.line_id).await {
        Ok(value) => value,
        Err(NativeSessionFailure::SignedOut) => {
            return Ok(ActivationFailure::SignedOut.projection(&request))
        }
        Err(NativeSessionFailure::ServerUnavailable) => {
            return Ok(ActivationFailure::ServerUnavailable.projection(&request))
        }
    };
    if let Err(error) =
        validate_model(&origin, &access_token, &request.model_id, &request.tool_id).await
    {
        return Ok(error.projection(&request));
    }
    let lease = match acquire_token(&origin, &access_token, &request.tool_id).await {
        Ok(value) => value,
        Err(error) => return Ok(error.projection(&request)),
    };
    if let Err(error) =
        configure_local_tool(&request.tool_id, &origin, &lease.key, &request.model_id)
    {
        delete_created_token(&origin, &access_token, &lease).await;
        return Ok(error.projection(&request));
    }
    Ok(ToolActivationProjection::new(
        &request,
        "configured",
        "configured",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn claude_merge_preserves_unowned_fields() {
        let before = br#"{"permissions":{"allow":["Read"]},"env":{"KEEP":"yes"}}"#;
        let bytes = claude_settings(Some(before), "https://yeschoy.com", "sk-secret", "glm-5.3")
            .expect("valid settings");
        let value: Value = serde_json::from_slice(&bytes).expect("json");
        assert_eq!(value["permissions"]["allow"][0], "Read");
        assert_eq!(value["env"]["KEEP"], "yes");
        assert_eq!(value["env"]["ANTHROPIC_MODEL"], "glm-5.3");
        assert_eq!(value["env"]["ANTHROPIC_AUTH_TOKEN"], "sk-secret");
    }

    #[test]
    fn codex_merge_preserves_unowned_fields_and_sets_responses_provider() {
        let before =
            b"model_reasoning_effort = \"high\"\n[notice]\nhide_full_access_warning = true\n";
        let bytes =
            codex_config(Some(before), "https://api.yeschoy.com", "glm-5.3").expect("valid config");
        let text = String::from_utf8(bytes).expect("utf8");
        let document = text.parse::<DocumentMut>().expect("toml");
        assert_eq!(document["model_provider"].as_str(), Some("yeschoy"));
        assert_eq!(document["model"].as_str(), Some("glm-5.3"));
        assert_eq!(document["model_reasoning_effort"].as_str(), Some("high"));
        assert_eq!(
            document["model_providers"]["yeschoy"]["base_url"].as_str(),
            Some("https://api.yeschoy.com/v1")
        );
        assert_eq!(
            document["model_providers"]["yeschoy"]["wire_api"].as_str(),
            Some("responses")
        );
    }

    #[test]
    fn renderer_projection_contains_no_secret_or_path() {
        let request = ToolActivationRequest {
            request_id: "activate-1".into(),
            line_id: "mainland_optimized".into(),
            tool_id: "codex_desktop".into(),
            model_id: "glm-5.3".into(),
        };
        let serialized = serde_json::to_string(&ToolActivationProjection::new(
            &request,
            "configured",
            "configured",
        ))
        .expect("projection");
        for forbidden in ["apiKey", "token", "path", "sk-"] {
            assert!(!serialized.contains(forbidden));
        }
    }

    #[test]
    fn model_must_support_the_selected_desktop_protocol() {
        let pricing = json!({
            "success": true,
            "data": [
                {
                    "model_name": "glm-5.3",
                    "supported_endpoint_types": ["anthropic", "openai-response"]
                },
                {
                    "model_name": "chat-only",
                    "supported_endpoint_types": ["openai"]
                }
            ]
        });
        assert!(model_supports_tool(&pricing, "glm-5.3", "claude_desktop"));
        assert!(model_supports_tool(&pricing, "glm-5.3", "codex_desktop"));
        assert!(!model_supports_tool(&pricing, "chat-only", "codex_desktop"));
        assert!(!model_supports_tool(&pricing, "missing", "claude_desktop"));
    }
}

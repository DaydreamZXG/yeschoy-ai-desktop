use std::io::{self, Read, Write};

use keyring::v1::{Entry, Error as KeyringError};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

const SERVICE: &str = "com.yeschoy.desktop.tool-credential.v2";
const ALLOWED_TOOLS: [&str; 7] = [
    "claude_code",
    "claude_desktop",
    "codex_desktop",
    "pi",
    "dsh_web",
    "hermes",
    "openclaw",
];

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ToolCredential {
    pub(crate) api_key: String,
    pub(crate) origin: String,
    pub(crate) model_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) local_gateway_token: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) codex_transport: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) claude_transport: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) models: Vec<ToolModelRoute>,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ToolModelRoute {
    pub(crate) model_id: String,
    pub(crate) billing_group: String,
    pub(crate) api_key: String,
    pub(crate) origin: String,
    pub(crate) claude_transport: Option<String>,
    pub(crate) codex_transport: Option<String>,
}

impl std::fmt::Debug for ToolCredential {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ToolCredential")
            .field("model_count", &self.model_ids().len())
            .field("credentials", &"[redacted]")
            .finish()
    }
}

impl ToolCredential {
    pub(crate) fn has_model_set(&self) -> bool {
        !self.models.is_empty()
    }
    pub(crate) fn model_ids(&self) -> Vec<String> {
        if self.models.is_empty() {
            vec![self.model_id.clone()]
        } else {
            self.models.iter().map(|r| r.model_id.clone()).collect()
        }
    }
    pub(crate) fn resolve_model(&self, requested: &str) -> Result<Self, CredentialFailure> {
        if self.models.is_empty() {
            return (requested == self.model_id)
                .then(|| self.clone())
                .ok_or(CredentialFailure::Invalid);
        }
        let r = self
            .models
            .iter()
            .find(|r| r.model_id == requested)
            .ok_or(CredentialFailure::Invalid)?;
        Ok(Self {
            api_key: r.api_key.clone(),
            origin: r.origin.clone(),
            model_id: r.model_id.clone(),
            local_gateway_token: self.local_gateway_token.clone(),
            codex_transport: r.codex_transport.clone(),
            claude_transport: r.claude_transport.clone(),
            models: Vec::new(),
        })
    }
    /// The credential a tool presents to whatever serves its endpoint. Only
    /// Claude Desktop still talks to a loopback gateway; every other surface
    /// presents the scoped relay key straight to the relay origin.
    pub(crate) fn client_token(&self, tool: &str) -> &str {
        if tool == "claude_desktop" {
            self.local_gateway_token.as_deref().unwrap_or("")
        } else {
            &self.api_key
        }
    }

    /// The scoped per-tool key issued by the relay. Tools that talk to the
    /// relay directly (every surface except the loopback-gateway one) present
    /// this key, never the loopback capability token.
    pub(crate) fn upstream_key(&self) -> &str {
        &self.api_key
    }
}

#[derive(Debug)]
pub(crate) enum CredentialFailure {
    Unavailable,
    Missing,
    Invalid,
}

fn allowed_tool(tool_id: &str) -> bool {
    ALLOWED_TOOLS.contains(&tool_id)
}

fn record_is_valid(tool_id: &str, record: &ToolCredential) -> bool {
    let key = record.api_key.as_str();
    let model = record.model_id.as_str();
    let local = record.local_gateway_token.as_deref().unwrap_or("");
    (16..=256).contains(&key.len())
        && !key
            .chars()
            .any(|value| value.is_control() || value.is_whitespace())
        && matches!(
            record.origin.as_str(),
            "https://yeschoy.com" | "https://api.yeschoy.com"
        )
        && !model.is_empty()
        && model.chars().count() <= 200
        && !model.chars().any(char::is_control)
        && (record.codex_transport.is_none()
            || (tool_id == "codex_desktop"
                && matches!(
                    record.codex_transport.as_deref(),
                    Some("direct_responses" | "chat_bridge")
                )))
        && (record.claude_transport.is_none()
            || (matches!(tool_id, "claude_code" | "claude_desktop")
                && matches!(
                    record.claude_transport.as_deref(),
                    Some("direct_anthropic" | "chat_bridge")
                )))
        && (local.is_empty()
            || ((32..=256).contains(&local.len())
                && !local
                    .chars()
                    .any(|value| value.is_control() || value.is_whitespace())))
        && (tool_id != "claude_desktop" || !local.is_empty())
        && (record.claude_transport.as_deref() != Some("chat_bridge") || !local.is_empty())
        && (record.models.is_empty() || model_set_is_valid(tool_id, record))
}

fn model_set_is_valid(tool: &str, record: &ToolCredential) -> bool {
    if record.models.len() > 200
        || !record
            .local_gateway_token
            .as_deref()
            .is_some_and(|t| t.starts_with("ycg-") && t.len() == 68)
    {
        return false;
    }
    let mut seen = std::collections::HashSet::new();
    for route in &record.models {
        if !seen.insert(&route.model_id)
            || route.origin != record.origin
            || route.billing_group.is_empty()
            || route.billing_group == "auto"
            || route.billing_group.chars().count() > 128
            || route.billing_group.chars().any(char::is_control)
        {
            return false;
        }
        let Ok(resolved) = record.resolve_model(&route.model_id) else {
            return false;
        };
        if !record_is_valid(tool, &resolved) {
            return false;
        }
    }
    record
        .models
        .iter()
        .find(|r| r.model_id == record.model_id)
        .is_some_and(|r| {
            r.api_key == record.api_key
                && r.origin == record.origin
                && r.claude_transport == record.claude_transport
                && r.codex_transport == record.codex_transport
        })
}

// Windows limits one credential blob to 2560 bytes. Keep chunks below that
// SDK limit; the root is a small atomic generation pointer, never plaintext on disk.
const CHUNK_BYTES: usize = 2048;
const MAX_PAYLOAD: usize = 200 * 2048;
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct StorageManifest {
    storage_version: u8,
    generation: String,
    chunks: usize,
    sha256: String,
}
trait SecretStore {
    fn get(&self, name: &str) -> Result<Option<String>, CredentialFailure>;
    fn set(&self, name: &str, value: &str) -> Result<(), CredentialFailure>;
    fn delete(&self, name: &str) -> Result<(), CredentialFailure>;
}
struct OsSecrets;
impl SecretStore for OsSecrets {
    fn get(&self, name: &str) -> Result<Option<String>, CredentialFailure> {
        match Entry::new(SERVICE, name)
            .map_err(|_| CredentialFailure::Unavailable)?
            .get_password()
        {
            Ok(v) => Ok(Some(v)),
            Err(KeyringError::NoEntry) => Ok(None),
            Err(_) => Err(CredentialFailure::Unavailable),
        }
    }
    fn set(&self, name: &str, value: &str) -> Result<(), CredentialFailure> {
        Entry::new(SERVICE, name)
            .map_err(|_| CredentialFailure::Unavailable)?
            .set_password(value)
            .map_err(|_| CredentialFailure::Unavailable)
    }
    fn delete(&self, name: &str) -> Result<(), CredentialFailure> {
        match Entry::new(SERVICE, name)
            .map_err(|_| CredentialFailure::Unavailable)?
            .delete_credential()
        {
            Ok(()) | Err(KeyringError::NoEntry) => Ok(()),
            Err(_) => Err(CredentialFailure::Unavailable),
        }
    }
}
fn manifest(value: &str) -> Result<Option<StorageManifest>, CredentialFailure> {
    let doc: serde_json::Value =
        serde_json::from_str(value).map_err(|_| CredentialFailure::Invalid)?;
    if doc.get("storageVersion").is_none() {
        return Ok(None);
    }
    let m: StorageManifest = serde_json::from_value(doc).map_err(|_| CredentialFailure::Invalid)?;
    if m.storage_version != 3
        || m.generation.len() != 32
        || !m.generation.bytes().all(|b| b.is_ascii_hexdigit())
        || m.chunks == 0
        || m.chunks > MAX_PAYLOAD / CHUNK_BYTES + 1
        || m.sha256.len() != 64
    {
        return Err(CredentialFailure::Invalid);
    }
    Ok(Some(m))
}
fn chunk_name(tool: &str, m: &StorageManifest, i: usize) -> String {
    format!("{tool}.{}.{}", m.generation, i)
}
fn logical_payload(
    store: &impl SecretStore,
    tool: &str,
) -> Result<Option<String>, CredentialFailure> {
    if !allowed_tool(tool) {
        return Err(CredentialFailure::Invalid);
    }
    let before = store.get(tool)?;
    let result = payload_from_root(store, tool, before.as_deref());
    // Another process may have published a generation while an OS helper was
    // reading old chunks. Retry the local read only when the root changed.
    if result.is_err() {
        let after = store.get(tool)?;
        if after != before {
            return payload_from_root(store, tool, after.as_deref());
        }
    }
    result
}

fn payload_from_root(
    store: &impl SecretStore,
    tool: &str,
    root: Option<&str>,
) -> Result<Option<String>, CredentialFailure> {
    let Some(root) = root else {
        return Ok(None);
    };
    let Some(m) = manifest(root)? else {
        return Ok(Some(root.to_owned()));
    };
    let mut value = String::new();
    for i in 0..m.chunks {
        let part = store
            .get(&chunk_name(tool, &m, i))?
            .ok_or(CredentialFailure::Invalid)?;
        if part.len() > CHUNK_BYTES || value.len() + part.len() > MAX_PAYLOAD {
            return Err(CredentialFailure::Invalid);
        }
        value.push_str(&part);
    }
    if format!("{:x}", Sha256::digest(value.as_bytes())) != m.sha256 {
        return Err(CredentialFailure::Invalid);
    }
    Ok(Some(value))
}
fn remove_chunks(store: &impl SecretStore, tool: &str, root: Option<&str>) {
    if let Some(m) = root.and_then(|r| manifest(r).ok().flatten()) {
        for i in 0..m.chunks {
            let _ = store.delete(&chunk_name(tool, &m, i));
        }
    }
}
fn publish_payload(
    store: &impl SecretStore,
    tool: &str,
    payload: Option<&str>,
) -> Result<(), CredentialFailure> {
    if !allowed_tool(tool) || payload.is_some_and(|p| p.len() > MAX_PAYLOAD) {
        return Err(CredentialFailure::Invalid);
    }
    let before = store.get(tool)?;
    let mut staged: Vec<String> = Vec::new();
    let next = if let Some(value) = payload {
        if value.len() <= CHUNK_BYTES {
            Some(value.to_owned())
        } else {
            let mut nonce = [0u8; 16];
            getrandom::fill(&mut nonce).map_err(|_| CredentialFailure::Unavailable)?;
            let generation = nonce.iter().map(|b| format!("{b:02x}")).collect();
            let mut m = StorageManifest {
                storage_version: 3,
                generation,
                chunks: 0,
                sha256: format!("{:x}", Sha256::digest(value.as_bytes())),
            };
            let mut rest = value;
            while !rest.is_empty() {
                let mut end = rest.len().min(CHUNK_BYTES);
                while !rest.is_char_boundary(end) {
                    end -= 1;
                }
                let name = chunk_name(tool, &m, m.chunks);
                staged.push(name.clone());
                if store.set(&name, &rest[..end]).is_err() {
                    for name in staged {
                        let _ = store.delete(&name);
                    }
                    return Err(CredentialFailure::Unavailable);
                }
                m.chunks += 1;
                rest = &rest[end..];
            }
            Some(serde_json::to_string(&m).map_err(|_| CredentialFailure::Invalid)?)
        }
    } else {
        None
    };
    let published = match &next {
        Some(v) => store.set(tool, v),
        None => store.delete(tool),
    };
    if published.is_err()
        || !matches!(logical_payload(store, tool), Ok(ref value) if value.as_deref() == payload)
    {
        match before.as_deref() {
            Some(v) => store.set(tool, v)?,
            None => store.delete(tool)?,
        };
        for name in staged {
            let _ = store.delete(&name);
        }
        return Err(CredentialFailure::Unavailable);
    }
    remove_chunks(store, tool, before.as_deref());
    Ok(())
}

pub(crate) fn load(tool_id: &str) -> Result<ToolCredential, CredentialFailure> {
    let payload = logical_payload(&OsSecrets, tool_id)?.ok_or(CredentialFailure::Missing)?;
    let record: ToolCredential =
        serde_json::from_str(&payload).map_err(|_| CredentialFailure::Invalid)?;
    record_is_valid(tool_id, &record)
        .then_some(record)
        .ok_or(CredentialFailure::Invalid)
}

pub(crate) fn snapshot(tool_id: &str) -> Result<Option<String>, CredentialFailure> {
    logical_payload(&OsSecrets, tool_id)
}

pub(crate) fn store(tool_id: &str, record: &ToolCredential) -> Result<(), CredentialFailure> {
    if !record_is_valid(tool_id, record) {
        return Err(CredentialFailure::Invalid);
    }
    let payload = serde_json::to_string(record).map_err(|_| CredentialFailure::Invalid)?;
    publish_payload(&OsSecrets, tool_id, Some(&payload))
}

pub(crate) fn restore(tool_id: &str, before: Option<&str>) -> Result<(), CredentialFailure> {
    if let Some(value) = before {
        let record: ToolCredential =
            serde_json::from_str(value).map_err(|_| CredentialFailure::Invalid)?;
        if !record_is_valid(tool_id, &record) {
            return Err(CredentialFailure::Invalid);
        }
    }
    publish_payload(&OsSecrets, tool_id, before)
}

pub(crate) fn executable_path() -> Result<String, CredentialFailure> {
    let executable = std::env::current_exe()
        .and_then(std::fs::canonicalize)
        .map_err(|_| CredentialFailure::Unavailable)?;
    let rendered = executable
        .to_str()
        .filter(|value| !value.is_empty())
        .ok_or(CredentialFailure::Unavailable)?;
    Ok(rendered.to_owned())
}

#[cfg(not(target_os = "windows"))]
fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

#[cfg(target_os = "windows")]
fn shell_quote(value: &str) -> String {
    // The helper path is compiled into a command string consumed by cmd.exe in
    // the Windows builds of Claude Code and Pi. Reject shell metacharacters
    // instead of trying to invent another parsing dialect.
    if value.chars().any(|character| {
        matches!(
            character,
            '"' | '&' | '|' | '<' | '>' | '^' | '%' | '!' | '\r' | '\n'
        )
    }) {
        return String::new();
    }
    format!("\"{value}\"")
}

pub(crate) fn shell_helper_command(tool_id: &str) -> Result<String, CredentialFailure> {
    if !allowed_tool(tool_id) {
        return Err(CredentialFailure::Invalid);
    }
    let executable = shell_quote(&executable_path()?);
    if executable.is_empty() {
        return Err(CredentialFailure::Unavailable);
    }
    Ok(format!("{executable} credential-helper {tool_id}"))
}

fn bare_helper(tool_id: &str) -> i32 {
    match load(tool_id) {
        Ok(record) => {
            // Helper consumers (Claude Code, Pi, Hermes) now present this key
            // to the relay origin directly, so the helper must return the
            // scoped relay key rather than the retired loopback token.
            let secret = record.upstream_key();
            let mut output = io::stdout().lock();
            if output.write_all(secret.as_bytes()).is_ok()
                && output.write_all(b"\n").is_ok()
                && output.flush().is_ok()
            {
                0
            } else {
                71
            }
        }
        Err(CredentialFailure::Missing) => 69,
        Err(CredentialFailure::Unavailable) => 70,
        Err(CredentialFailure::Invalid) => 65,
    }
}

fn openclaw_helper(tool_id: &str) -> i32 {
    const ID: &str = "providers/yeschoy/apiKey";
    let mut request = Vec::new();
    if io::stdin()
        .lock()
        .take(16 * 1024 + 1)
        .read_to_end(&mut request)
        .is_err()
        || request.len() > 16 * 1024
    {
        return 65;
    }
    let valid_request = serde_json::from_slice::<serde_json::Value>(&request)
        .ok()
        .and_then(|value| {
            let object = value.as_object()?;
            let ids = object.get("ids")?.as_array()?;
            (object.get("protocolVersion")?.as_u64()? == 1
                && object.get("provider")?.as_str()? == "yeschoy-keychain"
                && ids.len() == 1
                && ids[0].as_str()? == ID)
                .then_some(())
        })
        .is_some();
    if !valid_request {
        return 65;
    }
    let record = match load(tool_id) {
        Ok(value) => value,
        Err(CredentialFailure::Missing) => return 69,
        Err(CredentialFailure::Unavailable) => return 70,
        Err(CredentialFailure::Invalid) => return 65,
    };
    let response = serde_json::json!({
        "protocolVersion": 1,
        "values": { ID: record.upstream_key() }
    });
    let mut output = io::stdout().lock();
    if serde_json::to_writer(&mut output, &response).is_ok()
        && output.write_all(b"\n").is_ok()
        && output.flush().is_ok()
    {
        0
    } else {
        71
    }
}

/// Intercepts signed helper modes before Tauri is initialized. Bare-token
/// consumers use `credential-helper`; OpenClaw uses its bounded protocol-v1
/// exec SecretRef exchange.
pub fn credential_helper_exit_code() -> Option<i32> {
    let arguments = std::env::args().collect::<Vec<_>>();
    let code = match arguments.get(1).map(String::as_str) {
        Some("credential-helper") => match arguments.as_slice() {
            [_, _, tool_id] if allowed_tool(tool_id) => bare_helper(tool_id),
            _ => 64,
        },
        Some("credential-helper-openclaw") => match arguments.as_slice() {
            [_, _, tool_id] if tool_id == "openclaw" => openclaw_helper("openclaw"),
            _ => 64,
        },
        _ => return None,
    };
    Some(code)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct FixtureStore {
        values: std::cell::RefCell<std::collections::BTreeMap<String, String>>,
        fail_once: std::cell::RefCell<Option<String>>,
    }
    impl SecretStore for FixtureStore {
        fn get(&self, name: &str) -> Result<Option<String>, CredentialFailure> {
            Ok(self.values.borrow().get(name).cloned())
        }
        fn set(&self, name: &str, value: &str) -> Result<(), CredentialFailure> {
            assert!(value.len() <= 2560, "Windows credential byte limit");
            self.values.borrow_mut().insert(name.into(), value.into());
            if self
                .fail_once
                .borrow()
                .as_deref()
                .is_some_and(|v| name == v || (v == "chunk" && name.contains('.')))
            {
                self.fail_once.replace(None);
                return Err(CredentialFailure::Unavailable);
            }
            Ok(())
        }
        fn delete(&self, name: &str) -> Result<(), CredentialFailure> {
            self.values.borrow_mut().remove(name);
            Ok(())
        }
    }

    #[test]
    fn ru042_keyring_generations_obey_windows_size_and_restore_logical_snapshot() {
        let store = FixtureStore::default();
        let a = serde_json::json!({"synthetic":"测试".repeat(1500)}).to_string();
        let b = serde_json::json!({"synthetic":"b".repeat(6000)}).to_string();
        publish_payload(&store, "pi", Some(&a)).unwrap();
        let before = logical_payload(&store, "pi").unwrap().unwrap();
        let old_names: Vec<_> = store
            .values
            .borrow()
            .keys()
            .filter(|n| *n != "pi")
            .cloned()
            .collect();
        publish_payload(&store, "pi", Some(&b)).unwrap();
        assert!(old_names
            .iter()
            .all(|n| !store.values.borrow().contains_key(n)));
        publish_payload(&store, "pi", Some(&before)).unwrap();
        assert_eq!(
            logical_payload(&store, "pi").unwrap().as_deref(),
            Some(a.as_str())
        );
        publish_payload(&store, "pi", None).unwrap();
        assert!(store.values.borrow().is_empty());
    }

    #[test]
    fn ru042_keyring_partial_write_never_publishes_incomplete_generation() {
        for fail in ["chunk", "pi"] {
            let store = FixtureStore::default();
            let a = serde_json::json!({"synthetic":"a".repeat(3000)}).to_string();
            let b = serde_json::json!({"synthetic":"b".repeat(5000)}).to_string();
            publish_payload(&store, "pi", Some(&a)).unwrap();
            let exact_before = store.values.borrow().clone();
            store.fail_once.replace(Some(fail.into()));
            assert!(publish_payload(&store, "pi", Some(&b)).is_err());
            assert_eq!(*store.values.borrow(), exact_before);
            assert_eq!(
                logical_payload(&store, "pi").unwrap().as_deref(),
                Some(a.as_str())
            );
        }
    }

    #[test]
    fn ru042_model_credentials_keep_exact_group_keys_and_reject_duplicates() {
        let routes: Vec<_> = ["a", "b"]
            .iter()
            .map(|id| ToolModelRoute {
                model_id: id.to_string(),
                billing_group: format!("group-{id}"),
                api_key: format!("synthetic-key-for-{id}"),
                origin: "https://yeschoy.com".into(),
                codex_transport: None,
                claude_transport: None,
            })
            .collect();
        let mut c = ToolCredential {
            model_id: "a".into(),
            api_key: routes[0].api_key.clone(),
            origin: routes[0].origin.clone(),
            local_gateway_token: Some(format!("ycg-{}", "a".repeat(64))),
            models: routes,
            codex_transport: None,
            claude_transport: None,
        };
        assert!(record_is_valid("pi", &c));
        assert_eq!(c.resolve_model("b").unwrap().api_key, "synthetic-key-for-b");
        assert!(c.resolve_model("not-enrolled").is_err());
        assert_eq!(c.client_token("pi"), c.api_key);
        assert!(!format!("{c:?}").contains("synthetic-key"));
        c.models.push(c.models[0].clone());
        assert!(!record_is_valid("pi", &c));
    }

    #[test]
    fn ru042_helper_read_retries_only_a_changed_keyring_generation() {
        struct Rotating {
            store: FixtureStore,
            next: std::cell::RefCell<Option<std::collections::BTreeMap<String, String>>>,
        }
        impl SecretStore for Rotating {
            fn get(&self, name: &str) -> Result<Option<String>, CredentialFailure> {
                if name.contains('.') {
                    if let Some(next) = self.next.borrow_mut().take() {
                        self.store.values.replace(next);
                    }
                }
                self.store.get(name)
            }
            fn set(&self, n: &str, v: &str) -> Result<(), CredentialFailure> {
                self.store.set(n, v)
            }
            fn delete(&self, n: &str) -> Result<(), CredentialFailure> {
                self.store.delete(n)
            }
        }
        let old = FixtureStore::default();
        let new = FixtureStore::default();
        let a = serde_json::json!({"synthetic":"a".repeat(4000)}).to_string();
        let b = serde_json::json!({"synthetic":"b".repeat(4000)}).to_string();
        publish_payload(&old, "pi", Some(&a)).unwrap();
        publish_payload(&new, "pi", Some(&b)).unwrap();
        let rotating = Rotating {
            store: old,
            next: std::cell::RefCell::new(Some(new.values.into_inner())),
        };
        assert_eq!(
            logical_payload(&rotating, "pi").unwrap().as_deref(),
            Some(b.as_str())
        );
        let name = rotating
            .store
            .values
            .borrow()
            .keys()
            .find(|n| n.contains('.'))
            .unwrap()
            .clone();
        rotating.store.values.borrow_mut().remove(&name);
        assert!(logical_payload(&rotating, "pi").is_err());
    }

    #[tokio::test]
    async fn ru042_cancelled_setup_restores_real_files_and_secret_before_runtime_shutdown() {
        use crate::shutdown_coordinator::{DrainOutcome, ShutdownCoordinator};
        use crate::tool_adapters::common;
        use std::sync::{
            atomic::{AtomicBool, Ordering},
            Arc,
        };
        let coordinator = ShutdownCoordinator::default();
        let permit = coordinator.admit_operation().unwrap();
        let stopped = Arc::new(AtomicBool::new(false));
        let flag = stopped.clone();
        coordinator
            .register_stop("fixture", move || async move {
                flag.store(true, Ordering::SeqCst);
            })
            .unwrap();
        let home = common::temporary_working_directory("ru042-safe-cleanup").unwrap();
        let path = home.join("settings.json");
        std::fs::write(&path, b"{\"theme\":\"original\"}").unwrap();
        let mut files =
            common::FileTransaction::stage(path.clone(), b"{\"theme\":\"changed\"}".to_vec())
                .unwrap();
        let secrets = FixtureStore::default();
        let before = serde_json::json!({"fixture":"old".repeat(2000)}).to_string();
        let after = serde_json::json!({"fixture":"new".repeat(2000)}).to_string();
        publish_payload(&secrets, "pi", Some(&before)).unwrap();
        publish_payload(&secrets, "pi", Some(&after)).unwrap();
        files.commit().unwrap();
        coordinator.request_shutdown();
        assert!(permit
            .cancel_safe(std::future::pending::<()>())
            .await
            .is_err());
        assert_eq!(
            coordinator
                .wait_quiescent(tokio::time::Instant::now())
                .await,
            DrainOutcome::FinishingOperation
        );
        files.rollback().unwrap();
        publish_payload(&secrets, "pi", Some(&before)).unwrap();
        assert!(!stopped.load(Ordering::SeqCst));
        drop(permit);
        assert_eq!(
            coordinator
                .wait_quiescent(tokio::time::Instant::now())
                .await,
            DrainOutcome::Quiescent
        );
        assert!(
            coordinator
                .stop_registered(tokio::time::Instant::now() + std::time::Duration::from_secs(1))
                .await
                .ready_to_exit
        );
        assert!(stopped.load(Ordering::SeqCst));
        assert_eq!(std::fs::read(&path).unwrap(), b"{\"theme\":\"original\"}");
        assert_eq!(
            logical_payload(&secrets, "pi").unwrap().as_deref(),
            Some(before.as_str())
        );
        std::fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn helper_command_contains_no_secret_and_quotes_the_executable() {
        let command = shell_helper_command("pi").expect("helper command");
        assert!(command.contains("credential-helper pi"));
        assert!(!command.contains("sk-"));
        assert!(command.starts_with(['\'', '"']));
    }

    #[test]
    fn rejects_unknown_helper_targets() {
        assert!(matches!(
            shell_helper_command("opencode"),
            Err(CredentialFailure::Invalid)
        ));
    }

    #[test]
    fn openclaw_is_allowlisted_but_uses_a_separate_protocol_mode() {
        assert!(allowed_tool("openclaw"));
        assert!(shell_helper_command("openclaw")
            .expect("command")
            .contains("credential-helper openclaw"));
    }

    #[test]
    fn transport_metadata_is_codex_only_and_backward_compatible() {
        let mut record = ToolCredential {
            api_key: "synthetic-test-key-0001".into(),
            origin: "https://yeschoy.com".into(),
            model_id: "gpt-5.5".into(),
            local_gateway_token: None,
            codex_transport: None,
            claude_transport: None,
            models: vec![],
        };
        assert!(record_is_valid("codex_desktop", &record));
        record.codex_transport = Some("chat_bridge".into());
        assert!(record_is_valid("codex_desktop", &record));
        assert!(!record_is_valid("pi", &record));
        record.codex_transport = Some("caller_route".into());
        assert!(!record_is_valid("codex_desktop", &record));
    }

    #[test]
    fn claude_transport_requires_a_local_token_only_for_bridged_code() {
        let mut record = ToolCredential {
            api_key: "synthetic-test-key-0001".into(),
            origin: "https://yeschoy.com".into(),
            model_id: "claude-opus-5".into(),
            local_gateway_token: None,
            codex_transport: None,
            claude_transport: Some("direct_anthropic".into()),
            models: vec![],
        };
        assert!(record_is_valid("claude_code", &record));
        record.claude_transport = Some("chat_bridge".into());
        assert!(!record_is_valid("claude_code", &record));
        record.local_gateway_token = Some(format!("ycg-{}", "a".repeat(64)));
        assert!(record_is_valid("claude_code", &record));
        assert!(record_is_valid("claude_desktop", &record));
        assert!(!record_is_valid("pi", &record));
    }
}

//! The credential store: keyring persistence for the per-tool keys.
//!
//! The credential *shape* and its pure rules live in `domain::credential`;
//! they are re-exported here so callers that think in terms of "the store"
//! keep one import path.
pub(crate) use crate::domain::credential::{
    allowed_tool, CredentialFailure, ToolCredential, ToolModelRoute, ALLOWED_TOOLS,
};

use std::io::{self, Write};

use keyring::v1::{Entry, Error as KeyringError};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::domain::station::StationOrigin;

const SERVICE: &str = "com.yeschoy.desktop.tool-credential.v2";
fn record_is_valid(tool_id: &str, record: &ToolCredential) -> bool {
    let key = record.api_key.as_str();
    let model = record.model_id.as_str();
    let local = record.local_gateway_token.as_deref().unwrap_or("");
    (16..=256).contains(&key.len())
        && !key
            .chars()
            .any(|value| value.is_control() || value.is_whitespace())
        // Valid identity is not user authorization. Never consult the trust file
        // here: a missing/corrupt store must not strand installed credentials or
        // prevent restoring an encrypted baseline for a third-party station.
        && StationOrigin::parse(&record.origin).is_ok()
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
    load_using(&OsSecrets, tool_id)
}

fn load_using(
    store: &impl SecretStore,
    tool_id: &str,
) -> Result<ToolCredential, CredentialFailure> {
    let payload = logical_payload(store, tool_id)?.ok_or(CredentialFailure::Missing)?;
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
    restore_using(&OsSecrets, tool_id, before)
}

fn restore_using(
    store: &impl SecretStore,
    tool_id: &str,
    before: Option<&str>,
) -> Result<(), CredentialFailure> {
    if let Some(value) = before {
        let record: ToolCredential =
            serde_json::from_str(value).map_err(|_| CredentialFailure::Invalid)?;
        if !record_is_valid(tool_id, &record) {
            return Err(CredentialFailure::Invalid);
        }
    }
    publish_payload(store, tool_id, before)
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

pub(crate) fn shell_gateway_helper_command(tool_id: &str) -> Result<String, CredentialFailure> {
    if tool_id != "claude_code" {
        return Err(CredentialFailure::Invalid);
    }
    let executable = shell_quote(&executable_path()?);
    if executable.is_empty() {
        return Err(CredentialFailure::Unavailable);
    }
    Ok(format!("{executable} gateway-credential-helper {tool_id}"))
}

fn bare_helper(tool_id: &str) -> i32 {
    match load(tool_id) {
        Ok(record) => {
            // Claude Code and Pi keep the scoped relay key helper contract.
            // This also lets old direct Claude Code receipts keep working until
            // the user re-applies the new compatibility endpoint.
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

fn gateway_helper(tool_id: &str) -> i32 {
    match load(tool_id) {
        Ok(record) => {
            let Some(secret) = record
                .local_gateway_token
                .as_deref()
                .filter(|value| value.starts_with("ycg-") && value.len() == 68)
            else {
                return 65;
            };
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

/// Intercepts signed helper modes before Tauri is initialized. Bare-token
/// consumers use `credential-helper`.
pub fn credential_helper_exit_code() -> Option<i32> {
    let arguments = std::env::args().collect::<Vec<_>>();
    let code = match arguments.get(1).map(String::as_str) {
        Some("credential-helper") => match arguments.as_slice() {
            [_, _, tool_id] if allowed_tool(tool_id) => bare_helper(tool_id),
            _ => 64,
        },
        Some("gateway-credential-helper") => match arguments.as_slice() {
            [_, _, tool_id] if tool_id == "claude_code" => gateway_helper(tool_id),
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

    fn station_record(origin: &str) -> ToolCredential {
        ToolCredential {
            api_key: "synthetic-station-key-0001".into(),
            origin: origin.into(),
            model_id: "fixture-model".into(),
            local_gateway_token: None,
            codex_transport: None,
            claude_transport: None,
            models: vec![],
        }
    }

    #[test]
    fn station_credentials_load_and_restore_without_a_trust_store() {
        let origin = "https://credential-recovery.station.example";
        assert!(!crate::station_trust::is_trusted(origin));
        let secrets = FixtureStore::default();
        let record = station_record(origin);
        let before = serde_json::to_string(&record).unwrap();
        // Run the production load/restore path against in-memory secrets. It
        // must not ask trust-store authorization to recover an installed key.
        restore_using(&secrets, "pi", Some(&before)).unwrap();
        assert_eq!(load_using(&secrets, "pi").unwrap(), record);
        restore_using(&secrets, "pi", None).unwrap();
        restore_using(&secrets, "pi", Some(&before)).unwrap();
        assert_eq!(
            logical_payload(&secrets, "pi").unwrap().as_deref(),
            Some(before.as_str())
        );
        assert_eq!(load_using(&secrets, "pi").unwrap(), record);
        assert!(!crate::station_trust::is_trusted(origin));
    }

    #[test]
    fn station_credentials_preserve_legacy_json_and_api_identity() {
        for origin in ["https://yeschoy.com", "https://api.yeschoy.com"] {
            let payload = serde_json::json!({
                "apiKey": "synthetic-legacy-key-0001",
                "origin": origin,
                "modelId": "fixture-model"
            })
            .to_string();
            let secrets = FixtureStore::default();
            restore_using(&secrets, "pi", Some(&payload)).unwrap();
            let record = load_using(&secrets, "pi").unwrap();
            assert_eq!(record.origin, origin);
            assert!(record.models.is_empty());
            assert!(record.claude_transport.is_none());
            assert!(record.codex_transport.is_none());
            assert_eq!(
                logical_payload(&secrets, "pi").unwrap().as_deref(),
                Some(payload.as_str())
            );
        }
    }

    #[test]
    fn station_credentials_reject_unsafe_origins_before_restore_publication() {
        let secrets = FixtureStore::default();
        for origin in [
            "http://relay.example",
            "https://user:pass@relay.example",
            "https://relay.example:443",
            "https://relay.example/path",
            "https://relay.example?x=1",
            "https://relay.example#fragment",
            "https://RELAY.example",
            "https://%72elay.example",
            "https://127.0.0.1",
            "https://127.1",
            "https://0x7f000001",
            "https://[::1]",
        ] {
            let record = station_record(origin);
            assert!(!record_is_valid("pi", &record), "accepted {origin}");
            let payload = serde_json::to_string(&record).unwrap();
            assert!(matches!(
                restore_using(&secrets, "pi", Some(&payload)),
                Err(CredentialFailure::Invalid)
            ));
            assert!(secrets.values.borrow().is_empty());
        }
    }

    #[test]
    fn station_model_routes_support_third_party_but_cannot_cross_identity() {
        let mut record = station_record("https://routes.station.example");
        record.local_gateway_token = Some(format!("ycg-{}", "a".repeat(64)));
        record.models = vec![ToolModelRoute {
            model_id: record.model_id.clone(),
            billing_group: "fixture-group".into(),
            api_key: record.api_key.clone(),
            origin: record.origin.clone(),
            claude_transport: None,
            codex_transport: None,
        }];
        assert!(record_is_valid("pi", &record));
        record.models[0].origin = "https://other.station.example".into();
        assert!(!record_is_valid("pi", &record));
        record.models[0].origin = "https://user@routes.station.example".into();
        assert!(!record_is_valid("pi", &record));
    }

    #[test]
    fn helper_command_contains_no_secret_and_quotes_the_executable() {
        let command = shell_helper_command("pi").expect("helper command");
        assert!(command.contains("credential-helper pi"));
        assert!(!command.contains("sk-"));
        assert!(command.starts_with(['\'', '"']));
        let gateway = shell_gateway_helper_command("claude_code").expect("gateway helper");
        assert!(gateway.contains("gateway-credential-helper claude_code"));
        assert!(!gateway.contains("ycg-"));
    }

    #[test]
    fn rejects_unknown_helper_targets() {
        assert!(matches!(
            shell_helper_command("aider"),
            Err(CredentialFailure::Invalid)
        ));
        assert!(matches!(
            shell_gateway_helper_command("pi"),
            Err(CredentialFailure::Invalid)
        ));
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

//! A single encrypted recovery baseline per tool, never a backup history.
//! Nothing in this module exposes file contents or paths to the renderer.
use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
};

use aes_gcm::{
    aead::{Aead, KeyInit, Payload},
    Aes256Gcm, Nonce,
};
use base64::{engine::general_purpose::STANDARD, Engine};
use keyring::v1::{Entry, Error as KeyringError};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::tool_adapters::common::{self, FileChange};
use crate::tool_credentials::ToolCredential;

const SERVICE: &str = "com.yeschoy.desktop.connection-recovery.v1";
// Four config files, before/after and one in-flight predecessor, with JSON
// encoding overhead. This is a bounded journal, not an unbounded history.
const MAX_JOURNAL: u64 = 128 * 1024 * 1024;
const MAGIC: &[u8] = b"YC-RECOVERY-1\0";

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Receipt {
    pub(crate) tool_id: String,
    pub(crate) model_id: String,
    pub(crate) line_id: String,
    pub(crate) billing_group: String,
    pub(crate) updated_at_epoch_ms: u64,
    pub(crate) requires_background: bool,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Record {
    version: u8,
    pub(crate) receipt: Receipt,
    pub(crate) original_known: bool,
    pub(crate) files: Vec<FileChange>,
    pub(crate) pending: bool,
    // Only present while a configuration command is in flight. Discarded when
    // it commits; used to preserve the preceding baseline on failed switching.
    previous: Option<Box<Record>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    rollback_checkpoint: Option<RollbackCheckpoint>,
}

// Encrypted with the journal and present only during a desktop transaction.
// `files` retains the immediate predecessor, unlike the original-use baseline.
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RollbackCheckpoint {
    files: Vec<FileChange>,
    credential: Option<ToolCredential>,
}

impl Record {
    pub(crate) fn rollback_credential(&self) -> Option<&ToolCredential> {
        self.rollback_checkpoint
            .as_ref()
            .and_then(|checkpoint| checkpoint.credential.as_ref())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Failure {
    Storage,
    Invalid,
    Changed,
    Io,
}
type Result<T> = std::result::Result<T, Failure>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PendingRecoveryFailure {
    Load,
    Files,
    Credential,
    Receipt,
}

pub(crate) struct Store {
    root: PathBuf,
    key: [u8; 32],
}

// OS-released lock: a crash cannot strand the lock, and separate application
// processes cannot initialize different keys or interleave configuration writes.
pub(crate) fn operation_lock_at(root: &Path) -> Result<std::fs::File> {
    let path = root.join("operation.lock");
    common::ensure_safe_target(&path, 64).map_err(|_| Failure::Storage)?;
    std::fs::create_dir_all(root).map_err(|_| Failure::Storage)?;
    common::ensure_safe_target(&path, 64).map_err(|_| Failure::Storage)?;
    let mut options = std::fs::OpenOptions::new();
    options.read(true).write(true).create(true).truncate(false);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let file = options.open(path).map_err(|_| Failure::Storage)?;
    file.try_lock().map_err(|_| Failure::Changed)?;
    Ok(file)
}

pub(crate) fn operation_lock() -> Result<std::fs::File> {
    let home = crate::tool_adapters::user_home().ok_or(Failure::Storage)?;
    operation_lock_at(&home.join(".yeschoy/connection-recovery"))
}

fn allowed(tool: &str) -> bool {
    matches!(
        tool,
        "claude_code"
            | "claude_desktop"
            | "codex_desktop"
            | "pi"
            | "dsh_web"
            | "hermes"
            | "openclaw"
    )
}

fn encode(key: &[u8; 32], tool: &str, record: &Record) -> Result<Vec<u8>> {
    let cipher = Aes256Gcm::new_from_slice(key).map_err(|_| Failure::Storage)?;
    let mut nonce = [0; 12];
    getrandom::fill(&mut nonce).map_err(|_| Failure::Storage)?;
    let plain = serde_json::to_vec(record).map_err(|_| Failure::Invalid)?;
    if plain.len() as u64 > MAX_JOURNAL - 128 {
        return Err(Failure::Invalid);
    }
    let encrypted = cipher
        .encrypt(
            Nonce::from_slice(&nonce),
            Payload {
                msg: &plain,
                aad: tool.as_bytes(),
            },
        )
        .map_err(|_| Failure::Storage)?;
    let mut bytes = MAGIC.to_vec();
    bytes.extend_from_slice(&nonce);
    bytes.extend_from_slice(&encrypted);
    Ok(bytes)
}

fn decode(key: &[u8; 32], tool: &str, bytes: &[u8]) -> Result<Record> {
    if bytes.len() as u64 > MAX_JOURNAL
        || !bytes.starts_with(MAGIC)
        || bytes.len() < MAGIC.len() + 28
    {
        return Err(Failure::Invalid);
    }
    let cipher = Aes256Gcm::new_from_slice(key).map_err(|_| Failure::Storage)?;
    let nonce = &bytes[MAGIC.len()..MAGIC.len() + 12];
    let plain = cipher
        .decrypt(
            Nonce::from_slice(nonce),
            Payload {
                msg: &bytes[MAGIC.len() + 12..],
                aad: tool.as_bytes(),
            },
        )
        .map_err(|_| Failure::Invalid)?;
    let record: Record = serde_json::from_slice(&plain).map_err(|_| Failure::Invalid)?;
    validate_record(tool, &record)?;
    Ok(record)
}

fn validate_record(tool: &str, record: &Record) -> Result<()> {
    if !allowed(tool)
        || record.version != 1
        || record.receipt.tool_id != tool
        || record.files.is_empty()
        || record.files.len() > 4
        || record
            .previous
            .as_ref()
            .is_some_and(|previous| previous.pending || previous.previous.is_some())
    {
        return Err(Failure::Invalid);
    }
    let mut paths = BTreeSet::new();
    for file in &record.files {
        if !file.path.is_absolute()
            || file
                .path
                .components()
                .any(|p| matches!(p, std::path::Component::ParentDir))
            || !paths.insert(&file.path)
            || file.after.len() > 2 * 1024 * 1024
            || file
                .before
                .as_ref()
                .is_some_and(|b| b.len() > 2 * 1024 * 1024)
        {
            return Err(Failure::Invalid);
        }
    }
    if let Some(previous) = &record.previous {
        validate_record(tool, previous)?;
    }
    if let Some(checkpoint) = &record.rollback_checkpoint {
        if !record.pending
            || !matches!(tool, "claude_desktop" | "codex_desktop")
            || checkpoint
                .files
                .iter()
                .map(|file| &file.path)
                .collect::<BTreeSet<_>>()
                != paths
            || checkpoint.files.len() != record.files.len()
        {
            return Err(Failure::Invalid);
        }
        let mut attempt = record.clone();
        attempt.files = checkpoint.files.clone();
        attempt.previous = None;
        attempt.rollback_checkpoint = None;
        validate_record(tool, &attempt)?;
    }
    Ok(())
}

// Taking the operation lock creates this directory before legacy credentials
// are inspected. Its existence alone does not mean a recovery key was lost.
fn recovery_store_is_uninitialized(root: &Path) -> Result<bool> {
    common::ensure_safe_target(&root.join("operation.lock"), 64).map_err(|_| Failure::Storage)?;
    let entries = match std::fs::read_dir(root) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(true),
        Err(_) => return Err(Failure::Storage),
    };
    for entry in entries {
        let entry = entry.map_err(|_| Failure::Storage)?;
        // Unknown files, including unfinished records, may contain recovery
        // material. Never replace their missing key or silently ignore them.
        if entry.file_name() != "operation.lock" {
            return Ok(false);
        }
        let metadata = std::fs::symlink_metadata(entry.path()).map_err(|_| Failure::Storage)?;
        if !metadata.is_file() || metadata.file_type().is_symlink() || metadata.len() > 64 {
            return Err(Failure::Storage);
        }
    }
    Ok(true)
}

impl Store {
    pub(crate) fn open(create: bool) -> Result<Option<Self>> {
        let home = crate::tool_adapters::user_home().ok_or(Failure::Storage)?;
        let root = home.join(".yeschoy").join("connection-recovery");
        let entry = Entry::new(SERVICE, "encryption-key").map_err(|_| Failure::Storage)?;
        let key = match entry.get_password() {
            Ok(encoded) => STANDARD
                .decode(encoded)
                .map_err(|_| Failure::Storage)?
                .try_into()
                .map_err(|_| Failure::Storage)?,
            Err(KeyringError::NoEntry) => {
                if !recovery_store_is_uninitialized(&root)? {
                    return Err(Failure::Storage);
                }
                if !create {
                    return Ok(None);
                }
                let mut key = [0; 32];
                getrandom::fill(&mut key).map_err(|_| Failure::Storage)?;
                entry
                    .set_password(&STANDARD.encode(key))
                    .map_err(|_| Failure::Storage)?;
                key
            }
            Err(_) => return Err(Failure::Storage),
        };
        Ok(Some(Self { root, key }))
    }

    fn path(&self, tool: &str) -> Result<PathBuf> {
        if !allowed(tool) {
            return Err(Failure::Invalid);
        }
        Ok(self.root.join(format!("{tool}.sealed")))
    }

    pub(crate) fn load(&self, tool: &str) -> Result<Option<Record>> {
        common::snapshot_bounded(&self.path(tool)?, MAX_JOURNAL)
            .map_err(|_| Failure::Io)?
            .map(|bytes| decode(&self.key, tool, &bytes))
            .transpose()
    }

    pub(crate) fn save(&self, record: &Record) -> Result<()> {
        validate_record(&record.receipt.tool_id, record)?;
        let path = self.path(&record.receipt.tool_id)?;
        let bytes = encode(&self.key, &record.receipt.tool_id, record)?;
        common::atomic_write_bounded(&path, &bytes, MAX_JOURNAL).map_err(|_| Failure::Io)?;
        if common::snapshot_bounded(&path, MAX_JOURNAL).map_err(|_| Failure::Io)? != Some(bytes) {
            return Err(Failure::Io);
        }
        Ok(())
    }

    pub(crate) fn remove(&self, tool: &str) -> Result<()> {
        let path = self.path(tool)?;
        if common::snapshot_bounded(&path, MAX_JOURNAL)
            .map_err(|_| Failure::Io)?
            .is_some()
        {
            std::fs::remove_file(path).map_err(|_| Failure::Io)?;
        }
        Ok(())
    }

    /// Complete only an interrupted transaction for this exact tool. A
    /// completed baseline remains available for explicit restore and switching.
    /// The pending receipt is removed/replaced last so partial file or
    /// credential failures stay recoverable.
    pub(crate) fn recover_pending(
        &self,
        tool: &str,
        restore_credential: impl FnOnce() -> bool,
    ) -> std::result::Result<bool, PendingRecoveryFailure> {
        let Some(record) = self.load(tool).map_err(|_| PendingRecoveryFailure::Load)? else {
            return Ok(false);
        };
        if !record.pending {
            return Ok(false);
        }
        if record.rollback_checkpoint.is_some() {
            restore_attempt(&record).map_err(|_| PendingRecoveryFailure::Files)?;
        } else {
            restore_files(&record).map_err(|_| PendingRecoveryFailure::Files)?;
        }
        if !restore_credential() {
            return Err(PendingRecoveryFailure::Credential);
        }
        if record.rollback_checkpoint.is_some() {
            self.abandon(&record)
                .map_err(|_| PendingRecoveryFailure::Receipt)?;
        } else {
            self.remove(tool)
                .map_err(|_| PendingRecoveryFailure::Receipt)?;
        }
        Ok(true)
    }

    pub(crate) fn begin(
        &self,
        receipt: Receipt,
        changes: &[FileChange],
        legacy: Option<&ToolCredential>,
    ) -> Result<Record> {
        let previous = self.load(&receipt.tool_id)?;
        if previous.as_ref().is_some_and(|r| r.pending) {
            return Err(Failure::Changed);
        }
        let mut files = changes.to_vec();
        if let Some(previous) = &previous {
            // A relocated custom profile must be restored first. Never abandon
            // the original files silently or grow a cross-profile history.
            if previous
                .files
                .iter()
                .map(|f| &f.path)
                .collect::<BTreeSet<_>>()
                != files.iter().map(|f| &f.path).collect::<BTreeSet<_>>()
            {
                return Err(Failure::Changed);
            }
            for file in &mut files {
                let original = previous
                    .files
                    .iter()
                    .find(|f| f.path == file.path)
                    .ok_or(Failure::Invalid)?;
                // Rebase unrelated or later user edits, not the old YesChoy
                // values. This keeps the original owned values across switches.
                file.before = restore_bytes(original, file.before.as_deref())?.0;
            }
        } else if let Some(credential) = legacy {
            let owns_claude_mode = changes.iter().any(|file| {
                document(&file.path, file.before.as_deref())
                    .is_ok_and(|value| value["appliedId"] == "00000000-0000-4000-8000-000000157220")
            });
            for file in &mut files {
                file.before = legacy_clean(
                    &receipt.tool_id,
                    &file.path,
                    file.before.as_deref(),
                    credential,
                    owns_claude_mode,
                )?;
            }
        }
        let record = Record {
            version: 1,
            rollback_checkpoint: matches!(
                receipt.tool_id.as_str(),
                "claude_desktop" | "codex_desktop"
            )
            .then(|| RollbackCheckpoint {
                files: changes.to_vec(),
                credential: legacy.cloned(),
            }),
            receipt,
            original_known: previous
                .as_ref()
                .map_or(legacy.is_none(), |p| p.original_known),
            files,
            pending: true,
            previous: previous.map(Box::new),
        };
        self.save(&record)?;
        Ok(record)
    }

    pub(crate) fn finish(&self, record: &mut Record) -> Result<()> {
        let mut completed = record.clone();
        completed.pending = false;
        completed.previous = None;
        completed.rollback_checkpoint = None;
        self.save(&completed)?;
        *record = completed;
        Ok(())
    }

    pub(crate) fn abandon(&self, record: &Record) -> Result<()> {
        if let Some(previous) = &record.previous {
            self.save(previous)
        } else {
            self.remove(&record.receipt.tool_id)
        }
    }
}

fn document(path: &Path, bytes: Option<&[u8]>) -> Result<Value> {
    let source = std::str::from_utf8(bytes.unwrap_or_default()).map_err(|_| Failure::Invalid)?;
    if source.trim().is_empty() {
        return Ok(Value::Object(Map::new()));
    }
    let value: Value = match path.extension().and_then(|p| p.to_str()) {
        Some("toml") => toml_edit::de::from_str(source).map_err(|_| Failure::Invalid)?,
        Some("yaml" | "yml") => serde_yaml::from_str(source).map_err(|_| Failure::Invalid)?,
        _ => json5::from_str(source).map_err(|_| Failure::Invalid)?,
    };
    if !value.is_object() {
        return Err(Failure::Invalid);
    }
    Ok(value)
}

// Three-way merge: before = original values, after = values we wrote.
// No change by us => leave current alone. A conflicting owned leaf => preserve
// the user's newer value. Arrays are atomic, except keyed Claude profile lists.
fn undo(
    before: Option<&Value>,
    after: Option<&Value>,
    current: Option<&Value>,
    preserved: &mut bool,
) -> Option<Value> {
    if before == after {
        *preserved |= current != after;
        return current.cloned();
    }
    if current == before {
        return current.cloned();
    }
    if current == after {
        return before.cloned();
    }
    if after.is_some_and(Value::is_object)
        && current.is_some_and(Value::is_object)
        && before.is_none_or(Value::is_object)
    {
        let empty = Map::new();
        let base = before.and_then(Value::as_object).unwrap_or(&empty);
        let ours = after.and_then(Value::as_object).unwrap_or(&empty);
        let mut result = current
            .and_then(Value::as_object)
            .cloned()
            .unwrap_or_default();
        *preserved |= result
            .keys()
            .any(|key| !base.contains_key(key) && !ours.contains_key(key));
        for key in base.keys().chain(ours.keys()).collect::<BTreeSet<_>>() {
            match undo(base.get(key), ours.get(key), result.get(key), preserved) {
                Some(value) => {
                    result.insert(key.clone(), value);
                }
                None => {
                    result.remove(key);
                }
            }
        }
        return if result.is_empty() && before.is_none() {
            None
        } else {
            Some(Value::Object(result))
        };
    }
    // Claude's profile picker may acquire entries from another tool after setup.
    let empty_array = Vec::new();
    let base_array = before
        .and_then(Value::as_array)
        .or_else(|| before.is_none().then_some(&empty_array));
    if let (Some(base), Some(Value::Array(ours)), Some(Value::Array(now))) =
        (base_array, after, current)
    {
        if base
            .iter()
            .chain(ours)
            .chain(now)
            .all(|v| v.get("id").and_then(Value::as_str).is_some())
        {
            let mut result = now.clone();
            *preserved |= now.iter().any(|v| !ours.iter().any(|a| a["id"] == v["id"]));
            let ids = base
                .iter()
                .chain(ours)
                .filter_map(|v| v["id"].as_str())
                .collect::<BTreeSet<_>>();
            for id in ids {
                let b = base.iter().find(|v| v["id"] == id);
                let a = ours.iter().find(|v| v["id"] == id);
                let n = now.iter().find(|v| v["id"] == id);
                let restored = undo(b, a, n, preserved);
                if let Some(index) = result.iter().position(|v| v["id"] == id) {
                    if let Some(value) = restored {
                        result[index] = value;
                    } else {
                        result.remove(index);
                    }
                } else if let Some(value) = restored {
                    result.push(value);
                }
            }
            return Some(Value::Array(result));
        }
    }
    *preserved = true;
    current.cloned()
}

fn toml_patch(
    table: &mut dyn toml_edit::TableLike,
    before: &Map<String, Value>,
    after: &Map<String, Value>,
) -> Result<()> {
    for key in before.keys().chain(after.keys()).collect::<BTreeSet<_>>() {
        if before.get(key) == after.get(key) {
            continue;
        }
        match after.get(key) {
            None => {
                table.remove(key);
            }
            Some(Value::Object(next))
                if table
                    .get(key)
                    .and_then(toml_edit::Item::as_table_like)
                    .is_some()
                    && before.get(key).is_some_and(Value::is_object) =>
            {
                toml_patch(
                    table
                        .get_mut(key)
                        .and_then(toml_edit::Item::as_table_like_mut)
                        .ok_or(Failure::Invalid)?,
                    before[key].as_object().ok_or(Failure::Invalid)?,
                    next,
                )?;
            }
            Some(value) => {
                let mut wrapper = Map::new();
                wrapper.insert(key.clone(), value.clone());
                let mut parsed =
                    toml_edit::ser::to_document(&wrapper).map_err(|_| Failure::Invalid)?;
                let mut replacement = parsed.as_table_mut().remove(key).ok_or(Failure::Invalid)?;
                if let Some(existing) = table.get_mut(key) {
                    if let (Some(old), Some(next)) =
                        (existing.as_value(), replacement.as_value_mut())
                    {
                        *next.decor_mut() = old.decor().clone();
                    }
                    *existing = replacement;
                } else {
                    table.insert(key, replacement);
                }
            }
        }
    }
    Ok(())
}

fn serialize(path: &Path, current: Option<&[u8]>, value: &Value) -> Result<Vec<u8>> {
    match path.extension().and_then(|p| p.to_str()) {
        Some("toml") => {
            let source =
                std::str::from_utf8(current.unwrap_or_default()).map_err(|_| Failure::Invalid)?;
            let mut doc = source
                .parse::<toml_edit::DocumentMut>()
                .map_err(|_| Failure::Invalid)?;
            let old = document(path, current)?;
            toml_patch(
                doc.as_table_mut(),
                old.as_object().ok_or(Failure::Invalid)?,
                value.as_object().ok_or(Failure::Invalid)?,
            )?;
            Ok(doc.to_string().into_bytes())
        }
        Some("yaml" | "yml") => serde_yaml::to_string(value)
            .map(String::into_bytes)
            .map_err(|_| Failure::Invalid),
        _ => serde_json::to_vec_pretty(value).map_err(|_| Failure::Invalid),
    }
}

pub(crate) fn restore_bytes(
    file: &FileChange,
    current: Option<&[u8]>,
) -> Result<(Option<Vec<u8>>, bool)> {
    if current == file.before.as_deref() {
        return Ok((current.map(Vec::from), false));
    }
    if current == Some(file.after.as_slice()) {
        return Ok((file.before.clone(), false));
    }
    if current.is_none() {
        return Ok((None, true));
    } // Respect a user's deletion.
    let before = document(&file.path, file.before.as_deref())?;
    let after = document(&file.path, Some(&file.after))?;
    let now = document(&file.path, current)?;
    let mut preserved = false;
    let restored =
        undo(Some(&before), Some(&after), Some(&now), &mut preserved).ok_or(Failure::Invalid)?;
    if restored == now {
        return Ok((current.map(Vec::from), preserved));
    }
    if file.before.is_none() && restored.as_object().is_some_and(Map::is_empty) {
        return Ok((None, preserved));
    }
    Ok((Some(serialize(&file.path, current, &restored)?), preserved))
}

pub(crate) fn restore_files(record: &Record) -> Result<bool> {
    restore_files_inner(record, false)
}

// Restoring an interrupted switch is not the user's explicit "disconnect and
// restore original settings" action. Never remove the preceding good setup.
pub(crate) fn restore_attempt(record: &Record) -> Result<bool> {
    let checkpoint = record
        .rollback_checkpoint
        .as_ref()
        .ok_or(Failure::Invalid)?;
    let mut attempt = record.clone();
    attempt.files = checkpoint.files.clone();
    attempt.previous = None;
    attempt.rollback_checkpoint = None;
    attempt.pending = false;
    restore_files_inner(&attempt, true)
}

fn owned_values_restored(
    before: Option<&Value>,
    after: Option<&Value>,
    restored: Option<&Value>,
) -> bool {
    if before == after || before == restored {
        return true;
    }
    let empty = Map::new();
    fn object(value: Option<&Value>) -> Option<&Map<String, Value>> {
        value.and_then(Value::as_object)
    }
    if let (Some(before), Some(after), Some(restored)) = (
        object(before).or_else(|| before.is_none().then_some(&empty)),
        object(after).or_else(|| after.is_none().then_some(&empty)),
        object(restored).or_else(|| restored.is_none().then_some(&empty)),
    ) {
        return before
            .keys()
            .chain(after.keys())
            .all(|key| owned_values_restored(before.get(key), after.get(key), restored.get(key)));
    }
    // Match the keyed-array merge used by `undo`: a third-party Claude
    // profile added during startup is not an owned-field recovery conflict.
    let empty_array = Vec::new();
    fn array(value: Option<&Value>) -> Option<&Vec<Value>> {
        value.and_then(Value::as_array)
    }
    if let (Some(before), Some(after), Some(restored)) = (
        array(before).or_else(|| before.is_none().then_some(&empty_array)),
        array(after).or_else(|| after.is_none().then_some(&empty_array)),
        array(restored).or_else(|| restored.is_none().then_some(&empty_array)),
    ) {
        let unique_ids = |values: &[Value]| {
            let ids = values
                .iter()
                .filter_map(|v| v.get("id").and_then(Value::as_str))
                .collect::<BTreeSet<_>>();
            ids.len() == values.len()
        };
        if [before, after, restored]
            .iter()
            .all(|values| unique_ids(values))
        {
            return before.iter().chain(after).all(|entry| {
                let id = &entry["id"];
                owned_values_restored(
                    before.iter().find(|value| &value["id"] == id),
                    after.iter().find(|value| &value["id"] == id),
                    restored.iter().find(|value| &value["id"] == id),
                )
            });
        }
    }
    false
}

fn restore_files_inner(record: &Record, require_owned_restored: bool) -> Result<bool> {
    // Plan every file before changing any. An invalid document must not cause
    // half of an otherwise valid restoration to be applied.
    let mut plan = Vec::new();
    let mut preserved = false;
    for file in &record.files {
        let current = common::snapshot(&file.path).map_err(|_| Failure::Io)?;
        let (mut desired, kept) = restore_bytes(file, current.as_deref())?;
        preserved |= kept;
        // If a switch crashed before writing this file it may still contain
        // the previous receipt, not the pending receipt.
        if record.pending {
            if let Some(previous) = record
                .previous
                .as_ref()
                .and_then(|p| p.files.iter().find(|f| f.path == file.path))
            {
                let (previous_desired, kept) = restore_bytes(previous, desired.as_deref())?;
                desired = previous_desired;
                preserved |= kept;
            }
        }
        if require_owned_restored
            && !owned_values_restored(
                Some(&document(&file.path, file.before.as_deref())?),
                Some(&document(&file.path, Some(&file.after))?),
                Some(&document(&file.path, desired.as_deref())?),
            )
        {
            return Err(Failure::Changed);
        }
        plan.push((file.path.clone(), current, desired));
    }
    for (path, current, desired) in plan {
        if common::snapshot(&path).map_err(|_| Failure::Io)? != current {
            return Err(Failure::Changed);
        }
        common::restore(&path, desired.as_deref()).map_err(|_| Failure::Io)?;
        if common::snapshot(&path).map_err(|_| Failure::Io)? != desired {
            return Err(Failure::Io);
        }
    }
    Ok(preserved)
}

pub(crate) fn configuration_matches(record: &Record) -> bool {
    record.files.iter().all(|file| {
        common::snapshot(&file.path).ok().flatten().as_deref() == Some(file.after.as_slice())
    })
}

/// Detects a receipt written by a release that routed the tool through the
/// local gateway. Those files still match their receipt byte for byte, but the
/// gateway no longer exists, so the connection has to be re-applied against the
/// relay origin. Claude Desktop keeps its loopback gateway and is exempt.
pub(crate) fn requires_gateway_migration(tool: &str, record: &Record) -> bool {
    let needle = match tool {
        "codex_desktop" => "127.0.0.1:15722",
        "claude_code" => "127.0.0.1:15728",
        "pi" | "hermes" | "openclaw" | "dsh_web" => "127.0.0.1:15730",
        _ => return false,
    };
    record
        .files
        .iter()
        .any(|file| std::str::from_utf8(&file.after).is_ok_and(|text| text.contains(needle)))
}

fn remove_at(root: &mut Value, path: &[&str]) {
    if let Some((last, parents)) = path.split_last() {
        let mut node = root;
        for key in parents {
            let Some(next) = node.get_mut(*key) else {
                return;
            };
            node = next;
        }
        if let Some(object) = node.as_object_mut() {
            object.remove(*last);
        }
    }
}

/// A legacy cleanup never invents previous models, credentials or providers.
/// Only exact adapter-owned YesChoy identities are eligible for removal.
pub(crate) fn legacy_clean(
    tool: &str,
    path: &Path,
    bytes: Option<&[u8]>,
    credential: &ToolCredential,
    owns_claude_mode: bool,
) -> Result<Option<Vec<u8>>> {
    let Some(bytes) = bytes else {
        return Ok(None);
    };
    let mut value = document(path, Some(bytes))?;
    let original = value.clone();
    let origin_v1 = format!("{}/v1", credential.origin);
    let helper_matches = |value: &Value| {
        value
            .as_str()
            .is_some_and(|s| s.ends_with(&format!("credential-helper {tool}")))
    };
    match tool {
        "codex_desktop" => {
            // The old catalog has no authenticated ownership receipt. Preserve
            // it, but remove its reference; an orphan file is safer than data loss.
            if path
                .file_name()
                .is_some_and(|s| s == "yeschoy-model-catalog.json")
            {
                return Ok(Some(bytes.to_vec()));
            }
            if value["model_provider"] == "yeschoy" {
                remove_at(&mut value, &["model_provider"]);
                if value["model"] == credential.model_id {
                    remove_at(&mut value, &["model"]);
                }
            }
            if value["model_catalog_json"] == "yeschoy-model-catalog.json" {
                remove_at(&mut value, &["model_catalog_json"]);
            }
            let provider = &value["model_providers"]["yeschoy"];
            if (provider["base_url"] == origin_v1
                || provider["base_url"] == crate::codex_bridge::BASE_URL)
                && provider["auth"]["args"]
                    == serde_json::json!(["credential-helper", "codex_desktop"])
            {
                remove_at(&mut value, &["model_providers", "yeschoy"]);
            }
        }
        "claude_code" => {
            let owned = helper_matches(&value["apiKeyHelper"]);
            if owned {
                remove_at(&mut value, &["apiKeyHelper"]);
                for key in [
                    "ANTHROPIC_BASE_URL",
                    "ANTHROPIC_MODEL",
                    "ANTHROPIC_DEFAULT_HAIKU_MODEL",
                    "ANTHROPIC_DEFAULT_SONNET_MODEL",
                    "ANTHROPIC_DEFAULT_OPUS_MODEL",
                ] {
                    let current = &value["env"][key];
                    let matches = if key == "ANTHROPIC_BASE_URL" {
                        current == &credential.origin
                            || current == "http://127.0.0.1:15728/claude-code"
                    } else {
                        current == &credential.model_id
                    };
                    if matches {
                        remove_at(&mut value, &["env", key]);
                    }
                }
            }
        }
        "claude_desktop" => {
            const PROFILE: &str = "00000000-0000-4000-8000-000000157220";
            if path.file_stem().is_some_and(|s| s == PROFILE)
                && value["inferenceGatewayBaseUrl"] == "http://127.0.0.1:15729/claude-desktop"
            {
                return Ok(None);
            }
            if owns_claude_mode && value["deploymentMode"] == "3p" {
                remove_at(&mut value, &["deploymentMode"]);
            }
            if value["appliedId"] == PROFILE {
                remove_at(&mut value, &["appliedId"]);
            }
            if let Some(entries) = value.get_mut("entries").and_then(Value::as_array_mut) {
                entries.retain(|v| v["id"] != PROFILE);
            }
        }
        "pi" => {
            let provider = &value["providers"]["yeschoy"];
            if provider["baseUrl"] == origin_v1 && helper_matches(&provider["apiKey"]) {
                remove_at(&mut value, &["providers", "yeschoy"]);
            }
            if value["defaultProvider"] == "yeschoy" {
                remove_at(&mut value, &["defaultProvider"]);
                if value["defaultModel"] == credential.model_id {
                    remove_at(&mut value, &["defaultModel"]);
                }
            }
        }
        "dsh_web" => {
            let provider = &value["llm-pi-ai"]["providers"]["yeschoy"];
            if provider["baseURL"] == origin_v1 && provider["apiKeyEnv"] == "YESCHOY_DSH_API_KEY" {
                remove_at(&mut value, &["llm-pi-ai", "providers", "yeschoy"]);
            }
            if value["agent-default-model"]["provider"] == "yeschoy" {
                remove_at(&mut value, &["agent-default-model", "provider"]);
                if value["agent-default-model"]["model"] == credential.model_id {
                    remove_at(&mut value, &["agent-default-model", "model"]);
                }
            }
        }
        "hermes" => {
            let provider = &value["providers"]["yeschoy"];
            if provider["api"] == origin_v1 && helper_matches(&provider["key_cmd"]) {
                remove_at(&mut value, &["providers", "yeschoy"]);
            }
            if value["model"]["provider"] == "custom:yeschoy" {
                remove_at(&mut value, &["model", "provider"]);
                if value["model"]["default"] == credential.model_id {
                    remove_at(&mut value, &["model", "default"]);
                }
            }
        }
        "openclaw" => {
            let provider = &value["models"]["providers"]["yeschoy"];
            if provider["baseUrl"] == origin_v1
                && provider["apiKey"]["provider"] == "yeschoy-keychain"
            {
                remove_at(&mut value, &["models", "providers", "yeschoy"]);
            }
            if value["secrets"]["providers"]["yeschoy-keychain"]["args"]
                == serde_json::json!(["credential-helper-openclaw", "openclaw"])
            {
                remove_at(&mut value, &["secrets", "providers", "yeschoy-keychain"]);
            }
            if value["agents"]["defaults"]["model"]["primary"]
                .as_str()
                .is_some_and(|s| s.starts_with("yeschoy/"))
            {
                remove_at(&mut value, &["agents", "defaults", "model", "primary"]);
            }
            if let Some(catalog) = value
                .pointer_mut("/agents/defaults/models")
                .and_then(Value::as_object_mut)
            {
                catalog.retain(|key, _| !key.starts_with("yeschoy/"));
            }
        }
        _ => return Err(Failure::Invalid),
    }
    if value == original {
        Ok(Some(bytes.to_vec()))
    } else {
        serialize(path, Some(bytes), &value).map(Some)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    struct Fixture(Store);
    impl Fixture {
        fn new() -> Self {
            Self(Store {
                root: common::temporary_working_directory("recovery-test").unwrap(),
                key: [42; 32],
            })
        }
        fn path(&self, name: &str) -> PathBuf {
            self.0.root.join(name)
        }
        fn change(&self, name: &str, before: Option<&[u8]>, after: &[u8]) -> FileChange {
            common::FileTransaction::stage_with_snapshot(
                self.path(name),
                before.map(Vec::from),
                after.to_vec(),
            )
            .unwrap()
            .changes()[0]
                .clone()
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0.root);
        }
    }
    fn receipt(tool: &str) -> Receipt {
        Receipt {
            tool_id: tool.into(),
            model_id: "model-new".into(),
            line_id: "mainland_optimized".into(),
            billing_group: "default".into(),
            updated_at_epoch_ms: 1,
            requires_background: false,
        }
    }
    fn credential() -> ToolCredential {
        ToolCredential {
            api_key: "synthetic-never-a-real-key".into(),
            origin: "https://yeschoy.com".into(),
            model_id: "model-new".into(),
            local_gateway_token: None,
            codex_transport: None,
            claude_transport: None,
            models: vec![],
        }
    }
    fn merged(file: &FileChange, now: Value) -> (Value, bool) {
        let (bytes, kept) = restore_bytes(file, Some(&serde_json::to_vec(&now).unwrap())).unwrap();
        (document(&file.path, bytes.as_deref()).unwrap(), kept)
    }
    #[test]
    fn legacy_lock_only_store_does_not_require_recovery_key() {
        let f = Fixture::new();
        let root = f.path("legacy");
        assert_eq!(recovery_store_is_uninitialized(&root), Ok(true));
        assert!(!root.exists());
        std::fs::create_dir(&root).unwrap();
        assert_eq!(recovery_store_is_uninitialized(&root), Ok(true));
        let _guard = operation_lock_at(&root).unwrap();
        assert_eq!(recovery_store_is_uninitialized(&root), Ok(true));
        assert_eq!(std::fs::read_dir(&root).unwrap().count(), 1);
        assert!(root.join("operation.lock").is_file());
    }

    #[test]
    fn missing_recovery_key_never_discards_existing_or_unknown_material() {
        for name in ["pi.sealed", "pi.sealed.tmp", "unexpected.json"] {
            let f = Fixture::new();
            let bytes = b"synthetic-existing-recovery-material";
            std::fs::write(f.path(name), bytes).unwrap();
            assert_eq!(recovery_store_is_uninitialized(&f.0.root), Ok(false));
            assert_eq!(std::fs::read(f.path(name)).unwrap(), bytes);
        }
        let f = Fixture::new();
        std::fs::create_dir(f.path("unexpected-directory")).unwrap();
        assert_eq!(recovery_store_is_uninitialized(&f.0.root), Ok(false));
        let f = Fixture::new();
        std::fs::create_dir(f.path("operation.lock")).unwrap();
        assert_eq!(
            recovery_store_is_uninitialized(&f.0.root),
            Err(Failure::Storage)
        );
    }

    #[test]
    fn missing_recovery_key_requires_a_bounded_regular_operation_lock() {
        let f = Fixture::new();
        let lock = f.path("operation.lock");
        std::fs::write(&lock, [0u8; 64]).unwrap();
        assert_eq!(recovery_store_is_uninitialized(&f.0.root), Ok(true));
        let oversized = [0u8; 65];
        std::fs::write(&lock, oversized).unwrap();
        assert_eq!(
            recovery_store_is_uninitialized(&f.0.root),
            Err(Failure::Storage)
        );
        assert_eq!(std::fs::read(lock).unwrap(), oversized);
    }

    #[cfg(unix)]
    #[test]
    fn missing_recovery_key_rejects_symlinked_store_and_lock() {
        let f = Fixture::new();
        let target = f.path("real");
        std::fs::create_dir(&target).unwrap();
        std::os::unix::fs::symlink(&target, f.path("linked")).unwrap();
        assert_eq!(
            recovery_store_is_uninitialized(&f.path("linked")),
            Err(Failure::Storage)
        );
        let bytes = b"synthetic-untouched-target";
        std::fs::write(f.path("target"), bytes).unwrap();
        std::os::unix::fs::symlink(f.path("target"), target.join("operation.lock")).unwrap();
        assert_eq!(
            recovery_store_is_uninitialized(&target),
            Err(Failure::Storage)
        );
        assert_eq!(std::fs::read(f.path("target")).unwrap(), bytes);
    }

    #[test]
    fn encrypted_baseline_authenticates_tool_key_and_bytes() {
        let f = Fixture::new();
        let file = f.change(
            "settings.json",
            Some(br#"{"secret":"synthetic-original-secret"}"#),
            br#"{"model":"new"}"#,
        );
        let record = f.0.begin(receipt("pi"), &[file], None).unwrap();
        let bytes = std::fs::read(f.0.path("pi").unwrap()).unwrap();
        assert!(!String::from_utf8_lossy(&bytes).contains("synthetic-original-secret"));
        assert!(decode(&[41; 32], "pi", &bytes).is_err());
        assert!(decode(&f.0.key, "hermes", &bytes).is_err());
        let mut changed = bytes;
        *changed.last_mut().unwrap() ^= 1;
        assert!(decode(&f.0.key, "pi", &changed).is_err());
        assert!(f.0.load("pi").unwrap().unwrap().pending);
        f.0.abandon(&record).unwrap();
        assert!(f.0.load("pi").unwrap().is_none());
    }
    #[test]
    fn restores_exact_original_bytes_for_all_seven_tools() {
        for tool in [
            "claude_code",
            "claude_desktop",
            "codex_desktop",
            "pi",
            "dsh_web",
            "hermes",
            "openclaw",
        ] {
            let f = Fixture::new();
            let before = b"{\n  \"old\": true\n}\n";
            let file = f.change(
                "config.json",
                Some(before),
                br#"{"old":true,"model":"new"}"#,
            );
            let mut record =
                f.0.begin(receipt(tool), std::slice::from_ref(&file), None)
                    .unwrap();
            common::atomic_write(&file.path, &file.after).unwrap();
            f.0.finish(&mut record).unwrap();
            assert!(configuration_matches(&record));
            assert!(!restore_files(&record).unwrap());
            assert_eq!(std::fs::read(&file.path).unwrap(), before);
            assert!(!restore_files(&record).unwrap()); // retry is harmless
        }
    }
    #[test]
    fn desktop_failed_switch_recovers_previous_connection_not_factory_settings() {
        for tool in ["codex_desktop", "claude_desktop"] {
            let f = Fixture::new();
            let first = f.change(
                "desktop.json",
                Some(br#"{"model":"factory"}"#),
                br#"{"model":"a"}"#,
            );
            let mut ready =
                f.0.begin(receipt(tool), std::slice::from_ref(&first), None)
                    .unwrap();
            common::atomic_write(&first.path, &first.after).unwrap();
            f.0.finish(&mut ready).unwrap();
            let second = f.change("desktop.json", Some(&first.after), br#"{"model":"b"}"#);
            f.0.begin(receipt(tool), std::slice::from_ref(&second), None)
                .unwrap();
            common::atomic_write(&second.path, &second.after).unwrap();
            assert_eq!(f.0.recover_pending(tool, || true), Ok(true));
            assert_eq!(common::snapshot(&second.path).unwrap(), Some(first.after));
            assert!(!f.0.load(tool).unwrap().unwrap().pending);
            assert_eq!(
                f.0.recover_pending(tool, || panic!("must not replay completed recovery")),
                Ok(false)
            );
        }
    }
    #[test]
    fn desktop_checkpoint_keeps_credentials_encrypted_until_recovery_finishes() {
        let f = Fixture::new();
        let previous = credential();
        let file = f.change(
            "desktop.json",
            Some(br#"{"model":"a"}"#),
            br#"{"model":"b"}"#,
        );
        let mut pending =
            f.0.begin(
                receipt("codex_desktop"),
                std::slice::from_ref(&file),
                Some(&previous),
            )
            .unwrap();
        let bytes = std::fs::read(f.0.path("codex_desktop").unwrap()).unwrap();
        assert!(!String::from_utf8_lossy(&bytes).contains(&previous.api_key));
        assert_eq!(
            f.0.load("codex_desktop")
                .unwrap()
                .unwrap()
                .rollback_credential(),
            Some(&previous)
        );
        common::atomic_write(&file.path, &file.after).unwrap();
        assert_eq!(
            f.0.recover_pending("codex_desktop", || false),
            Err(PendingRecoveryFailure::Credential)
        );
        assert!(f.0.load("codex_desktop").unwrap().unwrap().pending);
        assert_eq!(common::snapshot(&file.path).unwrap(), file.before);
        assert_eq!(f.0.recover_pending("codex_desktop", || true), Ok(true));
        assert!(f.0.load("codex_desktop").unwrap().is_none());

        f.0.finish(&mut pending).unwrap();
        assert!(f
            .0
            .load("codex_desktop")
            .unwrap()
            .unwrap()
            .rollback_checkpoint
            .is_none());
    }

    #[test]
    fn desktop_rollback_preserves_unrelated_edits_but_stops_on_owned_conflicts() {
        let f = Fixture::new();
        let file = f.change(
            "desktop.json",
            Some(br#"{"model":"a","theme":"dark"}"#),
            br#"{"model":"b","theme":"dark"}"#,
        );
        f.0.begin(receipt("claude_desktop"), std::slice::from_ref(&file), None)
            .unwrap();
        common::atomic_write(&file.path, br#"{"model":"user-choice","theme":"light"}"#).unwrap();
        assert_eq!(
            f.0.recover_pending("claude_desktop", || panic!(
                "must not replace credential before files recover"
            )),
            Err(PendingRecoveryFailure::Files)
        );
        assert!(f.0.load("claude_desktop").unwrap().unwrap().pending);
        common::atomic_write(&file.path, br#"{"model":"b","theme":"light"}"#).unwrap();
        assert_eq!(f.0.recover_pending("claude_desktop", || true), Ok(true));
        assert_eq!(
            document(&file.path, common::snapshot(&file.path).unwrap().as_deref()).unwrap(),
            json!({"model":"a","theme":"light"})
        );
    }

    #[test]
    fn old_desktop_pending_journals_remain_readable_without_a_checkpoint() {
        let f = Fixture::new();
        let file = f.change(
            "desktop.json",
            Some(br#"{"model":"factory"}"#),
            br#"{"model":"b"}"#,
        );
        let mut legacy =
            f.0.begin(receipt("claude_desktop"), std::slice::from_ref(&file), None)
                .unwrap();
        legacy.rollback_checkpoint = None;
        f.0.save(&legacy).unwrap();
        common::atomic_write(&file.path, &file.after).unwrap();
        assert!(f
            .0
            .load("claude_desktop")
            .unwrap()
            .unwrap()
            .rollback_credential()
            .is_none());
        assert_eq!(f.0.recover_pending("claude_desktop", || true), Ok(true));
        assert_eq!(common::snapshot(&file.path).unwrap(), file.before);
    }

    #[test]
    fn desktop_partial_commit_recovers_previous_model_and_retains_original_baseline() {
        for tool in ["codex_desktop", "claude_desktop"] {
            for written_count in 0..=2 {
                let f = Fixture::new();
                let first = [
                    f.change(
                        "a.json",
                        Some(br#"{"model":"factory"}"#),
                        br#"{"model":"a"}"#,
                    ),
                    f.change("b.json", Some(b"{}"), br#"{"model":"a"}"#),
                ];
                let mut original = f.0.begin(receipt(tool), &first, None).unwrap();
                for file in &first {
                    common::atomic_write(&file.path, &file.after).unwrap();
                }
                f.0.finish(&mut original).unwrap();
                let next = [
                    f.change("a.json", Some(&first[0].after), br#"{"model":"b"}"#),
                    f.change("b.json", Some(&first[1].after), br#"{"model":"b"}"#),
                ];
                f.0.begin(receipt(tool), &next, Some(&credential()))
                    .unwrap();
                for file in next.iter().take(written_count) {
                    common::atomic_write(&file.path, &file.after).unwrap();
                }
                assert_eq!(f.0.recover_pending(tool, || true), Ok(true));
                for file in &first {
                    assert_eq!(
                        common::snapshot(&file.path).unwrap(),
                        Some(file.after.clone())
                    );
                }
                let previous = f.0.load(tool).unwrap().unwrap();
                assert!(!previous.pending);
                restore_files(&previous).unwrap();
                for file in &first {
                    assert_eq!(common::snapshot(&file.path).unwrap(), file.before);
                }
            }
        }
    }

    #[test]
    fn desktop_rollback_does_not_block_on_unrelated_claude_profile_entries() {
        let f = Fixture::new();
        let file = f.change(
            "meta.json",
            Some(br#"{"entries":[{"id":"ours","model":"a"}]}"#),
            br#"{"entries":[{"id":"ours","model":"b"}]}"#,
        );
        let pending =
            f.0.begin(receipt("claude_desktop"), std::slice::from_ref(&file), None)
                .unwrap();
        common::atomic_write(
            &file.path,
            br#"{"entries":[{"id":"foreign"},{"id":"ours","model":"b"}]}"#,
        )
        .unwrap();
        assert!(restore_attempt(&pending).is_ok());
        assert_eq!(
            document(&file.path, common::snapshot(&file.path).unwrap().as_deref()).unwrap(),
            json!({"entries":[{"id":"foreign"},{"id":"ours","model":"a"}]})
        );
        common::atomic_write(
            &file.path,
            br#"{"entries":[{"id":"foreign"},{"id":"ours","model":"user-choice"}]}"#,
        )
        .unwrap();
        assert_eq!(restore_attempt(&pending), Err(Failure::Changed));
    }
    #[test]
    fn switch_keeps_first_baseline_and_rebases_later_unrelated_changes() {
        let f = Fixture::new();
        let first = f.change(
            "settings.json",
            Some(br#"{"model":"old","theme":"dark"}"#),
            br#"{"model":"a","theme":"dark"}"#,
        );
        let mut record = f.0.begin(receipt("pi"), &[first], None).unwrap();
        f.0.finish(&mut record).unwrap();
        let second = f.change(
            "settings.json",
            Some(br#"{"model":"a","theme":"light"}"#),
            br#"{"model":"b","theme":"light"}"#,
        );
        let mut record =
            f.0.begin(receipt("pi"), std::slice::from_ref(&second), None)
                .unwrap();
        common::atomic_write(&second.path, &second.after).unwrap();
        f.0.finish(&mut record).unwrap();
        restore_files(&record).unwrap();
        assert_eq!(
            document(
                &second.path,
                common::snapshot(&second.path).unwrap().as_deref()
            )
            .unwrap(),
            json!({"model":"old","theme":"light"})
        );
    }
    #[test]
    fn preserves_owned_conflicts_unrelated_changes_and_profile_order() {
        let f = Fixture::new();
        let file = f.change(
            "settings.json",
            Some(br#"{"model":"old","theme":"dark"}"#),
            br#"{"model":"new","theme":"dark"}"#,
        );
        let (value, kept) = merged(
            &file,
            json!({"model":"new","theme":"light","userAdded":true}),
        );
        assert!(kept);
        assert_eq!(
            value,
            json!({"model":"old","theme":"light","userAdded":true})
        );
        let (value, kept) = merged(&file, json!({"model":"user-choice","theme":"dark"}));
        assert!(kept);
        assert_eq!(value["model"], "user-choice");
        let file = f.change(
            "meta.json",
            Some(br#"{"entries":[{"id":"a"},{"id":"b"}]}"#),
            br#"{"entries":[{"id":"a"},{"id":"b"},{"id":"yeschoy"}]}"#,
        );
        let (value, kept) = merged(
            &file,
            json!({"entries":[{"id":"a"},{"id":"b"},{"id":"yeschoy"},{"id":"c"}]}),
        );
        assert!(kept);
        assert_eq!(value, json!({"entries":[{"id":"a"},{"id":"b"},{"id":"c"}]}));
    }
    #[test]
    fn newly_created_file_removed_only_when_no_user_content_remains() {
        let f = Fixture::new();
        let file = f.change("settings.json", None, br#"{"model":"new"}"#);
        assert_eq!(
            restore_bytes(&file, Some(&file.after)).unwrap(),
            (None, false)
        );
        let (value, kept) = merged(&file, json!({"model":"new","user":true}));
        assert!(kept);
        assert_eq!(value, json!({"user":true}));
        assert_eq!(restore_bytes(&file, None).unwrap().0, None);
    }
    #[test]
    fn handles_toml_comments_and_yaml_and_rejects_invalid_document_before_any_write() {
        let f = Fixture::new();
        let file = f.change("config.toml", Some(b"model = 'old'\n"), b"model = 'new'\n");
        let (bytes, kept) =
            restore_bytes(&file, Some(b"# my comment\nmodel = 'new'\nuser = true\n")).unwrap();
        let text = String::from_utf8(bytes.unwrap()).unwrap();
        assert!(kept);
        assert!(text.contains("# my comment"));
        assert!(text.contains("user = true"));
        let yaml = f.change("config.yaml", Some(b"model: old\n"), b"model: new\n");
        let (bytes, kept) = restore_bytes(&yaml, Some(b"model: new\nuser: true\n")).unwrap();
        assert!(kept);
        assert_eq!(
            document(&yaml.path, bytes.as_deref()).unwrap(),
            json!({"model":"old","user":true})
        );
        let bad = f.change("bad.json", Some(b"{}"), br#"{"model":"new"}"#);
        let record =
            f.0.begin(receipt("pi"), &[file.clone(), bad.clone()], None)
                .unwrap();
        common::atomic_write(&file.path, &file.after).unwrap();
        common::atomic_write(&bad.path, b"invalid {").unwrap();
        assert_eq!(restore_files(&record), Err(Failure::Invalid));
        assert_eq!(std::fs::read(&file.path).unwrap(), file.after);
    }
    #[test]
    fn pending_switch_recovers_files_written_before_or_after_crash() {
        let f = Fixture::new();
        let first = [
            f.change("a.json", Some(br#"{"model":"old"}"#), br#"{"model":"a"}"#),
            f.change("b.json", Some(b"{}"), br#"{"model":"a"}"#),
        ];
        let mut original = f.0.begin(receipt("pi"), &first, None).unwrap();
        f.0.finish(&mut original).unwrap();
        let next = [
            f.change("a.json", Some(&first[0].after), br#"{"model":"b"}"#),
            f.change("b.json", Some(&first[1].after), br#"{"model":"b"}"#),
        ];
        let pending = f.0.begin(receipt("pi"), &next, None).unwrap();
        common::atomic_write(&next[0].path, &next[0].after).unwrap();
        common::atomic_write(&next[1].path, &first[1].after).unwrap();
        assert!(f.0.begin(receipt("pi"), &next, None).is_err());
        restore_files(&pending).unwrap();
        restore_files(&pending).unwrap();
        for file in first {
            assert_eq!(common::snapshot(&file.path).unwrap(), file.before);
        }
    }
    #[test]
    fn ru054_pending_retry_recovers_once_and_preserves_completed_records() {
        use std::cell::Cell;

        let f = Fixture::new();
        let file = f.change(
            "retry.json",
            Some(br#"{"model":"old","theme":"dark"}"#),
            br#"{"model":"new","theme":"dark"}"#,
        );
        let pending =
            f.0.begin(receipt("pi"), std::slice::from_ref(&file), None)
                .unwrap();
        common::atomic_write(
            &file.path,
            br#"{"model":"new","theme":"light","later":true}"#,
        )
        .unwrap();
        assert!(pending.pending);
        let clears = Cell::new(0);
        assert_eq!(
            f.0.recover_pending("pi", || {
                clears.set(clears.get() + 1);
                true
            }),
            Ok(true)
        );
        assert_eq!(clears.get(), 1);
        assert!(f.0.load("pi").unwrap().is_none());
        assert_eq!(
            document(&file.path, common::snapshot(&file.path).unwrap().as_deref()).unwrap(),
            json!({"model":"old","theme":"light","later":true})
        );
        assert_eq!(f.0.recover_pending("pi", || false), Ok(false));

        let completed_file = f.change(
            "completed.json",
            Some(br#"{"model":"old"}"#),
            br#"{"model":"ready"}"#,
        );
        let mut completed =
            f.0.begin(
                receipt("claude_code"),
                std::slice::from_ref(&completed_file),
                None,
            )
            .unwrap();
        common::atomic_write(&completed_file.path, &completed_file.after).unwrap();
        f.0.finish(&mut completed).unwrap();
        assert_eq!(
            f.0.recover_pending("claude_code", || panic!("completed credential cleared")),
            Ok(false)
        );
        assert_eq!(
            common::snapshot(&completed_file.path).unwrap(),
            Some(completed_file.after)
        );
        assert!(!f.0.load("claude_code").unwrap().unwrap().pending);

        let failed_file = f.change(
            "credential.json",
            Some(br#"{"model":"before"}"#),
            br#"{"model":"during"}"#,
        );
        f.0.begin(receipt("hermes"), std::slice::from_ref(&failed_file), None)
            .unwrap();
        common::atomic_write(&failed_file.path, &failed_file.after).unwrap();
        assert_eq!(
            f.0.recover_pending("hermes", || false),
            Err(PendingRecoveryFailure::Credential)
        );
        assert!(f.0.load("hermes").unwrap().unwrap().pending);
    }
    #[test]
    fn legacy_cleanup_never_removes_other_claude_mode_or_later_model_endpoint() {
        let f = Fixture::new();
        let credential = credential();
        let bytes = br#"{"deploymentMode":"3p","other":true}"#;
        assert_eq!(
            legacy_clean(
                "claude_desktop",
                &f.path("config.json"),
                Some(bytes),
                &credential,
                false
            )
            .unwrap(),
            Some(bytes.to_vec())
        );
        let bytes=br#"{"apiKeyHelper":"'/test/yeschoy' credential-helper claude_code","env":{"ANTHROPIC_BASE_URL":"https://custom.invalid","ANTHROPIC_MODEL":"user-model","ANTHROPIC_DEFAULT_HAIKU_MODEL":"model-new"}}"#;
        let value = document(
            &f.path("settings.json"),
            legacy_clean(
                "claude_code",
                &f.path("settings.json"),
                Some(bytes),
                &credential,
                false,
            )
            .unwrap()
            .as_deref(),
        )
        .unwrap();
        assert_eq!(value["env"]["ANTHROPIC_BASE_URL"], "https://custom.invalid");
        assert_eq!(value["env"]["ANTHROPIC_MODEL"], "user-model");
        assert!(value.get("apiKeyHelper").is_none());
        assert!(value["env"].get("ANTHROPIC_DEFAULT_HAIKU_MODEL").is_none());
    }
    #[test]
    fn process_lock_is_exclusive_and_released_on_drop() {
        let f = Fixture::new();
        let first = operation_lock_at(&f.0.root).unwrap();
        assert!(operation_lock_at(&f.0.root).is_err());
        drop(first);
        assert!(operation_lock_at(&f.0.root).is_ok());
    }
    #[test]
    fn read_only_snapshot_never_creates_parent_and_rejects_symlink_ancestors() {
        let f = Fixture::new();
        let path = f.path("absent/child/config.json");
        assert_eq!(common::snapshot(&path).unwrap(), None);
        assert!(!path.parent().unwrap().exists());
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(&f.0.root, f.path("alias")).unwrap();
            assert!(common::snapshot(&f.path("alias/absent/config.json")).is_err());
        }
    }
}

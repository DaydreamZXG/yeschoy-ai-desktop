//! Re-bucket only Codex's provider metadata while YesChoy owns the live config.
//!
//! Codex pins `model_provider` in both a session JSONL's `session_meta` record
//! and the `threads` table in `state_5.sqlite`. Changing config.toml alone does
//! not update an already-created thread. This sidecar transaction moves only
//! the built-in `openai` bucket to the custom provider selected by our adapter.
//! It never reads or changes message bodies, encrypted reasoning or auth.json.

use std::{
    collections::BTreeSet,
    fs,
    path::{Component, Path, PathBuf},
    time::Duration,
};

use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use toml_edit::{DocumentMut, Item};

use crate::tool_adapters::{codex_desktop, common, AdapterFailure};

const SOURCE_PROVIDER: &str = "openai";
const MANIFEST_VERSION: u8 = 1;
const MANIFEST_NAME: &str = "codex-history-takeover-v1.json";
const MAX_MANIFEST_BYTES: u64 = 32 * 1024 * 1024;
const MAX_SESSION_BYTES: u64 = 512 * 1024 * 1024;
const MAX_SESSION_META_BYTES: u64 = 2 * 1024 * 1024;
const MAX_SESSION_FILES: usize = 50_000;

#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
enum Phase {
    Pending,
    Active,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SessionEntry {
    path: PathBuf,
    session_id: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StateEntry {
    path: PathBuf,
    thread_ids: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    version: u8,
    phase: Phase,
    codex_dir: PathBuf,
    target_provider: String,
    sessions: Vec<SessionEntry>,
    state: Vec<StateEntry>,
}

pub(crate) struct Takeover {
    home: PathBuf,
    created: bool,
    apply_on_commit: bool,
}

fn failure(code: &'static str) -> AdapterFailure {
    AdapterFailure::ConfigurationFailed(code)
}

fn valid_provider(value: &str) -> bool {
    value != SOURCE_PROVIDER
        && (1..=64).contains(&value.len())
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
}

fn manifest_path(home: &Path) -> PathBuf {
    home.join(".yeschoy")
        .join("connection-recovery")
        .join(MANIFEST_NAME)
}

fn load_manifest(home: &Path) -> Result<Option<Manifest>, AdapterFailure> {
    let path = manifest_path(home);
    let bytes = common::snapshot_bounded(&path, MAX_MANIFEST_BYTES)
        .map_err(|_| failure("codex_history_takeover_recovery_failed"))?;
    let Some(bytes) = bytes else { return Ok(None) };
    let manifest: Manifest = serde_json::from_slice(&bytes)
        .map_err(|_| failure("codex_history_takeover_recovery_failed"))?;
    validate_manifest(home, &manifest)?;
    Ok(Some(manifest))
}

fn save_manifest(home: &Path, manifest: &Manifest) -> Result<(), AdapterFailure> {
    validate_manifest(home, manifest)?;
    let bytes =
        serde_json::to_vec(manifest).map_err(|_| failure("codex_history_takeover_failed"))?;
    common::atomic_write_bounded(&manifest_path(home), &bytes, MAX_MANIFEST_BYTES)
        .map_err(|_| failure("codex_history_takeover_failed"))
}

fn remove_manifest(home: &Path) -> Result<(), AdapterFailure> {
    let path = manifest_path(home);
    if path.exists() {
        fs::remove_file(path).map_err(|_| failure("codex_history_takeover_recovery_failed"))?;
    }
    Ok(())
}

fn no_parent_components(path: &Path) -> bool {
    path.is_absolute()
        && !path
            .components()
            .any(|part| matches!(part, Component::ParentDir))
}

fn session_path_allowed(codex_dir: &Path, path: &Path) -> bool {
    no_parent_components(path)
        && ["sessions", "archived_sessions"]
            .iter()
            .any(|name| path.starts_with(codex_dir.join(name)))
        && path.extension().and_then(|value| value.to_str()) == Some("jsonl")
}

fn state_path_allowed(codex_dir: &Path, path: &Path) -> bool {
    no_parent_components(path)
        && path.file_name().and_then(|value| value.to_str()) == Some("state_5.sqlite")
        && (path.starts_with(codex_dir) || path.parent().is_some_and(Path::is_absolute))
}

fn validate_manifest(home: &Path, manifest: &Manifest) -> Result<(), AdapterFailure> {
    let expected_dir = codex_desktop::config_dir(home)
        .map_err(|_| failure("codex_history_takeover_recovery_failed"))?;
    let config_text = common::snapshot_bounded(&expected_dir.join("config.toml"), 2 * 1024 * 1024)
        .ok()
        .flatten()
        .and_then(|bytes| String::from_utf8(bytes).ok())
        .unwrap_or_default();
    let allowed_state = state_db_paths(home, &expected_dir, &config_text);
    let valid = manifest.version == MANIFEST_VERSION
        && manifest.codex_dir == expected_dir
        && valid_provider(&manifest.target_provider)
        && manifest.sessions.len() <= MAX_SESSION_FILES
        && manifest.sessions.iter().all(|entry| {
            session_path_allowed(&manifest.codex_dir, &entry.path) && valid_id(&entry.session_id)
        })
        && manifest.state.len() <= 2
        && manifest.state.iter().all(|entry| {
            state_path_allowed(&manifest.codex_dir, &entry.path)
                && allowed_state.contains(&entry.path)
                && entry.thread_ids.len() <= MAX_SESSION_FILES
                && entry.thread_ids.iter().all(|id| valid_id(id))
        });
    if valid {
        Ok(())
    } else {
        Err(failure("codex_history_takeover_recovery_failed"))
    }
}

fn valid_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && !value
            .chars()
            .any(|value| value.is_control() || value == '\0')
}

fn collect_session_files(dir: &Path, depth: u8, result: &mut Vec<PathBuf>) {
    if depth > 8 || result.len() >= MAX_SESSION_FILES {
        return;
    }
    let Ok(metadata) = fs::symlink_metadata(dir) else {
        return;
    };
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return;
    }
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        if result.len() >= MAX_SESSION_FILES {
            return;
        }
        let path = entry.path();
        let Ok(metadata) = fs::symlink_metadata(&path) else {
            continue;
        };
        if metadata.file_type().is_symlink() {
            continue;
        }
        if metadata.is_dir() {
            collect_session_files(&path, depth + 1, result);
        } else if metadata.is_file()
            && metadata.len() <= MAX_SESSION_BYTES
            && path.extension().and_then(|value| value.to_str()) == Some("jsonl")
        {
            result.push(path);
        }
    }
}

fn session_meta(bytes: &[u8]) -> Option<(Value, usize, bool)> {
    let end = bytes
        .iter()
        .position(|byte| *byte == b'\n')
        .unwrap_or(bytes.len());
    let mut line = &bytes[..end];
    if line.ends_with(b"\r") {
        line = &line[..line.len() - 1];
    }
    let value: Value = serde_json::from_slice(line).ok()?;
    (value.get("type").and_then(Value::as_str) == Some("session_meta")).then_some((
        value,
        end,
        bytes.get(end) == Some(&b'\n'),
    ))
}

fn session_identity(bytes: &[u8], provider: &str) -> Option<String> {
    let (value, _, _) = session_meta(bytes)?;
    let payload = value.get("payload")?;
    (payload.get("model_provider").and_then(Value::as_str) == Some(provider))
        .then(|| {
            payload
                .get("id")
                .and_then(Value::as_str)
                .filter(|id| valid_id(id))
        })
        .flatten()
        .map(str::to_owned)
}

fn rewrite_session(entry: &SessionEntry, from: &str, to: &str) -> Result<(), AdapterFailure> {
    let prefix =
        match common::first_line_bounded(&entry.path, MAX_SESSION_BYTES, MAX_SESSION_META_BYTES) {
            Ok(Some(bytes)) => bytes,
            Ok(None) => return Ok(()),
            Err(_) => return Err(failure("codex_history_takeover_recovery_failed")),
        };
    let Some((mut value, _, newline)) = session_meta(&prefix) else {
        return Ok(());
    };
    let Some(payload) = value.get_mut("payload").and_then(Value::as_object_mut) else {
        return Ok(());
    };
    if payload.get("id").and_then(Value::as_str) != Some(entry.session_id.as_str())
        || payload.get("model_provider").and_then(Value::as_str) != Some(from)
    {
        return Ok(());
    }
    payload.insert("model_provider".into(), Value::String(to.to_owned()));
    let mut rewritten = serde_json::to_vec(&value)
        .map_err(|_| failure("codex_history_takeover_recovery_failed"))?;
    if newline {
        rewritten.push(b'\n');
    }
    common::atomic_replace_prefix_bounded(&entry.path, &prefix, &rewritten, MAX_SESSION_BYTES)
        .map_err(|_| failure("codex_history_takeover_recovery_failed"))?;
    let confirmed =
        common::first_line_bounded(&entry.path, MAX_SESSION_BYTES, MAX_SESSION_META_BYTES)
            .ok()
            .flatten()
            .and_then(|bytes| session_identity(&bytes, to));
    if confirmed.as_deref() == Some(entry.session_id.as_str()) {
        Ok(())
    } else {
        Err(failure("codex_history_takeover_recovery_failed"))
    }
}

fn state_db_paths(home: &Path, codex_dir: &Path, config_text: &str) -> Vec<PathBuf> {
    let mut paths = vec![codex_dir.join("state_5.sqlite")];
    let from_config = config_text
        .parse::<DocumentMut>()
        .ok()
        .and_then(|document| {
            document
                .get("sqlite_home")
                .and_then(Item::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(|value| resolve_user_path(home, value))
        });
    let override_dir = from_config.or_else(|| {
        std::env::var_os("CODEX_SQLITE_HOME")
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
    });
    if let Some(directory) = override_dir.filter(|path| path.is_absolute()) {
        let candidate = directory.join("state_5.sqlite");
        if !paths.contains(&candidate) {
            paths.push(candidate);
        }
    }
    paths
}

fn resolve_user_path(home: &Path, raw: &str) -> PathBuf {
    if raw == "~" {
        home.to_owned()
    } else if let Some(rest) = raw.strip_prefix("~/").or_else(|| raw.strip_prefix("~\\")) {
        home.join(rest)
    } else {
        PathBuf::from(raw)
    }
}

fn has_provider_column(connection: &Connection) -> Result<bool, AdapterFailure> {
    let exists = connection
        .query_row(
            "SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'threads'",
            [],
            |_| Ok(()),
        )
        .optional()
        .map_err(|_| failure("codex_history_takeover_failed"))?
        .is_some();
    if !exists {
        return Ok(false);
    }
    let mut statement = connection
        .prepare("PRAGMA table_info(threads)")
        .map_err(|_| failure("codex_history_takeover_failed"))?;
    let columns = statement
        .query_map([], |row| row.get::<_, String>(1))
        .map_err(|_| failure("codex_history_takeover_failed"))?;
    for column in columns {
        if column.map_err(|_| failure("codex_history_takeover_failed"))? == "model_provider" {
            return Ok(true);
        }
    }
    Ok(false)
}

fn collect_state_entry(path: &Path) -> Result<Option<StateEntry>, AdapterFailure> {
    if !path.is_file() {
        return Ok(None);
    }
    common::ensure_safe_target(path, u64::MAX)
        .map_err(|_| failure("codex_history_takeover_failed"))?;
    let connection =
        Connection::open(path).map_err(|_| failure("codex_history_takeover_failed"))?;
    connection
        .busy_timeout(Duration::from_secs(5))
        .map_err(|_| failure("codex_history_takeover_failed"))?;
    if !has_provider_column(&connection)? {
        return Ok(None);
    }
    let mut statement = connection
        .prepare("SELECT id FROM threads WHERE model_provider = ?1")
        .map_err(|_| failure("codex_history_takeover_failed"))?;
    let rows = statement
        .query_map([SOURCE_PROVIDER], |row| row.get::<_, String>(0))
        .map_err(|_| failure("codex_history_takeover_failed"))?;
    let mut ids = BTreeSet::new();
    for id in rows {
        let id = id.map_err(|_| failure("codex_history_takeover_failed"))?;
        if valid_id(&id) {
            ids.insert(id);
        }
        if ids.len() > MAX_SESSION_FILES {
            return Err(failure("codex_history_takeover_failed"));
        }
    }
    Ok((!ids.is_empty()).then(|| StateEntry {
        path: path.to_owned(),
        thread_ids: ids.into_iter().collect(),
    }))
}

fn rewrite_state(
    entry: &StateEntry,
    from: &str,
    to: &str,
    error_code: &'static str,
) -> Result<(), AdapterFailure> {
    if !entry.path.is_file() {
        return Ok(());
    }
    common::ensure_safe_target(&entry.path, u64::MAX).map_err(|_| failure(error_code))?;
    let mut connection = Connection::open(&entry.path).map_err(|_| failure(error_code))?;
    connection
        .busy_timeout(Duration::from_secs(5))
        .map_err(|_| failure(error_code))?;
    if !has_provider_column(&connection).map_err(|_| failure(error_code))? {
        return Ok(());
    }
    let transaction = connection.transaction().map_err(|_| failure(error_code))?;
    {
        let mut statement = transaction
            .prepare("UPDATE threads SET model_provider = ?1 WHERE id = ?2 AND model_provider = ?3")
            .map_err(|_| failure(error_code))?;
        for id in &entry.thread_ids {
            statement
                .execute(params![to, id, from])
                .map_err(|_| failure(error_code))?;
        }
    }
    transaction.commit().map_err(|_| failure(error_code))
}

fn build_manifest(home: &Path, target: &str) -> Result<Manifest, AdapterFailure> {
    let codex_dir = codex_desktop::config_dir(home)?;
    let mut paths = Vec::new();
    collect_session_files(&codex_dir.join("sessions"), 0, &mut paths);
    collect_session_files(&codex_dir.join("archived_sessions"), 0, &mut paths);
    let mut sessions = Vec::new();
    for path in paths {
        let bytes = common::first_line_bounded(&path, MAX_SESSION_BYTES, MAX_SESSION_META_BYTES)
            .map_err(|_| failure("codex_history_takeover_failed"))?;
        if let Some(session_id) = bytes
            .as_deref()
            .and_then(|bytes| session_identity(bytes, SOURCE_PROVIDER))
        {
            sessions.push(SessionEntry { path, session_id });
        }
    }
    let config_text = common::snapshot_bounded(&codex_dir.join("config.toml"), 2 * 1024 * 1024)
        .ok()
        .flatten()
        .and_then(|bytes| String::from_utf8(bytes).ok())
        .unwrap_or_default();
    let mut state = Vec::new();
    for path in state_db_paths(home, &codex_dir, &config_text) {
        if let Some(entry) = collect_state_entry(&path)? {
            state.push(entry);
        }
    }
    Ok(Manifest {
        version: MANIFEST_VERSION,
        phase: Phase::Pending,
        codex_dir,
        target_provider: target.to_owned(),
        sessions,
        state,
    })
}

fn apply_manifest(manifest: &Manifest) -> Result<(), AdapterFailure> {
    for entry in &manifest.sessions {
        rewrite_session(entry, SOURCE_PROVIDER, &manifest.target_provider)?;
    }
    for entry in &manifest.state {
        rewrite_state(
            entry,
            SOURCE_PROVIDER,
            &manifest.target_provider,
            "codex_history_takeover_failed",
        )?;
    }
    Ok(())
}

fn restore_manifest(home: &Path, manifest: &Manifest) -> Result<(), AdapterFailure> {
    for entry in &manifest.sessions {
        rewrite_session(entry, &manifest.target_provider, SOURCE_PROVIDER)?;
    }
    for entry in &manifest.state {
        rewrite_state(
            entry,
            &manifest.target_provider,
            SOURCE_PROVIDER,
            "codex_history_takeover_recovery_failed",
        )?;
    }
    remove_manifest(home)
}

impl Takeover {
    pub(crate) fn begin(home: &Path, target: &str) -> Result<Self, AdapterFailure> {
        if !valid_provider(target) {
            return Err(failure("codex_history_takeover_failed"));
        }
        if let Some(existing) = load_manifest(home)? {
            if existing.phase == Phase::Active && existing.target_provider == target {
                return Ok(Self {
                    home: home.to_owned(),
                    created: false,
                    apply_on_commit: true,
                });
            }
            if existing.phase == Phase::Pending {
                restore_manifest(home, &existing)?;
            } else {
                return Err(failure("codex_history_takeover_conflict"));
            }
        }
        let manifest = build_manifest(home, target)?;
        if manifest.sessions.is_empty() && manifest.state.is_empty() {
            return Ok(Self {
                home: home.to_owned(),
                created: false,
                apply_on_commit: false,
            });
        }
        save_manifest(home, &manifest)?;
        Ok(Self {
            home: home.to_owned(),
            created: true,
            apply_on_commit: true,
        })
    }

    pub(crate) fn commit(&mut self) -> Result<(), AdapterFailure> {
        if !self.apply_on_commit {
            return Ok(());
        }
        let Some(mut manifest) = load_manifest(&self.home)? else {
            return Err(failure("codex_history_takeover_failed"));
        };
        if let Err(error) = apply_manifest(&manifest) {
            if self.created {
                restore_manifest(&self.home, &manifest)?;
            }
            return Err(error);
        }
        if self.created {
            manifest.phase = Phase::Active;
            save_manifest(&self.home, &manifest)?;
        }
        self.apply_on_commit = false;
        Ok(())
    }

    pub(crate) fn disarm(&mut self) {
        self.created = false;
        self.apply_on_commit = false;
    }

    pub(crate) fn rollback(&mut self) -> Result<(), AdapterFailure> {
        if !self.created {
            return Ok(());
        }
        let Some(manifest) = load_manifest(&self.home)? else {
            return Err(failure("codex_history_takeover_recovery_failed"));
        };
        restore_manifest(&self.home, &manifest)?;
        self.created = false;
        self.apply_on_commit = false;
        Ok(())
    }
}

pub(crate) fn restore(home: &Path) -> Result<(), AdapterFailure> {
    let Some(manifest) = load_manifest(home)? else {
        return Ok(());
    };
    restore_manifest(home, &manifest)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(label: &str) -> PathBuf {
        common::temporary_working_directory(label).unwrap()
    }

    #[test]
    fn rewrites_only_session_provider_metadata() {
        let root = temp("codex-history-session");
        let path = root.join("sessions/2026/example.jsonl");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let original = b"{\"type\":\"session_meta\",\"payload\":{\"id\":\"thread-1\",\"model_provider\":\"openai\",\"cwd\":\"C:/Work\"}}\n{\"type\":\"response_item\",\"payload\":{\"encrypted_content\":\"untouched\"}}\n";
        common::atomic_write_bounded(&path, original, MAX_SESSION_BYTES).unwrap();
        let entry = SessionEntry {
            path: path.clone(),
            session_id: "thread-1".into(),
        };
        rewrite_session(&entry, "openai", "yeschoy").unwrap();
        let changed = fs::read(&path).unwrap();
        assert_eq!(
            session_identity(&changed, "yeschoy").as_deref(),
            Some("thread-1")
        );
        assert!(String::from_utf8(changed)
            .unwrap()
            .contains("encrypted_content\":\"untouched"));
        rewrite_session(&entry, "yeschoy", "openai").unwrap();
        assert_eq!(
            session_identity(&fs::read(&path).unwrap(), "openai").as_deref(),
            Some("thread-1")
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn streams_large_session_tail_and_rejects_oversized_metadata() {
        let root = temp("codex-history-streamed-session");
        let path = root.join("sessions/2026/large.jsonl");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let first = b"{\"type\":\"session_meta\",\"payload\":{\"id\":\"thread-large\",\"model_provider\":\"openai\"}}\n";
        let tail = vec![b'x'; 4 * 1024 * 1024];
        let mut original = first.to_vec();
        original.extend_from_slice(&tail);
        common::atomic_write_bounded(&path, &original, MAX_SESSION_BYTES).unwrap();
        let entry = SessionEntry {
            path: path.clone(),
            session_id: "thread-large".into(),
        };
        rewrite_session(&entry, "openai", "yeschoy").unwrap();
        let changed = fs::read(&path).unwrap();
        assert!(changed.ends_with(&tail));
        assert_eq!(
            common::first_line_bounded(&path, MAX_SESSION_BYTES, MAX_SESSION_META_BYTES)
                .unwrap()
                .and_then(|line| session_identity(&line, "yeschoy"))
                .as_deref(),
            Some("thread-large")
        );

        let oversized = root.join("sessions/2026/oversized.jsonl");
        fs::write(&oversized, vec![b'x'; MAX_SESSION_META_BYTES as usize + 1]).unwrap();
        assert!(
            common::first_line_bounded(&oversized, MAX_SESSION_BYTES, MAX_SESSION_META_BYTES,)
                .is_err()
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn state_updates_are_scoped_to_manifest_ids_and_provider() {
        let root = temp("codex-history-state");
        let path = root.join("state_5.sqlite");
        let connection = Connection::open(&path).unwrap();
        connection
            .execute(
                "CREATE TABLE threads (id TEXT PRIMARY KEY, model_provider TEXT)",
                [],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO threads VALUES ('a', 'openai'), ('b', 'custom'), ('c', 'openai')",
                [],
            )
            .unwrap();
        drop(connection);
        let entry = StateEntry {
            path: path.clone(),
            thread_ids: vec!["a".into()],
        };
        rewrite_state(&entry, "openai", "yeschoy", "test").unwrap();
        let connection = Connection::open(&path).unwrap();
        let provider = |id: &str| {
            connection
                .query_row(
                    "SELECT model_provider FROM threads WHERE id = ?1",
                    [id],
                    |row| row.get::<_, String>(0),
                )
                .unwrap()
        };
        assert_eq!(provider("a"), "yeschoy");
        assert_eq!(provider("b"), "custom");
        assert_eq!(provider("c"), "openai");
        drop(connection);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn begin_inventories_without_rewriting_sessions_until_commit() {
        let root = temp("codex-history-deferred-apply");
        let session = root.join(".codex/sessions/2026/example.jsonl");
        fs::create_dir_all(session.parent().unwrap()).unwrap();
        let original = b"{\"type\":\"session_meta\",\"payload\":{\"id\":\"thread-1\",\"model_provider\":\"openai\"}}\n";
        common::atomic_write_bounded(&session, original, MAX_SESSION_BYTES).unwrap();
        let mut takeover = Takeover::begin(&root, "yeschoy").unwrap();
        assert_eq!(fs::read(&session).unwrap(), original);
        takeover.commit().unwrap();
        assert_eq!(
            session_identity(&fs::read(&session).unwrap(), "yeschoy").as_deref(),
            Some("thread-1")
        );
        takeover.rollback().unwrap();
        assert_eq!(
            session_identity(&fs::read(&session).unwrap(), "openai").as_deref(),
            Some("thread-1")
        );
        fs::remove_dir_all(root).unwrap();
    }
}

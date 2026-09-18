//! Re-bucket only Codex's provider metadata while YesChoy owns the live config.
//!
//! Codex pins `model_provider` in both a session JSONL's `session_meta` record
//! and the `threads` table in `state_5.sqlite`. Changing config.toml alone does
//! not update an already-created thread, so a user who switches provider finds
//! their old conversations gone from the resume list — the data is intact, it
//! is just filed in a drawer Codex no longer opens.
//!
//! This sidecar transaction moves those drawers into the provider our adapter
//! selected, and remembers where each one came from so the move is exactly
//! reversible. It never reads or changes message bodies, encrypted reasoning
//! or auth.json.
//!
//! It used to move only the built-in `openai` drawer, which left anyone
//! arriving from another switcher (their sessions are filed under that tool's
//! own provider id) still staring at an empty list.
//!
//! Visible is not the same as resumable: a conversation's encrypted reasoning
//! was sealed by whichever backend produced it, so continuing an imported
//! thread against a different one can still fail. Reading, searching and
//! copying out of it all work, which is what the drawer was hiding.

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

/// Where a v1 manifest's entries came from, since v1 only ever moved this one.
const LEGACY_SOURCE_PROVIDER: &str = "openai";
/// v1 recorded no per-entry origin. Both are accepted so a manifest written by
/// an older build mid-transaction can still be rolled back.
const MANIFEST_VERSION: u8 = 2;
const MIN_MANIFEST_VERSION: u8 = 1;
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
    /// The provider this session was filed under before we moved it, and the
    /// one a restore must put it back to. Absent in a v1 manifest, which only
    /// ever moved `openai`.
    #[serde(default = "legacy_source_provider")]
    origin_provider: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StateEntry {
    path: PathBuf,
    thread_ids: Vec<String>,
    /// Parallel to `thread_ids`. Empty means a v1 manifest, where every thread
    /// came from `openai`. Kept as a second list rather than a richer element
    /// type so a v1 manifest still deserializes unchanged.
    #[serde(default)]
    thread_origins: Vec<String>,
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

fn legacy_source_provider() -> String {
    LEGACY_SOURCE_PROVIDER.to_owned()
}

/// The origin recorded for `thread_ids[index]`, tolerating a v1 manifest.
fn thread_origin(entry: &StateEntry, index: usize) -> &str {
    entry
        .thread_origins
        .get(index)
        .map_or(LEGACY_SOURCE_PROVIDER, String::as_str)
}

/// A provider we may move history *into*.
///
/// Never the built-in `openai`: that bucket belongs to the user's official
/// subscription, and filing our sessions there would both mislead Codex and
/// leave a restore with nowhere distinct to put them back. `valid_provider`
/// used to carry this rule, but it now has to accept `openai` as a source.
fn valid_target_provider(value: &str) -> bool {
    value != LEGACY_SOURCE_PROVIDER && valid_provider(value)
}

fn failure(code: &'static str) -> AdapterFailure {
    AdapterFailure::ConfigurationFailed(code)
}

/// Any provider id Codex could have filed a drawer under, including the
/// built-in `openai` — that one is now a legitimate *source*.
fn valid_provider(value: &str) -> bool {
    (1..=64).contains(&value.len())
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
    // This manifest is the only record of where each conversation came from.
    // If it is wrong, a restore files history under a provider it never
    // belonged to, so every field it carries is checked before it is trusted.
    let valid = (MIN_MANIFEST_VERSION..=MANIFEST_VERSION).contains(&manifest.version)
        && manifest.codex_dir == expected_dir
        && valid_target_provider(&manifest.target_provider)
        && manifest.sessions.len() <= MAX_SESSION_FILES
        && manifest.sessions.iter().all(|entry| {
            session_path_allowed(&manifest.codex_dir, &entry.path)
                && valid_id(&entry.session_id)
                && valid_provider(&entry.origin_provider)
                // A session recorded as coming from the target has nowhere to
                // be restored to; it should never have been collected.
                && entry.origin_provider != manifest.target_provider
        })
        && manifest.state.len() <= 2
        && manifest.state.iter().all(|entry| {
            state_path_allowed(&manifest.codex_dir, &entry.path)
                && allowed_state.contains(&entry.path)
                && entry.thread_ids.len() <= MAX_SESSION_FILES
                && entry.thread_ids.iter().all(|id| valid_id(id))
                // Empty is the v1 shape (all `openai`). Anything else has to
                // line up positionally, or the origins address the wrong rows.
                && (entry.thread_origins.is_empty()
                    || entry.thread_origins.len() == entry.thread_ids.len())
                && entry.thread_origins.iter().all(|origin| {
                    valid_provider(origin) && *origin != manifest.target_provider
                })
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

/// The session's id and the provider it is currently filed under.
///
/// The single-provider `session_identity` stays for verifying a rewrite landed;
/// discovery needs to accept whatever drawer a session happens to be in.
fn session_origin(bytes: &[u8]) -> Option<(String, String)> {
    let (value, _, _) = session_meta(bytes)?;
    let payload = value.get("payload")?;
    let provider = payload.get("model_provider").and_then(Value::as_str)?;
    let id = payload.get("id").and_then(Value::as_str)?;
    (valid_id(id) && valid_provider(provider))
        .then(|| (id.to_owned(), provider.to_owned()))
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

fn collect_state_entry(path: &Path, target: &str) -> Result<Option<StateEntry>, AdapterFailure> {
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
        .prepare("SELECT id, model_provider FROM threads WHERE model_provider <> ?1")
        .map_err(|_| failure("codex_history_takeover_failed"))?;
    let rows = statement
        .query_map([target], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .map_err(|_| failure("codex_history_takeover_failed"))?;
    // BTreeSet keeps the pairing between the two lists deterministic, which is
    // what lets `thread_origins` be positional.
    let mut found = BTreeSet::new();
    for row in rows {
        let (id, origin) = row.map_err(|_| failure("codex_history_takeover_failed"))?;
        if valid_id(&id) && valid_provider(&origin) {
            found.insert((id, origin));
        }
        if found.len() > MAX_SESSION_FILES {
            return Err(failure("codex_history_takeover_failed"));
        }
    }
    if found.is_empty() {
        return Ok(None);
    }
    let (thread_ids, thread_origins) = found.into_iter().unzip();
    Ok(Some(StateEntry {
        path: path.to_owned(),
        thread_ids,
        thread_origins,
    }))
}

/// Which way a state rewrite runs. Each thread has its own origin, so only the
/// target side is a single value; the other end is read per row.
#[derive(Clone, Copy)]
enum Direction<'a> {
    Into(&'a str),
    OutOf(&'a str),
}

fn rewrite_state(
    entry: &StateEntry,
    direction: Direction<'_>,
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
        for (index, id) in entry.thread_ids.iter().enumerate() {
            let origin = thread_origin(entry, index);
            let (to, from) = match direction {
                Direction::Into(target) => (target, origin),
                Direction::OutOf(target) => (origin, target),
            };
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
        // Any drawer but the one we are moving into. A session already filed
        // under the target needs no move, and moving it would make the restore
        // put it somewhere it never was.
        if let Some((session_id, origin_provider)) =
            bytes.as_deref().and_then(session_origin)
        {
            if origin_provider != target {
                sessions.push(SessionEntry {
                    path,
                    session_id,
                    origin_provider,
                });
            }
        }
    }
    let config_text = common::snapshot_bounded(&codex_dir.join("config.toml"), 2 * 1024 * 1024)
        .ok()
        .flatten()
        .and_then(|bytes| String::from_utf8(bytes).ok())
        .unwrap_or_default();
    let mut state = Vec::new();
    for path in state_db_paths(home, &codex_dir, &config_text) {
        if let Some(entry) = collect_state_entry(&path, target)? {
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
        rewrite_session(entry, &entry.origin_provider, &manifest.target_provider)?;
    }
    for entry in &manifest.state {
        rewrite_state(
            entry,
            Direction::Into(&manifest.target_provider),
            "codex_history_takeover_failed",
        )?;
    }
    Ok(())
}

fn restore_manifest(home: &Path, manifest: &Manifest) -> Result<(), AdapterFailure> {
    for entry in &manifest.sessions {
        rewrite_session(entry, &manifest.target_provider, &entry.origin_provider)?;
    }
    for entry in &manifest.state {
        rewrite_state(
            entry,
            Direction::OutOf(&manifest.target_provider),
            "codex_history_takeover_recovery_failed",
        )?;
    }
    remove_manifest(home)
}

impl Takeover {
    pub(crate) fn begin(home: &Path, target: &str) -> Result<Self, AdapterFailure> {
        if !valid_target_provider(target) {
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
            origin_provider: "openai".into(),
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
            origin_provider: "openai".into(),
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
    fn a_drawer_from_another_switcher_moves_and_goes_back_where_it_came_from() {
        // Someone arriving from another switcher has their conversations filed
        // under that tool's provider id, not the built-in `openai`. Moving only
        // `openai` left them staring at an empty resume list, which is the whole
        // complaint this feature exists to answer.
        let root = temp("codex-history-foreign-origin");
        let path = root.join("sessions/2026/foreign.jsonl");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let original = b"{\"type\":\"session_meta\",\"payload\":{\"id\":\"thread-9\",\"model_provider\":\"packycode\"}}\n{\"type\":\"response_item\",\"payload\":{\"encrypted_content\":\"untouched\"}}\n";
        common::atomic_write_bounded(&path, original, MAX_SESSION_BYTES).unwrap();

        let (id, origin) = session_origin(&fs::read(&path).unwrap()).unwrap();
        assert_eq!((id.as_str(), origin.as_str()), ("thread-9", "packycode"));

        let entry = SessionEntry {
            path: path.clone(),
            session_id: id,
            origin_provider: origin,
        };
        rewrite_session(&entry, &entry.origin_provider, "yeschoy").unwrap();
        assert_eq!(
            session_identity(&fs::read(&path).unwrap(), "yeschoy").as_deref(),
            Some("thread-9")
        );
        // Restoring must return it to `packycode`, not to `openai` — that is
        // what the recorded origin is for.
        rewrite_session(&entry, "yeschoy", &entry.origin_provider).unwrap();
        assert_eq!(
            session_identity(&fs::read(&path).unwrap(), "packycode").as_deref(),
            Some("thread-9")
        );
        assert!(String::from_utf8(fs::read(&path).unwrap())
            .unwrap()
            .contains("encrypted_content\":\"untouched"));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn threads_return_to_their_own_providers_not_a_single_default() {
        let root = temp("codex-history-mixed-origins");
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
                "INSERT INTO threads VALUES ('a', 'openai'), ('b', 'packycode'), ('c', 'yeschoy')",
                [],
            )
            .unwrap();
        drop(connection);

        // Discovery takes every drawer but the one being moved into.
        let entry = collect_state_entry(&path, "yeschoy").unwrap().unwrap();
        assert_eq!(entry.thread_ids, vec!["a".to_string(), "b".to_string()]);
        assert_eq!(
            entry.thread_origins,
            vec!["openai".to_string(), "packycode".to_string()]
        );

        rewrite_state(&entry, Direction::Into("yeschoy"), "test").unwrap();
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
        assert_eq!(provider("b"), "yeschoy");
        assert_eq!(provider("c"), "yeschoy");
        drop(connection);

        rewrite_state(&entry, Direction::OutOf("yeschoy"), "test").unwrap();
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
        assert_eq!(provider("a"), "openai");
        assert_eq!(provider("b"), "packycode");
        // Never collected, never touched in either direction.
        assert_eq!(provider("c"), "yeschoy");
        drop(connection);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn a_v1_manifest_still_restores_to_openai() {
        // An older build could have written a manifest and then crashed. Its
        // entries carry no origin, and v1 only ever moved `openai`.
        let entry: SessionEntry = serde_json::from_str(
            r#"{"path":"/tmp/x.jsonl","session_id":"thread-1"}"#,
        )
        .unwrap();
        assert_eq!(entry.origin_provider, LEGACY_SOURCE_PROVIDER);
        let state: StateEntry = serde_json::from_str(
            r#"{"path":"/tmp/state_5.sqlite","thread_ids":["a","b"]}"#,
        )
        .unwrap();
        assert!(state.thread_origins.is_empty());
        assert_eq!(thread_origin(&state, 0), LEGACY_SOURCE_PROVIDER);
        assert_eq!(thread_origin(&state, 7), LEGACY_SOURCE_PROVIDER);
    }

    #[test]
    fn the_official_bucket_is_never_a_takeover_target() {
        // `openai` is now a legitimate source, so the rule that it must never
        // be a destination needs its own guard.
        assert!(!valid_target_provider("openai"));
        assert!(valid_provider("openai"));
        assert!(valid_target_provider("yeschoy"));
        assert!(!valid_target_provider(""));
        assert!(!valid_target_provider("has space"));
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
            thread_origins: vec!["openai".into()],
        };
        rewrite_state(&entry, Direction::Into("yeschoy"), "test").unwrap();
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

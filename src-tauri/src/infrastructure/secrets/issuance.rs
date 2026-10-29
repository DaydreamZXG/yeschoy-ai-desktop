//! Durable record of keys this client is about to mint on the relay.
//!
//! The relay, the keychain and local files cannot share one transaction. A create
//! whose reply is lost may or may not have minted a key, so after a crash the
//! only trustworthy evidence is a record written *before* the create: the exact
//! token name, its tool and group. The compensator may delete a token only when
//! that exact name is still present, and only for records older than the current
//! attempt. Never a prefix heuristic, never a key another run may be using.
//!
//! Stored beside the device id under `~/.yeschoy/` with owner-only permissions.
//! It holds names (not secrets); key material never leaves the keychain.
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

const FILE: &str = "activation-issuance.json";
const MAX_RECORDS: usize = 64;
/// A record older than this cannot belong to an activation still running.
/// The startup recovery step reads it through `IssuanceLog::stale`; only the
/// module's own tests name it directly.
#[allow(dead_code)]
pub(crate) const STALE_AFTER: Duration = Duration::from_secs(30 * 60);

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct IssuanceRecord {
    pub(crate) name: String,
    pub(crate) tool_id: String,
    pub(crate) billing_group: String,
    pub(crate) recorded_at_epoch_ms: u64,
    /// Activation-local nonce carried in the token name, allocated from the
    /// persisted counter. Any minted key whose nonce is at most the last one we
    /// recorded before it belongs to us and to an attempt that ended.
    pub(crate) sequence: u64,
}

#[derive(Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct File {
    schema_version: u8,
    records: Vec<IssuanceRecord>,
    /// Largest sequence number this installation has ever recorded. Persisted so
    /// a later activation can never mistake an earlier run's token for one it
    /// minted itself (that is what makes crash compensation provable).
    #[serde(default)]
    last_sequence: u64,
}

pub(crate) struct IssuanceLog {
    path: PathBuf,
}

impl IssuanceLog {
    pub(crate) fn at(home: &Path) -> Self {
        Self {
            path: home.join(".yeschoy").join(FILE),
        }
    }

    fn read(&self) -> File {
        std::fs::read_to_string(&self.path)
            .ok()
            .and_then(|text| serde_json::from_str::<File>(&text).ok())
            .filter(|file| file.schema_version == 1)
            .unwrap_or(File {
                schema_version: 1,
                records: Vec::new(),
                last_sequence: 0,
            })
    }

    fn write(&self, file: &File) -> std::io::Result<()> {
        let text = serde_json::to_vec_pretty(file)?;
        crate::infrastructure::config::files::atomic_write(&self.path, &text)?;
        crate::infrastructure::config::files::restrict_to_owner(&self.path)
    }

    #[allow(dead_code)]
    pub(crate) fn records(&self) -> Vec<IssuanceRecord> {
        self.read().records
    }

    /// Allocate and persist the next sequence number for a new activation.
    pub(crate) fn next_sequence(&self) -> std::io::Result<u64> {
        let mut file = self.read();
        file.last_sequence = file.last_sequence.saturating_add(1);
        self.write(&file)?;
        Ok(file.last_sequence)
    }

    /// Persist the intent to create this exact name before the create is sent.
    pub(crate) fn begin(&self, record: IssuanceRecord) -> std::io::Result<()> {
        let mut file = self.read();
        file.records.retain(|existing| existing.name != record.name);
        file.records.push(record);
        if file.records.len() > MAX_RECORDS {
            file.records.drain(..file.records.len() - MAX_RECORDS);
        }
        self.write(&file)
    }

    /// Drop the record once this run owns the key (minted and read back, or
    /// proven reused); a later crash can no longer orphan it.
    pub(crate) fn finish(&self, name: &str) {
        let mut file = self.read();
        let before = file.records.len();
        file.records.retain(|record| record.name != name);
        if file.records.len() != before {
            let _ = self.write(&file);
        }
    }

    /// Records that cannot belong to an attempt still running now.
    #[allow(dead_code)]
    pub(crate) fn stale(&self, now_epoch_ms: u64) -> Vec<IssuanceRecord> {
        let stale_after = STALE_AFTER.as_millis() as u64;
        self.read()
            .records
            .into_iter()
            .filter(|record| {
                now_epoch_ms.saturating_sub(record.recorded_at_epoch_ms) >= stale_after
            })
            .collect()
    }

    #[allow(dead_code)]
    pub(crate) fn forget(&self, names: &[String]) {
        let mut file = self.read();
        file.records.retain(|record| !names.contains(&record.name));
        let _ = self.write(&file);
    }
}

#[allow(dead_code)] // Shared clock for the manifest and its recovery reader; activation keeps its own copy.
pub(crate) fn now_epoch_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u64::MAX as u128) as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    fn home(label: &str) -> PathBuf {
        let root =
            std::env::temp_dir().join(format!("yeschoy-issuance-{}-{label}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        root
    }

    fn record(name: &str, at: u64) -> IssuanceRecord {
        IssuanceRecord {
            name: name.into(),
            tool_id: "pi".into(),
            billing_group: "default".into(),
            recorded_at_epoch_ms: at,
            sequence: at,
        }
    }

    #[test]
    fn intent_is_written_before_use_and_removed_once_owned() {
        let root = home("roundtrip");
        let log = IssuanceLog::at(&root);
        assert!(log.records().is_empty());
        log.begin(record("synthetic-name-a", 1)).unwrap();
        let persisted: File =
            serde_json::from_slice(&std::fs::read(root.join(".yeschoy").join(FILE)).unwrap())
                .unwrap();
        assert_eq!(
            persisted.records.len(),
            1,
            "intent survives a process restart"
        );
        assert!(
            !String::from_utf8(std::fs::read(root.join(".yeschoy").join(FILE)).unwrap())
                .unwrap()
                .contains("sk-")
        );
        log.finish("synthetic-name-a");
        assert!(log.records().is_empty());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn only_old_records_are_stale_and_duplicates_do_not_accumulate() {
        let root = home("stale");
        let log = IssuanceLog::at(&root);
        log.begin(record("synthetic-old", 1_000)).unwrap();
        log.begin(record("synthetic-new", STALE_AFTER.as_millis() as u64 * 2))
            .unwrap();
        log.begin(record("synthetic-new", STALE_AFTER.as_millis() as u64 * 2))
            .unwrap();
        assert_eq!(log.records().len(), 2);
        let stale = log.stale(STALE_AFTER.as_millis() as u64 * 2 + 1);
        assert_eq!(
            stale.iter().map(|r| r.name.as_str()).collect::<Vec<_>>(),
            ["synthetic-old"]
        );
        log.forget(&["synthetic-old".into()]);
        assert_eq!(log.records().len(), 1);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn sequence_numbers_are_persisted_before_use_and_never_repeat() {
        let root = home("sequence");
        let log = IssuanceLog::at(&root);
        assert_eq!(log.next_sequence().unwrap(), 1);
        assert_eq!(log.next_sequence().unwrap(), 2);
        // A fresh handle, as after a restart, continues rather than restarting.
        assert_eq!(IssuanceLog::at(&root).next_sequence().unwrap(), 3);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn unreadable_or_foreign_files_fail_closed_to_no_records() {
        let root = home("broken");
        std::fs::create_dir_all(root.join(".yeschoy")).unwrap();
        std::fs::write(root.join(".yeschoy").join(FILE), b"not json").unwrap();
        assert!(IssuanceLog::at(&root).records().is_empty());
        assert!(IssuanceLog::at(&root).stale(u64::MAX).is_empty());
        let _ = std::fs::remove_dir_all(root);
    }
}

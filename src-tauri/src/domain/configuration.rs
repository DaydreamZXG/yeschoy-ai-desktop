//! Immutable native configuration intent; not an IPC payload or a file writer.
use std::path::PathBuf;

#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub(crate) struct FileChange {
    pub(crate) path: PathBuf,
    pub(crate) before: Option<Vec<u8>>,
    pub(crate) after: Vec<u8>,
}

impl std::fmt::Debug for FileChange {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FileChange")
            .field("existed_before", &self.before.is_some())
            .field("contents", &"[redacted]")
            .finish()
    }
}

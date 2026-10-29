//! What one tool's restore path may do to a file that nobody else can decide.
//!
//! Recovery owns the journal, the merge rules and the compensation order. It
//! must not own *format* knowledge: "is this DSH's credentials file", "strip
//! the DSH patch our old version wrote" — those belong to the tool whose file it
//! is. Before this hook existed, `connection_recovery` reached into
//! `tool_adapters::dsh_web` directly, which made the infrastructure layer depend
//! on a specific adapter.
//!
//! The rules it enforces are the reason a hook is safe to hand over:
//!
//! - **Scope**: `owns` decides purely by path, and answering `false` means the
//!   generic merge handles the file. A tool can never claim a file it was not
//!   offered.
//! - **Idempotence**: recovery may ask about the same file more than once
//!   (rebase, then restore). Implementations must not mutate disk here.
//! - **No invention**: every document-returning hook may answer `None`, which
//!   means "not mine to rewrite" and falls back to the generic path. Returning a
//!   document the user did not have is the one failure recovery cannot undo.
//! - **Preserve the user's edits**: hooks run *after* recovery has already
//!   detected `before`/`after`/`current`; they must keep anything the user added
//!   since our write, even when that means deleting the whole file instead.
use std::path::Path;

use serde_json::Value;

pub(crate) trait RecoveryHooks {
    /// Does this path belong to the tool's own restore semantics?
    fn owns(&self, path: &Path) -> bool;

    /// Remove values our older versions wrote, leaving the rest of the file.
    fn strip_owned(&self, bytes: &[u8]) -> Option<Result<Vec<u8>, ()>>;

    /// Rewrite a whole document during restore.
    ///
    /// `None` means "the generic merge should handle this file". Otherwise the
    /// document to write back (`None` = delete the file) and whether a value was
    /// deliberately left in place because it no longer holds what we wrote.
    fn restore_document(
        &self,
        path: &Path,
        before: Option<&[u8]>,
        after: &[u8],
        current: &[u8],
    ) -> Option<Result<(Option<Value>, bool), ()>>;
}

/// The hook an unknown tool gets: claim nothing, rewrite nothing.
pub(crate) struct NoHooks;

impl RecoveryHooks for NoHooks {
    fn owns(&self, _path: &Path) -> bool {
        false
    }

    fn strip_owned(&self, _bytes: &[u8]) -> Option<Result<Vec<u8>, ()>> {
        None
    }

    fn restore_document(
        &self,
        _path: &Path,
        _before: Option<&[u8]>,
        _after: &[u8],
        _current: &[u8],
    ) -> Option<Result<(Option<Value>, bool), ()>> {
        None
    }
}

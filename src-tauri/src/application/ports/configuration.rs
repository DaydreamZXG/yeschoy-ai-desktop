#![allow(dead_code)] // Fault fixtures inject the port once activation tests use it.
//! Configuration IO boundary used by the common transaction and fault fixtures.
use std::path::Path;

pub(crate) trait ConfigFiles {
    fn snapshot(&self, path: &Path) -> Result<Option<Vec<u8>>, ()>;
    fn write(&self, path: &Path, bytes: &[u8]) -> Result<(), ()>;
    fn restore(&self, path: &Path, before: Option<&[u8]>) -> Result<(), ()>;
}

/// A tool contributes a plan and format/lifecycle-specific verification only.
/// The application drives the one common transaction through this contract.
pub(crate) trait ConfigCommit {
    fn changes(&self) -> &[crate::domain::configuration::FileChange];
    fn commit(&mut self) -> Result<(), crate::domain::error::ConfigFailure>;
    fn rollback(&mut self) -> Result<(), crate::domain::error::ConfigFailure>;
}

/// A tool's configuration plan. Every adapter commits through the one shared
/// transaction below and then verifies the fields it owns; the plan is the only
/// surface the activation service uses, so adapters stay independent of each
/// other and of the command layer.
pub(crate) trait ConfigurationPlan {
    /// Shared transaction (write, read-back, conflict detection, compensation).
    fn transaction(&mut self) -> &mut dyn ConfigCommit;
    /// Format-specific verification after the shared transaction committed.
    fn verify_written(&self) -> Result<(), crate::domain::error::AdapterFailure>;
}

/// Shared helper for plans whose `commit` is "transaction, then read-back".
///
/// Adapter `commit` implementations call this so every tool gets identical
/// commit ordering; adapters that wrap the transaction (locking, permission
/// narrowing) still do so inside their own `commit` before calling it.
/// Verification capability a plan hands to the shared commit helper. It holds
/// only an immutable borrow, so the helper can own the mutable transaction
/// borrow and still run the adapter's read-back.
pub(crate) trait WrittenPlan {
    fn verify(&self) -> Result<(), crate::domain::error::AdapterFailure>;
}

/// Order shared by every adapter. Implementations commit the one transaction,
/// read back through it, then verify the fields their format owns; the order is
/// identical for every tool and lives here rather than in each adapter.
pub(crate) trait SharedCommit {
    fn commit_with_plan(&mut self) -> Result<(), crate::domain::error::AdapterFailure>;
}

pub(crate) fn commit_with_plan<P: SharedCommit>(
    plan: &mut P,
) -> Result<(), crate::domain::error::AdapterFailure> {
    plan.commit_with_plan()
}

/// The one compensation order: undo the shared transaction (newest file first,
/// leaving files somebody else has since edited reported, never clobbered).
pub(crate) trait SharedRollback {
    fn rollback_with_plan(&mut self) -> Result<(), crate::domain::error::AdapterFailure>;
}

pub(crate) fn rollback_with_plan<P: SharedRollback>(
    plan: &mut P,
) -> Result<(), crate::domain::error::AdapterFailure> {
    plan.rollback_with_plan()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infrastructure::config::ConfigTransaction;

    struct Check(Option<&'static str>, ConfigTransaction);

    impl SharedCommit for Check {
        fn commit_with_plan(&mut self) -> Result<(), crate::domain::error::AdapterFailure> {
            self.1
                .commit()
                .map_err(crate::domain::error::AdapterFailure::from)?;
            match self.0 {
                None => Ok(()),
                Some(reason) => Err(crate::domain::error::AdapterFailure::ConfigurationFailed(
                    reason,
                )),
            }
        }
    }

    fn fixture(label: &str) -> std::path::PathBuf {
        let root =
            std::env::temp_dir().join(format!("yeschoy-plan-{}-{label}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        root.join("config.json")
    }

    #[test]
    fn commit_with_plan_publishes_then_runs_adapter_verification() {
        let path = fixture("ok");
        let transaction =
            ConfigTransaction::stage_with_snapshot(path.clone(), None, b"{}".to_vec()).unwrap();
        let mut plan = Check(None, transaction);
        commit_with_plan(&mut plan).unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"{}");
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn verification_failure_is_reported_with_its_reason_code_after_the_commit() {
        let path = fixture("verify");
        let transaction =
            ConfigTransaction::stage_with_snapshot(path.clone(), None, b"{}".to_vec()).unwrap();
        assert_eq!(
            commit_with_plan(&mut Check(
                Some("configuration_readback_failed"),
                transaction
            )),
            Err(crate::domain::error::AdapterFailure::ConfigurationFailed(
                "configuration_readback_failed"
            ))
        );
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }
}

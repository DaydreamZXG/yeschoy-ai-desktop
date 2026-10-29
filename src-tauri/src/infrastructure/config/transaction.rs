//! Shared byte transaction: optimistic version check, atomic commit/readback and compensation.
use super::files::{snapshot, NativeConfigFiles, MAX_CONFIG_BYTES};
use crate::{
    application::ports::configuration::{ConfigCommit, ConfigFiles},
    domain::{configuration::FileChange, error::ConfigFailure},
};
use std::{collections::HashSet, path::PathBuf};

#[derive(Debug, Default)]
pub(crate) struct ConfigTransaction {
    changes: Vec<FileChange>,
    written: HashSet<PathBuf>,
}

impl ConfigTransaction {
    pub(crate) fn changes(&self) -> &[FileChange] {
        &self.changes
    }

    #[cfg(test)]
    pub(crate) fn stage(path: PathBuf, after: Vec<u8>) -> Result<Self, ConfigFailure> {
        let mut transaction = Self::default();
        transaction.push(path, after)?;
        Ok(transaction)
    }

    pub(crate) fn stage_with_snapshot(
        path: PathBuf,
        before: Option<Vec<u8>>,
        after: Vec<u8>,
    ) -> Result<Self, ConfigFailure> {
        if after.len() as u64 > MAX_CONFIG_BYTES {
            return Err(ConfigFailure::Write);
        }
        Ok(Self {
            changes: vec![FileChange {
                path,
                before,
                after,
            }],
            written: HashSet::new(),
        })
    }

    pub(crate) fn push_with_snapshot(
        &mut self,
        path: PathBuf,
        before: Option<Vec<u8>>,
        after: Vec<u8>,
    ) -> Result<(), ConfigFailure> {
        if after.len() as u64 > MAX_CONFIG_BYTES {
            return Err(ConfigFailure::Write);
        }
        self.changes.push(FileChange {
            path,
            before,
            after,
        });
        Ok(())
    }

    pub(crate) fn push(&mut self, path: PathBuf, after: Vec<u8>) -> Result<(), ConfigFailure> {
        if after.len() as u64 > MAX_CONFIG_BYTES {
            return Err(ConfigFailure::Write);
        }
        let before = snapshot(&path).map_err(|_| ConfigFailure::Read)?;
        self.changes.push(FileChange {
            path,
            before,
            after,
        });
        Ok(())
    }

    pub(crate) fn commit(&mut self) -> Result<(), ConfigFailure> {
        self.commit_using(&NativeConfigFiles)
    }

    pub(crate) fn commit_using(&mut self, files: &impl ConfigFiles) -> Result<(), ConfigFailure> {
        for index in 0..self.changes.len() {
            // Every exit from this loop rolls back what it already wrote.
            // A bare `?` on either snapshot used to leave the earlier files of
            // a multi-file transaction written, stranding the user half
            // configured with no error that says so. Each snapshot is bound to
            // a local first: a borrow of `self` held across the match arms
            // would block the rollback call inside them.
            let existing = files.snapshot(&self.changes[index].path);
            let current = match existing {
                Ok(value) => value,
                Err(_) => {
                    self.rollback_using(files)?;
                    return Err(ConfigFailure::Read);
                }
            };
            if current != self.changes[index].before {
                self.rollback_using(files)?;
                return Err(ConfigFailure::ExternalChange);
            }
            if files
                .write(&self.changes[index].path, &self.changes[index].after)
                .is_err()
            {
                self.rollback_using(files)?;
                return Err(ConfigFailure::Write);
            }
            self.written.insert(self.changes[index].path.clone());
            let verified = files.snapshot(&self.changes[index].path);
            let readback = match verified {
                Ok(value) => value,
                Err(_) => {
                    self.rollback_using(files)?;
                    return Err(ConfigFailure::Readback);
                }
            };
            if readback.as_deref() != Some(self.changes[index].after.as_slice()) {
                self.rollback_using(files)?;
                return Err(ConfigFailure::Readback);
            }
        }
        Ok(())
    }

    pub(crate) fn rollback(&mut self) -> Result<(), ConfigFailure> {
        self.rollback_using(&NativeConfigFiles)
    }

    /// Undo every file this transaction wrote, newest first.
    ///
    /// One file being unrestorable must not strand the others. Leaving a file
    /// that somebody else has since edited is correct — we will not clobber
    /// their work — but it used to `return` on the spot, so in a multi-file
    /// transaction (Claude Desktop writes four, Pi two) the files not yet
    /// reached stayed rewritten with no error naming them. The loop now runs to
    /// the end and reports the worst thing that happened.
    ///
    /// `Rollback` outranks `ExternalChange`: the first means a file is stuck in
    /// our state and could not be put back, the second means we deliberately
    /// left an externally owned file alone.
    pub(crate) fn rollback_using(&mut self, files: &impl ConfigFiles) -> Result<(), ConfigFailure> {
        let mut stuck = false;
        let mut externally_owned = false;
        let mut restored = HashSet::new();
        let written = std::mem::take(&mut self.written);
        for change in self
            .changes
            .iter()
            .rev()
            .filter(|change| written.contains(&change.path))
        {
            let Ok(current) = files.snapshot(&change.path) else {
                stuck = true;
                continue;
            };
            if current.as_deref() != Some(change.after.as_slice()) {
                externally_owned = true;
                continue;
            }
            if files
                .restore(&change.path, change.before.as_deref())
                .is_err()
            {
                stuck = true;
                continue;
            }
            restored.insert(change.path.clone());
        }
        // A file we could not put back stays tracked so an explicit retry still
        // knows it is in our state. A restored one must never be written again:
        // the bytes on disk may now belong to the user.
        self.written = written.difference(&restored).cloned().collect();
        if stuck {
            Err(ConfigFailure::Rollback)
        } else if externally_owned {
            Err(ConfigFailure::ExternalChange)
        } else {
            Ok(())
        }
    }
}

impl ConfigCommit for ConfigTransaction {
    fn changes(&self) -> &[FileChange] {
        self.changes()
    }
    fn commit(&mut self) -> Result<(), ConfigFailure> {
        self.commit()
    }
    fn rollback(&mut self) -> Result<(), ConfigFailure> {
        self.rollback()
    }
}

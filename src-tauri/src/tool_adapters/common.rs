//! What is left of the former shared adapter module, re-exported so adapters
//! keep one import path. Process execution moved to `infrastructure::process`;
//! model-catalog predicates moved to `domain::model`; file IO is re-exported
//! from `infrastructure::config`, which is its real home.
#[cfg(test)]
use std::{fs, path::PathBuf};

#[cfg(all(test, unix))]
use tokio::process::Command;

pub(crate) use crate::domain::model::{catalog_matches, default_matches, validate_catalog};
pub(crate) use crate::domain::{configuration::FileChange, error::ConfigFailure};
pub(crate) use crate::infrastructure::config::{files::*, ConfigTransaction as FileTransaction};

#[cfg(test)]
pub(crate) fn temporary_working_directory(label: &str) -> std::io::Result<PathBuf> {
    for attempt in 0..16u8 {
        let path = std::env::temp_dir().join(format!(
            "yeschoy-{label}-{}-{}-{attempt}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis()
        ));
        match fs::create_dir(&path) {
            Ok(()) => return Ok(path),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    }
    Err(std::io::Error::new(
        std::io::ErrorKind::AlreadyExists,
        "temporary directory collision",
    ))
}

#[cfg(all(test, unix))]
pub(crate) mod test_support {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    pub(crate) struct Script {
        directory: PathBuf,
        pub(crate) path: PathBuf,
    }

    impl Script {
        pub(crate) fn new(body: &str) -> Self {
            let directory = temporary_working_directory("synthetic-cli").unwrap();
            let path = directory.join("fixture-cli");
            fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
            fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
            Self { directory, path }
        }

        pub(crate) fn installation(&self) -> super::super::ResolvedInstallation {
            super::super::ResolvedInstallation {
                path: self.path.clone(),
            }
        }

        pub(crate) fn command(&self) -> Command {
            Command::new(&self.path)
        }
    }

    impl Drop for Script {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.directory);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transaction_preserves_unrelated_bytes_and_rolls_back() {
        let directory = temporary_working_directory("transaction-test").unwrap();
        let path = directory.join("config.json");
        atomic_write(&path, b"before\n").unwrap();
        let mut transaction = FileTransaction::stage(path.clone(), b"after\n".to_vec()).unwrap();
        transaction.commit().unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"after\n");
        transaction.rollback().unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"before\n");
        let _ = fs::remove_dir_all(directory);
    }

    #[test]
    fn transaction_rolls_back_earlier_files_when_a_later_snapshot_fails() {
        // A multi-file commit that cannot read its second target must not leave
        // the first one written. Otherwise the user ends up half configured —
        // one tool already pointing at 野菜API — while the assistant reports
        // the activation as failed and the recovery record covers neither.
        let directory = temporary_working_directory("transaction-read-failure").unwrap();
        let first = directory.join("first.json");
        let second = directory.join("second.json");
        atomic_write(&first, b"before-first").unwrap();
        atomic_write(&second, b"before-second").unwrap();

        let mut transaction =
            FileTransaction::stage(first.clone(), b"after-first".to_vec()).unwrap();
        transaction
            .push(second.clone(), b"after-second".to_vec())
            .unwrap();

        // Make the second target unreadable in a way every platform agrees on:
        // the path still exists, but it cannot be read as a file.
        fs::remove_file(&second).unwrap();
        fs::create_dir(&second).unwrap();

        assert_eq!(transaction.commit(), Err(ConfigFailure::Read));
        assert_eq!(
            fs::read(&first).unwrap(),
            b"before-first",
            "the first file must be restored when a later one cannot be read"
        );
        let _ = fs::remove_dir_all(directory);
    }

    #[test]
    fn rollback_restores_every_other_file_when_one_is_externally_owned() {
        // Rolling back used to stop at the first file somebody else had edited,
        // leaving the files it had not reached yet still carrying our values.
        // Claude Desktop writes four files, so the untouched remainder could be
        // most of the transaction — with no error naming them.
        let directory = temporary_working_directory("rollback-partial").unwrap();
        let first = directory.join("first.json");
        let second = directory.join("second.json");
        let third = directory.join("third.json");
        atomic_write(&first, b"before-first").unwrap();
        atomic_write(&second, b"before-second").unwrap();
        atomic_write(&third, b"before-third").unwrap();

        let mut transaction =
            FileTransaction::stage(first.clone(), b"after-first".to_vec()).unwrap();
        transaction
            .push(second.clone(), b"after-second".to_vec())
            .unwrap();
        transaction
            .push(third.clone(), b"after-third".to_vec())
            .unwrap();
        transaction.commit().unwrap();

        // Rollback walks newest first, so an outside edit to the middle file is
        // reached before the first file is restored.
        atomic_write(&second, b"edited-by-someone-else").unwrap();
        assert_eq!(transaction.rollback(), Err(ConfigFailure::ExternalChange));

        assert_eq!(
            fs::read(&second).unwrap(),
            b"edited-by-someone-else",
            "a file somebody else changed is left alone"
        );
        assert_eq!(
            fs::read(&third).unwrap(),
            b"before-third",
            "the file reached before the conflict is restored"
        );
        assert_eq!(
            fs::read(&first).unwrap(),
            b"before-first",
            "the file after the conflict must be restored too"
        );
        let _ = fs::remove_dir_all(directory);
    }

    #[cfg(unix)]
    #[test]
    fn a_world_readable_secret_file_is_narrowed_before_it_is_rewritten() {
        use std::os::unix::fs::PermissionsExt;
        // WorkBuddy is the one adapter with no credential-helper indirection,
        // so its config file holds a live relay key. A user who already had
        // that file at 0644 kept a world-readable billable credential.
        let directory = temporary_working_directory("restrict-owner").unwrap();
        let path = directory.join("models.json");
        atomic_write(&path, b"{}").unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();

        narrow_third_party_file(&path).unwrap();
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );

        // Rewriting keeps it narrow: atomic_write copies the target's mode.
        atomic_write(&path, b"{\"models\":[]}").unwrap();
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        // A path that does not exist is not an error; new files are born 0600.
        narrow_third_party_file(&directory.join("absent.json")).unwrap();
        // Files we create ourselves go through the stricter path, which on
        // Windows also replaces the DACL.
        let ours = directory.join("break-glass");
        atomic_write(&ours, b"original").unwrap();
        fs::set_permissions(&ours, fs::Permissions::from_mode(0o644)).unwrap();
        restrict_to_owner(&ours).unwrap();
        assert_eq!(
            fs::metadata(&ours).unwrap().permissions().mode() & 0o777,
            0o600
        );
        let _ = fs::remove_dir_all(directory);
    }

    #[test]
    fn transaction_fails_closed_on_external_change() {
        let directory = temporary_working_directory("transaction-conflict").unwrap();
        let path = directory.join("config.json");
        atomic_write(&path, b"before").unwrap();
        let mut transaction = FileTransaction::stage(path.clone(), b"after".to_vec()).unwrap();
        atomic_write(&path, b"external").unwrap();
        assert_eq!(transaction.commit(), Err(ConfigFailure::ExternalChange));
        assert_eq!(fs::read(&path).unwrap(), b"external");
        let _ = fs::remove_dir_all(directory);
    }
}

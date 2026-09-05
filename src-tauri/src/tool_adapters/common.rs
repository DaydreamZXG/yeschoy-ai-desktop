use std::{
    fs::{self, File, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
    process::Stdio,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use tokio::{io::AsyncReadExt, process::Command, time::timeout};

const MAX_CONFIG_BYTES: u64 = 2 * 1024 * 1024;
const MAX_PROCESS_OUTPUT_BYTES: u64 = 256 * 1024;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ConfigFailure {
    Read,
    #[allow(dead_code)]
    // Closed shared adapter error mapping; some parsers return their own reason.
    Parse,
    ExternalChange,
    Write,
    Readback,
    Rollback,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub(crate) struct FileChange {
    pub(crate) path: PathBuf,
    pub(crate) before: Option<Vec<u8>>,
    pub(crate) after: Vec<u8>,
    #[serde(skip)]
    written: bool,
}

#[derive(Debug, Default)]
pub(crate) struct FileTransaction {
    changes: Vec<FileChange>,
}

impl FileTransaction {
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
                written: false,
            }],
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
            written: false,
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
            written: false,
        });
        Ok(())
    }

    pub(crate) fn commit(&mut self) -> Result<(), ConfigFailure> {
        for index in 0..self.changes.len() {
            let change = &self.changes[index];
            let current = snapshot(&change.path).map_err(|_| ConfigFailure::Read)?;
            if current != change.before {
                self.rollback_written()?;
                return Err(ConfigFailure::ExternalChange);
            }
            if atomic_write(&change.path, &change.after).is_err() {
                self.rollback_written()?;
                return Err(ConfigFailure::Write);
            }
            self.changes[index].written = true;
            let readback =
                snapshot(&self.changes[index].path).map_err(|_| ConfigFailure::Readback)?;
            if readback.as_deref() != Some(self.changes[index].after.as_slice()) {
                self.rollback_written()?;
                return Err(ConfigFailure::Readback);
            }
        }
        Ok(())
    }

    pub(crate) fn rollback(&mut self) -> Result<(), ConfigFailure> {
        self.rollback_written()
    }

    fn rollback_written(&mut self) -> Result<(), ConfigFailure> {
        for change in self
            .changes
            .iter_mut()
            .rev()
            .filter(|change| change.written)
        {
            let current = snapshot(&change.path).map_err(|_| ConfigFailure::Rollback)?;
            if current.as_deref() != Some(change.after.as_slice()) {
                return Err(ConfigFailure::ExternalChange);
            }
            restore(&change.path, change.before.as_deref()).map_err(|_| ConfigFailure::Rollback)?;
            change.written = false;
        }
        Ok(())
    }
}

fn epoch_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u64::MAX as u128) as u64
}

pub(crate) fn ensure_safe_target(path: &Path, maximum: u64) -> io::Result<()> {
    if !path.is_absolute()
        || path
            .components()
            .any(|part| matches!(part, std::path::Component::ParentDir))
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "relative target",
        ));
    }
    let parent = path
        .parent()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "missing parent"))?;
    // Read-only inspection must not create directories. Reject symlinks in
    // every existing ancestor, not just the immediate parent.
    for ancestor in parent.ancestors() {
        match fs::symlink_metadata(ancestor) {
            #[cfg(target_os = "macos")]
            Ok(metadata)
                if metadata.file_type().is_symlink()
                    && ((ancestor == Path::new("/var")
                        && fs::read_link(ancestor).ok().as_deref()
                            == Some(Path::new("private/var")))
                        || (ancestor == Path::new("/tmp")
                            && fs::read_link(ancestor).ok().as_deref()
                                == Some(Path::new("private/tmp")))) => {}
            Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
                return Err(io::Error::new(io::ErrorKind::InvalidData, "unsafe parent"));
            }
            Ok(_) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
    }
    if let Ok(metadata) = fs::symlink_metadata(path) {
        if metadata.file_type().is_symlink() || !metadata.is_file() || metadata.len() > maximum {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "unsafe target"));
        }
    }
    Ok(())
}

pub(crate) fn snapshot(path: &Path) -> io::Result<Option<Vec<u8>>> {
    snapshot_bounded(path, MAX_CONFIG_BYTES)
}

pub(crate) fn snapshot_bounded(path: &Path, maximum: u64) -> io::Result<Option<Vec<u8>>> {
    ensure_safe_target(path, maximum)?;
    if path.exists() {
        fs::read(path).map(Some)
    } else {
        Ok(None)
    }
}

fn temporary_file(path: &Path) -> io::Result<(PathBuf, File)> {
    let parent = path
        .parent()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "missing parent"))?;
    let name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("config");
    for attempt in 0..16u8 {
        let candidate = parent.join(format!(
            ".{name}.yeschoy-{}-{}-{attempt}.tmp",
            std::process::id(),
            epoch_ms()
        ));
        let mut options = OpenOptions::new();
        options.create_new(true).write(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        match options.open(&candidate) {
            Ok(file) => return Ok((candidate, file)),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    }
    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "temporary file collision",
    ))
}

#[cfg(target_os = "windows")]
fn replace(temp: &Path, target: &Path) -> io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{
        MoveFileExW, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH,
    };
    let from = temp
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect::<Vec<_>>();
    let to = target
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect::<Vec<_>>();
    let result = unsafe {
        MoveFileExW(
            from.as_ptr(),
            to.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    };
    (result != 0)
        .then_some(())
        .ok_or_else(io::Error::last_os_error)
}

#[cfg(not(target_os = "windows"))]
fn replace(temp: &Path, target: &Path) -> io::Result<()> {
    fs::rename(temp, target)
}

pub(crate) fn atomic_write(path: &Path, bytes: &[u8]) -> io::Result<()> {
    atomic_write_bounded(path, bytes, MAX_CONFIG_BYTES)
}

pub(crate) fn atomic_write_bounded(path: &Path, bytes: &[u8], maximum: u64) -> io::Result<()> {
    ensure_safe_target(path, maximum)?;
    if bytes.len() as u64 > maximum {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "oversized content",
        ));
    }
    fs::create_dir_all(path.parent().ok_or(io::ErrorKind::InvalidInput)?)?;
    ensure_safe_target(path, maximum)?;
    let permissions = fs::metadata(path)
        .ok()
        .map(|metadata| metadata.permissions());
    let (temp, mut file) = temporary_file(path)?;
    let result = (|| {
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        if let Some(permissions) = permissions {
            fs::set_permissions(&temp, permissions)?;
        }
        replace(&temp, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temp);
    }
    result
}

pub(crate) fn restore(path: &Path, before: Option<&[u8]>) -> io::Result<()> {
    ensure_safe_target(path, MAX_CONFIG_BYTES)?;
    match before {
        Some(bytes) => atomic_write(path, bytes),
        None if path.exists() => fs::remove_file(path),
        None => Ok(()),
    }
}

#[derive(Debug)]
pub(crate) struct ProcessResult {
    pub(crate) success: bool,
    pub(crate) stdout: Vec<u8>,
    #[allow(dead_code)] // Always drain/bound stderr, but never render raw output or credentials.
    pub(crate) stderr: Vec<u8>,
}

pub(crate) async fn run_bounded(
    mut command: Command,
    duration: Duration,
) -> Result<ProcessResult, ()> {
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    let mut child = command.spawn().map_err(|_| ())?;
    let stdout = child.stdout.take().ok_or(())?;
    let stderr = child.stderr.take().ok_or(())?;
    let stdout_task = tokio::spawn(async move {
        let mut bytes = Vec::new();
        stdout
            .take(MAX_PROCESS_OUTPUT_BYTES)
            .read_to_end(&mut bytes)
            .await
            .map(|_| bytes)
    });
    let stderr_task = tokio::spawn(async move {
        let mut bytes = Vec::new();
        stderr
            .take(MAX_PROCESS_OUTPUT_BYTES)
            .read_to_end(&mut bytes)
            .await
            .map(|_| bytes)
    });

    let status = match timeout(duration, child.wait()).await {
        Ok(Ok(status)) => status,
        Ok(Err(_)) => return Err(()),
        Err(_) => {
            let _ = child.kill().await;
            let _ = child.wait().await;
            return Err(());
        }
    };
    let stdout = stdout_task.await.map_err(|_| ())?.map_err(|_| ())?;
    let stderr = stderr_task.await.map_err(|_| ())?.map_err(|_| ())?;
    Ok(ProcessResult {
        success: status.success(),
        stdout,
        stderr,
    })
}

pub(crate) fn temporary_working_directory(label: &str) -> io::Result<PathBuf> {
    for attempt in 0..16u8 {
        let path = std::env::temp_dir().join(format!(
            "yeschoy-{label}-{}-{}-{attempt}",
            std::process::id(),
            epoch_ms()
        ));
        match fs::create_dir(&path) {
            Ok(()) => return Ok(path),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    }
    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "temporary directory collision",
    ))
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

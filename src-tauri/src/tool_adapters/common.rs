use std::{
    collections::HashSet,
    env,
    ffi::OsString,
    fs::{self, File, OpenOptions},
    io::{self, BufRead, BufReader, Read, Write},
    path::{Path, PathBuf},
    process::Stdio,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use tokio::{
    io::{AsyncRead, AsyncReadExt},
    process::Command,
    time::timeout,
};

const MAX_CONFIG_BYTES: u64 = 2 * 1024 * 1024;
const MAX_PROCESS_OUTPUT_BYTES: u64 = 256 * 1024;
const MAX_RUNTIME_SYMLINK_HOPS: usize = 8;
const MAX_RUNTIME_PATH_DIRECTORIES: usize = 16;

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
            // Every exit from this loop rolls back what it already wrote.
            // A bare `?` on either snapshot used to leave the earlier files of
            // a multi-file transaction written, stranding the user half
            // configured with no error that says so. Each snapshot is bound to
            // a local first: a borrow of `self` held across the match arms
            // would block the rollback call inside them.
            let existing = snapshot(&self.changes[index].path);
            let current = match existing {
                Ok(value) => value,
                Err(_) => {
                    self.rollback_written()?;
                    return Err(ConfigFailure::Read);
                }
            };
            if current != self.changes[index].before {
                self.rollback_written()?;
                return Err(ConfigFailure::ExternalChange);
            }
            if atomic_write(&self.changes[index].path, &self.changes[index].after).is_err() {
                self.rollback_written()?;
                return Err(ConfigFailure::Write);
            }
            self.changes[index].written = true;
            let verified = snapshot(&self.changes[index].path);
            let readback = match verified {
                Ok(value) => value,
                Err(_) => {
                    self.rollback_written()?;
                    return Err(ConfigFailure::Readback);
                }
            };
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
    fn rollback_written(&mut self) -> Result<(), ConfigFailure> {
        let mut stuck = false;
        let mut externally_owned = false;
        for change in self
            .changes
            .iter_mut()
            .rev()
            .filter(|change| change.written)
        {
            let Ok(current) = snapshot(&change.path) else {
                stuck = true;
                continue;
            };
            if current.as_deref() != Some(change.after.as_slice()) {
                externally_owned = true;
                continue;
            }
            if restore(&change.path, change.before.as_deref()).is_err() {
                stuck = true;
                continue;
            }
            change.written = false;
        }
        if stuck {
            Err(ConfigFailure::Rollback)
        } else if externally_owned {
            Err(ConfigFailure::ExternalChange)
        } else {
            Ok(())
        }
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

/// Read one bounded JSONL-style record without allocating the rest of a large
/// append-only file. The returned prefix includes its newline when present.
pub(crate) fn first_line_bounded(
    path: &Path,
    maximum_file: u64,
    maximum_line: u64,
) -> io::Result<Option<Vec<u8>>> {
    ensure_safe_target(path, maximum_file)?;
    if !path.exists() {
        return Ok(None);
    }
    let file_size = fs::metadata(path)?.len();
    let file = File::open(path)?;
    let mut reader = BufReader::new(file).take(maximum_line.saturating_add(1));
    let mut line = Vec::new();
    reader.read_until(b'\n', &mut line)?;
    if line.len() as u64 > maximum_line || (!line.ends_with(b"\n") && file_size > line.len() as u64)
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "oversized first line",
        ));
    }
    Ok(Some(line))
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

/// Atomically replace a verified prefix while streaming the untouched tail.
/// This is used for large append-only records whose metadata is the first line;
/// callers never need to hold the complete file in memory.
pub(crate) fn atomic_replace_prefix_bounded(
    path: &Path,
    expected_prefix: &[u8],
    replacement: &[u8],
    maximum: u64,
) -> io::Result<()> {
    ensure_safe_target(path, maximum)?;
    if expected_prefix.is_empty() || replacement.len() as u64 > maximum {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "invalid prefix replacement",
        ));
    }
    let permissions = fs::metadata(path)?.permissions();
    let mut input = File::open(path)?;
    let mut observed = vec![0; expected_prefix.len()];
    input.read_exact(&mut observed)?;
    if observed != expected_prefix {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "source prefix changed",
        ));
    }
    let (temp, mut output) = temporary_file(path)?;
    let result = (|| {
        output.write_all(replacement)?;
        let remaining = maximum.saturating_sub(replacement.len() as u64);
        let copied = io::copy(&mut input.take(remaining.saturating_add(1)), &mut output)?;
        if copied > remaining {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "oversized content",
            ));
        }
        output.sync_all()?;
        drop(output);
        fs::set_permissions(&temp, permissions)?;
        replace(&temp, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temp);
    }
    result
}

/// Narrow an existing file to owner-only before it is rewritten.
///
/// `atomic_write` copies the target's current permissions onto the replacement,
/// so tightening here also tightens everything written afterwards. A file that
/// does not exist yet needs nothing: `temporary_file` already creates at 0600.
///
/// For files **this application creates**: the break-glass copies of the user's
/// original configuration, which sit in our own directory.
///
/// Callers must treat an `Err` as "do not write the readable copy". On Windows
/// the ACL work below is unverifiable from a build machine, so failing closed is
/// what keeps a bug here from turning into a leak: the worst outcome is the same
/// behaviour as before the copy existed.
///
/// Use [`narrow_third_party_file`] for a file another application owns.
pub(crate) fn restrict_to_owner(path: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        match fs::symlink_metadata(path) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(io::Error::new(io::ErrorKind::InvalidData, "unsafe target"));
            }
            Ok(metadata) if metadata.permissions().mode() & 0o077 != 0 => {
                fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
            }
            Ok(_) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
    }
    #[cfg(windows)]
    {
        match fs::symlink_metadata(path) {
            Ok(_) => windows_acl::restrict_to_current_user(path)?,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
    }
    #[cfg(not(any(unix, windows)))]
    let _ = path;
    Ok(())
}

/// Narrow a file that belongs to another application.
///
/// Only the POSIX mode is touched. `chmod 600` on a config file is ordinary and
/// expected; replacing a third-party file's Windows DACL with a *protected* one
/// is not — it would strip the inherited SYSTEM and Administrators entries and
/// can surprise a backup or endpoint agent, on a file this application did not
/// create and does not own.
///
/// It would also buy almost nothing. The entries it removes belong to accounts
/// that can already read the whole user profile. What actually threatens the
/// WorkBuddy key is another *user* on the machine, which the profile ACL already
/// covers, and a cloud-sync folder, which no ACL can help with — that one is
/// covered by telling the user, in `readyWorkBuddy`.
pub(crate) fn narrow_third_party_file(path: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        match fs::symlink_metadata(path) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(io::Error::new(io::ErrorKind::InvalidData, "unsafe target"));
            }
            Ok(metadata) if metadata.permissions().mode() & 0o077 != 0 => {
                fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
            }
            Ok(_) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
    }
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}

/// Narrow a directory we own to owner-only. Same reasoning and the same
/// fail-closed contract as [`restrict_to_owner`].
pub(crate) fn restrict_directory(path: &Path) -> io::Result<()> {
    let metadata = fs::symlink_metadata(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "unsafe target"));
        }
        if metadata.permissions().mode() & 0o077 != 0 {
            fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
        }
    }
    #[cfg(windows)]
    {
        if !metadata.is_dir() {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "unsafe target"));
        }
        windows_acl::restrict_to_current_user(path)?;
    }
    #[cfg(not(any(unix, windows)))]
    let _ = metadata;
    Ok(())
}

/// Replace a path's DACL with a single entry granting the current user full
/// access, and stop it inheriting anything else.
///
/// A file under the user profile is not private by default: the profile's ACL
/// typically also grants SYSTEM and Administrators, and a machine with several
/// accounts, a managed endpoint agent, or a roaming profile can widen it
/// further. `PROTECTED_DACL_SECURITY_INFORMATION` is the part that matters —
/// without it the inherited entries come straight back.
#[cfg(windows)]
mod windows_acl {
    use std::{ffi::c_void, io, os::windows::ffi::OsStrExt, path::Path, ptr};

    use windows_sys::Win32::{
        Foundation::{CloseHandle, HANDLE},
        Security::{
            Authorization::{
                ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW,
                SetNamedSecurityInfoW, SE_FILE_OBJECT,
            },
            GetSecurityDescriptorDacl, GetTokenInformation, TokenUser, ACL,
            DACL_SECURITY_INFORMATION, PROTECTED_DACL_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR,
            TOKEN_QUERY, TOKEN_USER,
        },
        System::{
            Memory::LocalFree,
            Threading::{GetCurrentProcess, OpenProcessToken},
        },
    };

    fn refused(context: &'static str) -> io::Error {
        io::Error::new(io::ErrorKind::PermissionDenied, context)
    }

    /// The current process token's user SID, formatted as `S-1-5-21-...`.
    fn current_user_sid() -> io::Result<Vec<u16>> {
        unsafe {
            let mut token: HANDLE = ptr::null_mut();
            if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
                return Err(refused("open process token"));
            }
            let mut needed: u32 = 0;
            // The first call only sizes the buffer, so it is expected to fail.
            let _ = GetTokenInformation(token, TokenUser, ptr::null_mut(), 0, &mut needed);
            if needed == 0 {
                let _ = CloseHandle(token);
                return Err(refused("token user size"));
            }
            let mut buffer = vec![0u8; needed as usize];
            let read = GetTokenInformation(
                token,
                TokenUser,
                buffer.as_mut_ptr().cast::<c_void>(),
                needed,
                &mut needed,
            );
            let _ = CloseHandle(token);
            if read == 0 {
                return Err(refused("token user"));
            }
            let user = buffer.as_ptr().cast::<TOKEN_USER>();
            let mut raw: *mut u16 = ptr::null_mut();
            if ConvertSidToStringSidW((*user).User.Sid, &mut raw) == 0 || raw.is_null() {
                return Err(refused("sid to string"));
            }
            let mut text = Vec::new();
            let mut cursor = raw;
            while *cursor != 0 {
                text.push(*cursor);
                cursor = cursor.add(1);
            }
            LocalFree(raw.cast::<c_void>());
            Ok(text)
        }
    }

    pub(super) fn restrict_to_current_user(path: &Path) -> io::Result<()> {
        let sid = current_user_sid()?;
        // D: a DACL follows. P: protected, i.e. stop inheriting — this is the
        // load-bearing flag. Without it the entries the user profile hands down
        // (often SYSTEM and Administrators, more on a managed or roaming
        // machine) come straight back and the file is not private at all.
        // OICI propagates the entry into a directory's future contents and is
        // ignored on a file. FA is full access, for that one SID and nobody.
        let mut sddl: Vec<u16> = "D:P(A;OICI;FA;;;".encode_utf16().collect();
        sddl.extend_from_slice(&sid);
        sddl.extend(")\0".encode_utf16());
        let mut wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
        unsafe {
            let mut descriptor: PSECURITY_DESCRIPTOR = ptr::null_mut();
            if ConvertStringSecurityDescriptorToSecurityDescriptorW(
                sddl.as_ptr(),
                // SDDL_REVISION_1. Written out so the module depends on one
                // fewer name from the bindings; the revision is fixed at 1.
                1,
                &mut descriptor,
                ptr::null_mut(),
            ) == 0
            {
                return Err(refused("parse sddl"));
            }
            let mut present: i32 = 0;
            let mut defaulted: i32 = 0;
            let mut dacl: *mut ACL = ptr::null_mut();
            let read =
                GetSecurityDescriptorDacl(descriptor, &mut present, &mut dacl, &mut defaulted);
            let applied = read != 0
                && present != 0
                && !dacl.is_null()
                && SetNamedSecurityInfoW(
                    wide.as_mut_ptr(),
                    SE_FILE_OBJECT,
                    DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
                    ptr::null_mut(),
                    ptr::null_mut(),
                    dacl,
                    ptr::null_mut(),
                ) == 0;
            LocalFree(descriptor.cast::<c_void>());
            if applied {
                Ok(())
            } else {
                Err(refused("apply dacl"))
            }
        }
    }
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ProcessFailure {
    Start,
    TimedOut,
    Wait,
    OutputRead,
    OutputLimit,
}

async fn read_process_output(reader: impl AsyncRead + Unpin) -> Result<Vec<u8>, ProcessFailure> {
    let mut bytes = Vec::new();
    reader
        .take(MAX_PROCESS_OUTPUT_BYTES + 1)
        .read_to_end(&mut bytes)
        .await
        .map_err(|_| ProcessFailure::OutputRead)?;
    if bytes.len() as u64 > MAX_PROCESS_OUTPUT_BYTES {
        return Err(ProcessFailure::OutputLimit);
    }
    Ok(bytes)
}

/// Returns a closed, bounded set of directories needed to execute a
/// native-discovered CLI wrapper. GUI applications on macOS commonly inherit
/// only `/usr/bin:/bin:/usr/sbin:/sbin`, while npm launchers use
/// `#!/usr/bin/env node`. Keeping every symlink hop's parent lets `env` find the
/// runtime that owns the wrapper without changing the assistant's global PATH.
pub(crate) fn cli_runtime_directories(executable: &Path) -> Vec<PathBuf> {
    if !executable.is_absolute() {
        return Vec::new();
    }
    let mut directories = Vec::new();
    let mut visited = HashSet::new();
    let mut current = executable.to_path_buf();
    for hop in 0..=MAX_RUNTIME_SYMLINK_HOPS {
        if !visited.insert(current.clone()) {
            break;
        }
        if let Some(parent) = current.parent() {
            let directory = fs::canonicalize(parent).unwrap_or_else(|_| parent.to_path_buf());
            if directory.is_absolute() && directory.is_dir() && !directories.contains(&directory) {
                directories.push(directory);
                if directories.len() >= MAX_RUNTIME_PATH_DIRECTORIES {
                    break;
                }
            }
        }
        if hop == MAX_RUNTIME_SYMLINK_HOPS {
            break;
        }
        let Ok(metadata) = fs::symlink_metadata(&current) else {
            break;
        };
        if !metadata.file_type().is_symlink() {
            break;
        }
        let Ok(target) = fs::read_link(&current) else {
            break;
        };
        current = if target.is_absolute() {
            target
        } else if let Some(parent) = current.parent() {
            parent.join(target)
        } else {
            break;
        };
    }
    directories
}

pub(crate) fn cli_runtime_path(executable: &Path) -> Option<OsString> {
    let mut directories = cli_runtime_directories(executable);
    if let Some(inherited) = env::var_os("PATH") {
        for directory in env::split_paths(&inherited) {
            if !directories.contains(&directory) {
                directories.push(directory);
            }
        }
    }
    (!directories.is_empty())
        .then(|| env::join_paths(directories).ok())
        .flatten()
}

pub(crate) fn apply_cli_runtime_path(command: &mut Command, executable: &Path) {
    if let Some(path) = cli_runtime_path(executable) {
        command.env("PATH", path);
    }
}

pub(crate) async fn run_bounded(
    mut command: Command,
    duration: Duration,
) -> Result<ProcessResult, ProcessFailure> {
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    let mut child = command.spawn().map_err(|_| ProcessFailure::Start)?;
    let stdout = child.stdout.take().ok_or(ProcessFailure::OutputRead)?;
    let stderr = child.stderr.take().ok_or(ProcessFailure::OutputRead)?;
    // Own all three futures here. Dropping this future cancels both pipe reads;
    // no detached reader tasks survive an error, deadline or caller cancellation.
    // The deadline includes EOF even if a child exits while descendants retain
    // its pipes. Output limits fail instead of accepting a truncated response.
    let observed = timeout(duration, async {
        let (status, stdout, stderr) = tokio::try_join!(
            async { child.wait().await.map_err(|_| ProcessFailure::Wait) },
            read_process_output(stdout),
            read_process_output(stderr),
        )?;
        Ok::<_, ProcessFailure>(ProcessResult {
            success: status.success(),
            stdout,
            stderr,
        })
    })
    .await;
    match observed {
        Ok(Ok(result)) => Ok(result),
        failure => {
            // Nonblocking kill plus kill_on_drop uses Tokio's child reaper. Do
            // not add an unbounded wait after the verification deadline.
            let _ = child.start_kill();
            match failure {
                Ok(Err(error)) => Err(error),
                Err(_) => Err(ProcessFailure::TimedOut),
                Ok(Ok(_)) => unreachable!(),
            }
        }
    }
}

#[cfg(test)]
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

// Shared adapter predicates concern only public model IDs, never credentials.
// They were hosted by the retired loopback Chat gateway.
pub(crate) fn validate_catalog(default: &str, ids: &[String]) -> Result<(), super::AdapterFailure> {
    let unique: HashSet<_> = ids.iter().collect();
    if ids.is_empty()
        || ids.len() > 200
        || unique.len() != ids.len()
        || !ids.iter().any(|id| id == default)
        || ids
            .iter()
            .any(|id| id.is_empty() || id.chars().count() > 200 || id.chars().any(char::is_control))
    {
        return Err(super::AdapterFailure::ConfigurationFailed(
            "configuration_parse_failed",
        ));
    }
    Ok(())
}

pub(crate) fn catalog_matches(
    value: &serde_json::Value,
    field: Option<&str>,
    ids: &[String],
) -> bool {
    value.as_array().is_some_and(|rows| {
        let actual: Option<HashSet<&str>> = rows
            .iter()
            .map(|row| field.map_or(row, |key| &row[key]).as_str())
            .collect();
        rows.len() == ids.len()
            && actual.is_some_and(|actual| {
                actual.len() == ids.len() && ids.iter().all(|id| actual.contains(id.as_str()))
            })
    })
}

pub(crate) fn default_matches(
    actual: Option<&str>,
    default: &str,
    ids: &[String],
    strict: bool,
) -> bool {
    actual.is_some_and(|actual| {
        if strict {
            actual == default
        } else {
            ids.iter().any(|id| id == actual)
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[tokio::test]
    async fn bounded_process_distinguishes_start_exit_timeout_and_output_limit() {
        use test_support::Script;
        let success = Script::new("printf 'YESCHOY_OK\\n'; printf 'synthetic diagnostic\\n' >&2");
        let result = run_bounded(success.command(), Duration::from_secs(2))
            .await
            .unwrap();
        assert!(result.success && result.stdout == b"YESCHOY_OK\n");
        assert_eq!(result.stderr, b"synthetic diagnostic\n");

        let failure = Script::new("printf 'YESCHOY_OK\\n'; exit 17");
        assert!(
            !run_bounded(failure.command(), Duration::from_secs(2))
                .await
                .unwrap()
                .success
        );
        let missing = Command::new(success.path.with_file_name("does-not-exist"));
        assert_eq!(
            run_bounded(missing, Duration::from_secs(2))
                .await
                .unwrap_err(),
            ProcessFailure::Start
        );

        let stalled = Script::new("exec /bin/sleep 2");
        assert_eq!(
            run_bounded(stalled.command(), Duration::from_millis(30))
                .await
                .unwrap_err(),
            ProcessFailure::TimedOut
        );
        let flooded = Script::new("head -c 262145 /dev/zero");
        assert_eq!(
            run_bounded(flooded.command(), Duration::from_secs(2))
                .await
                .unwrap_err(),
            ProcessFailure::OutputLimit
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn cli_runtime_path_follows_wrapper_chain_without_global_mutation() {
        use std::os::unix::fs::{symlink, PermissionsExt};

        let root = temporary_working_directory("cli-runtime-path").unwrap();
        let shims = root.join("shims");
        let runtime_bin = root.join("runtime/bin");
        let package = root.join("runtime/lib/package");
        fs::create_dir_all(&shims).unwrap();
        fs::create_dir_all(&runtime_bin).unwrap();
        fs::create_dir_all(&package).unwrap();
        let target = package.join("cli.js");
        fs::write(&target, b"#!/usr/bin/env node\nfixture\n").unwrap();
        fs::set_permissions(&target, fs::Permissions::from_mode(0o700)).unwrap();
        let owned_wrapper = runtime_bin.join("dsh");
        symlink("../lib/package/cli.js", &owned_wrapper).unwrap();
        let discovered_wrapper = shims.join("dsh");
        symlink(&owned_wrapper, &discovered_wrapper).unwrap();
        let node = runtime_bin.join("node");
        fs::write(&node, b"#!/bin/sh\nprintf 'YESCHOY_OK\\n'\n").unwrap();
        fs::set_permissions(&node, fs::Permissions::from_mode(0o700)).unwrap();

        let directories = cli_runtime_directories(&discovered_wrapper);
        assert_eq!(directories[0], shims.canonicalize().unwrap());
        assert!(directories.contains(&runtime_bin.canonicalize().unwrap()));
        assert!(directories.len() <= MAX_RUNTIME_PATH_DIRECTORIES);
        let original_path = env::var_os("PATH");
        let mut command = Command::new(&discovered_wrapper);
        apply_cli_runtime_path(&mut command, &discovered_wrapper);
        let result = run_bounded(command, Duration::from_secs(2)).await.unwrap();
        assert!(result.success && result.stdout == b"YESCHOY_OK\n");
        assert_eq!(env::var_os("PATH"), original_path);
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn bounded_process_deadline_includes_pipes_retained_after_parent_exit() {
        let script = test_support::Script::new("/bin/sleep 0.4 &\nexit 0");
        let result = timeout(
            Duration::from_millis(300),
            run_bounded(script.command(), Duration::from_millis(30)),
        )
        .await;
        assert_eq!(
            result
                .expect("must not wait for the inherited pipe")
                .unwrap_err(),
            ProcessFailure::TimedOut
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn cancelling_bounded_process_reaps_its_owned_child() {
        let script = test_support::Script::new(
            "printf '%s' $$ > \"$YESCHOY_TEST_PID_FILE\"\nexec /bin/sleep 10",
        );
        let marker = script.path.with_file_name("pid");
        let mut command = script.command();
        command.env("YESCHOY_TEST_PID_FILE", &marker);
        let pending = tokio::spawn(run_bounded(command, Duration::from_secs(20)));
        let pid = timeout(Duration::from_secs(2), async {
            loop {
                if let Ok(value) = fs::read_to_string(&marker) {
                    if let Ok(pid) = value.parse::<u32>() {
                        break pid;
                    }
                }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .unwrap();
        pending.abort();
        assert!(pending.await.unwrap_err().is_cancelled());
        timeout(Duration::from_secs(2), async {
            loop {
                let status = Command::new("/bin/kill")
                    .args(["-0", &pid.to_string()])
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .status()
                    .await
                    .unwrap();
                if !status.success() {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .expect("cancelled verification child must be reaped");
    }

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
        assert_eq!(fs::metadata(&ours).unwrap().permissions().mode() & 0o777, 0o600);
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

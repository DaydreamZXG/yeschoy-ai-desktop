//! Bounded, symlink-safe native configuration IO and owner-only atomic replacement.
use crate::application::ports::configuration::ConfigFiles;
use std::{
    fs::{self, File, OpenOptions},
    io::{self, BufRead, BufReader, Read, Write},
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};
pub(crate) const MAX_CONFIG_BYTES: u64 = 2 * 1024 * 1024;
pub(crate) struct NativeConfigFiles;
impl ConfigFiles for NativeConfigFiles {
    fn snapshot(&self, path: &Path) -> Result<Option<Vec<u8>>, ()> {
        snapshot(path).map_err(|_| ())
    }
    fn write(&self, path: &Path, bytes: &[u8]) -> Result<(), ()> {
        atomic_write(path, bytes).map_err(|_| ())
    }
    fn restore(&self, path: &Path, before: Option<&[u8]>) -> Result<(), ()> {
        restore(path, before).map_err(|_| ())
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
        Foundation::{CloseHandle, LocalFree, HANDLE},
        Security::{
            Authorization::{
                ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW,
                SetNamedSecurityInfoW, SE_FILE_OBJECT,
            },
            GetSecurityDescriptorDacl, GetTokenInformation, TokenUser, ACL,
            DACL_SECURITY_INFORMATION, PROTECTED_DACL_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR,
            TOKEN_QUERY, TOKEN_USER,
        },
        System::Threading::{GetCurrentProcess, OpenProcessToken},
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

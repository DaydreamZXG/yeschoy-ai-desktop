use super::Result;
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
};

pub(super) fn io_error(error: std::io::Error) -> &'static str {
    match error.raw_os_error() {
        Some(28 | 112) => "disk_full",
        _ if error.kind() == std::io::ErrorKind::PermissionDenied => "permission_denied",
        _ => "cache_unavailable",
    }
}

pub(super) fn directory(path: &Path) -> Result<()> {
    match fs::symlink_metadata(path) {
        Ok(meta) if meta.is_dir() && !redirected(&meta) => Ok(()),
        Ok(_) => Err("unsafe_cache"),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            fs::create_dir(path).map_err(io_error)?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                fs::set_permissions(path, fs::Permissions::from_mode(0o700)).map_err(io_error)?;
            }
            Ok(())
        }
        Err(error) => Err(io_error(error)),
    }
}

pub(super) fn regular(path: &Path) -> Result<Option<u64>> {
    match fs::symlink_metadata(path) {
        Ok(meta) => {
            if !meta.is_file() || redirected(&meta) {
                return Err("unsafe_cache");
            }
            #[cfg(unix)]
            {
                use std::os::unix::fs::MetadataExt;
                if meta.nlink() != 1 {
                    return Err("unsafe_cache");
                }
            }
            Ok(Some(meta.len()))
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(io_error(error)),
    }
}

pub(super) fn open(path: &Path, append: bool) -> Result<File> {
    regular(path)?;
    let mut options = OpenOptions::new();
    options.create(true).write(true).append(append);
    secure_options(&mut options);
    let file = options.open(path).map_err(io_error)?;
    validate_handle(&file)?;
    // Never truncate until the opened handle, not just its path, is checked.
    if !append {
        file.set_len(0).map_err(io_error)?;
    }
    Ok(file)
}

fn redirected(meta: &fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        meta.file_attributes() & 0x400 != 0
    }
    #[cfg(not(windows))]
    {
        meta.file_type().is_symlink()
    }
}

fn secure_options(options: &mut OpenOptions) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
        #[cfg(target_os = "macos")]
        options.custom_flags(0x100); // O_NOFOLLOW (Darwin SDK).
        #[cfg(target_os = "linux")]
        options.custom_flags(0x20000);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.custom_flags(0x00200000).share_mode(1); // Open reparse point itself; deny writers/deletion.
    }
}

fn validate_handle(file: &File) -> Result<()> {
    let meta = file.metadata().map_err(io_error)?;
    if !meta.is_file() || redirected(&meta) {
        return Err("unsafe_cache");
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if meta.nlink() != 1 {
            return Err("unsafe_cache");
        }
    }
    #[cfg(windows)]
    {
        use std::os::windows::io::AsRawHandle;
        #[link(name = "kernel32")]
        unsafe extern "system" {
            fn GetFileInformationByHandle(handle: *mut std::ffi::c_void, info: *mut u32) -> i32;
        }
        // BY_HANDLE_FILE_INFORMATION contains 13 DWORDs; nNumberOfLinks is 10.
        let mut info = [0u32; 13];
        // SAFETY: live file handle and correctly sized/aligned output storage.
        if unsafe { GetFileInformationByHandle(file.as_raw_handle(), info.as_mut_ptr()) } == 0
            || info[10] != 1
        {
            return Err("unsafe_cache");
        }
    }
    Ok(())
}

fn read_handle(path: &Path) -> Result<File> {
    let mut options = OpenOptions::new();
    options.read(true);
    secure_options(&mut options);
    let file = options.open(path).map_err(io_error)?;
    validate_handle(&file)?;
    Ok(file)
}

pub(super) fn read_small(path: &Path) -> Result<Vec<u8>> {
    let file = read_handle(path)?;
    if file.metadata().map_err(io_error)?.len() > 4096 {
        return Err("unsafe_cache");
    }
    let mut bytes = Vec::new();
    file.take(4097).read_to_end(&mut bytes).map_err(io_error)?;
    if bytes.len() > 4096 {
        return Err("unsafe_cache");
    }
    Ok(bytes)
}

pub(super) fn write(path: &Path, value: &[u8]) -> Result<()> {
    let mut file = open(path, false)?;
    file.write_all(value).map_err(io_error)?;
    file.sync_all().map_err(io_error)
}

pub(super) fn digest(path: &Path) -> Result<String> {
    regular(path)?.ok_or("download_incomplete")?;
    let mut file = read_handle(path)?;
    let mut hash = Sha256::new();
    let mut bytes = [0u8; 64 * 1024];
    loop {
        let size = file.read(&mut bytes).map_err(io_error)?;
        if size == 0 {
            break;
        }
        hash.update(&bytes[..size]);
    }
    Ok(format!("{:x}", hash.finalize()))
}

#[cfg(any(target_os = "macos", test))]
pub(super) fn unique_dir(parent: &Path) -> Result<PathBuf> {
    let path = parent.join(format!(".yeschoy-install-{}", super::new_id()?));
    // create_dir is exclusive: never adopt or remove someone else's staging dir.
    fs::create_dir(&path).map_err(io_error)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).map_err(io_error)?;
    }
    Ok(path)
}

// `test` alone was wider than the two things this function needs: its
// `publish_handoff` exists only under `windows` or `test + macos`, and its only
// test is macOS-gated. Under `cargo test` on Linux it therefore compiled a call
// to a function that was not there (E0425). CI runs the native suite on Windows
// and both macOS runners, so nothing caught it; it only shows up building the
// test target on Linux.
#[cfg(any(windows, all(test, target_os = "macos")))]
pub(super) fn handoff_package(folder: &Path, file: &Path, hash: &str) -> Result<PathBuf> {
    if hash.len() != 64 || !hash.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err("invalid_download");
    }
    let root = folder.join("opened-packages");
    directory(&root)?;
    let destination = root.join(format!("{hash}.msix"));
    if regular(&destination)?.is_some() {
        return if digest(&destination)? == hash {
            Ok(destination)
        } else {
            Err("download_changed")
        };
    }
    // A handed-off package may still be open in Windows after our guidance ends.
    // Never rewrite or delete it to make room for another version.
    if fs::read_dir(&root).map_err(io_error)?.count() >= 2 {
        return Err("cache_full");
    }
    let mut input = read_handle(file)?;
    let pending = folder.join("handoff.pending");
    let mut output = open(&pending, false)?;
    let result = std::io::copy(&mut input, &mut output)
        .map_err(io_error)
        .and_then(|_| output.sync_all().map_err(io_error));
    drop(output);
    result?;
    if digest(&pending)? != hash {
        return Err("download_changed");
    }
    publish_handoff(&pending, &destination)?;
    Ok(destination)
}

#[cfg(all(test, target_os = "macos"))]
fn publish_handoff(from: &Path, to: &Path) -> Result<()> {
    super::platform::publish_exclusive(from, to)
}
#[cfg(windows)]
fn publish_handoff(from: &Path, to: &Path) -> Result<()> {
    use std::os::windows::ffi::OsStrExt;
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn MoveFileExW(from: *const u16, to: *const u16, flags: u32) -> i32;
    }
    let from: Vec<u16> = from.as_os_str().encode_wide().chain(Some(0)).collect();
    let to: Vec<u16> = to.as_os_str().encode_wide().chain(Some(0)).collect();
    // SAFETY: valid terminated paths. WRITE_THROUGH, deliberately no REPLACE_EXISTING.
    if unsafe { MoveFileExW(from.as_ptr(), to.as_ptr(), 8) } == 0 {
        return Err("cache_unavailable");
    }
    Ok(())
}

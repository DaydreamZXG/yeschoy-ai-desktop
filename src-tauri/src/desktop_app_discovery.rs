use crate::desktop_app_discovery_core::{
    classify, DesktopAppObservation, DesktopAppResult, DesktopAppSpec, DesktopLaunchTarget,
    LocationHint, DESKTOP_APP_SPECS, PRIMARY_DESKTOP_APP_SPECS,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::env;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

#[cfg(target_os = "windows")]
use windows_sys::Win32::Storage::FileSystem::{
    GetFileVersionInfoSizeW, GetFileVersionInfoW, VerQueryValueW, VS_FIXEDFILEINFO,
};
#[cfg(target_os = "windows")]
use winreg::enums::{KEY_READ, KEY_WOW64_32KEY, KEY_WOW64_64KEY};
#[cfg(target_os = "windows")]
use winreg::{RegKey, HKCR, HKCU, HKLM};

#[cfg(target_os = "macos")]
const MAX_PLIST_BYTES: u64 = 256 * 1024;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DesktopAppScanRequest {
    request_id: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DesktopAppScanResponse {
    request_id: String,
    platform: &'static str,
    started_at_epoch_ms: u64,
    completed_at_epoch_ms: u64,
    apps: Vec<DesktopAppProjection>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct DesktopAppProjection {
    app_id: &'static str,
    display_name: &'static str,
    status: &'static str,
    version: String,
    candidate_count: usize,
    location_hint: &'static str,
    bundle_identifier: String,
    configuration_status: &'static str,
    reason_code: &'static str,
}

impl From<DesktopAppResult> for DesktopAppProjection {
    fn from(result: DesktopAppResult) -> Self {
        Self {
            app_id: result.app_id,
            display_name: result.display_name,
            status: result.status,
            version: result.version,
            candidate_count: result.candidate_count,
            location_hint: result.location_hint.as_str(),
            bundle_identifier: result.bundle_identifier,
            configuration_status: result.configuration_status,
            reason_code: result.reason_code,
        }
    }
}

#[derive(Clone, Debug)]
struct Candidate {
    path: PathBuf,
    location_hint: LocationHint,
    bundle_identifier: String,
    version: String,
    launch_target: DesktopLaunchTarget,
}

#[derive(Clone, Debug)]
pub(crate) struct DesktopActivationCandidate {
    pub(crate) path: PathBuf,
    pub(crate) location_hint: &'static str,
    pub(crate) version: String,
}

pub(crate) fn activation_candidates(app_id: &str) -> Vec<DesktopActivationCandidate> {
    let Some(spec) = DESKTOP_APP_SPECS.into_iter().find(|spec| spec.id == app_id) else {
        return Vec::new();
    };

    #[cfg(target_os = "macos")]
    let candidates = discover_macos_candidates(spec);
    #[cfg(target_os = "windows")]
    let candidates = discover_windows_candidates(spec);
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let candidates: Vec<Candidate> = Vec::new();

    candidates
        .into_iter()
        .filter(|candidate| candidate.path.is_absolute() && activation_path_exists(&candidate.path))
        .map(|candidate| DesktopActivationCandidate {
            path: candidate.path,
            location_hint: candidate.location_hint.as_str(),
            version: candidate.version,
        })
        .collect()
}

/// Recheck the exact native-discovered installation when opening. In
/// particular a failed/stale package lookup must not become an EXE fallback.
pub(crate) fn resolve_launch_target(app_id: &str, path: &Path) -> Option<DesktopLaunchTarget> {
    let spec = DESKTOP_APP_SPECS
        .into_iter()
        .find(|spec| spec.id == app_id)?;
    if !path.is_absolute() {
        return None;
    }
    let canonical = std::fs::canonicalize(path).ok()?;
    #[cfg(target_os = "macos")]
    let candidates = discover_macos_candidates(spec);
    #[cfg(target_os = "windows")]
    let candidates = discover_windows_candidates(spec);
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let candidates: Vec<Candidate> = {
        let _ = spec;
        Vec::new()
    };
    select_launch_target(candidates, &canonical)
}

fn select_launch_target(candidates: Vec<Candidate>, path: &Path) -> Option<DesktopLaunchTarget> {
    let mut matches = candidates
        .into_iter()
        .filter(|candidate| candidate.path == path);
    let first = matches.next()?;
    matches.next().is_none().then_some(first.launch_target)
}

#[cfg(target_os = "macos")]
fn activation_path_exists(path: &Path) -> bool {
    path.is_dir() && path.extension().is_some_and(|extension| extension == "app")
}

#[cfg(target_os = "windows")]
fn activation_path_exists(path: &Path) -> bool {
    path.is_file()
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn activation_path_exists(_path: &Path) -> bool {
    false
}

/// Implements the frozen `desktop-app-discovery@v1` query.
#[tauri::command]
pub fn scan_desktop_apps_read_only(
    request: DesktopAppScanRequest,
) -> Result<DesktopAppScanResponse, String> {
    validate_request_id(&request.request_id)?;
    let started_at_epoch_ms = unix_epoch_ms();
    let apps = PRIMARY_DESKTOP_APP_SPECS
        .into_iter()
        .map(|spec| DesktopAppProjection::from(classify(spec, observe_app(spec))))
        .collect();

    Ok(DesktopAppScanResponse {
        request_id: request.request_id,
        platform: platform_name(),
        started_at_epoch_ms,
        completed_at_epoch_ms: unix_epoch_ms().max(started_at_epoch_ms),
        apps,
    })
}

fn observe_app(spec: DesktopAppSpec) -> DesktopAppObservation {
    #[cfg(target_os = "macos")]
    {
        let candidates = discover_macos_candidates(spec);
        classify_candidates(candidates)
    }

    #[cfg(target_os = "windows")]
    {
        let candidates = discover_windows_candidates(spec);
        classify_candidates(candidates)
    }

    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        let _ = spec;
        DesktopAppObservation::Unsupported
    }
}

fn classify_candidates(candidates: Vec<Candidate>) -> DesktopAppObservation {
    match candidates.as_slice() {
        [] => DesktopAppObservation::NotFound,
        [candidate] => DesktopAppObservation::Found {
            version: candidate.version.clone(),
            location_hint: candidate.location_hint,
            bundle_identifier: candidate.bundle_identifier.clone(),
        },
        many => DesktopAppObservation::Multiple {
            candidate_count: many.len(),
        },
    }
}

#[cfg(target_os = "macos")]
fn discover_macos_candidates(spec: DesktopAppSpec) -> Vec<Candidate> {
    let app_names: &[&str] = match spec.id {
        "claude_desktop" => &["Claude.app"],
        "codex_desktop" => &["ChatGPT.app", "Codex.app"],
        "workbuddy" => &["WorkBuddy.app"],
        _ => &[],
    };
    let mut roots = vec![(PathBuf::from("/Applications"), LocationHint::Applications)];
    if let Some(home) = env::var_os("HOME") {
        roots.push((
            PathBuf::from(home).join("Applications"),
            LocationHint::UserApplications,
        ));
    }

    let mut accepted = HashMap::new();
    for (root, location_hint) in roots {
        for app_name in app_names {
            let path = root.join(app_name);
            let Some((bundle_identifier, version)) = read_macos_bundle_metadata(&path) else {
                continue;
            };
            if bundle_identifier != spec.expected_bundle_id {
                continue;
            }
            let canonical = std::fs::canonicalize(&path).unwrap_or(path);
            accepted.entry(canonical.clone()).or_insert(Candidate {
                path: canonical.clone(),
                location_hint,
                bundle_identifier,
                version,
                launch_target: DesktopLaunchTarget::MacBundle(canonical),
            });
        }
    }
    let mut candidates: Vec<_> = accepted.into_values().collect();
    candidates.sort_by(|left, right| left.path.cmp(&right.path));
    candidates.truncate(8);
    candidates
}

#[cfg(target_os = "macos")]
fn read_macos_bundle_metadata(app_path: &Path) -> Option<(String, String)> {
    let plist_path = app_path.join("Contents/Info.plist");
    let metadata = plist_path.metadata().ok()?;
    if !metadata.is_file() || metadata.len() > MAX_PLIST_BYTES {
        return None;
    }
    let source = std::fs::read_to_string(plist_path).ok()?;
    let bundle_id = plist_string(&source, "CFBundleIdentifier")?;
    let version = plist_string(&source, "CFBundleShortVersionString").unwrap_or_default();
    Some((bundle_id, version))
}

#[cfg(target_os = "macos")]
fn plist_string(source: &str, key: &str) -> Option<String> {
    let marker = format!("<key>{key}</key>");
    let tail = source.split_once(&marker)?.1;
    let value = tail.split_once("<string>")?.1.split_once("</string>")?.0;
    let value = value.trim();
    if value.is_empty() || value.len() > 256 || value.contains(['<', '>', '\n', '\r']) {
        return None;
    }
    Some(value.to_string())
}

#[cfg(target_os = "windows")]
fn discover_windows_candidates(spec: DesktopAppSpec) -> Vec<Candidate> {
    let mut accepted = HashMap::new();
    for candidate in discover_windows_package_candidates(spec)
        .into_iter()
        .chain(discover_windows_uninstall_candidates(spec))
    {
        accepted.entry(candidate.path.clone()).or_insert(candidate);
    }
    if let Some(root) = env::var_os("LOCALAPPDATA") {
        collect_windows_path_candidates(
            &mut accepted,
            spec,
            PathBuf::from(root),
            LocationHint::LocalAppData,
            windows_relative_paths(spec.id, false),
        );
    }
    for variable in ["ProgramFiles", "ProgramW6432"] {
        if let Some(root) = env::var_os(variable) {
            collect_windows_path_candidates(
                &mut accepted,
                spec,
                PathBuf::from(root),
                LocationHint::ProgramFiles,
                windows_relative_paths(spec.id, true),
            );
        }
    }
    let mut candidates: Vec<_> = accepted.into_values().collect();
    candidates.sort_by(|left, right| left.path.cmp(&right.path));
    candidates.truncate(8);
    if !candidates.is_empty() {
        return candidates;
    }

    discover_windows_protocol_candidate(spec)
}

#[cfg(any(target_os = "windows", test))]
fn windows_app_relative_executables(app_id: &str) -> &'static [&'static str] {
    match app_id {
        "claude_desktop" => &[
            "Claude.exe",
            "app\\Claude.exe",
            "Claude\\Claude.exe",
            "VFS\\ProgramFilesX64\\Anthropic\\Claude\\Claude.exe",
        ],
        "codex_desktop" => &[
            "Codex.exe",
            "ChatGPT.exe",
            "app\\Codex.exe",
            "app\\ChatGPT.exe",
            "OpenAI\\Codex\\Codex.exe",
            "OpenAI\\ChatGPT\\ChatGPT.exe",
            "VFS\\ProgramFilesX64\\OpenAI\\ChatGPT\\ChatGPT.exe",
        ],
        "workbuddy" => &[
            "WorkBuddy.exe",
            "app\\WorkBuddy.exe",
            "WorkBuddy\\WorkBuddy.exe",
        ],
        _ => &[],
    }
}

#[cfg(target_os = "windows")]
fn executable_below(root: &Path, app_id: &str) -> Option<PathBuf> {
    windows_program_below(root, app_id, |path| {
        windows_standalone_identity_matches(app_id, path)
    })
}

#[cfg(any(target_os = "windows", test))]
fn windows_program_below(
    root: &Path,
    app_id: &str,
    verify: impl Fn(&Path) -> bool,
) -> Option<PathBuf> {
    if !root.is_absolute() {
        return None;
    }
    let canonical_root = std::fs::canonicalize(root).ok()?;
    let resolve = |root: &Path| {
        windows_app_relative_executables(app_id)
            .iter()
            .map(|relative| {
                relative
                    .split('\\')
                    .fold(root.to_path_buf(), |path, part| path.join(part))
            })
            .filter(|path| path.is_file())
            .filter_map(|path| std::fs::canonicalize(path).ok())
            .find(|path| path.starts_with(&canonical_root) && verify(path))
    };
    if let Some(path) = resolve(&canonical_root) {
        return Some(path);
    }
    // Squirrel installers keep the GUI in app-<version>. Inspect only a
    // bounded, shallow product directory; never execute Update.exe arguments.
    let entries = std::fs::read_dir(&canonical_root)
        .ok()?
        .take(257)
        .collect::<Result<Vec<_>, _>>()
        .ok()?;
    if entries.len() > 256 {
        return None;
    }
    let mut versioned = entries
        .into_iter()
        .filter_map(|entry| {
            let name = entry.file_name();
            let version = name.to_str()?.strip_prefix("app-")?;
            let parts = version.split('.').collect::<Vec<_>>();
            if !(2..=4).contains(&parts.len())
                || !parts
                    .iter()
                    .all(|part| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit()))
            {
                return None;
            }
            let path = std::fs::canonicalize(entry.path()).ok()?;
            (path.is_dir() && path.starts_with(&canonical_root))
                .then(|| (windows_version_key(version), path))
        })
        .collect::<Vec<_>>();
    versioned.sort_by(|left, right| right.0.cmp(&left.0).then_with(|| left.1.cmp(&right.1)));
    let mut versions = versioned.into_iter().peekable();
    while let Some((version, directory)) = versions.next() {
        let mut candidates = resolve(&directory).into_iter().collect::<Vec<_>>();
        while versions.peek().is_some_and(|(other, _)| *other == version) {
            if let Some((_, directory)) = versions.next() {
                candidates.extend(resolve(&directory));
            }
        }
        candidates.sort();
        candidates.dedup();
        match candidates.as_slice() {
            [only] => return Some(only.clone()),
            [] => (),
            _ => return None,
        }
    }
    None
}

#[cfg(target_os = "windows")]
fn windows_file_version(path: &Path) -> Option<String> {
    let buffer = windows_version_resource(path)?;
    let bytes = windows_version_value(&buffer, "\\", false)?;
    if bytes.len() < std::mem::size_of::<VS_FIXEDFILEINFO>() {
        return None;
    }
    let info = unsafe { std::ptr::read_unaligned(bytes.as_ptr().cast::<VS_FIXEDFILEINFO>()) };
    if info.dwSignature != 0xFEEF04BD {
        return None;
    }
    Some(format!(
        "{}.{}.{}.{}",
        info.dwFileVersionMS >> 16,
        info.dwFileVersionMS & 0xffff,
        info.dwFileVersionLS >> 16,
        info.dwFileVersionLS & 0xffff
    ))
}

#[cfg(target_os = "windows")]
fn windows_version_resource(path: &Path) -> Option<Vec<u32>> {
    use std::os::windows::ffi::OsStrExt;

    let wide = path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let mut ignored = 0u32;
    let size = unsafe { GetFileVersionInfoSizeW(wide.as_ptr(), &mut ignored) };
    if size == 0 || size > 16 * 1024 * 1024 {
        return None;
    }
    let mut buffer = vec![0u32; (size as usize).div_ceil(4)];
    if unsafe { GetFileVersionInfoW(wide.as_ptr(), 0, size, buffer.as_mut_ptr().cast()) } == 0 {
        return None;
    }
    Some(buffer)
}

#[cfg(target_os = "windows")]
fn windows_version_value<'a>(buffer: &'a [u32], query: &str, is_string: bool) -> Option<&'a [u8]> {
    let query = query
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let mut value = std::ptr::null_mut();
    let mut value_len = 0u32;
    if unsafe {
        VerQueryValueW(
            buffer.as_ptr().cast(),
            query.as_ptr(),
            &mut value,
            &mut value_len,
        )
    } == 0
        || value.is_null()
        || value_len == 0
    {
        return None;
    }
    let bytes = unsafe {
        std::slice::from_raw_parts(buffer.as_ptr().cast::<u8>(), std::mem::size_of_val(buffer))
    };
    let start = (value as usize).checked_sub(bytes.as_ptr() as usize)?;
    let length = (value_len as usize).checked_mul(if is_string { 2 } else { 1 })?;
    bytes.get(start..start.checked_add(length)?)
}

#[cfg(target_os = "windows")]
fn windows_version_string(
    buffer: &[u32],
    language: u16,
    code_page: u16,
    field: &str,
) -> Option<String> {
    let query = format!("\\StringFileInfo\\{language:04x}{code_page:04x}\\{field}");
    let bytes = windows_version_value(buffer, &query, true)?;
    if bytes.len() > 1024 || bytes.len() % 2 != 0 {
        return None;
    }
    let wide = bytes
        .chunks_exact(2)
        .map(|bytes| u16::from_le_bytes([bytes[0], bytes[1]]))
        .take_while(|value| *value != 0)
        .collect::<Vec<_>>();
    String::from_utf16(&wide).ok()
}

#[cfg(target_os = "windows")]
fn windows_standalone_identity_matches(app_id: &str, path: &Path) -> bool {
    if is_windows_packaged_path(path)
        || !windows_gui_executable(path)
        || !windows_authenticode_is_trusted(path)
    {
        return false;
    }
    let Some(buffer) = windows_version_resource(path) else {
        return false;
    };
    let Some(translations) = windows_version_value(&buffer, "\\VarFileInfo\\Translation", false)
    else {
        return false;
    };
    translations.chunks_exact(4).take(16).any(|pair| {
        let language = u16::from_le_bytes([pair[0], pair[1]]);
        let code_page = u16::from_le_bytes([pair[2], pair[3]]);
        let product =
            windows_version_string(&buffer, language, code_page, "ProductName").unwrap_or_default();
        let company =
            windows_version_string(&buffer, language, code_page, "CompanyName").unwrap_or_default();
        crate::desktop_app_discovery_core::windows_desktop_file_identity_matches(
            app_id, path, &product, &company, true,
        )
    })
}

#[cfg(target_os = "windows")]
fn windows_authenticode_is_trusted(path: &Path) -> bool {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Security::WinTrust::{
        WinVerifyTrustEx, WINTRUST_ACTION_GENERIC_VERIFY_V2, WINTRUST_DATA, WINTRUST_DATA_0,
        WINTRUST_FILE_INFO, WTD_CACHE_ONLY_URL_RETRIEVAL, WTD_CHOICE_FILE, WTD_DISABLE_MD2_MD4,
        WTD_REVOKE_NONE, WTD_STATEACTION_IGNORE, WTD_UICONTEXT_EXECUTE, WTD_UI_NONE,
    };

    let wide = path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let mut file = WINTRUST_FILE_INFO {
        cbStruct: std::mem::size_of::<WINTRUST_FILE_INFO>() as u32,
        pcwszFilePath: wide.as_ptr(),
        ..Default::default()
    };
    let mut trust = WINTRUST_DATA {
        cbStruct: std::mem::size_of::<WINTRUST_DATA>() as u32,
        dwUIChoice: WTD_UI_NONE,
        fdwRevocationChecks: WTD_REVOKE_NONE,
        dwUnionChoice: WTD_CHOICE_FILE,
        Anonymous: WINTRUST_DATA_0 { pFile: &mut file },
        dwStateAction: WTD_STATEACTION_IGNORE,
        // Keep discovery bounded and offline. Windows' cached trust chain plus
        // the embedded signature still rejects unsigned or untrusted EXEs.
        dwProvFlags: WTD_CACHE_ONLY_URL_RETRIEVAL | WTD_DISABLE_MD2_MD4,
        dwUIContext: WTD_UICONTEXT_EXECUTE,
        ..Default::default()
    };
    let mut action = WINTRUST_ACTION_GENERIC_VERIFY_V2;
    (unsafe { WinVerifyTrustEx(std::ptr::null_mut(), &mut action, &mut trust) }) == 0
}

#[cfg(any(target_os = "windows", test))]
fn is_windows_packaged_path(path: &Path) -> bool {
    path.to_string_lossy()
        .split(['/', '\\'])
        .any(|part| part.eq_ignore_ascii_case("WindowsApps"))
        || path
            .ancestors()
            .skip(1)
            .any(|parent| parent.join("AppxManifest.xml").is_file())
}

#[cfg(any(target_os = "windows", test))]
fn windows_gui_executable(path: &Path) -> bool {
    use std::io::Read;
    let Ok(file) = std::fs::File::open(path) else {
        return false;
    };
    if !file.metadata().is_ok_and(|metadata| metadata.is_file()) {
        return false;
    }
    let mut header = Vec::new();
    file.take(64 * 1024).read_to_end(&mut header).is_ok()
        && crate::desktop_app_discovery_core::is_windows_gui_pe(&header)
}

#[cfg(target_os = "windows")]
fn read_bounded_file(path: &Path, maximum: u64) -> Option<Vec<u8>> {
    use std::io::Read;
    let file = std::fs::File::open(path).ok()?;
    let metadata = file.metadata().ok()?;
    if !metadata.is_file() || metadata.len() > maximum {
        return None;
    }
    let mut bytes = Vec::new();
    file.take(maximum + 1).read_to_end(&mut bytes).ok()?;
    (bytes.len() as u64 <= maximum).then_some(bytes)
}

#[cfg(any(target_os = "windows", test))]
fn native_buffer_wide_string(buffer: &[u8], pointer: usize) -> Option<String> {
    let start = pointer.checked_sub(buffer.as_ptr() as usize)?;
    let tail = buffer.get(start..)?;
    if start % 2 != 0 {
        return None;
    }
    let mut wide = Vec::new();
    for bytes in tail.chunks_exact(2).take(32768) {
        let unit = u16::from_le_bytes([bytes[0], bytes[1]]);
        if unit == 0 {
            return String::from_utf16(&wide).ok();
        }
        wide.push(unit);
    }
    None
}

#[cfg(target_os = "windows")]
fn installed_windows_package(full_name: &str) -> Option<(PathBuf, String, Vec<String>)> {
    use windows_sys::Win32::{
        Foundation::{ERROR_INSUFFICIENT_BUFFER, ERROR_SUCCESS},
        Storage::Packaging::Appx::{
            ClosePackageInfo, GetPackageApplicationIds, GetPackagePathByFullName,
            OpenPackageInfoByFullName, PackageFamilyNameFromFullName, _PACKAGE_INFO_REFERENCE,
        },
    };
    let full = full_name
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let mut reference = std::ptr::null_mut();
    let status = unsafe { OpenPackageInfoByFullName(full.as_ptr(), 0, &mut reference) };
    if status != ERROR_SUCCESS || reference.is_null() {
        log::debug!("desktop_launch stage=package_lookup os_code={status}");
        return None;
    }
    struct PackageInfo(*mut _PACKAGE_INFO_REFERENCE);
    impl Drop for PackageInfo {
        fn drop(&mut self) {
            unsafe {
                ClosePackageInfo(self.0);
            }
        }
    }
    let reference = PackageInfo(reference);
    fn wide_result(mut query: impl FnMut(*mut u32, *mut u16) -> u32) -> Option<String> {
        let mut length = 0;
        if query(&mut length, std::ptr::null_mut()) != ERROR_INSUFFICIENT_BUFFER
            || !(2..=32768).contains(&length)
        {
            return None;
        }
        let mut buffer = vec![0u16; length as usize];
        if query(&mut length, buffer.as_mut_ptr()) != ERROR_SUCCESS
            || length as usize > buffer.len()
        {
            return None;
        }
        let end = buffer.iter().position(|unit| *unit == 0)?;
        String::from_utf16(&buffer[..end]).ok()
    }
    let root = wide_result(|length, buffer| unsafe {
        GetPackagePathByFullName(full.as_ptr(), length, buffer)
    })?;
    let root = std::fs::canonicalize(root)
        .ok()
        .filter(|path| path.is_absolute() && path.is_dir())?;
    let family = wide_result(|length, buffer| unsafe {
        PackageFamilyNameFromFullName(full.as_ptr(), length, buffer)
    })?;
    let mut length = 0u32;
    let mut count = 0u32;
    if unsafe {
        GetPackageApplicationIds(reference.0, &mut length, std::ptr::null_mut(), &mut count)
    } != ERROR_INSUFFICIENT_BUFFER
        || length == 0
        || length > 1024 * 1024
        || count > 64
    {
        return None;
    }
    // The API returns a pointer table followed by UTF-16 strings. Use aligned
    // storage and bounds-check every pointer before decoding its string.
    let mut aligned = vec![0usize; (length as usize).div_ceil(std::mem::size_of::<usize>())];
    if unsafe {
        GetPackageApplicationIds(
            reference.0,
            &mut length,
            aligned.as_mut_ptr().cast(),
            &mut count,
        )
    } != ERROR_SUCCESS
        || count == 0
        || count > 64
        || length as usize > std::mem::size_of_val(aligned.as_slice())
    {
        return None;
    }
    let bytes =
        unsafe { std::slice::from_raw_parts(aligned.as_ptr().cast::<u8>(), length as usize) };
    let table_length = (count as usize).checked_mul(std::mem::size_of::<usize>())?;
    let table = bytes.get(..table_length)?;
    let mut ids = Vec::new();
    for pointer in table.chunks_exact(std::mem::size_of::<usize>()) {
        let pointer = usize::from_ne_bytes(pointer.try_into().ok()?);
        if pointer.checked_sub(bytes.as_ptr() as usize)? < table_length {
            return None;
        }
        ids.push(native_buffer_wide_string(bytes, pointer)?);
    }
    Some((root, family, ids))
}

#[cfg(any(target_os = "windows", test))]
fn windows_relative_paths(app_id: &str, program_files: bool) -> &'static [&'static str] {
    match (app_id, program_files) {
        ("claude_desktop", false) => &[
            "AnthropicClaude\\Claude.exe",
            "Programs\\Claude\\Claude.exe",
            "Programs\\Claude Desktop\\Claude.exe",
            "Programs\\Anthropic\\Claude\\Claude.exe",
            "Claude\\Claude.exe",
        ],
        ("claude_desktop", true) => &[
            "Claude\\Claude.exe",
            "Claude Desktop\\Claude.exe",
            "Anthropic\\Claude\\Claude.exe",
        ],
        ("codex_desktop", false) => &[
            "Programs\\Codex\\Codex.exe",
            "Programs\\OpenAI\\Codex\\Codex.exe",
            "Codex\\Codex.exe",
            "Programs\\ChatGPT\\ChatGPT.exe",
            "Programs\\OpenAI\\ChatGPT.exe",
            "Programs\\OpenAI\\ChatGPT\\ChatGPT.exe",
            "ChatGPT\\ChatGPT.exe",
        ],
        ("codex_desktop", true) => &[
            "Codex\\Codex.exe",
            "OpenAI\\Codex\\Codex.exe",
            "ChatGPT\\ChatGPT.exe",
            "OpenAI\\ChatGPT\\ChatGPT.exe",
        ],
        ("workbuddy", false) => &[
            "Programs\\WorkBuddy\\WorkBuddy.exe",
            "WorkBuddy\\WorkBuddy.exe",
        ],
        ("workbuddy", true) => &["WorkBuddy\\WorkBuddy.exe"],
        _ => &[],
    }
}

#[cfg(target_os = "windows")]
fn collect_windows_path_candidates(
    accepted: &mut HashMap<PathBuf, Candidate>,
    spec: DesktopAppSpec,
    root: PathBuf,
    location_hint: LocationHint,
    relative_paths: &[&str],
) {
    for relative_path in relative_paths {
        let path = root.join(relative_path);
        if !path.is_absolute() {
            continue;
        }
        let canonical = std::fs::canonicalize(&path)
            .ok()
            .filter(|path| windows_standalone_identity_matches(spec.id, path))
            .or_else(|| {
                path.parent()
                    .and_then(|parent| executable_below(parent, spec.id))
            });
        let Some(canonical) = canonical else {
            continue;
        };
        let version = windows_file_version(&canonical).unwrap_or_default();
        accepted.entry(canonical.clone()).or_insert(Candidate {
            path: canonical.clone(),
            location_hint,
            bundle_identifier: String::new(),
            version,
            launch_target: DesktopLaunchTarget::WindowsExecutable(canonical),
        });
    }
}

#[cfg(any(target_os = "windows", test))]
fn compact_windows_name(raw: &str) -> String {
    raw.chars()
        .filter(|character| character.is_ascii_alphanumeric())
        .map(|character| character.to_ascii_lowercase())
        .take(256)
        .collect()
}

#[cfg(any(target_os = "windows", test))]
fn windows_package_identity_matches(app_id: &str, identity: &str) -> bool {
    crate::desktop_app_discovery_core::windows_package_name_matches(app_id, identity)
}

#[cfg(any(target_os = "windows", test))]
fn windows_registered_name_matches(app_id: &str, name: &str, publisher: &str) -> bool {
    let name = compact_windows_name(name);
    let publisher = compact_windows_name(publisher);
    match app_id {
        "claude_desktop" => {
            name == "claude"
                || name == "claudedesktop"
                || ((name == "anthropicclaude" || name == "anthropicclaudedesktop")
                    && publisher.contains("anthropic"))
        }
        "codex_desktop" => {
            name == "codex"
                || name == "codexdesktop"
                || ((name == "chatgpt" || name == "chatgptdesktop")
                    && (publisher.is_empty() || publisher.contains("openai")))
        }
        "workbuddy" => {
            (name == "workbuddy" || name == "tencentworkbuddy") && publisher.contains("tencent")
        }
        _ => false,
    }
}

#[cfg(any(target_os = "windows", test))]
fn parse_windows_package_identity(raw: &str) -> Option<(String, String)> {
    if raw.is_empty() || raw.len() > 512 {
        return None;
    }
    let parts: Vec<_> = raw.split('_').collect();
    let version_index = parts.iter().position(|part| {
        let components: Vec<_> = part.split('.').collect();
        components.len() >= 2
            && components.iter().all(|component| {
                !component.is_empty() && component.bytes().all(|b| b.is_ascii_digit())
            })
    });
    let (identity, version) = match version_index {
        Some(0) => return None,
        Some(index) => (parts[..index].join("_"), parts[index].to_owned()),
        None => (parts.first()?.to_string(), String::new()),
    };
    Some((identity, version))
}

#[cfg(any(target_os = "windows", test))]
fn windows_version_key(raw: &str) -> [u64; 4] {
    let mut key = [0; 4];
    for (index, component) in raw.split('.').take(4).enumerate() {
        key[index] = component.parse().unwrap_or(0);
    }
    key
}

#[cfg(any(target_os = "windows", test))]
fn preferred_windows_package_version(
    package_version: &str,
    executable_version: Option<String>,
) -> String {
    if package_version.is_empty() {
        executable_version.unwrap_or_default()
    } else {
        package_version.to_owned()
    }
}

#[cfg(target_os = "windows")]
fn discover_windows_package_candidates(spec: DesktopAppSpec) -> Vec<Candidate> {
    const REPOSITORY: &str = "Software\\Classes\\Local Settings\\Software\\Microsoft\\Windows\\CurrentVersion\\AppModel\\Repository\\Packages";
    let mut accepted: HashMap<PathBuf, Candidate> = HashMap::new();
    let Ok(packages) = HKCU.open_subkey_with_flags(REPOSITORY, KEY_READ) else {
        return Vec::new();
    };
    for package_full_name in packages.enum_keys().filter_map(Result::ok) {
        let Some((identity, package_version)) = parse_windows_package_identity(&package_full_name)
        else {
            continue;
        };
        if !windows_package_identity_matches(spec.id, &identity) {
            continue;
        }
        // Registry names are enumeration hints only. Path, family and AUMIDs
        // must come from Windows' current-user installed-package APIs.
        let Some((root, family, application_ids)) = installed_windows_package(&package_full_name)
        else {
            continue;
        };
        let Some(manifest) = read_bounded_file(&root.join("AppxManifest.xml"), 1024 * 1024) else {
            continue;
        };
        let mapped = windows_package_launch_candidates(
            spec.id,
            &package_full_name,
            &root,
            &family,
            &application_ids,
            &manifest,
        );
        for (path, identity) in &mapped {
            // Store/MSIX packages are versioned by their package identity.
            // Their launcher executable can carry the bundled Chromium or
            // WebView version instead, which is not the app version users see.
            let version = preferred_windows_package_version(
                &package_version,
                windows_file_version(path).filter(|value| !value.is_empty()),
            );
            accepted.entry(path.clone()).or_insert(Candidate {
                path: path.clone(),
                location_hint: LocationHint::LocalAppData,
                bundle_identifier: String::new(),
                version,
                launch_target: DesktopLaunchTarget::WindowsPackage(identity.clone()),
            });
        }
    }
    let mut candidates: Vec<_> = accepted.into_values().collect();
    candidates.sort_by(|left, right| left.path.cmp(&right.path));
    candidates.truncate(8);
    candidates
}

#[cfg(any(target_os = "windows", test))]
fn windows_package_launch_candidates(
    app_id: &str,
    full_name: &str,
    root: &Path,
    family: &str,
    application_ids: &[String],
    manifest: &[u8],
) -> Vec<(
    PathBuf,
    crate::desktop_app_discovery_core::WindowsPackageApplication,
)> {
    let Some(applications) =
        crate::desktop_app_discovery_core::windows_manifest_applications(app_id, manifest)
    else {
        return Vec::new();
    };
    let Ok(root) = std::fs::canonicalize(root) else {
        return Vec::new();
    };
    let mut mapped = Vec::new();
    for (relative_id, executable) in applications {
        let aumid = format!("{family}!{relative_id}");
        if !application_ids.contains(&aumid) {
            continue;
        }
        let Some(identity) =
            crate::desktop_app_discovery_core::WindowsPackageApplication::from_installed_package(
                app_id, full_name, family, &aumid,
            )
        else {
            continue;
        };
        let path = executable
            .split(['/', '\\'])
            .fold(root.clone(), |path, part| path.join(part));
        let Ok(path) = std::fs::canonicalize(path) else {
            continue;
        };
        if path.starts_with(&root) && windows_gui_executable(&path) {
            mapped.push((path, identity));
        }
    }
    // A path-only selection cannot disambiguate two apps sharing one EXE.
    let mut unique = Vec::new();
    for (path, identity) in &mapped {
        if mapped.iter().filter(|(other, _)| other == path).count() == 1 {
            unique.push((path.clone(), identity.clone()));
        }
    }
    unique
}

#[cfg(target_os = "windows")]
fn discover_windows_uninstall_candidates(spec: DesktopAppSpec) -> Vec<Candidate> {
    let mut accepted: HashMap<PathBuf, Candidate> = HashMap::new();
    for (hive_name, hive) in [("hkcu", &HKCU), ("hklm", &HKLM)] {
        for view in [KEY_WOW64_64KEY, KEY_WOW64_32KEY] {
            collect_windows_uninstall_candidates(&mut accepted, spec, hive_name, hive, view);
        }
    }
    let mut candidates: Vec<_> = accepted.into_values().collect();
    candidates.sort_by(|left, right| left.path.cmp(&right.path));
    candidates.truncate(8);
    candidates
}

#[cfg(target_os = "windows")]
fn collect_windows_uninstall_candidates(
    accepted: &mut HashMap<PathBuf, Candidate>,
    spec: DesktopAppSpec,
    hive_name: &str,
    hive: &RegKey,
    view: u32,
) {
    const UNINSTALL: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall";
    let Ok(parent) = hive.open_subkey_with_flags(UNINSTALL, KEY_READ | view) else {
        return;
    };
    for subkey_name in parent.enum_keys().filter_map(Result::ok) {
        let Ok(item) = parent.open_subkey_with_flags(&subkey_name, KEY_READ | view) else {
            continue;
        };
        let display_name: String = item.get_value("DisplayName").unwrap_or_default();
        let publisher: String = item.get_value("Publisher").unwrap_or_default();
        if !windows_registered_name_matches(spec.id, &display_name, &publisher) {
            continue;
        }
        let version: String = item.get_value("DisplayVersion").unwrap_or_default();
        let display_icon: String = item.get_value("DisplayIcon").unwrap_or_default();
        let install_location: String = item.get_value("InstallLocation").unwrap_or_default();
        let icon_path = parse_windows_display_icon(&display_icon)
            .map(PathBuf::from)
            .filter(|path| path.is_absolute() && path.is_file())
            .and_then(|path| std::fs::canonicalize(&path).ok())
            .filter(|path| windows_standalone_identity_matches(spec.id, path));
        let install_path = (!install_location.is_empty())
            .then(|| executable_below(Path::new(&install_location), spec.id))
            .flatten();
        let Some(path) = preferred_windows_program(install_path, icon_path) else {
            continue;
        };
        let candidate = Candidate {
            path: path.clone(),
            location_hint: if hive_name == "hkcu" {
                LocationHint::LocalAppData
            } else {
                LocationHint::ProgramFiles
            },
            bundle_identifier: String::new(),
            version: windows_file_version(&path)
                .filter(|value| !value.is_empty())
                .unwrap_or(version),
            launch_target: DesktopLaunchTarget::WindowsExecutable(path.clone()),
        };
        let replace = accepted
            .get(&path)
            .map(|current| {
                windows_version_key(&candidate.version) > windows_version_key(&current.version)
            })
            .unwrap_or(true);
        if replace {
            accepted.insert(path, candidate);
        }
    }
}

#[cfg(any(target_os = "windows", test))]
fn parse_windows_display_icon(raw: &str) -> Option<String> {
    let value = raw.trim();
    if value.is_empty() || value.len() > 32_768 || value.chars().any(char::is_control) {
        return None;
    }
    let path = if let Some(rest) = value.strip_prefix('"') {
        let (path, tail) = rest.split_once('"')?;
        if !valid_icon_index(tail) {
            return None;
        }
        path
    } else {
        match value.rsplit_once(',') {
            Some((path, index)) if index.trim().parse::<i32>().is_ok() => path,
            _ => value,
        }
    }
    .trim();
    (!path.is_empty() && !path.contains('"') && path.to_ascii_lowercase().ends_with(".exe"))
        .then(|| path.to_owned())
}

#[cfg(any(target_os = "windows", test))]
fn valid_icon_index(tail: &str) -> bool {
    tail.trim().is_empty()
        || tail
            .trim()
            .strip_prefix(',')
            .is_some_and(|index| index.trim().parse::<i32>().is_ok())
}

#[cfg(any(target_os = "windows", test))]
fn preferred_windows_program(
    install_path: Option<PathBuf>,
    verified_icon: Option<PathBuf>,
) -> Option<PathBuf> {
    install_path.or(verified_icon)
}

#[cfg(any(target_os = "windows", test))]
fn parse_windows_protocol_command(raw: &str) -> Option<String> {
    let value = raw.trim();
    if value.is_empty() || value.len() > 32_768 || value.chars().any(char::is_control) {
        return None;
    }
    let (path, arguments) = if let Some(rest) = value.strip_prefix('"') {
        rest.split_once('"')?
    } else {
        let end = value.find(char::is_whitespace).unwrap_or(value.len());
        (&value[..end], &value[end..])
    };
    // Generic Open can omit a URI placeholder. Other registered switches may
    // be meaningful (e.g. Update.exe --processStart); never silently drop them.
    if !matches!(arguments.trim(), "" | "%1" | "\"%1\"" | "%L" | "\"%L\"") {
        return None;
    }
    (!path.is_empty() && path.to_ascii_lowercase().ends_with(".exe")).then(|| path.to_owned())
}

#[cfg(target_os = "windows")]
fn discover_windows_protocol_candidate(spec: DesktopAppSpec) -> Vec<Candidate> {
    let protocols: &[&str] = match spec.id {
        "claude_desktop" => &["claude"],
        "codex_desktop" => &["codex", "chatgpt"],
        _ => &[],
    };
    for protocol in protocols {
        let user_path = format!("Software\\Classes\\{protocol}\\shell\\open\\command");
        let command = HKCU
            .open_subkey_with_flags(&user_path, KEY_READ)
            .ok()
            .and_then(|key| key.get_value::<String, _>("").ok())
            .or_else(|| {
                HKCR.open_subkey_with_flags(format!("{protocol}\\shell\\open\\command"), KEY_READ)
                    .ok()
                    .and_then(|key| key.get_value::<String, _>("").ok())
            });
        let path = command
            .as_deref()
            .and_then(parse_windows_protocol_command)
            .map(PathBuf::from)
            .filter(|path| path.is_absolute() && path.is_file())
            .and_then(|path| std::fs::canonicalize(&path).ok())
            .filter(|path| windows_standalone_identity_matches(spec.id, path));
        if let Some(path) = path {
            let version = windows_file_version(&path).unwrap_or_default();
            return vec![Candidate {
                path: path.clone(),
                location_hint: LocationHint::LocalAppData,
                bundle_identifier: String::new(),
                version,
                launch_target: DesktopLaunchTarget::WindowsExecutable(path),
            }];
        }
    }
    Vec::new()
}

fn validate_request_id(request_id: &str) -> Result<(), String> {
    if request_id.is_empty()
        || request_id.len() > 64
        || !request_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
    {
        return Err("invalid_request".into());
    }
    Ok(())
}

fn unix_epoch_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX)
}

const fn platform_name() -> &'static str {
    if cfg!(target_os = "windows") {
        "windows"
    } else if cfg!(target_os = "macos") {
        "macos"
    } else if cfg!(target_os = "linux") {
        "linux"
    } else {
        "unknown"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_pe_fixture(path: &Path, subsystem: u16) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let mut header = vec![0u8; 256];
        header[..2].copy_from_slice(b"MZ");
        header[0x3c..0x40].copy_from_slice(&64u32.to_le_bytes());
        header[64..68].copy_from_slice(b"PE\0\0");
        header[88..90].copy_from_slice(&0x20bu16.to_le_bytes());
        header[156..158].copy_from_slice(&subsystem.to_le_bytes());
        std::fs::write(path, header).unwrap();
    }

    #[test]
    fn request_identity_is_closed() {
        assert!(validate_request_id("desktop-123_ok").is_ok());
        assert!(validate_request_id("").is_err());
        assert!(validate_request_id("bad/path").is_err());
        assert!(validate_request_id(&"a".repeat(65)).is_err());
    }

    #[test]
    fn candidate_projection_is_sanitized() {
        let projection = classify_candidates(vec![Candidate {
            path: PathBuf::from("/private/Users/example/Applications/Claude.app"),
            location_hint: LocationHint::UserApplications,
            bundle_identifier: "com.anthropic.claudefordesktop".into(),
            version: "1.2.3".into(),
            launch_target: DesktopLaunchTarget::MacBundle(PathBuf::from(
                "/private/Users/example/Applications/Claude.app",
            )),
        }]);
        assert!(matches!(
            projection,
            DesktopAppObservation::Found {
                location_hint: LocationHint::UserApplications,
                ..
            }
        ));
    }

    #[test]
    fn windows_package_registration_matches_current_desktop_apps() {
        let (claude, claude_version) =
            parse_windows_package_identity("Claude_1.4.2.0_x64__examplepublisher").unwrap();
        let (chatgpt, chatgpt_version) =
            parse_windows_package_identity("OPENAI.ChatGPT_2026.901.1200.0_x64__examplepublisher")
                .unwrap();
        let (codex_family, family_version) =
            parse_windows_package_identity("OpenAI.Codex_2p2nqsd0c76g0").unwrap();

        assert_eq!(claude, "Claude");
        assert_eq!(claude_version, "1.4.2.0");
        assert_eq!(chatgpt, "OPENAI.ChatGPT");
        assert_eq!(chatgpt_version, "2026.901.1200.0");
        assert_eq!(codex_family, "OpenAI.Codex");
        assert!(family_version.is_empty());
        assert_eq!(
            preferred_windows_package_version("26.901.6511.0", Some("152.0.7977.83".into())),
            "26.901.6511.0"
        );
        assert_eq!(
            preferred_windows_package_version("", Some("1.2.3.4".into())),
            "1.2.3.4"
        );
        assert!(windows_package_identity_matches("claude_desktop", &claude));
        assert!(windows_package_identity_matches("codex_desktop", &chatgpt));
        assert!(windows_package_identity_matches(
            "codex_desktop",
            &codex_family
        ));
        assert!(!windows_package_identity_matches(
            "claude_desktop",
            "ClaudeTheme"
        ));
        assert!(!windows_package_identity_matches(
            "codex_desktop",
            "OpenAI.API"
        ));
        assert_eq!(windows_version_key("26.825.51511.0"), [26, 825, 51511, 0]);
    }

    #[test]
    fn windows_registered_and_path_fallbacks_cover_supported_installers() {
        assert!(windows_registered_name_matches(
            "claude_desktop",
            "Claude Desktop",
            "Anthropic PBC"
        ));
        assert!(windows_registered_name_matches(
            "codex_desktop",
            "ChatGPT",
            "OpenAI, L.L.C."
        ));
        assert!(!windows_registered_name_matches(
            "codex_desktop",
            "ChatGPT Helper",
            "Unknown"
        ));
        assert!(windows_registered_name_matches(
            "workbuddy",
            "WorkBuddy",
            "Tencent"
        ));
        assert!(!windows_registered_name_matches(
            "workbuddy",
            "WorkBuddy",
            ""
        ));
        assert!(!windows_registered_name_matches(
            "workbuddy",
            "WorkBuddy",
            "Contoso"
        ));

        assert!(windows_relative_paths("claude_desktop", false)
            .contains(&"Programs\\Claude Desktop\\Claude.exe"));
        assert!(
            windows_relative_paths("codex_desktop", false).contains(&"Programs\\Codex\\Codex.exe")
        );
        assert!(
            windows_relative_paths("codex_desktop", true).contains(&"OpenAI\\ChatGPT\\ChatGPT.exe")
        );
        assert!(windows_relative_paths("workbuddy", false)
            .contains(&"Programs\\WorkBuddy\\WorkBuddy.exe"));
        assert_eq!(
            parse_windows_display_icon("\"C:\\Program Files\\OpenAI\\ChatGPT.exe\",0"),
            Some("C:\\Program Files\\OpenAI\\ChatGPT.exe".into())
        );
        assert_eq!(
            parse_windows_protocol_command("\"C:\\Apps\\Claude.exe\" \"%1\""),
            Some("C:\\Apps\\Claude.exe".into())
        );
    }

    #[test]
    fn install_program_precedes_display_icon_and_meaningful_protocol_arguments_are_rejected() {
        let program = PathBuf::from("C:\\Apps\\Claude.exe");
        let icon = PathBuf::from("C:\\Other\\Claude.exe");
        assert_eq!(
            preferred_windows_program(Some(program.clone()), Some(icon.clone())),
            Some(program)
        );
        assert_eq!(
            preferred_windows_program(None, Some(icon.clone())),
            Some(icon)
        );
        assert_eq!(
            parse_windows_display_icon("C:\\Apps\\Claude.exe,0"),
            Some("C:\\Apps\\Claude.exe".into())
        );
        for icon in [
            "\"C:\\Apps\\Claude.exe\" --run",
            "\"C:\\Apps\\Claude.exe\",bad",
            "C:\\Apps\\Claude.exe\0,0",
        ] {
            assert!(parse_windows_display_icon(icon).is_none());
        }
        for command in [
            "\"C:\\Apps\\Update.exe\" --processStart Claude.exe --process-start-args \"%1\"",
            "\"C:\\Apps\\Claude.exe\" --unsafe",
            "\"C:\\Apps\\Claude.exe\" \"%1\" & calc",
            "C:\\Program Files\\Claude.exe \"%1\"",
            "\"C:\\Apps\\Claude.exe\" \"%1\"\0extra",
        ] {
            assert!(parse_windows_protocol_command(command).is_none());
        }
    }

    #[test]
    fn launch_selection_requires_exact_unique_discovered_path() {
        let path = PathBuf::from("C:\\Apps\\Codex.exe");
        let candidate = Candidate {
            path: path.clone(),
            location_hint: LocationHint::LocalAppData,
            bundle_identifier: String::new(),
            version: String::new(),
            launch_target: DesktopLaunchTarget::WindowsExecutable(path.clone()),
        };
        assert_eq!(
            select_launch_target(vec![candidate.clone()], &path),
            Some(candidate.launch_target.clone())
        );
        assert!(
            select_launch_target(vec![candidate.clone()], Path::new("C:\\Other\\Codex.exe"))
                .is_none()
        );
        assert!(select_launch_target(vec![candidate.clone(), candidate], &path).is_none());
    }

    #[test]
    fn native_package_strings_are_bounds_checked() {
        let bytes = vec![b'A', 0, b'p', 0, b'p', 0, 0, 0];
        let base = bytes.as_ptr() as usize;
        assert_eq!(native_buffer_wide_string(&bytes, base), Some("App".into()));
        assert!(native_buffer_wide_string(&bytes, base - 2).is_none());
        assert!(native_buffer_wide_string(&bytes, base + 1).is_none());
        assert!(native_buffer_wide_string(&bytes, base + bytes.len()).is_none());
        assert!(native_buffer_wide_string(&bytes[..6], base).is_none());
    }

    #[test]
    fn executable_fixture_keeps_package_files_out_of_standalone_fallback() {
        let root = crate::tool_adapters::common::temporary_working_directory("desktop-pe-identity")
            .unwrap();
        let executable = root.join("Codex.exe");
        let mut header = vec![0u8; 256];
        header[..2].copy_from_slice(b"MZ");
        header[0x3c..0x40].copy_from_slice(&64u32.to_le_bytes());
        header[64..68].copy_from_slice(b"PE\0\0");
        header[88..90].copy_from_slice(&0x20bu16.to_le_bytes());
        header[156..158].copy_from_slice(&2u16.to_le_bytes());
        std::fs::write(&executable, &header).unwrap();
        assert!(windows_gui_executable(&executable));
        assert!(!is_windows_packaged_path(&executable));
        std::fs::write(root.join("AppxManifest.xml"), b"<Package/>").unwrap();
        assert!(is_windows_packaged_path(&executable));
        assert!(is_windows_packaged_path(Path::new(
            "C:\\Program Files\\WindowsApps\\Codex\\Codex.exe"
        )));
        header[156..158].copy_from_slice(&3u16.to_le_bytes());
        std::fs::write(&executable, &header).unwrap();
        assert!(!windows_gui_executable(&executable));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn versioned_standalone_fixture_selects_latest_verified_gui() {
        let root =
            crate::tool_adapters::common::temporary_working_directory("desktop-versioned-program")
                .unwrap();
        let older = root.join("app-1.8.0/Codex.exe");
        let latest = root.join("app-1.10.0/Codex.exe");
        let cli = root.join("app-1.20.0/Codex.exe");
        write_pe_fixture(&older, 2);
        write_pe_fixture(&latest, 2);
        write_pe_fixture(&cli, 3);
        write_pe_fixture(&root.join("app-9.0.0/Update.exe"), 2);
        assert_eq!(
            windows_program_below(&root, "codex_desktop", windows_gui_executable),
            Some(std::fs::canonicalize(&latest).unwrap())
        );
        std::fs::remove_file(&latest).unwrap();
        assert_eq!(
            windows_program_below(&root, "codex_desktop", windows_gui_executable),
            Some(std::fs::canonicalize(&older).unwrap())
        );
        assert!(windows_program_below(&root, "claude_desktop", windows_gui_executable).is_none());
        write_pe_fixture(&root.join("app-01.8.0/Codex.exe"), 2);
        assert!(windows_program_below(&root, "codex_desktop", windows_gui_executable).is_none());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn installed_package_fixture_requires_native_application_membership_and_unambiguous_gui() {
        let root =
            crate::tool_adapters::common::temporary_working_directory("desktop-package-mapping")
                .unwrap();
        let path = root.join("app/Codex.exe");
        write_pe_fixture(&path, 2);
        let full = "OpenAI.Codex_1.2.3.4_x64__2p2nqsd0c76g0";
        let family = "OpenAI.Codex_2p2nqsd0c76g0";
        let ids = vec![format!("{family}!ActualCodex"), format!("{family}!Updater")];
        let manifest = br#"<Package><Applications><Application Id="Updater" Executable="Update.exe"/><Application Id="ActualCodex" Executable="app\Codex.exe"/></Applications></Package>"#;
        let mapped =
            windows_package_launch_candidates("codex_desktop", full, &root, family, &ids, manifest);
        assert_eq!(mapped.len(), 1);
        assert_eq!(mapped[0].0, std::fs::canonicalize(&path).unwrap());
        assert_eq!(mapped[0].1.app_user_model_id(), ids[0]);
        assert!(windows_package_launch_candidates(
            "codex_desktop",
            full,
            &root,
            family,
            &[],
            manifest
        )
        .is_empty());
        assert!(windows_package_launch_candidates(
            "codex_desktop",
            full,
            &root,
            "Other.Codex_2p2nqsd0c76g0",
            &ids,
            manifest
        )
        .is_empty());
        let ambiguous = br#"<Package><Applications><Application Id="ActualCodex" Executable="app\Codex.exe"/><Application Id="Updater" Executable="app\Codex.exe"/></Applications></Package>"#;
        assert!(windows_package_launch_candidates(
            "codex_desktop",
            full,
            &root,
            family,
            &ids,
            ambiguous
        )
        .is_empty());
        write_pe_fixture(&path, 3);
        assert!(windows_package_launch_candidates(
            "codex_desktop",
            full,
            &root,
            family,
            &ids,
            manifest
        )
        .is_empty());
        std::fs::remove_dir_all(root).unwrap();
    }
}

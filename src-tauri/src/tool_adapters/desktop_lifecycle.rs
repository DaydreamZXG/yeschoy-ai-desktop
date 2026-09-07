//! Exact-installation lifecycle checks for desktop tools that reload settings
//! only after a normal application restart. This module never force-terminates.

use std::{path::Path, time::Duration};

use super::AdapterFailure;

const EXIT_POLL: Duration = Duration::from_millis(100);
const NORMAL_QUIT_LIMIT: Duration = Duration::from_secs(10);

pub(crate) fn requires_reload(tool_id: &str) -> bool {
    matches!(tool_id, "claude_desktop" | "codex_desktop")
}

pub(crate) fn is_running(tool_id: &str, path: &Path) -> Result<bool, AdapterFailure> {
    if !requires_reload(tool_id) {
        return Ok(false);
    }
    platform::is_running(tool_id, path)
}

/// Ask the exact discovered desktop installation to quit normally and wait
/// for its top-level process to leave. `Ok(true)` means a live application was
/// closed; `Ok(false)` means it was already stopped. A refusal/timeout is
/// recoverable and must not be followed by configuration writes.
pub(crate) async fn quit_normally(tool_id: &str, path: &Path) -> Result<bool, AdapterFailure> {
    if !requires_reload(tool_id) {
        return Ok(false);
    }
    let requested = platform::request_normal_quit(tool_id, path)?;
    if !requested {
        return Ok(false);
    }
    let deadline = tokio::time::Instant::now() + NORMAL_QUIT_LIMIT;
    while tokio::time::Instant::now() < deadline {
        if !platform::is_running(tool_id, path)? {
            return Ok(true);
        }
        tokio::time::sleep(EXIT_POLL).await;
    }
    Err(AdapterFailure::LaunchError("graceful_restart_required"))
}

#[cfg(target_os = "macos")]
mod platform {
    use std::path::{Path, PathBuf};

    use objc2_app_kit::NSRunningApplication;
    use objc2_foundation::NSString;

    use super::AdapterFailure;

    fn bundle_identifier(tool_id: &str) -> Option<&'static str> {
        match tool_id {
            "claude_desktop" => Some("com.anthropic.claudefordesktop"),
            "codex_desktop" => Some("com.openai.codex"),
            _ => None,
        }
    }

    fn normalized(path: &Path) -> PathBuf {
        std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
    }

    fn matches_path(app: &NSRunningApplication, expected: &Path) -> bool {
        app.bundleURL()
            .and_then(|url| url.path())
            .map(|path| normalized(Path::new(&path.to_string())) == normalized(expected))
            .unwrap_or(false)
    }

    pub(super) fn is_running(tool_id: &str, path: &Path) -> Result<bool, AdapterFailure> {
        let identifier = bundle_identifier(tool_id).ok_or(AdapterFailure::UnsupportedProfile)?;
        let identifier = NSString::from_str(identifier);
        let applications =
            NSRunningApplication::runningApplicationsWithBundleIdentifier(&identifier);
        Ok(applications.iter().any(|app| matches_path(&app, path)))
    }

    pub(super) fn request_normal_quit(tool_id: &str, path: &Path) -> Result<bool, AdapterFailure> {
        let identifier = bundle_identifier(tool_id).ok_or(AdapterFailure::UnsupportedProfile)?;
        let identifier = NSString::from_str(identifier);
        let applications =
            NSRunningApplication::runningApplicationsWithBundleIdentifier(&identifier);
        let mut matched = false;
        for application in applications.iter().filter(|app| matches_path(app, path)) {
            matched = true;
            if !application.terminate() {
                return Err(AdapterFailure::LaunchError("graceful_restart_required"));
            }
        }
        Ok(matched)
    }
}

#[cfg(target_os = "windows")]
mod platform {
    use std::{collections::HashSet, path::Path};

    use windows::{
        core::{BOOL, PWSTR},
        Win32::{
            Foundation::{CloseHandle, HANDLE, HWND, LPARAM, WPARAM},
            System::{
                Diagnostics::ToolHelp::{
                    CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
                    TH32CS_SNAPPROCESS,
                },
                Threading::{
                    OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32,
                    PROCESS_QUERY_LIMITED_INFORMATION,
                },
            },
            UI::WindowsAndMessaging::{
                EnumWindows, GetWindowThreadProcessId, PostMessageW, WM_CLOSE,
            },
        },
    };

    use super::AdapterFailure;

    struct WindowSearch {
        windows: Vec<HWND>,
        pids: HashSet<u32>,
    }

    struct Snapshot(HANDLE);

    impl Drop for Snapshot {
        fn drop(&mut self) {
            let _ = unsafe { CloseHandle(self.0) };
        }
    }

    fn normalized(path: &Path) -> String {
        std::fs::canonicalize(path)
            .unwrap_or_else(|_| path.to_path_buf())
            .to_string_lossy()
            .trim_start_matches(r"\\?\")
            .replace('/', "\\")
            .to_lowercase()
    }

    fn process_path(pid: u32) -> Option<String> {
        let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()? };
        let mut buffer = vec![0u16; 32_768];
        let mut length = buffer.len() as u32;
        let queried = unsafe {
            QueryFullProcessImageNameW(
                process,
                PROCESS_NAME_WIN32,
                PWSTR(buffer.as_mut_ptr()),
                &mut length,
            )
        };
        let _ = unsafe { CloseHandle(process) };
        queried.ok()?;
        String::from_utf16(buffer.get(..length as usize)?).ok()
    }

    unsafe extern "system" fn collect_window(hwnd: HWND, lparam: LPARAM) -> BOOL {
        // SAFETY: `lparam` is a live mutable WindowSearch for the duration of
        // synchronous EnumWindows; the callback never stores the pointer.
        let search = unsafe { &mut *(lparam.0 as *mut WindowSearch) };
        let mut pid = 0u32;
        unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
        if pid != 0 && search.pids.contains(&pid) {
            search.windows.push(hwnd);
        }
        BOOL(1)
    }

    fn matching_processes(path: &Path) -> Result<HashSet<u32>, AdapterFailure> {
        let target = normalized(path);
        let snapshot = Snapshot(
            unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) }
                .map_err(|_| AdapterFailure::LaunchError("desktop_state_unavailable"))?,
        );
        let mut entry = PROCESSENTRY32W {
            dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
            ..Default::default()
        };
        unsafe { Process32FirstW(snapshot.0, &mut entry) }
            .map_err(|_| AdapterFailure::LaunchError("desktop_state_unavailable"))?;
        let mut pids = HashSet::new();
        loop {
            let pid = entry.th32ProcessID;
            if pid != 0
                && process_path(pid)
                    .map(|path| normalized(Path::new(&path)) == target)
                    .unwrap_or(false)
            {
                pids.insert(pid);
            }
            if unsafe { Process32NextW(snapshot.0, &mut entry) }.is_err() {
                break;
            }
        }
        Ok(pids)
    }

    fn matching_windows(pids: HashSet<u32>) -> Result<Vec<HWND>, AdapterFailure> {
        let mut search = WindowSearch {
            windows: Vec::new(),
            pids,
        };
        unsafe {
            EnumWindows(
                Some(collect_window),
                LPARAM((&mut search as *mut WindowSearch) as isize),
            )
        }
        .map_err(|_| AdapterFailure::LaunchError("desktop_state_unavailable"))?;
        Ok(search.windows)
    }

    pub(super) fn is_running(_tool_id: &str, path: &Path) -> Result<bool, AdapterFailure> {
        Ok(!matching_processes(path)?.is_empty())
    }

    pub(super) fn request_normal_quit(_tool_id: &str, path: &Path) -> Result<bool, AdapterFailure> {
        let pids = matching_processes(path)?;
        if pids.is_empty() {
            return Ok(false);
        }
        let windows = matching_windows(pids)?;
        if windows.is_empty() {
            return Err(AdapterFailure::LaunchError("graceful_restart_required"));
        }
        for hwnd in &windows {
            unsafe { PostMessageW(Some(*hwnd), WM_CLOSE, WPARAM(0), LPARAM(0)) }
                .map_err(|_| AdapterFailure::LaunchError("graceful_restart_required"))?;
        }
        Ok(!windows.is_empty())
    }
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
mod platform {
    use std::path::Path;

    use super::AdapterFailure;

    pub(super) fn is_running(_tool_id: &str, _path: &Path) -> Result<bool, AdapterFailure> {
        Ok(false)
    }

    pub(super) fn request_normal_quit(
        _tool_id: &str,
        _path: &Path,
    ) -> Result<bool, AdapterFailure> {
        Ok(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_desktop_targets_are_eligible_for_reload_control() {
        assert!(requires_reload("claude_desktop"));
        assert!(requires_reload("codex_desktop"));
        for tool in ["claude_code", "pi", "dsh_web", "hermes", "openclaw"] {
            assert!(!requires_reload(tool), "{tool}");
        }
    }
}

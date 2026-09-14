//! Exact-installation lifecycle checks for desktop tools that reload settings
//! only after a restart. A user-confirmed Windows handoff first asks the app to
//! close normally, then terminates only residual processes whose executable
//! path still matches the exact discovered installation.

use std::{path::Path, time::Duration};

use super::AdapterFailure;

const EXIT_POLL: Duration = Duration::from_millis(100);
#[cfg(target_os = "windows")]
const NORMAL_QUIT_LIMIT: Duration = Duration::from_secs(4);
#[cfg(not(target_os = "windows"))]
const NORMAL_QUIT_LIMIT: Duration = Duration::from_secs(10);
const FORCED_QUIT_LIMIT: Duration = Duration::from_secs(5);
// Reuse the existing desktop launch deadline, not an unbounded model probe.
const START_LIMIT: Duration = Duration::from_secs(15);

/// A bounded OS observation, not proof that the app left its splash screen,
/// authenticated, or completed a model request. Do not cancel/drop a dispatched
/// launch: finish it before deciding whether it is safe to roll back files.
pub(crate) async fn open_and_wait(tool_id: &str, path: &Path) -> Result<(), AdapterFailure> {
    let tool = tool_id.to_owned();
    let target = path.to_owned();
    tokio::task::spawn_blocking(move || super::desktop_launch::launch(&tool, &target))
        .await
        .map_err(|_| AdapterFailure::LaunchError("desktop_launch_start_failed"))??;
    wait_for_start(
        || async {
            let tool = tool_id.to_owned();
            let target = path.to_owned();
            tokio::task::spawn_blocking(move || platform::startup_observed(&tool, &target))
                .await
                .map_err(|_| AdapterFailure::LaunchError("desktop_state_unavailable"))?
        },
        START_LIMIT,
    )
    .await
}

async fn wait_for_start<F, Fut>(mut observe: F, limit: Duration) -> Result<(), AdapterFailure>
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = Result<bool, AdapterFailure>>,
{
    let deadline = tokio::time::Instant::now() + limit;
    loop {
        if tokio::time::timeout_at(deadline, observe())
            .await
            .map_err(|_| AdapterFailure::LaunchError("desktop_start_unconfirmed"))??
        {
            return Ok(());
        }
        if tokio::time::Instant::now() >= deadline {
            return Err(AdapterFailure::LaunchError("desktop_start_unconfirmed"));
        }
        tokio::time::sleep(
            EXIT_POLL.min(deadline.saturating_duration_since(tokio::time::Instant::now())),
        )
        .await;
    }
}

pub(crate) fn requires_reload(tool_id: &str) -> bool {
    matches!(tool_id, "claude_desktop" | "codex_desktop")
}

pub(crate) async fn is_running(tool_id: &str, path: &Path) -> Result<bool, AdapterFailure> {
    if !requires_reload(tool_id) {
        return Ok(false);
    }
    let tool_id = tool_id.to_owned();
    let path = path.to_owned();
    // A read-only OS query may outlive its caller, but it cannot later close
    // an app or mutate configuration. Bound it so state checks release the UI.
    tokio::time::timeout(
        START_LIMIT,
        tokio::task::spawn_blocking(move || platform::is_running(&tool_id, &path)),
    )
    .await
    .map_err(|_| AdapterFailure::LaunchError("desktop_state_unavailable"))?
    .map_err(|_| AdapterFailure::LaunchError("desktop_state_unavailable"))?
}

async fn blocking_normal_quit(tool_id: &str, path: &Path) -> Result<bool, AdapterFailure> {
    let tool_id = tool_id.to_owned();
    let path = path.to_owned();
    tokio::task::spawn_blocking(move || platform::request_normal_quit(&tool_id, &path))
        .await
        .map_err(|_| AdapterFailure::LaunchError("desktop_state_unavailable"))?
}

async fn blocking_force_quit(tool_id: &str, path: &Path) -> Result<bool, AdapterFailure> {
    let tool_id = tool_id.to_owned();
    let path = path.to_owned();
    tokio::task::spawn_blocking(move || platform::force_quit(&tool_id, &path))
        .await
        .map_err(|_| AdapterFailure::LaunchError("desktop_state_unavailable"))?
}

async fn wait_until_stopped(
    tool_id: &str,
    path: &Path,
    limit: Duration,
) -> Result<bool, AdapterFailure> {
    let deadline = tokio::time::Instant::now() + limit;
    while tokio::time::Instant::now() < deadline {
        if !tokio::time::timeout_at(deadline, is_running(tool_id, path))
            .await
            .map_err(|_| AdapterFailure::LaunchError("desktop_state_unavailable"))??
        {
            return Ok(true);
        }
        tokio::time::sleep(EXIT_POLL).await;
    }
    Ok(false)
}

/// Exit restoration must not use the reconfiguration force-quit fallback.
/// Unsaved-work dialogs remain authoritative; never reopen on the user's behalf.
pub(crate) async fn quit_for_exit_restore(
    tool_id: &str,
    path: &Path,
) -> Result<(), AdapterFailure> {
    if !requires_reload(tool_id) { return Ok(()); }
    blocking_normal_quit(tool_id, path).await?;
    if wait_until_stopped(tool_id, path, NORMAL_QUIT_LIMIT).await? {
        Ok(())
    } else {
        Err(AdapterFailure::LaunchError("graceful_restart_required"))
    }
}

/// Close the exact discovered desktop installation after the user has
/// confirmed that work is saved. `Ok(true)` means a live application was
/// closed; `Ok(false)` means it was already stopped. Windows gets a controlled
/// exact-path fallback for background processes left behind after WM_CLOSE.
pub(crate) async fn quit_for_reconfigure(
    tool_id: &str,
    path: &Path,
) -> Result<bool, AdapterFailure> {
    if !requires_reload(tool_id) {
        return Ok(false);
    }
    let requested = blocking_normal_quit(tool_id, path).await?;
    if !requested {
        return Ok(false);
    }
    if wait_until_stopped(tool_id, path, NORMAL_QUIT_LIMIT).await? {
        return Ok(true);
    }
    if blocking_force_quit(tool_id, path).await?
        && wait_until_stopped(tool_id, path, FORCED_QUIT_LIMIT).await?
    {
        return Ok(true);
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

    pub(super) fn startup_observed(tool_id: &str, path: &Path) -> Result<bool, AdapterFailure> {
        let identifier = bundle_identifier(tool_id).ok_or(AdapterFailure::UnsupportedProfile)?;
        let applications = NSRunningApplication::runningApplicationsWithBundleIdentifier(
            &NSString::from_str(identifier),
        );
        Ok(applications.iter().any(|app| {
            matches_path(&app, path) && !app.isTerminated() && app.isFinishedLaunching()
        }))
    }

    pub(super) fn force_quit(_tool_id: &str, _path: &Path) -> Result<bool, AdapterFailure> {
        Ok(false)
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
                    OpenProcess, QueryFullProcessImageNameW, TerminateProcess, PROCESS_NAME_WIN32,
                    PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_TERMINATE,
                },
            },
            UI::WindowsAndMessaging::{
                EnumWindows, GetWindowThreadProcessId, IsHungAppWindow, IsWindowVisible,
                PostMessageW, WM_CLOSE,
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

    struct Process(HANDLE);

    impl Drop for Process {
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

    fn process_path_from_handle(process: HANDLE) -> Option<String> {
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
        queried.ok()?;
        String::from_utf16(buffer.get(..length as usize)?).ok()
    }

    fn process_path(pid: u32) -> Option<String> {
        let process =
            Process(unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()? });
        process_path_from_handle(process.0)
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

    pub(super) fn startup_observed(_tool_id: &str, path: &Path) -> Result<bool, AdapterFailure> {
        let windows = matching_windows(matching_processes(path)?)?;
        Ok(windows.into_iter().any(|hwnd| unsafe {
            IsWindowVisible(hwnd).as_bool() && !IsHungAppWindow(hwnd).as_bool()
        }))
    }

    pub(super) fn request_normal_quit(_tool_id: &str, path: &Path) -> Result<bool, AdapterFailure> {
        let pids = matching_processes(path)?;
        if pids.is_empty() {
            return Ok(false);
        }
        let windows = matching_windows(pids)?;
        for hwnd in &windows {
            unsafe { PostMessageW(Some(*hwnd), WM_CLOSE, WPARAM(0), LPARAM(0)) }
                .map_err(|_| AdapterFailure::LaunchError("graceful_restart_required"))?;
        }
        // A background-only process has no window to receive WM_CLOSE. It is
        // still a live exact installation and is handled by the controlled
        // fallback after the normal grace period.
        Ok(true)
    }

    pub(super) fn force_quit(_tool_id: &str, path: &Path) -> Result<bool, AdapterFailure> {
        let target = normalized(path);
        let pids = matching_processes(path)?;
        if pids.is_empty() {
            return Ok(false);
        }
        // Consent promises a fallback for background remnants, not killing a
        // still-visible editor or its save dialog after the grace period.
        if matching_windows(pids.clone())?
            .into_iter()
            .any(|hwnd| unsafe { IsWindowVisible(hwnd).as_bool() })
        {
            return Err(AdapterFailure::LaunchError("graceful_restart_required"));
        }
        let mut terminated = false;
        for pid in pids {
            let process = Process(
                unsafe {
                    OpenProcess(
                        PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_TERMINATE,
                        false,
                        pid,
                    )
                }
                .map_err(|_| AdapterFailure::LaunchError("graceful_restart_required"))?,
            );
            // Revalidate on the same process handle immediately before the
            // destructive call; a recycled PID or different installation is
            // never terminated.
            let still_matches = process_path_from_handle(process.0)
                .map(|candidate| normalized(Path::new(&candidate)) == target)
                .unwrap_or(false);
            if !still_matches {
                continue;
            }
            unsafe { TerminateProcess(process.0, 0) }
                .map_err(|_| AdapterFailure::LaunchError("graceful_restart_required"))?;
            terminated = true;
        }
        Ok(terminated)
    }
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
mod platform {
    use std::path::Path;

    use super::AdapterFailure;

    pub(super) fn is_running(_tool_id: &str, _path: &Path) -> Result<bool, AdapterFailure> {
        Ok(false)
    }

    pub(super) fn startup_observed(_tool_id: &str, _path: &Path) -> Result<bool, AdapterFailure> {
        Err(AdapterFailure::UnsupportedProfile)
    }

    pub(super) fn request_normal_quit(
        _tool_id: &str,
        _path: &Path,
    ) -> Result<bool, AdapterFailure> {
        Ok(false)
    }

    pub(super) fn force_quit(_tool_id: &str, _path: &Path) -> Result<bool, AdapterFailure> {
        Ok(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn dispatch_without_startup_evidence_is_not_success() {
        assert!(matches!(
            wait_for_start(|| async { Ok(false) }, Duration::ZERO).await,
            Err(AdapterFailure::LaunchError("desktop_start_unconfirmed"))
        ));
        assert!(wait_for_start(|| async { Ok(true) }, Duration::ZERO)
            .await
            .is_ok());
        assert!(matches!(
            wait_for_start(
                || async { Err(AdapterFailure::LaunchError("desktop_state_unavailable")) },
                Duration::ZERO
            )
            .await,
            Err(AdapterFailure::LaunchError("desktop_state_unavailable"))
        ));
        assert!(matches!(
            wait_for_start(
                std::future::pending::<Result<bool, AdapterFailure>>,
                Duration::from_millis(1),
            )
            .await,
            Err(AdapterFailure::LaunchError("desktop_start_unconfirmed"))
        ));
    }

    #[tokio::test]
    async fn delayed_start_is_observed_without_launching_a_real_application() {
        let mut observations = [false, true].into_iter();
        assert!(wait_for_start(
            || std::future::ready(Ok(observations.next().unwrap_or(false))),
            Duration::from_secs(1)
        )
        .await
        .is_ok());
    }

    #[test]
    fn only_desktop_targets_are_eligible_for_reload_control() {
        assert!(requires_reload("claude_desktop"));
        assert!(requires_reload("codex_desktop"));
        for tool in ["claude_code", "pi", "dsh_web"] {
            assert!(!requires_reload(tool), "{tool}");
        }
    }
}

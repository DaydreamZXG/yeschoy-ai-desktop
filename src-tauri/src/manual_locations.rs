//! Installations the user located by hand.
//!
//! Discovery reads the places an installer records itself: the uninstall
//! registry, App Paths, protocol handlers, PATH, npm's prefix. A portable copy
//! unzipped onto any drive, an installer that registered nothing, or a CLI in a
//! folder no PATH mentions is invisible to all of them. For those the user
//! points at the file once and it is remembered.
//!
//! The renderer never supplies a path: the native picker returns one, it is
//! held to the same identity rules discovery applies, and every later read
//! checks it again. A location that stops passing (uninstalled, moved, an
//! external drive that is not plugged in) is skipped, not deleted, so the
//! automatic scan takes over and the entry works again once the drive is back.

use crate::tool_discovery::{executable_filenames, is_executable_candidate, validate_request_id};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

const STORE_VERSION: u32 = 1;
const MAX_STORE_BYTES: u64 = 64 * 1024;
const MAX_PATH_BYTES: usize = 4096;

/// Serialises read-modify-write of the store between concurrent commands.
static STORE_LOCK: Mutex<()> = Mutex::new(());

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Kind {
    /// A command-line tool, by executable name.
    Cli(&'static str),
    /// A desktop application, by discovery app id.
    Desktop(&'static str),
}

pub(crate) fn kind(tool_id: &str) -> Option<Kind> {
    crate::tool_adapters::executable_for(tool_id)
        .map(Kind::Cli)
        .or_else(|| crate::tool_adapters::desktop_id_for(tool_id).map(Kind::Desktop))
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Rejection {
    /// Not the file this tool is started from.
    WrongFile,
    /// The right name, but not the vendor's signed application.
    #[cfg_attr(not(any(target_os = "macos", target_os = "windows")), allow(dead_code))]
    NotGenuine,
    /// A private copy inside another application's bundle.
    BundledCopy,
    #[cfg_attr(any(target_os = "macos", target_os = "windows"), allow(dead_code))]
    Unsupported,
}

impl Rejection {
    const fn as_str(self) -> &'static str {
        match self {
            Self::WrongFile => "wrong_file",
            Self::NotGenuine => "not_genuine",
            Self::BundledCopy => "bundled_copy",
            Self::Unsupported => "unsupported_platform",
        }
    }
}

// Read tolerantly; a file from another version is ignored by `version`.
#[derive(Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct Store {
    version: u32,
    #[serde(default)]
    locations: BTreeMap<String, String>,
}

fn store_path() -> Option<PathBuf> {
    Some(
        crate::tool_adapters::user_home()?
            .join(".yeschoy")
            .join("manual-locations.json"),
    )
}

fn read_store_at(path: &Path) -> Store {
    let readable = std::fs::metadata(path)
        .is_ok_and(|metadata| metadata.is_file() && metadata.len() <= MAX_STORE_BYTES);
    if !readable {
        return Store::default();
    }
    std::fs::read(path)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Store>(&bytes).ok())
        .filter(|store| store.version == STORE_VERSION)
        .unwrap_or_default()
}

fn write_store_at(path: &Path, store: &Store) -> std::io::Result<()> {
    let bytes = serde_json::to_vec_pretty(store)?;
    crate::tool_adapters::common::atomic_write_bounded(path, &bytes, MAX_STORE_BYTES)
}

/// The remembered location for `tool_id`, unchecked. Callers validate it.
fn stored(tool_id: &str) -> Option<PathBuf> {
    let path = read_store_at(&store_path()?)
        .locations
        .remove(tool_id)
        .map(PathBuf::from)?;
    path.is_absolute().then_some(path)
}

fn remember(tool_id: &str, location: &Path) -> std::io::Result<()> {
    let path = store_path().ok_or(std::io::ErrorKind::NotFound)?;
    let location = location
        .to_str()
        .filter(|text| text.len() <= MAX_PATH_BYTES)
        .ok_or(std::io::ErrorKind::InvalidInput)?;
    let _guard = STORE_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let mut store = read_store_at(&path);
    store.version = STORE_VERSION;
    store
        .locations
        .insert(tool_id.to_owned(), location.to_owned());
    write_store_at(&path, &store)
}

fn forget(tool_id: &str) -> std::io::Result<()> {
    let Some(path) = store_path() else {
        return Ok(());
    };
    let _guard = STORE_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let mut store = read_store_at(&path);
    if store.locations.remove(tool_id).is_none() {
        return Ok(());
    }
    store.version = STORE_VERSION;
    write_store_at(&path, &store)
}

/// Hold a picked CLI to the rules discovery applies to the ones it finds: the
/// exact executable name, runnable, and not another app's bundled copy.
pub(crate) fn validate_cli(executable: &str, path: &Path) -> Result<PathBuf, Rejection> {
    if !path.is_absolute() {
        return Err(Rejection::WrongFile);
    }
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or(Rejection::WrongFile)?;
    let named = executable_filenames(executable).iter().any(|expected| {
        if cfg!(target_os = "windows") {
            expected.eq_ignore_ascii_case(name)
        } else {
            expected == name
        }
    });
    if !named || !is_executable_candidate(path) {
        return Err(Rejection::WrongFile);
    }
    let canonical = std::fs::canonicalize(path).map_err(|_| Rejection::WrongFile)?;
    if cfg!(target_os = "macos")
        && (crate::tool_selection_core::is_app_component(path)
            || crate::tool_selection_core::is_app_component(&canonical))
    {
        return Err(Rejection::BundledCopy);
    }
    // The wrapper itself, not its target: an npm symlink resolves to a .js
    // file that cannot be started directly, same as `discover_candidates`.
    Ok(path.to_path_buf())
}

/// The remembered CLI for this executable, if it still checks out.
pub(crate) fn cli_location(executable: &str) -> Option<PathBuf> {
    let tool_id = crate::tool_adapters::cli_tool_for(executable)?;
    validate_cli(executable, &stored(tool_id)?).ok()
}

/// The remembered desktop application, unchecked; desktop discovery owns the
/// platform identity rules and applies them before using it.
pub(crate) fn desktop_location(app_id: &str) -> Option<PathBuf> {
    stored(app_id)
}

/// Whether the user has picked a location for `tool_id`, usable or not.
pub(crate) fn is_stored(tool_id: &str) -> bool {
    stored(tool_id).is_some()
}

fn validate(kind: Kind, path: &Path) -> Result<PathBuf, Rejection> {
    match kind {
        Kind::Cli(executable) => validate_cli(executable, path),
        Kind::Desktop(app_id) => crate::desktop_app_discovery::validate_manual(app_id, path),
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ManualLocationRequest {
    request_id: String,
    tool_id: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ManualLocationResult {
    request_id: String,
    /// `saved`, `cleared`, `cancelled`, `unavailable`, or a rejection code.
    outcome: &'static str,
}

fn checked(request: &ManualLocationRequest) -> Result<Kind, String> {
    validate_request_id(&request.request_id)?;
    kind(&request.tool_id).ok_or_else(|| "invalid_tool".to_owned())
}

fn result(request: ManualLocationRequest, outcome: &'static str) -> ManualLocationResult {
    ManualLocationResult {
        request_id: request.request_id,
        outcome,
    }
}

/// Let the user point at an installation discovery could not find.
#[tauri::command]
pub async fn manual_location_pick(
    window: tauri::Window,
    request: ManualLocationRequest,
) -> Result<ManualLocationResult, String> {
    let kind = checked(&request)?;
    let display_name = crate::tool_adapters::display_name_for(&request.tool_id).unwrap_or_default();
    let picked = match picker::pick(&window, kind, display_name).await {
        Ok(Some(path)) => path,
        Ok(None) => return Ok(result(request, "cancelled")),
        Err(picker::Failure::Unsupported) => return Ok(result(request, "unsupported_platform")),
        Err(picker::Failure::Unavailable) => return Ok(result(request, "unavailable")),
    };
    let tool_id = request.tool_id.clone();
    let outcome = tokio::task::spawn_blocking(move || match validate(kind, &picked) {
        Ok(location) => match remember(&tool_id, &location) {
            Ok(()) => "saved",
            Err(error) => {
                log::warn!(
                    "manual_location stage=save tool={tool_id} kind={:?}",
                    error.kind()
                );
                "unavailable"
            }
        },
        Err(rejection) => {
            // The reason only, never the path: it can carry a user name.
            log::info!(
                "manual_location stage=rejected tool={tool_id} reason={}",
                rejection.as_str()
            );
            rejection.as_str()
        }
    })
    .await
    .unwrap_or("unavailable");
    Ok(result(request, outcome))
}

/// Go back to automatic discovery for this tool.
#[tauri::command]
pub async fn manual_location_clear(
    request: ManualLocationRequest,
) -> Result<ManualLocationResult, String> {
    checked(&request)?;
    let tool_id = request.tool_id.clone();
    let cleared = tokio::task::spawn_blocking(move || forget(&tool_id))
        .await
        .map_err(|_| "manual_location_unavailable".to_owned())?;
    Ok(result(
        request,
        if cleared.is_ok() {
            "cleared"
        } else {
            "unavailable"
        },
    ))
}

mod picker {
    use super::Kind;
    use std::path::PathBuf;

    pub(super) enum Failure {
        #[cfg_attr(any(target_os = "windows", target_os = "macos"), allow(dead_code))]
        Unsupported,
        #[cfg_attr(not(any(target_os = "windows", target_os = "macos")), allow(dead_code))]
        Unavailable,
    }

    #[cfg_attr(not(any(target_os = "macos", target_os = "windows")), allow(dead_code))]
    fn prompt(kind: Kind, display_name: &str) -> String {
        let language = crate::ui_language::current();
        match kind {
            Kind::Desktop(_) => format!(
                "{} {display_name}",
                language.pick("选择要使用的", "Choose the copy of")
            ),
            Kind::Cli(executable) => format!(
                "{} {executable} {}",
                language.pick("选择", "Choose the folder that contains"),
                language.pick("所在的文件夹", "")
            )
            .trim_end()
            .to_owned(),
        }
    }

    /// Windows: the system Open dialog, owned by our window so it stays in
    /// front, filtered to the exact file names this tool starts from.
    #[cfg(target_os = "windows")]
    pub(super) async fn pick(
        window: &tauri::Window,
        kind: Kind,
        display_name: &str,
    ) -> Result<Option<PathBuf>, Failure> {
        let owner = window
            .hwnd()
            .map(|hwnd| hwnd.0 as isize)
            .unwrap_or_default();
        let title = match kind {
            Kind::Desktop(_) => prompt(kind, display_name),
            Kind::Cli(executable) => {
                let language = crate::ui_language::current();
                format!("{} {executable}", language.pick("选择", "Choose"))
            }
        };
        let (filter_name, filter) = match kind {
            Kind::Desktop(app_id) => {
                let name = match app_id {
                    "claude_desktop" => "Claude.exe",
                    "codex_desktop" => "Codex.exe;ChatGPT.exe",
                    "workbuddy" => "WorkBuddy.exe",
                    "dsh_desktop" => "DeepSeek Harness.exe",
                    _ => "*.exe",
                };
                (name.replace(';', ", "), name.to_owned())
            }
            Kind::Cli(executable) => {
                let names = crate::tool_discovery::executable_filenames(executable);
                (names.join(", "), names.join(";"))
            }
        };
        let (sender, receiver) = tokio::sync::oneshot::channel();
        // COM needs a single-threaded apartment of its own; a dedicated thread
        // gets one without touching the event loop's.
        std::thread::spawn(move || {
            let _ = sender.send(open_dialog(owner, &title, &filter_name, &filter));
        });
        receiver.await.map_err(|_| Failure::Unavailable)?
    }

    #[cfg(target_os = "windows")]
    fn open_dialog(
        owner: isize,
        title: &str,
        filter_name: &str,
        filter: &str,
    ) -> Result<Option<PathBuf>, Failure> {
        use std::os::windows::ffi::OsStringExt;
        use windows::core::{HRESULT, PCWSTR};
        use windows::Win32::Foundation::{ERROR_CANCELLED, HWND};
        use windows::Win32::System::Com::{
            CoCreateInstance, CoInitializeEx, CoTaskMemFree, CoUninitialize, CLSCTX_INPROC_SERVER,
            COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE,
        };
        use windows::Win32::UI::Shell::Common::COMDLG_FILTERSPEC;
        use windows::Win32::UI::Shell::{
            FileOpenDialog, IFileOpenDialog, FOS_DONTADDTORECENT, FOS_FILEMUSTEXIST,
            FOS_FORCEFILESYSTEM, FOS_NOCHANGEDIR, FOS_PATHMUSTEXIST, SIGDN_FILESYSPATH,
        };

        let wide = |text: &str| text.encode_utf16().chain([0]).collect::<Vec<u16>>();
        let title = wide(title);
        let filter_name = wide(filter_name);
        let filter = wide(filter);
        unsafe {
            let initialized =
                CoInitializeEx(None, COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE).is_ok();
            let outcome = (|| {
                let dialog: IFileOpenDialog =
                    CoCreateInstance(&FileOpenDialog, None, CLSCTX_INPROC_SERVER)
                        .map_err(|_| Failure::Unavailable)?;
                let options = dialog.GetOptions().map_err(|_| Failure::Unavailable)?;
                dialog
                    .SetOptions(
                        options
                            | FOS_FORCEFILESYSTEM
                            | FOS_FILEMUSTEXIST
                            | FOS_PATHMUSTEXIST
                            | FOS_NOCHANGEDIR
                            | FOS_DONTADDTORECENT,
                    )
                    .map_err(|_| Failure::Unavailable)?;
                let _ = dialog.SetTitle(PCWSTR(title.as_ptr()));
                let _ = dialog.SetFileTypes(&[COMDLG_FILTERSPEC {
                    pszName: PCWSTR(filter_name.as_ptr()),
                    pszSpec: PCWSTR(filter.as_ptr()),
                }]);
                let owner = (owner != 0).then_some(HWND(owner as *mut core::ffi::c_void));
                if let Err(error) = dialog.Show(owner) {
                    return if error.code() == HRESULT::from_win32(ERROR_CANCELLED.0) {
                        Ok(None)
                    } else {
                        Err(Failure::Unavailable)
                    };
                }
                let item = dialog.GetResult().map_err(|_| Failure::Unavailable)?;
                let name = item
                    .GetDisplayName(SIGDN_FILESYSPATH)
                    .map_err(|_| Failure::Unavailable)?;
                let path = std::ffi::OsString::from_wide(name.as_wide());
                CoTaskMemFree(Some(name.0 as *const core::ffi::c_void));
                Ok(Some(PathBuf::from(path)))
            })();
            if initialized {
                CoUninitialize();
            }
            outcome
        }
    }

    /// macOS: AppleScript's chooser, run by osascript so no window or main
    /// thread of ours is involved. An application is picked as a bundle; a CLI
    /// by its folder, because the chooser resolves symlinks and an npm shim
    /// would come back as the JavaScript file it points at.
    #[cfg(target_os = "macos")]
    pub(super) async fn pick(
        _window: &tauri::Window,
        kind: Kind,
        display_name: &str,
    ) -> Result<Option<PathBuf>, Failure> {
        let chooser = match kind {
            Kind::Desktop(_) => {
                "choose file with prompt (item 1 of argv) of type {\"com.apple.application-bundle\"} default location (path to applications folder)"
            }
            Kind::Cli(_) => {
                "choose folder with prompt (item 1 of argv) default location (path to home folder) with invisibles"
            }
        };
        let output = tokio::process::Command::new("/usr/bin/osascript")
            .args([
                "-e",
                "on run argv",
                "-e",
                "activate",
                "-e",
                &format!("return POSIX path of ({chooser})"),
                "-e",
                "end run",
                &prompt(kind, display_name),
            ])
            .kill_on_drop(true)
            .output()
            .await
            .map_err(|_| Failure::Unavailable)?;
        if !output.status.success() {
            // -128 is "User canceled."; anything else is a real failure.
            return if String::from_utf8_lossy(&output.stderr).contains("-128") {
                Ok(None)
            } else {
                Err(Failure::Unavailable)
            };
        }
        let text = String::from_utf8(output.stdout).map_err(|_| Failure::Unavailable)?;
        let chosen = PathBuf::from(text.trim_end_matches(['\n', '\r']));
        Ok(Some(match kind {
            Kind::Desktop(_) => chosen,
            Kind::Cli(executable) => chosen.join(executable),
        }))
    }

    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    pub(super) async fn pick(
        _window: &tauri::Window,
        _kind: Kind,
        _display_name: &str,
    ) -> Result<Option<PathBuf>, Failure> {
        Err(Failure::Unsupported)
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn fixture(name: &str) -> PathBuf {
        crate::tool_adapters::common::temporary_working_directory(name).unwrap()
    }

    #[test]
    fn a_cli_must_be_the_exact_runnable_executable() {
        let directory = fixture("manual-cli");
        let dsh = directory.join("dsh");
        std::fs::write(&dsh, "#!/bin/sh\n").unwrap();
        std::fs::set_permissions(&dsh, std::fs::Permissions::from_mode(0o700)).unwrap();
        assert_eq!(validate_cli("dsh", &dsh), Ok(dsh.clone()));
        assert_eq!(validate_cli("claude", &dsh), Err(Rejection::WrongFile));

        let other = directory.join("dsh-old");
        std::fs::copy(&dsh, &other).unwrap();
        assert_eq!(validate_cli("dsh", &other), Err(Rejection::WrongFile));

        std::fs::set_permissions(&dsh, std::fs::Permissions::from_mode(0o600)).unwrap();
        assert_eq!(validate_cli("dsh", &dsh), Err(Rejection::WrongFile));
        assert_eq!(
            validate_cli("dsh", Path::new("dsh")),
            Err(Rejection::WrongFile)
        );
        let _ = std::fs::remove_dir_all(directory);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn a_cli_inside_another_app_is_refused() {
        let directory = fixture("manual-bundled");
        let bin = directory.join("ChatGPT.app/Contents/Resources");
        std::fs::create_dir_all(&bin).unwrap();
        let codex = bin.join("dsh");
        std::fs::write(&codex, "#!/bin/sh\n").unwrap();
        std::fs::set_permissions(&codex, std::fs::Permissions::from_mode(0o700)).unwrap();
        assert_eq!(validate_cli("dsh", &codex), Err(Rejection::BundledCopy));
        let _ = std::fs::remove_dir_all(directory);
    }

    #[test]
    fn the_store_survives_unknown_and_broken_files() {
        let directory = fixture("manual-store");
        let path = directory.join("manual-locations.json");
        assert!(read_store_at(&path).locations.is_empty());

        std::fs::write(&path, b"{not json").unwrap();
        assert!(read_store_at(&path).locations.is_empty());

        std::fs::write(&path, br#"{"version":2,"locations":{"pi":"/x/pi"}}"#).unwrap();
        assert!(read_store_at(&path).locations.is_empty());

        let mut store = Store {
            version: STORE_VERSION,
            ..Store::default()
        };
        store.locations.insert(
            "dsh_desktop".into(),
            "/Volumes/E/DeepSeek Harness.app".into(),
        );
        write_store_at(&path, &store).unwrap();
        assert_eq!(
            read_store_at(&path)
                .locations
                .get("dsh_desktop")
                .map(String::as_str),
            Some("/Volumes/E/DeepSeek Harness.app")
        );
        let _ = std::fs::remove_dir_all(directory);
    }

    #[test]
    fn only_activation_tools_can_be_located() {
        assert_eq!(kind("dsh_web"), Some(Kind::Cli("dsh")));
        assert_eq!(kind("dsh_desktop"), Some(Kind::Desktop("dsh_desktop")));
        assert_eq!(kind("opencode"), None);
        assert_eq!(kind("../../etc"), None);
    }
}

//! Shared, native-resolved desktop launching for initialization and daily Open.
#[cfg(any(target_os = "macos", all(test, unix)))]
use std::time::{Duration, Instant};
use std::{
    path::Path,
    process::{Command, Stdio},
};

use crate::desktop_app_discovery_core::{DesktopLaunchTarget, WindowsPackageApplication};

use super::AdapterFailure;

pub(crate) fn launch(app_id: &str, path: &Path) -> Result<(), AdapterFailure> {
    let target = crate::desktop_app_discovery::resolve_launch_target(app_id, path)
        .ok_or_else(|| launch_error("resolve", "desktop_launch_target_changed", None))?;
    dispatch(&target, &mut NativeLauncher)
}

trait Launcher {
    fn open_bundle(&mut self, path: &Path) -> Result<(), AdapterFailure>;
    fn spawn_executable(&mut self, path: &Path) -> Result<(), AdapterFailure>;
    fn activate_package(&mut self, app: &WindowsPackageApplication) -> Result<(), AdapterFailure>;
}

fn dispatch(
    target: &DesktopLaunchTarget,
    launcher: &mut impl Launcher,
) -> Result<(), AdapterFailure> {
    match target {
        DesktopLaunchTarget::MacBundle(path) => launcher.open_bundle(path),
        DesktopLaunchTarget::WindowsExecutable(path) => launcher.spawn_executable(path),
        DesktopLaunchTarget::WindowsPackage(app) => launcher.activate_package(app),
    }
}

fn launch_error(stage: &'static str, reason: &'static str, code: Option<i32>) -> AdapterFailure {
    // Never format std::io::Error / windows::Error, command lines, or targets.
    log::warn!("desktop_launch stage={stage} os_code={code:?}");
    AdapterFailure::LaunchError(reason)
}

fn io_error(stage: &'static str, reason: &'static str, error: std::io::Error) -> AdapterFailure {
    let reason = if error.kind() == std::io::ErrorKind::PermissionDenied {
        "desktop_launch_access_denied"
    } else {
        reason
    };
    launch_error(stage, reason, error.raw_os_error())
}

struct NativeLauncher;

impl Launcher for NativeLauncher {
    fn open_bundle(&mut self, path: &Path) -> Result<(), AdapterFailure> {
        #[cfg(target_os = "macos")]
        {
            let mut command = Command::new("/usr/bin/open");
            command.arg(path);
            run_open_command(command, Duration::from_secs(15))
        }
        #[cfg(not(target_os = "macos"))]
        {
            let _ = path;
            Err(AdapterFailure::UnsupportedProfile)
        }
    }

    fn spawn_executable(&mut self, path: &Path) -> Result<(), AdapterFailure> {
        #[cfg(target_os = "windows")]
        {
            Command::new(path)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .map(|_| ())
                .map_err(|error| io_error("spawn", "desktop_launch_start_failed", error))
        }
        #[cfg(not(target_os = "windows"))]
        {
            let _ = path;
            Err(AdapterFailure::UnsupportedProfile)
        }
    }

    fn activate_package(&mut self, app: &WindowsPackageApplication) -> Result<(), AdapterFailure> {
        #[cfg(target_os = "windows")]
        {
            activate_windows_package(app)
        }
        #[cfg(not(target_os = "windows"))]
        {
            let _ = app;
            Err(AdapterFailure::UnsupportedProfile)
        }
    }
}

#[cfg(any(target_os = "macos", all(test, unix)))]
fn run_open_command(mut command: Command, maximum: Duration) -> Result<(), AdapterFailure> {
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|error| io_error("open_start", "desktop_launch_start_failed", error))?;
    let deadline = Instant::now() + maximum;
    loop {
        match child.try_wait() {
            Ok(Some(status)) if status.success() => return Ok(()),
            Ok(Some(status)) => {
                return Err(launch_error(
                    "open_exit",
                    "desktop_launch_exit_failed",
                    status.code(),
                ))
            }
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(io_error("open_wait", "desktop_launch_wait_failed", error));
            }
            Ok(None) if Instant::now() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(launch_error(
                    "open_timeout",
                    "desktop_launch_wait_failed",
                    None,
                ));
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(25)),
        }
    }
}

#[cfg(target_os = "windows")]
fn activate_windows_package(app: &WindowsPackageApplication) -> Result<(), AdapterFailure> {
    use windows::{
        core::PCWSTR,
        Win32::{
            Foundation::RPC_E_CHANGED_MODE,
            System::Com::{
                CoAllowSetForegroundWindow, CoCreateInstance, CoInitializeEx, CoUninitialize,
                CLSCTX_LOCAL_SERVER, COINIT_APARTMENTTHREADED,
            },
            UI::Shell::{
                ApplicationActivationManager, IApplicationActivationManager, AO_NOERRORUI,
            },
        },
    };

    let map_error = |stage, reason, code: i32| {
        launch_error(
            stage,
            if code as u32 == 0x80070005 {
                "desktop_launch_access_denied"
            } else {
                reason
            },
            Some(code),
        )
    };
    let initialized = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
    if initialized.is_err() && initialized != RPC_E_CHANGED_MODE {
        return Err(map_error(
            "com_initialize",
            "desktop_launch_activation_unavailable",
            initialized.0,
        ));
    }
    struct Apartment(bool);
    impl Drop for Apartment {
        fn drop(&mut self) {
            if self.0 {
                unsafe {
                    CoUninitialize();
                }
            }
        }
    }
    let _apartment = Apartment(initialized.is_ok());
    let manager: IApplicationActivationManager =
        unsafe { CoCreateInstance(&ApplicationActivationManager, None, CLSCTX_LOCAL_SERVER) }
            .map_err(|error| {
                map_error(
                    "activation_create",
                    "desktop_launch_activation_unavailable",
                    error.code().0,
                )
            })?;
    // Foreground permission is best effort; activation itself supplies the receipt.
    let _ = unsafe { CoAllowSetForegroundWindow(&manager, None) };
    let aumid = app
        .app_user_model_id()
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    // The AUMID is returned by GetPackageApplicationIds and matched against the
    // installed manifest. Never fabricate an !App suffix or replay registry argv.
    unsafe { manager.ActivateApplication(PCWSTR(aumid.as_ptr()), PCWSTR::null(), AO_NOERRORUI) }
        .map(|_| ())
        .map_err(|error| {
            map_error(
                "package_activate",
                "desktop_launch_activation_failed",
                error.code().0,
            )
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[derive(Default)]
    struct RecordingLauncher {
        actions: Vec<&'static str>,
        failure: Option<AdapterFailure>,
    }
    impl RecordingLauncher {
        fn record(&mut self, action: &'static str) -> Result<(), AdapterFailure> {
            self.actions.push(action);
            self.failure.map_or(Ok(()), Err)
        }
    }
    impl Launcher for RecordingLauncher {
        fn open_bundle(&mut self, _: &Path) -> Result<(), AdapterFailure> {
            self.record("bundle")
        }
        fn spawn_executable(&mut self, _: &Path) -> Result<(), AdapterFailure> {
            self.record("exe")
        }
        fn activate_package(
            &mut self,
            _: &WindowsPackageApplication,
        ) -> Result<(), AdapterFailure> {
            self.record("package")
        }
    }

    #[test]
    fn package_dispatch_never_spawns_its_executable_or_guesses_application_id() {
        let app = WindowsPackageApplication::from_installed_package(
            "codex_desktop",
            "OpenAI.Codex_1.2.3.4_x64__2p2nqsd0c76g0",
            "OpenAI.Codex_2p2nqsd0c76g0",
            "OpenAI.Codex_2p2nqsd0c76g0!CodexDesktop",
        )
        .unwrap();
        assert!(app.app_user_model_id().ends_with("!CodexDesktop"));
        let mut runner = RecordingLauncher::default();
        dispatch(&DesktopLaunchTarget::WindowsPackage(app), &mut runner).unwrap();
        assert_eq!(runner.actions, ["package"]);
        dispatch(
            &DesktopLaunchTarget::WindowsExecutable(PathBuf::from("C:\\Apps\\Codex.exe")),
            &mut runner,
        )
        .unwrap();
        dispatch(
            &DesktopLaunchTarget::MacBundle(PathBuf::from("/Applications/Codex.app")),
            &mut runner,
        )
        .unwrap();
        assert_eq!(runner.actions, ["package", "exe", "bundle"]);
    }

    #[test]
    fn dispatch_failure_is_preserved_without_executable_fallback() {
        let app = WindowsPackageApplication::from_installed_package(
            "claude_desktop",
            "Claude_1.2.3.4_x64__123456789abcd",
            "Claude_123456789abcd",
            "Claude_123456789abcd!Claude",
        )
        .unwrap();
        let failure = AdapterFailure::LaunchError("desktop_launch_activation_failed");
        let mut runner = RecordingLauncher {
            failure: Some(failure),
            ..Default::default()
        };
        assert_eq!(
            dispatch(&DesktopLaunchTarget::WindowsPackage(app), &mut runner),
            Err(failure)
        );
        assert_eq!(runner.actions, ["package"]);
    }

    #[test]
    fn errors_are_closed_and_do_not_contain_native_error_text() {
        let failure = io_error(
            "spawn",
            "desktop_launch_start_failed",
            std::io::Error::new(std::io::ErrorKind::PermissionDenied, "secret/path/api-key"),
        );
        assert_eq!(
            failure,
            AdapterFailure::LaunchError("desktop_launch_access_denied")
        );
        assert!(!format!("{failure:?}").contains("secret"));
    }

    #[cfg(unix)]
    #[test]
    fn executable_fixture_requires_successful_open_exit_and_reports_start_and_timeout() {
        let directory =
            super::super::common::temporary_working_directory("desktop-launch").unwrap();
        let fixture = directory.join("open-fixture");
        use std::os::unix::fs::PermissionsExt;
        std::fs::write(&fixture, b"#!/bin/sh\nexit 7\n").unwrap();
        std::fs::set_permissions(&fixture, std::fs::Permissions::from_mode(0o700)).unwrap();
        assert_eq!(
            run_open_command(Command::new(&fixture), Duration::from_secs(1)),
            Err(AdapterFailure::LaunchError("desktop_launch_exit_failed"))
        );
        std::fs::write(&fixture, b"#!/bin/sh\nexit 0\n").unwrap();
        assert_eq!(
            run_open_command(Command::new(&fixture), Duration::from_secs(1)),
            Ok(())
        );
        std::fs::write(&fixture, b"#!/bin/sh\nwhile :; do :; done\n").unwrap();
        assert_eq!(
            run_open_command(Command::new(&fixture), Duration::from_millis(30)),
            Err(AdapterFailure::LaunchError("desktop_launch_wait_failed"))
        );
        assert_eq!(
            run_open_command(
                Command::new(directory.join("absent")),
                Duration::from_secs(1)
            ),
            Err(AdapterFailure::LaunchError("desktop_launch_start_failed"))
        );
        std::fs::remove_dir_all(directory).unwrap();
    }
}

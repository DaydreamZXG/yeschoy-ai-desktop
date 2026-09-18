//! Explicit launch-only terminal opening. Inputs are native discovery paths;
//! renderer commands, model requests and credentials are never accepted here.
use std::{
    path::Path,
    process::Command,
    time::{Duration, Instant},
};

use base64::{engine::general_purpose::STANDARD, Engine};

use super::{common, AdapterFailure, ResolvedInstallation};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Platform {
    #[cfg_attr(target_os = "windows", allow(dead_code))]
    MacOs,
    Windows,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct LaunchPlan {
    program: String,
    args: Vec<String>,
    new_console: bool,
    working_directory: String,
    mac_command: Option<String>,
}

fn failure(reason: &'static str) -> AdapterFailure {
    AdapterFailure::LaunchError(reason)
}

fn quoted_shell(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn powershell_literal(value: &str) -> String {
    // PowerShell treats Unicode smart quotes as delimiters too. Keep every
    // native path out of the parser, including quote and metacharacter bytes.
    format!(
        "([Text.Encoding]::UTF8.GetString([Convert]::FromBase64String('{}')))",
        STANDARD.encode(value.as_bytes())
    )
}

fn windows_absolute(value: &str) -> bool {
    let bytes = value.as_bytes();
    (bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && matches!(bytes[2], b'\\' | b'/'))
        || value.strip_prefix("\\\\").is_some_and(|tail| {
            let parts: Vec<_> = tail.split('\\').collect();
            parts.len() >= 2
                && parts[0] != "."
                && parts[0] != "?"
                && !parts[0].is_empty()
                && !parts[1].is_empty()
        })
}

fn path_text(path: &Path, platform: Platform) -> Result<String, AdapterFailure> {
    let raw = path
        .to_str()
        .ok_or_else(|| failure("invalid_launch_target"))?;
    // Canonical Windows discovery may return the extended-length prefix.
    let value = if platform == Platform::Windows {
        if let Some(tail) = raw.strip_prefix("\\\\?\\UNC\\") {
            format!("\\\\{tail}")
        } else {
            raw.strip_prefix("\\\\?\\").unwrap_or(raw).to_owned()
        }
    } else {
        raw.to_owned()
    };
    let absolute = match platform {
        Platform::MacOs => value.starts_with('/'),
        Platform::Windows => windows_absolute(&value),
    };
    if !absolute
        || value.chars().any(char::is_control)
        || (platform == Platform::Windows && value.contains('"'))
    {
        return Err(failure("invalid_launch_target"));
    }
    Ok(value)
}

fn mac_shell_command(executable: &str, home: &str, runtime_directories: &[String]) -> String {
    let prefix = runtime_directories
        .iter()
        .map(|directory| quoted_shell(directory))
        .collect::<Vec<_>>()
        .join(":");
    let path = if prefix.is_empty() {
        "\"${PATH:-/usr/bin:/bin:/usr/sbin:/sbin}\"".to_owned()
    } else {
        format!("{prefix}:\"${{PATH:-/usr/bin:/bin:/usr/sbin:/sbin}}\"")
    };
    format!(
        "cd -- {} || exit 1\nunset ANTHROPIC_BASE_URL ANTHROPIC_AUTH_TOKEN ANTHROPIC_API_KEY\nPATH={path}\nexport PATH\nexec {}",
        quoted_shell(home),
        quoted_shell(executable)
    )
}

fn build_plan(
    platform: Platform,
    installation: &ResolvedInstallation,
    tool_id: &str,
    home: &Path,
    windows_root: Option<&Path>,
) -> Result<LaunchPlan, AdapterFailure> {
    if !matches!(tool_id, "claude_code" | "pi") {
        return Err(failure("invalid_launch_target"));
    }
    let executable = path_text(&installation.path, platform)?;
    let home = path_text(home, platform)?;
    match platform {
        Platform::MacOs => {
            let runtime_directories = common::cli_runtime_directories(&installation.path)
                .iter()
                .map(|path| path_text(path, platform))
                .collect::<Result<Vec<_>, _>>()?;
            Ok(LaunchPlan {
                // A normal LaunchServices document-open, not AppleScript
                // automation. Hardened candidates need no AppleEvents grant.
                program: "/usr/bin/open".into(),
                args: vec!["-b".into(), "com.apple.Terminal".into()],
                new_console: false,
                mac_command: Some(mac_shell_command(&executable, &home, &runtime_directories)),
                working_directory: home,
            })
        }
        Platform::Windows => {
            let root = path_text(
                windows_root.ok_or_else(|| failure("terminal_unavailable"))?,
                platform,
            )?;
            let root = root.trim_end_matches(['\\', '/']);
            let suffix = executable
                .rsplit('.')
                .next()
                .unwrap_or("")
                .to_ascii_lowercase();
            let invocation = match suffix.as_str() {
                "exe" => format!("& {}", powershell_literal(&executable)),
                "cmd" | "bat" => {
                    // Expand the target through a child-only environment value
                    // once. Literal %...% inside a path is not recursively
                    // expanded; /V:OFF also keeps ! literal. ProcessStartInfo
                    // avoids PowerShell's native argument re-quoting of /C.
                    format!("$p = New-Object System.Diagnostics.ProcessStartInfo; $p.FileName = {}; $p.Arguments = '/D /V:OFF /S /C \"\"%YESCHOY_CLI_TARGET%\"\"'; $p.UseShellExecute = $false; $p.WorkingDirectory = {}; $p.EnvironmentVariables['YESCHOY_CLI_TARGET'] = {}; $c = [System.Diagnostics.Process]::Start($p); $c.WaitForExit()", powershell_literal(&format!("{root}\\System32\\cmd.exe")), powershell_literal(&home), powershell_literal(&executable))
                }
                _ => return Err(failure("invalid_launch_target")),
            };
            let script = format!(
                "$ErrorActionPreference = 'Stop'; Remove-Item Env:ANTHROPIC_BASE_URL,Env:ANTHROPIC_AUTH_TOKEN,Env:ANTHROPIC_API_KEY -ErrorAction SilentlyContinue; Set-Location -LiteralPath {}; {invocation}",
                powershell_literal(&home)
            );
            let utf16 = script
                .encode_utf16()
                .flat_map(u16::to_le_bytes)
                .collect::<Vec<_>>();
            Ok(LaunchPlan {
                program: format!("{root}\\System32\\WindowsPowerShell\\v1.0\\powershell.exe"),
                args: vec![
                    "-NoLogo".into(),
                    "-NoProfile".into(),
                    "-NoExit".into(),
                    "-EncodedCommand".into(),
                    STANDARD.encode(utf16),
                ],
                new_console: true,
                working_directory: home,
                mac_command: None,
            })
        }
    }
}

#[cfg(unix)]
struct TemporaryCommandFile {
    directory: std::path::PathBuf,
    path: std::path::PathBuf,
    handed_off: bool,
}

#[cfg(unix)]
impl TemporaryCommandFile {
    fn new(command: &str) -> Result<Self, AdapterFailure> {
        use std::{
            io::Write,
            os::unix::fs::{DirBuilderExt, OpenOptionsExt},
        };

        let base = std::env::temp_dir()
            .canonicalize()
            .map_err(|_| failure("terminal_launch_failed"))?;
        for _ in 0..16 {
            let mut random = [0u8; 16];
            getrandom::fill(&mut random).map_err(|_| failure("terminal_launch_failed"))?;
            let name = random
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>();
            let directory = base.join(format!("yeschoy-terminal-{name}"));
            match std::fs::DirBuilder::new().mode(0o700).create(&directory) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(_) => return Err(failure("terminal_launch_failed")),
            }
            let staged = Self {
                path: directory.join("launch.command"),
                directory,
                handed_off: false,
            };
            let path = path_text(&staged.path, Platform::MacOs)?;
            let directory = path_text(&staged.directory, Platform::MacOs)?;
            // The open request is asynchronous. Unlink only after Terminal has
            // actually started reading this script, before replacing it by the
            // CLI. Each cleanup target is an exact file/directory we created.
            let script = format!(
                "#!/bin/sh\n/bin/rm -f -- {} || exit 1\n/bin/rmdir -- {} || exit 1\n{command}\n",
                quoted_shell(&path),
                quoted_shell(&directory)
            );
            let mut file = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o700)
                .open(&staged.path)
                .map_err(|_| failure("terminal_launch_failed"))?;
            file.write_all(script.as_bytes())
                .and_then(|()| file.sync_all())
                .map_err(|_| failure("terminal_launch_failed"))?;
            return Ok(staged);
        }
        Err(failure("terminal_launch_failed"))
    }
}

#[cfg(unix)]
impl Drop for TemporaryCommandFile {
    fn drop(&mut self) {
        if !self.handed_off {
            let _ = std::fs::remove_file(&self.path);
            // Never recursively remove a directory, even this private one.
            let _ = std::fs::remove_dir(&self.directory);
        }
    }
}

fn launch_with(
    plan: &LaunchPlan,
    executor: impl FnOnce(&LaunchPlan) -> Result<(), AdapterFailure>,
) -> Result<(), AdapterFailure> {
    if let Some(command) = &plan.mac_command {
        #[cfg(unix)]
        {
            let mut staged = TemporaryCommandFile::new(command)?;
            let mut resolved = plan.clone();
            resolved
                .args
                .push(path_text(&staged.path, Platform::MacOs)?);
            resolved.mac_command = None;
            let result = executor(&resolved);
            staged.handed_off = result.is_ok();
            return result;
        }
        #[cfg(not(unix))]
        {
            let _ = command;
            return Err(AdapterFailure::UnsupportedProfile);
        }
    }
    executor(plan)
}

fn execute(plan: &LaunchPlan) -> Result<(), AdapterFailure> {
    let mut command = Command::new(&plan.program);
    command
        .args(&plan.args)
        .current_dir(&plan.working_directory);
    if !plan.new_console {
        command
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());
    }
    #[cfg(target_os = "windows")]
    if plan.new_console {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0000_0010); // CREATE_NEW_CONSOLE, no elevation.
    }
    let mut child = command.spawn().map_err(|error| {
        failure(if error.kind() == std::io::ErrorKind::NotFound {
            "terminal_unavailable"
        } else {
            "terminal_launch_failed"
        })
    })?;
    if plan.new_console {
        return Ok(());
    } // Windows console intentionally stays open.
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                return if status.success() {
                    Ok(())
                } else {
                    Err(failure("terminal_launch_failed"))
                }
            }
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(10)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(failure("terminal_launch_failed"));
            }
        }
    }
}

/// Run bounded native terminal handoff away from the async command executor.
pub(crate) async fn launch_async(
    installation: &ResolvedInstallation,
    tool_id: &str,
    home: &Path,
) -> Result<(), AdapterFailure> {
    let installation = installation.clone();
    let tool_id = tool_id.to_owned();
    let home = home.to_owned();
    tauri::async_runtime::spawn_blocking(move || launch(&installation, &tool_id, &home))
        .await
        .map_err(|_| failure("terminal_launch_failed"))?
}

pub(crate) fn launch(
    installation: &ResolvedInstallation,
    tool_id: &str,
    home: &Path,
) -> Result<(), AdapterFailure> {
    #[cfg(target_os = "macos")]
    let platform = Platform::MacOs;
    #[cfg(target_os = "windows")]
    let platform = Platform::Windows;
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        let _ = (installation, tool_id, home);
        return Err(AdapterFailure::UnsupportedProfile);
    }

    #[cfg(any(target_os = "macos", target_os = "windows"))]
    {
        let system_root = std::env::var_os("SystemRoot").map(std::path::PathBuf::from);
        let plan = build_plan(
            platform,
            installation,
            tool_id,
            home,
            system_root.as_deref(),
        )?;
        if !installation.path.is_file() || !home.is_dir() {
            return Err(failure("launch_target_missing"));
        }
        launch_with(&plan, execute)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn terminal_async_handoff_preserves_validation_failure_without_launching() {
        let result = launch_async(
            &ResolvedInstallation {
                path: "/synthetic/nonexistent/pi".into(),
            },
            "not-a-supported-tool",
            Path::new("/synthetic"),
        )
        .await;
        assert!(result.is_err());
    }

    #[test]
    fn activation_and_open_button_share_terminal_handoff() {
        let activation = include_str!("../tool_activation.rs");
        let opener = activation
            .split("async fn open_configured_adapter(")
            .nth(1)
            .unwrap()
            .split("struct DesktopReloadGuard")
            .next()
            .unwrap();
        assert!(opener.contains("terminal_launch::launch_async("));
        assert!(!opener.contains("\"claude_code\" | \"pi\" => Ok(())"));
        assert!(include_str!("../open_connection.rs").contains("terminal_launch::launch_async("));
    }

    #[test]
    fn terminal_plans_are_closed_and_propagate_injected_launcher_failure() {
        let installation = ResolvedInstallation {
            path: "/native/含 空格 O'Brian;$()&`/pi".into(),
        };
        let plan = build_plan(
            Platform::MacOs,
            &installation,
            "pi",
            Path::new("/native/我的项目"),
            None,
        )
        .unwrap();
        assert_eq!(plan.program, "/usr/bin/open");
        assert_eq!(plan.args, ["-b", "com.apple.Terminal"]);
        assert_eq!(
            plan.mac_command.as_deref(),
            Some(
                mac_shell_command(installation.path.to_str().unwrap(), "/native/我的项目", &[],)
                    .as_str()
            )
        );
        // Inject a non-Mac plan so this pure dispatch test also runs on Windows.
        let dispatch = build_plan(
            Platform::Windows,
            &ResolvedInstallation {
                path: r"C:\native\pi.exe".into(),
            },
            "pi",
            Path::new(r"C:\native"),
            Some(Path::new(r"C:\Windows")),
        )
        .unwrap();
        let mut called = false;
        assert!(matches!(
            launch_with(&dispatch, |received| {
                called = true;
                assert_eq!(received, &dispatch);
                Err(failure("terminal_launch_failed"))
            }),
            Err(AdapterFailure::LaunchError("terminal_launch_failed"))
        ));
        assert!(called);
        for tool in ["dsh_web", "codex_desktop", "pi;id", "../../pi"] {
            assert!(build_plan(
                Platform::MacOs,
                &installation,
                tool,
                Path::new("/native"),
                None
            )
            .is_err());
        }
        for path in ["relative/pi", "/native/pi\ncommand"] {
            assert!(build_plan(
                Platform::MacOs,
                &ResolvedInstallation { path: path.into() },
                "pi",
                Path::new("/native"),
                None
            )
            .is_err());
        }
    }

    #[test]
    fn windows_terminal_plan_preserves_literal_unicode_and_shell_metacharacters() {
        let target = r"C:\Users\用户 O'Brian ‘smart’ & %NAME% ! $() `;\pi.cmd";
        let plan = build_plan(
            Platform::Windows,
            &ResolvedInstallation {
                path: target.into(),
            },
            "pi",
            Path::new(r"C:\Users\用户 O'Brian"),
            Some(Path::new(r"C:\Windows")),
        )
        .unwrap();
        assert!(plan.new_console);
        assert!(plan.mac_command.is_none());
        assert_eq!(
            plan.program,
            r"C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe"
        );
        assert_eq!(
            &plan.args[..4],
            &["-NoLogo", "-NoProfile", "-NoExit", "-EncodedCommand"]
        );
        let bytes = STANDARD.decode(&plan.args[4]).unwrap();
        let text = String::from_utf16(
            &bytes
                .chunks_exact(2)
                .map(|b| u16::from_le_bytes([b[0], b[1]]))
                .collect::<Vec<_>>(),
        )
        .unwrap();
        assert!(text.contains(&powershell_literal(target)));
        assert!(!text.contains(target));
        assert!(text.contains("/D /V:OFF /S /C \"\"%YESCHOY_CLI_TARGET%\"\""));
        assert!(text.contains(
            "Remove-Item Env:ANTHROPIC_BASE_URL,Env:ANTHROPIC_AUTH_TOKEN,Env:ANTHROPIC_API_KEY"
        ));
        assert!(text.contains(&format!(
            "Set-Location -LiteralPath {}",
            powershell_literal(r"C:\Users\用户 O'Brian")
        )));
        assert!(text.contains(&format!(
            "$p.WorkingDirectory = {}",
            powershell_literal(r"C:\Users\用户 O'Brian")
        )));
        for forbidden in [
            "ExecutionPolicy",
            "RunAs",
            "credential-helper",
            "https://",
            "apiKey",
        ] {
            assert!(!text.contains(forbidden));
        }
        for invalid in [r"C:pi.exe", r"\\.\device\pi.exe", r"C:\Users\pi.ps1"] {
            assert!(build_plan(
                Platform::Windows,
                &ResolvedInstallation {
                    path: invalid.into()
                },
                "pi",
                Path::new(r"C:\Users\example"),
                Some(Path::new(r"C:\Windows"))
            )
            .is_err());
        }
    }

    #[cfg(unix)]
    #[test]
    fn mac_shell_command_executes_only_the_literal_fixture_path_in_home() {
        use std::os::unix::fs::PermissionsExt;
        let directory =
            super::super::common::temporary_working_directory("terminal-escaping").unwrap();
        let home = directory.join("项目 O'Brian;$()&`");
        std::fs::create_dir(&home).unwrap();
        let executable = home.join("fake cli ';&$()`");
        std::fs::write(&executable, b"#!/bin/sh\nprintf 'fixture-only\\n'\npwd\n").unwrap();
        std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700)).unwrap();
        let plan = build_plan(
            Platform::MacOs,
            &ResolvedInstallation { path: executable },
            "pi",
            &home,
            None,
        )
        .unwrap();
        let mut command_file = None;
        launch_with(&plan, |received| {
            assert_eq!(received.program, "/usr/bin/open");
            assert_eq!(&received.args[..2], &["-b", "com.apple.Terminal"]);
            assert_eq!(received.args.len(), 3);
            command_file = Some(std::path::PathBuf::from(&received.args[2]));
            // Simulate accepted LaunchServices handoff without opening Terminal.
            Ok(())
        })
        .unwrap();
        let command_file = command_file.unwrap();
        let command_directory = command_file.parent().unwrap().to_owned();
        assert!(
            command_file.is_file(),
            "must not delete before Terminal reads it"
        );
        assert_eq!(
            std::fs::metadata(&command_file)
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o700
        );
        assert_eq!(
            std::fs::metadata(&command_directory)
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o700
        );
        let script = std::fs::read_to_string(&command_file).unwrap();
        assert!(script.contains("export PATH"));
        assert!(script.contains("unset ANTHROPIC_BASE_URL ANTHROPIC_AUTH_TOKEN ANTHROPIC_API_KEY"));
        assert!(script.contains(&quoted_shell(home.to_str().unwrap())));
        for forbidden in ["osascript", "credential-helper", "https://", "apiKey"] {
            assert!(!script.contains(forbidden));
        }
        let result = Command::new("/bin/sh").arg(&command_file).output().unwrap();
        assert!(result.status.success());
        let output = String::from_utf8(result.stdout).unwrap();
        assert!(output.starts_with("fixture-only\n"));
        // macOS /var may resolve to /private/var; compare filesystem identities.
        let printed_home = Path::new(output.lines().nth(1).unwrap())
            .canonicalize()
            .unwrap();
        assert_eq!(printed_home, home.canonicalize().unwrap());
        assert!(
            !command_file.exists(),
            "script must unlink itself before exec"
        );
        assert!(
            !command_directory.exists(),
            "script must remove its own empty directory"
        );
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn mac_command_launch_failure_cleans_private_files_after_injected_process_exit() {
        let launcher = super::super::common::test_support::Script::new("exit 17");
        let plan = build_plan(
            Platform::MacOs,
            &launcher.installation(),
            "pi",
            launcher.path.parent().unwrap(),
            None,
        )
        .unwrap();
        let mut staged = None;
        let result = launch_with(&plan, |received| {
            let path = std::path::PathBuf::from(&received.args[2]);
            assert!(path.is_file());
            staged = Some(path);
            let mut fixture = received.clone();
            fixture.program = launcher.path.to_str().unwrap().into();
            execute(&fixture)
        });
        assert!(matches!(
            result,
            Err(AdapterFailure::LaunchError("terminal_launch_failed"))
        ));
        let staged = staged.unwrap();
        assert!(!staged.exists());
        assert!(!staged.parent().unwrap().exists());
    }
}

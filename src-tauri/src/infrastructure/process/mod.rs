//! Bounded process execution for native-discovered CLIs.
//!
//! Every child is spawned with a closed set of pipes, a deadline and an output
//! cap, because these commands are user-installed wrappers we do not control.
//! Kept in `infrastructure` rather than in `tool_adapters` because discovery,
//! installation and backup also run CLIs; nothing here knows about a tool's
//! configuration format.
use std::{
    collections::HashSet,
    env,
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
    process::Stdio,
    time::Duration,
};

use tokio::{
    io::{AsyncRead, AsyncReadExt},
    process::Command,
    time::timeout,
};

const MAX_PROCESS_OUTPUT_BYTES: u64 = 256 * 1024;
const MAX_RUNTIME_SYMLINK_HOPS: usize = 8;
const MAX_RUNTIME_PATH_DIRECTORIES: usize = 16;

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
mod process_tests {
    use super::*;
    use crate::tool_adapters::common::temporary_working_directory;
    use crate::tool_adapters::common::test_support::Script;

    #[cfg(unix)]
    #[tokio::test]
    async fn bounded_process_distinguishes_start_exit_timeout_and_output_limit() {
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
        let script = Script::new("/bin/sleep 0.4 &\nexit 0");
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
        let script = Script::new("printf '%s' $$ > \"$YESCHOY_TEST_PID_FILE\"\nexec /bin/sleep 10");
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
}

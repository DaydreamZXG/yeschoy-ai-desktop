use crate::tool_adapters::common;
use crate::tool_discovery_core::{
    classify, normalize_version_output, DiscoveryResult, LocationHint, ProbeObservation, ToolSpec,
    TOOL_SPECS,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::env;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::process::Command;
use tokio::time::timeout;

// Large packaged JavaScript CLIs such as Claude Code can take several seconds
// on a cold start (and development StrictMode can trigger two scans together).
// Eight seconds is still bounded, but avoids telling beginners an installed
// application is missing just because its version process warmed up slowly.
const PROBE_TIMEOUT: Duration = Duration::from_secs(8);
const MAX_CANDIDATES: usize = 32;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ScanRequest {
    request_id: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanResponse {
    request_id: String,
    platform: &'static str,
    started_at_epoch_ms: u64,
    completed_at_epoch_ms: u64,
    tools: Vec<ToolProjection>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ToolProjection {
    tool_id: &'static str,
    display_name: &'static str,
    status: &'static str,
    version: String,
    candidate_count: usize,
    location_hint: &'static str,
    compatibility: &'static str,
    reason_code: &'static str,
}

impl From<DiscoveryResult> for ToolProjection {
    fn from(result: DiscoveryResult) -> Self {
        Self {
            tool_id: result.tool_id,
            display_name: result.display_name,
            status: result.status.as_str(),
            version: result.version,
            candidate_count: result.candidate_count,
            location_hint: result.location_hint.as_str(),
            compatibility: result.compatibility.as_str(),
            reason_code: result.reason_code.as_str(),
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct Candidate {
    pub(crate) path: PathBuf,
    pub(crate) location_hint: LocationHint,
}

/// Implements the frozen `tool-discovery-scan@v1` query.
#[tauri::command]
pub async fn scan_tools_read_only(request: ScanRequest) -> Result<ScanResponse, String> {
    validate_request_id(&request.request_id)?;
    let started_at_epoch_ms = unix_epoch_ms();

    let (claude, codex, opencode, pi, dsh) = tokio::join!(
        scan_tool(TOOL_SPECS[0]),
        scan_tool(TOOL_SPECS[1]),
        scan_tool(TOOL_SPECS[2]),
        scan_tool(TOOL_SPECS[3]),
        scan_tool(TOOL_SPECS[4]),
    );

    Ok(ScanResponse {
        request_id: request.request_id,
        platform: platform_name(),
        started_at_epoch_ms,
        completed_at_epoch_ms: unix_epoch_ms().max(started_at_epoch_ms),
        tools: vec![claude, codex, opencode, pi, dsh]
            .into_iter()
            .map(ToolProjection::from)
            .collect(),
    })
}

async fn scan_tool(spec: ToolSpec) -> DiscoveryResult {
    let candidates = discover_candidates(spec.executable_name);
    let observation = match candidates.as_slice() {
        [] => ProbeObservation::NotFound,
        [candidate] => probe_version(candidate).await,
        many => ProbeObservation::MultipleInstallations {
            candidate_count: many.len(),
        },
    };
    classify(spec, observation)
}

pub(crate) fn discover_candidates(executable_name: &str) -> Vec<Candidate> {
    let mut canonical_candidates: HashMap<PathBuf, Candidate> = HashMap::new();

    // The login shell's PATH first, then this process's. An app opened from the
    // Dock inherits launchd's short list, which is why a CLI installed under
    // nvm, bun, volta or fnm used to be reported as not installed even though
    // it runs fine in the user's terminal.
    let search_directories = crate::shell_environment::search_directories();
    for directory in &search_directories {
        add_candidates(
            &mut canonical_candidates,
            directory,
            executable_name,
            LocationHint::Path,
        );
        if canonical_candidates.len() >= MAX_CANDIDATES {
            break;
        }
    }

    for directory in common_binary_directories() {
        if canonical_candidates.len() >= MAX_CANDIDATES {
            break;
        }
        add_candidates(
            &mut canonical_candidates,
            &directory,
            executable_name,
            LocationHint::CommonLocation,
        );
    }

    let mut candidates: Vec<Candidate> = canonical_candidates.into_values().collect();
    let path_priority: Vec<PathBuf> = search_directories
        .into_iter()
        .flat_map(|dir| {
            executable_filenames(executable_name)
                .into_iter()
                .map(move |name| dir.join(name))
        })
        .filter_map(|path| std::fs::canonicalize(path).ok())
        .collect();
    candidates.sort_by_key(|candidate| {
        let canonical =
            std::fs::canonicalize(&candidate.path).unwrap_or_else(|_| candidate.path.clone());
        (
            path_priority
                .iter()
                .position(|path| path == &canonical)
                .unwrap_or(usize::MAX),
            candidate.path.clone(),
        )
    });
    candidates.truncate(MAX_CANDIDATES);
    candidates
}

fn add_candidates(
    candidates: &mut HashMap<PathBuf, Candidate>,
    directory: &Path,
    executable_name: &str,
    location_hint: LocationHint,
) {
    for filename in executable_filenames(executable_name) {
        let path = directory.join(filename);
        if !is_executable_candidate(&path) {
            continue;
        }
        let canonical = std::fs::canonicalize(&path).unwrap_or_else(|_| path.clone());
        candidates.entry(canonical).or_insert(Candidate {
            // Keep the executable wrapper selected by native discovery. npm
            // symlinks often resolve to JavaScript source that cannot be
            // launched directly from a restricted desktop PATH.
            path,
            location_hint,
        });
        if candidates.len() >= MAX_CANDIDATES {
            return;
        }
    }
}

#[cfg(target_os = "windows")]
pub(crate) fn executable_filenames(executable_name: &str) -> Vec<String> {
    ["exe", "cmd", "bat"]
        .into_iter()
        .map(|extension| format!("{executable_name}.{extension}"))
        .collect()
}

#[cfg(not(target_os = "windows"))]
pub(crate) fn executable_filenames(executable_name: &str) -> Vec<String> {
    vec![executable_name.to_string()]
}

#[cfg(unix)]
pub(crate) fn is_executable_candidate(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    path.metadata()
        .map(|metadata| metadata.is_file() && metadata.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

#[cfg(not(unix))]
pub(crate) fn is_executable_candidate(path: &Path) -> bool {
    path.is_file()
}

pub(crate) async fn probe_version(candidate: &Candidate) -> ProbeObservation {
    let Some(mut command) = version_command(&candidate.path) else {
        return ProbeObservation::Failed {
            location_hint: candidate.location_hint,
        };
    };
    common::apply_cli_runtime_path(&mut command, &candidate.path);
    command
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);

    match timeout(PROBE_TIMEOUT, command.output()).await {
        Err(_) => ProbeObservation::TimedOut {
            location_hint: candidate.location_hint,
        },
        Ok(Err(_)) => ProbeObservation::Failed {
            location_hint: candidate.location_hint,
        },
        Ok(Ok(output)) if output.status.success() => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);
            let raw = if stdout.trim().is_empty() {
                stderr.as_ref()
            } else {
                stdout.as_ref()
            };
            match normalize_version_output(raw) {
                Some(version) => ProbeObservation::Found {
                    version,
                    location_hint: candidate.location_hint,
                },
                None => ProbeObservation::Failed {
                    location_hint: candidate.location_hint,
                },
            }
        }
        Ok(Ok(_)) => ProbeObservation::Failed {
            location_hint: candidate.location_hint,
        },
    }
}

#[cfg(not(target_os = "windows"))]
fn version_command(path: &Path) -> Option<Command> {
    let mut command = Command::new(path);
    command.arg("--version");
    Some(command)
}

#[cfg(target_os = "windows")]
fn version_command(path: &Path) -> Option<Command> {
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    if extension == "exe" {
        let mut command = Command::new(path);
        command.arg("--version");
        return Some(command);
    }

    let rendered = path.to_str()?;
    if rendered.chars().any(|character| {
        matches!(
            character,
            '"' | '&' | '|' | '<' | '>' | '^' | '%' | '\r' | '\n'
        )
    }) {
        return None;
    }
    let mut command = Command::new("cmd.exe");
    command.args(["/D", "/S", "/C"]);
    command.arg(format!("\"\"{rendered}\" --version\""));
    Some(command)
}

/// How many version directories a single manager may contribute. Someone who
/// has collected thirty Node releases should not turn a scan into a directory
/// crawl; the newest names come first, and `PATH` still wins the ordering.
const MAX_VERSIONS_PER_MANAGER: usize = 12;

/// Expand one `<root>/<version>/<suffix>` layout.
///
/// Version managers install each runtime under its own directory, so they
/// cannot be listed as fixed paths the way `/usr/local/bin` can. Names are
/// sorted descending so newer releases are examined first; this is a string
/// sort, which orders `v8` above `v20`, but every match is a real executable
/// either way and `PATH` priority decides what is actually recommended.
fn versioned_directories(root: &Path, suffix: &str) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(root) else {
        return Vec::new();
    };
    let mut names: Vec<PathBuf> = entries
        .flatten()
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
        .map(|entry| entry.path())
        .collect();
    names.sort_unstable();
    names.reverse();
    names.truncate(MAX_VERSIONS_PER_MANAGER);
    names
        .into_iter()
        .map(|path| if suffix.is_empty() { path } else { path.join(suffix) })
        .collect()
}

/// Where these CLIs land when they are not on the inherited `PATH`.
///
/// This exists because a desktop process's `PATH` is not the user's. Even with
/// the login shell resolved it stays worth checking: a user who installed a
/// tool without restarting their terminal has it on disk and nowhere on any
/// `PATH` yet, and telling them it is not installed is the worst answer.
pub(crate) fn common_binary_directories() -> Vec<PathBuf> {
    let mut directories = Vec::new();

    #[cfg(target_os = "windows")]
    {
        if let Some(app_data) = env::var_os("APPDATA") {
            let app_data = PathBuf::from(app_data);
            directories.push(app_data.join("npm"));
            // fnm keeps the executables directly in the installation directory.
            directories.extend(versioned_directories(
                &app_data.join("fnm").join("node-versions"),
                "installation",
            ));
        }
        if let Some(local_app_data) = env::var_os("LOCALAPPDATA") {
            let local_app_data = PathBuf::from(local_app_data);
            directories.push(local_app_data.join("pnpm"));
            directories.push(local_app_data.join("Volta").join("bin"));
            directories.push(local_app_data.join("Yarn").join("bin"));
        }
        if let Some(profile) = crate::tool_adapters::user_home() {
            directories.push(profile.join(".bun").join("bin"));
            directories.push(profile.join("scoop").join("shims"));
        }
    }

    #[cfg(not(target_os = "windows"))]
    {
        if let Some(home) = env::var_os("HOME") {
            let home = PathBuf::from(home);
            directories.push(home.join(".local/bin"));
            directories.push(home.join(".npm-global/bin"));
            directories.push(home.join(".local/share/pnpm"));
            #[cfg(target_os = "macos")]
            directories.push(home.join("Library/pnpm"));
            // Version managers, in rough order of how often they show up on a
            // machine that has Node at all.
            directories.extend(versioned_directories(
                &home.join(".nvm").join("versions").join("node"),
                "bin",
            ));
            directories.push(home.join(".volta/bin"));
            directories.push(home.join(".bun/bin"));
            directories.push(home.join(".asdf/shims"));
            directories.push(home.join(".yarn/bin"));
            directories.push(home.join(".config/yarn/global/node_modules/.bin"));
            #[cfg(target_os = "macos")]
            directories.extend(versioned_directories(
                &home.join("Library/Application Support/fnm/node-versions"),
                "installation/bin",
            ));
            #[cfg(not(target_os = "macos"))]
            directories.extend(versioned_directories(
                &home.join(".local/share/fnm/node-versions"),
                "installation/bin",
            ));
        }
        directories.push(PathBuf::from("/usr/local/bin"));
        #[cfg(target_os = "macos")]
        directories.push(PathBuf::from("/opt/homebrew/bin"));
    }

    directories
}

pub(crate) fn validate_request_id(request_id: &str) -> Result<(), String> {
    if request_id.is_empty()
        || request_id.len() > 64
        || !request_id
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '_' | '-'))
    {
        return Err("invalid_request_id".to_string());
    }
    Ok(())
}

pub(crate) fn unix_epoch_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX)
}

pub(crate) const fn platform_name() -> &'static str {
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

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn canonical_deduplication_keeps_the_first_native_wrapper() {
        use std::os::unix::fs::{symlink, PermissionsExt};

        let root =
            crate::tool_adapters::common::temporary_working_directory("discovery-wrapper").unwrap();
        let first = root.join("first");
        let second = root.join("second");
        let package = root.join("package");
        std::fs::create_dir_all(&first).unwrap();
        std::fs::create_dir_all(&second).unwrap();
        std::fs::create_dir_all(&package).unwrap();
        let target = package.join("dsh.js");
        std::fs::write(&target, b"#!/usr/bin/env node\n").unwrap();
        std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o700)).unwrap();
        symlink(&target, first.join("dsh")).unwrap();
        symlink(&target, second.join("dsh")).unwrap();
        let mut candidates = HashMap::new();
        add_candidates(&mut candidates, &first, "dsh", LocationHint::Path);
        add_candidates(
            &mut candidates,
            &second,
            "dsh",
            LocationHint::CommonLocation,
        );
        assert_eq!(candidates.len(), 1);
        let retained = candidates.into_values().next().unwrap();
        assert_eq!(retained.path, first.join("dsh"));
        assert_eq!(retained.location_hint, LocationHint::Path);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn version_manager_layouts_are_expanded_newest_first_and_bounded() {
        // nvm and fnm install every runtime under its own directory, so they
        // cannot be listed as fixed paths. A `claude` installed this way used to
        // be invisible: it is not in a Dock-launched app's PATH and it was not
        // in the fallback list either, so the app said "not installed" about a
        // CLI that runs fine in the user's terminal.
        let root =
            crate::tool_adapters::common::temporary_working_directory("nvm-layout").unwrap();
        let versions = root.join("versions").join("node");
        for name in ["v18.20.4", "v20.11.1", "v22.14.0"] {
            std::fs::create_dir_all(versions.join(name).join("bin")).unwrap();
        }
        // A stray file beside the version directories must not become a path.
        std::fs::write(versions.join("alias"), b"default").unwrap();

        let expanded = versioned_directories(&versions, "bin");
        assert_eq!(
            expanded,
            vec![
                versions.join("v22.14.0").join("bin"),
                versions.join("v20.11.1").join("bin"),
                versions.join("v18.20.4").join("bin"),
            ]
        );

        // Someone who has collected many runtimes does not turn a scan into a
        // directory crawl.
        let many = root.join("many");
        for index in 0..MAX_VERSIONS_PER_MANAGER + 5 {
            std::fs::create_dir_all(many.join(format!("v{index:02}"))).unwrap();
        }
        assert_eq!(
            versioned_directories(&many, "").len(),
            MAX_VERSIONS_PER_MANAGER
        );

        // A directory that is not there at all is not an error.
        assert!(versioned_directories(&root.join("absent"), "bin").is_empty());
        std::fs::remove_dir_all(root).unwrap();
    }
}

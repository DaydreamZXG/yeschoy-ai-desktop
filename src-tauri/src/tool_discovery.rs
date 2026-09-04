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

    let (claude, codex, opencode, pi, dsh, hermes, openclaw) = tokio::join!(
        scan_tool(TOOL_SPECS[0]),
        scan_tool(TOOL_SPECS[1]),
        scan_tool(TOOL_SPECS[2]),
        scan_tool(TOOL_SPECS[3]),
        scan_tool(TOOL_SPECS[4]),
        scan_tool(TOOL_SPECS[5]),
        scan_tool(TOOL_SPECS[6]),
    );

    Ok(ScanResponse {
        request_id: request.request_id,
        platform: platform_name(),
        started_at_epoch_ms,
        completed_at_epoch_ms: unix_epoch_ms().max(started_at_epoch_ms),
        tools: vec![claude, codex, opencode, pi, dsh, hermes, openclaw]
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

    if let Some(path_value) = env::var_os("PATH") {
        for directory in env::split_paths(&path_value) {
            add_candidates(
                &mut canonical_candidates,
                &directory,
                executable_name,
                LocationHint::Path,
            );
            if canonical_candidates.len() >= MAX_CANDIDATES {
                break;
            }
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
    candidates.sort_by(|left, right| left.path.cmp(&right.path));
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
        let canonical = std::fs::canonicalize(&path).unwrap_or(path);
        candidates.entry(canonical.clone()).or_insert(Candidate {
            path: canonical,
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

pub(crate) fn common_binary_directories() -> Vec<PathBuf> {
    let mut directories = Vec::new();

    #[cfg(target_os = "windows")]
    {
        if let Some(app_data) = env::var_os("APPDATA") {
            directories.push(PathBuf::from(app_data).join("npm"));
        }
        if let Some(local_app_data) = env::var_os("LOCALAPPDATA") {
            directories.push(PathBuf::from(local_app_data).join("pnpm"));
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

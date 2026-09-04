pub(crate) mod claude_code;
pub(crate) mod claude_desktop;
pub(crate) mod codex_desktop;
pub(crate) mod common;
pub(crate) mod dsh_web;
pub(crate) mod hermes;
pub(crate) mod openclaw;
pub(crate) mod pi;

use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::{
    desktop_app_discovery,
    tool_discovery::{self, probe_version, Candidate},
    tool_discovery_core::ProbeObservation,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum AdapterFailure {
    ToolNotFound,
    MultipleInstallations,
    MissingRuntime,
    UnsupportedProfile,
    ExternalOverride,
    SecureStorageUnavailable,
    ConfigurationFailed(&'static str),
    LaunchFailed,
    VerificationFailed(&'static str),
}

#[derive(Clone, Debug)]
pub(crate) struct ResolvedInstallation {
    pub(crate) path: PathBuf,
    pub(crate) version: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct InstallationProjection {
    pub(crate) installation_id: String,
    pub(crate) label: String,
    pub(crate) version: String,
    pub(crate) supported: bool,
    pub(crate) recommended: bool,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TargetProjection {
    pub(crate) tool_id: &'static str,
    pub(crate) display_name: &'static str,
    pub(crate) surface: &'static str,
    pub(crate) status: &'static str,
    pub(crate) installations: Vec<InstallationProjection>,
}

#[derive(Clone, Debug)]
struct ObservedInstallation {
    path: PathBuf,
    version: String,
    location: &'static str,
}

const TARGETS: [(&str, &str, &str); 7] = [
    ("claude_code", "Claude Code", "命令行与编辑器工作区"),
    ("claude_desktop", "Claude Desktop", "Claude 桌面应用"),
    (
        "codex_desktop",
        "Codex Desktop",
        "ChatGPT 桌面应用中的 Codex",
    ),
    ("pi", "Pi", "Pi 编程助手"),
    ("dsh_web", "DSH web", "DeepSeek Harness 浏览器工作台"),
    ("hermes", "Hermes", "Hermes 桌面与命令行助手"),
    ("openclaw", "OpenClaw", "OpenClaw 智能助手"),
];

pub(crate) fn user_home() -> Option<PathBuf> {
    #[cfg(target_os = "windows")]
    {
        std::env::var_os("USERPROFILE")
            .map(PathBuf::from)
            .or_else(|| {
                let drive = std::env::var_os("HOMEDRIVE")?;
                let path = std::env::var_os("HOMEPATH")?;
                let mut home = PathBuf::from(drive);
                home.push(path);
                Some(home)
            })
    }
    #[cfg(not(target_os = "windows"))]
    {
        std::env::var_os("HOME").map(PathBuf::from)
    }
}

fn executable_for(tool_id: &str) -> Option<&'static str> {
    match tool_id {
        "claude_code" => Some("claude"),
        "pi" => Some("pi"),
        "dsh_web" => Some("dsh"),
        "hermes" => Some("hermes"),
        "openclaw" => Some("openclaw"),
        _ => None,
    }
}

fn desktop_id_for(tool_id: &str) -> Option<&'static str> {
    match tool_id {
        "claude_desktop" => Some("claude_desktop"),
        "codex_desktop" => Some("codex_desktop"),
        _ => None,
    }
}

fn opaque_installation_id(path: &Path) -> String {
    // FNV-1a is used only as an opaque, local selection handle. The canonical
    // path never crosses IPC and collisions are rejected during resolution.
    let mut hash = 0xcbf29ce484222325u64;
    for byte in path.to_string_lossy().as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("i{hash:016x}")
}

fn can_attempt(tool_id: &str, installation: &ObservedInstallation) -> bool {
    // Discovery already establishes product identity. A version is metadata,
    // not evidence that a configuration contract has changed. The transaction
    // parses/readbacks owned fields and the tool request proves usability.
    match tool_id {
        "codex_desktop" => codex_desktop::bundled_runtime(&installation.path).is_some(),
        "claude_desktop" => installation.path.exists(),
        "claude_code" | "pi" | "dsh_web" | "hermes" | "openclaw" => installation.path.is_file(),
        _ => false,
    }
}

async fn observe_cli(executable: &str) -> Vec<ObservedInstallation> {
    let candidates = tool_discovery::discover_candidates(executable);
    let mut observed = Vec::new();
    for candidate in candidates.into_iter().take(8) {
        match probe_version(&candidate).await {
            ProbeObservation::Found {
                version,
                location_hint,
            } => observed.push(ObservedInstallation {
                path: candidate.path,
                version,
                location: location_hint.as_str(),
            }),
            ProbeObservation::Failed { location_hint }
            | ProbeObservation::TimedOut { location_hint } => {
                observed.push(ObservedInstallation {
                    path: candidate.path,
                    version: String::new(),
                    location: location_hint.as_str(),
                });
            }
            ProbeObservation::NotFound | ProbeObservation::MultipleInstallations { .. } => {}
        }
    }
    observed
}

async fn observe(tool_id: &str) -> Vec<ObservedInstallation> {
    if let Some(executable) = executable_for(tool_id) {
        return observe_cli(executable).await;
    }
    let Some(app_id) = desktop_id_for(tool_id) else {
        return Vec::new();
    };
    desktop_app_discovery::activation_candidates(app_id)
        .into_iter()
        .map(|candidate| ObservedInstallation {
            path: candidate.path,
            version: candidate.version,
            location: candidate.location_hint,
        })
        .collect()
}

fn projections(tool_id: &str, observed: &[ObservedInstallation]) -> Vec<InstallationProjection> {
    let supported_count = observed
        .iter()
        .filter(|installation| can_attempt(tool_id, installation))
        .count();
    observed
        .iter()
        .enumerate()
        .map(|(index, installation)| InstallationProjection {
            installation_id: opaque_installation_id(&installation.path),
            label: format!(
                "安装 {} · {}",
                index + 1,
                location_label(installation.location)
            ),
            version: installation.version.clone(),
            supported: can_attempt(tool_id, installation),
            recommended: can_attempt(tool_id, installation) && supported_count == 1,
        })
        .collect()
}

fn location_label(value: &str) -> &'static str {
    match value {
        "path" => "系统 PATH",
        "common_location" => "常用目录",
        "applications" => "系统应用",
        "user_applications" => "用户应用",
        "local_app_data" => "用户应用目录",
        "program_files" => "程序目录",
        _ => "本机",
    }
}

pub(crate) async fn scan_targets() -> Vec<TargetProjection> {
    let mut result = Vec::with_capacity(TARGETS.len());
    for (tool_id, display_name, surface) in TARGETS {
        let observed = observe(tool_id).await;
        let supported_count = observed
            .iter()
            .filter(|item| can_attempt(tool_id, item))
            .count();
        let status = if observed.is_empty() {
            "not_found"
        } else if supported_count == 0 {
            "missing_runtime"
        } else {
            if observed.len() == 1 {
                "available"
            } else {
                "selection_required"
            }
        };
        result.push(TargetProjection {
            tool_id,
            display_name,
            surface,
            status,
            installations: projections(tool_id, &observed),
        });
    }
    result
}

pub(crate) async fn resolve_installation(
    tool_id: &str,
    installation_id: &str,
) -> Result<ResolvedInstallation, AdapterFailure> {
    let observed = observe(tool_id).await;
    if observed.is_empty() {
        return Err(AdapterFailure::ToolNotFound);
    }
    let selected = if installation_id.is_empty() {
        match observed.as_slice() {
            [only] => only,
            _ => return Err(AdapterFailure::MultipleInstallations),
        }
    } else {
        let matches = observed
            .iter()
            .filter(|candidate| opaque_installation_id(&candidate.path) == installation_id)
            .collect::<Vec<_>>();
        match matches.as_slice() {
            [only] => *only,
            [] => return Err(AdapterFailure::ToolNotFound),
            _ => return Err(AdapterFailure::MultipleInstallations),
        }
    };
    if !can_attempt(tool_id, selected) {
        return Err(AdapterFailure::MissingRuntime);
    }
    Ok(ResolvedInstallation {
        path: selected.path.clone(),
        version: selected.version.clone(),
    })
}

pub(crate) fn candidate_for_test(path: PathBuf) -> Candidate {
    Candidate {
        path,
        location_hint: crate::tool_discovery_core::LocationHint::CommonLocation,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opaque_ids_are_stable_and_do_not_contain_paths() {
        let path = Path::new("/Users/example/My Tools/claude");
        let first = opaque_installation_id(path);
        assert_eq!(first, opaque_installation_id(path));
        assert!(!first.contains("Users"));
        assert_eq!(first.len(), 17);
    }

    #[test]
    fn ordinary_future_and_unread_versions_do_not_gate_eligibility() {
        let directory = common::temporary_working_directory("version-metadata").unwrap();
        let path = directory.join("tool");
        std::fs::write(&path, b"fixture").unwrap();
        for tool_id in ["claude_code", "pi", "dsh_web"] {
            for version in ["0.84.4", "0.84.5", "2027.999.123.0", "", "future-beta"] {
                let observed = ObservedInstallation {
                    path: path.clone(),
                    version: version.into(),
                    location: "path",
                };
                assert!(can_attempt(tool_id, &observed));
                assert!(projections(tool_id, &[observed])[0].supported);
            }
        }
        std::fs::remove_file(&path).unwrap();
        let disappeared = ObservedInstallation {
            path,
            version: "0.84.4".into(),
            location: "path",
        };
        assert!(!can_attempt("pi", &disappeared));
        let _ = std::fs::remove_dir_all(directory);
    }
}

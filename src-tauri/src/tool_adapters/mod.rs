pub(crate) mod claude_code;
pub(crate) mod claude_desktop;
pub(crate) mod codex_desktop;
pub(crate) mod common;
pub(crate) mod dsh_web;
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
    UnsupportedVersion,
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

const TARGETS: [(&str, &str, &str); 5] = [
    ("claude_code", "Claude Code", "命令行与编辑器工作区"),
    ("claude_desktop", "Claude Desktop", "Claude 桌面应用"),
    (
        "codex_desktop",
        "Codex Desktop",
        "ChatGPT 桌面应用中的 Codex",
    ),
    ("pi", "Pi", "Pi 编程助手"),
    ("dsh_web", "DSH web", "DeepSeek Harness 浏览器工作台"),
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

fn version_supported(tool_id: &str, version: &str) -> bool {
    match tool_id {
        "claude_code" => matches!(version, "2.1.233" | "2.1.248" | "2.1.250" | "2.1.251"),
        "claude_desktop" => matches!(version, "1.40609.0" | "1.40609.1" | "1.4.2.0"),
        "codex_desktop" => matches!(version, "26.825.51511" | "2026.901.1200.0"),
        "pi" => matches!(version, "0.84.2" | "0.84.3" | "0.84.4"),
        "dsh_web" => matches!(version, "0.1.0-rc.6" | "0.1.1-rc.2" | "0.1.2-alpha.1"),
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
        .filter(|installation| version_supported(tool_id, &installation.version))
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
            supported: version_supported(tool_id, &installation.version),
            recommended: version_supported(tool_id, &installation.version) && supported_count == 1,
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
            .filter(|item| version_supported(tool_id, &item.version))
            .count();
        let status = if observed.is_empty() {
            "not_found"
        } else if supported_count == 0 {
            "unsupported_version"
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
    if !version_supported(tool_id, &selected.version) {
        return Err(AdapterFailure::UnsupportedVersion);
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
    fn supported_versions_are_exact_not_ranges() {
        assert!(version_supported("pi", "0.84.4"));
        assert!(!version_supported("pi", "0.84.5"));
        assert!(version_supported("dsh_web", "0.1.1-rc.2"));
        assert!(!version_supported("dsh_web", "0.1.2"));
    }
}

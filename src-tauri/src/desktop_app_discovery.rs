use crate::desktop_app_discovery_core::{
    classify, DesktopAppObservation, DesktopAppResult, DesktopAppSpec, LocationHint,
    DESKTOP_APP_SPECS,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::env;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

const MAX_PLIST_BYTES: u64 = 256 * 1024;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DesktopAppScanRequest {
    request_id: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DesktopAppScanResponse {
    request_id: String,
    platform: &'static str,
    started_at_epoch_ms: u64,
    completed_at_epoch_ms: u64,
    apps: Vec<DesktopAppProjection>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct DesktopAppProjection {
    app_id: &'static str,
    display_name: &'static str,
    status: &'static str,
    version: String,
    candidate_count: usize,
    location_hint: &'static str,
    bundle_identifier: String,
    configuration_status: &'static str,
    reason_code: &'static str,
}

impl From<DesktopAppResult> for DesktopAppProjection {
    fn from(result: DesktopAppResult) -> Self {
        Self {
            app_id: result.app_id,
            display_name: result.display_name,
            status: result.status,
            version: result.version,
            candidate_count: result.candidate_count,
            location_hint: result.location_hint.as_str(),
            bundle_identifier: result.bundle_identifier,
            configuration_status: result.configuration_status,
            reason_code: result.reason_code,
        }
    }
}

#[derive(Clone, Debug)]
struct Candidate {
    path: PathBuf,
    location_hint: LocationHint,
    bundle_identifier: String,
    version: String,
}

/// Implements the frozen `desktop-app-discovery@v1` query.
#[tauri::command]
pub fn scan_desktop_apps_read_only(
    request: DesktopAppScanRequest,
) -> Result<DesktopAppScanResponse, String> {
    validate_request_id(&request.request_id)?;
    let started_at_epoch_ms = unix_epoch_ms();
    let apps = DESKTOP_APP_SPECS
        .into_iter()
        .map(|spec| DesktopAppProjection::from(classify(spec, observe_app(spec))))
        .collect();

    Ok(DesktopAppScanResponse {
        request_id: request.request_id,
        platform: platform_name(),
        started_at_epoch_ms,
        completed_at_epoch_ms: unix_epoch_ms().max(started_at_epoch_ms),
        apps,
    })
}

fn observe_app(spec: DesktopAppSpec) -> DesktopAppObservation {
    #[cfg(target_os = "macos")]
    {
        let candidates = discover_macos_candidates(spec);
        return classify_candidates(candidates);
    }

    #[cfg(target_os = "windows")]
    {
        let candidates = discover_windows_candidates(spec);
        return classify_candidates(candidates);
    }

    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        let _ = spec;
        DesktopAppObservation::Unsupported
    }
}

fn classify_candidates(candidates: Vec<Candidate>) -> DesktopAppObservation {
    match candidates.as_slice() {
        [] => DesktopAppObservation::NotFound,
        [candidate] => DesktopAppObservation::Found {
            version: candidate.version.clone(),
            location_hint: candidate.location_hint,
            bundle_identifier: candidate.bundle_identifier.clone(),
        },
        many => DesktopAppObservation::Multiple {
            candidate_count: many.len(),
        },
    }
}

#[cfg(target_os = "macos")]
fn discover_macos_candidates(spec: DesktopAppSpec) -> Vec<Candidate> {
    let app_names: &[&str] = match spec.id {
        "claude_desktop" => &["Claude.app"],
        "codex_desktop" => &["ChatGPT.app", "Codex.app"],
        _ => &[],
    };
    let mut roots = vec![(PathBuf::from("/Applications"), LocationHint::Applications)];
    if let Some(home) = env::var_os("HOME") {
        roots.push((
            PathBuf::from(home).join("Applications"),
            LocationHint::UserApplications,
        ));
    }

    let mut accepted = HashMap::new();
    for (root, location_hint) in roots {
        for app_name in app_names {
            let path = root.join(app_name);
            let Some((bundle_identifier, version)) = read_macos_bundle_metadata(&path) else {
                continue;
            };
            if bundle_identifier != spec.expected_bundle_id {
                continue;
            }
            let canonical = std::fs::canonicalize(&path).unwrap_or(path);
            accepted.entry(canonical.clone()).or_insert(Candidate {
                path: canonical,
                location_hint,
                bundle_identifier,
                version,
            });
        }
    }
    let mut candidates: Vec<_> = accepted.into_values().collect();
    candidates.sort_by(|left, right| left.path.cmp(&right.path));
    candidates.truncate(8);
    candidates
}

#[cfg(target_os = "macos")]
fn read_macos_bundle_metadata(app_path: &Path) -> Option<(String, String)> {
    let plist_path = app_path.join("Contents/Info.plist");
    let metadata = plist_path.metadata().ok()?;
    if !metadata.is_file() || metadata.len() > MAX_PLIST_BYTES {
        return None;
    }
    let source = std::fs::read_to_string(plist_path).ok()?;
    let bundle_id = plist_string(&source, "CFBundleIdentifier")?;
    let version = plist_string(&source, "CFBundleShortVersionString").unwrap_or_default();
    Some((bundle_id, version))
}

#[cfg(target_os = "macos")]
fn plist_string(source: &str, key: &str) -> Option<String> {
    let marker = format!("<key>{key}</key>");
    let tail = source.split_once(&marker)?.1;
    let value = tail.split_once("<string>")?.1.split_once("</string>")?.0;
    let value = value.trim();
    if value.is_empty() || value.len() > 256 || value.contains(['<', '>', '\n', '\r']) {
        return None;
    }
    Some(value.to_string())
}

#[cfg(target_os = "windows")]
fn discover_windows_candidates(spec: DesktopAppSpec) -> Vec<Candidate> {
    let relative_paths: &[&str] = match spec.id {
        "claude_desktop" => &[
            "AnthropicClaude\\Claude.exe",
            "Programs\\Claude\\Claude.exe",
        ],
        "codex_desktop" => &[
            "Programs\\ChatGPT\\ChatGPT.exe",
            "Programs\\OpenAI\\ChatGPT.exe",
            "ChatGPT\\ChatGPT.exe",
        ],
        _ => &[],
    };
    let mut roots = Vec::new();
    if let Some(root) = env::var_os("LOCALAPPDATA") {
        roots.push((PathBuf::from(root), LocationHint::LocalAppData));
    }
    for variable in ["ProgramFiles", "ProgramW6432"] {
        if let Some(root) = env::var_os(variable) {
            roots.push((PathBuf::from(root), LocationHint::ProgramFiles));
        }
    }
    let mut accepted = HashMap::new();
    for (root, location_hint) in roots {
        for relative_path in relative_paths {
            let path = root.join(relative_path);
            if !path.is_file() {
                continue;
            }
            let canonical = std::fs::canonicalize(&path).unwrap_or(path);
            accepted.entry(canonical.clone()).or_insert(Candidate {
                path: canonical,
                location_hint,
                bundle_identifier: String::new(),
                version: String::new(),
            });
        }
    }
    let mut candidates: Vec<_> = accepted.into_values().collect();
    candidates.sort_by(|left, right| left.path.cmp(&right.path));
    candidates.truncate(8);
    candidates
}

fn validate_request_id(request_id: &str) -> Result<(), String> {
    if request_id.is_empty()
        || request_id.len() > 64
        || !request_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
    {
        return Err("invalid_request".into());
    }
    Ok(())
}

fn unix_epoch_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX)
}

const fn platform_name() -> &'static str {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_identity_is_closed() {
        assert!(validate_request_id("desktop-123_ok").is_ok());
        assert!(validate_request_id("").is_err());
        assert!(validate_request_id("bad/path").is_err());
        assert!(validate_request_id(&"a".repeat(65)).is_err());
    }

    #[test]
    fn candidate_projection_is_sanitized() {
        let projection = classify_candidates(vec![Candidate {
            path: PathBuf::from("/private/Users/example/Applications/Claude.app"),
            location_hint: LocationHint::UserApplications,
            bundle_identifier: "com.anthropic.claudefordesktop".into(),
            version: "1.2.3".into(),
        }]);
        assert!(matches!(
            projection,
            DesktopAppObservation::Found {
                location_hint: LocationHint::UserApplications,
                ..
            }
        ));
    }
}

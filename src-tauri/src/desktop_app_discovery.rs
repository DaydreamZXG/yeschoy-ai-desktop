use crate::desktop_app_discovery_core::{
    classify, DesktopAppObservation, DesktopAppResult, DesktopAppSpec, LocationHint,
    DESKTOP_APP_SPECS,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::env;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

#[cfg(target_os = "windows")]
use winreg::enums::{KEY_READ, KEY_WOW64_32KEY, KEY_WOW64_64KEY};
#[cfg(target_os = "windows")]
use winreg::{RegKey, HKCR, HKCU, HKLM};

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
    let packaged = discover_windows_package_candidates(spec);
    if !packaged.is_empty() {
        return packaged;
    }

    let registered = discover_windows_uninstall_candidates(spec);
    if !registered.is_empty() {
        return registered;
    }

    let mut accepted = HashMap::new();
    if let Some(root) = env::var_os("LOCALAPPDATA") {
        collect_windows_path_candidates(
            &mut accepted,
            PathBuf::from(root),
            LocationHint::LocalAppData,
            windows_relative_paths(spec.id, false),
        );
    }
    for variable in ["ProgramFiles", "ProgramW6432"] {
        if let Some(root) = env::var_os(variable) {
            collect_windows_path_candidates(
                &mut accepted,
                PathBuf::from(root),
                LocationHint::ProgramFiles,
                windows_relative_paths(spec.id, true),
            );
        }
    }
    let mut candidates: Vec<_> = accepted.into_values().collect();
    candidates.sort_by(|left, right| left.path.cmp(&right.path));
    candidates.truncate(8);
    if !candidates.is_empty() {
        return candidates;
    }

    discover_windows_protocol_candidate(spec)
}

#[cfg(any(target_os = "windows", test))]
fn windows_relative_paths(app_id: &str, program_files: bool) -> &'static [&'static str] {
    match (app_id, program_files) {
        ("claude_desktop", false) => &[
            "AnthropicClaude\\Claude.exe",
            "Programs\\Claude\\Claude.exe",
            "Programs\\Claude Desktop\\Claude.exe",
            "Programs\\Anthropic\\Claude\\Claude.exe",
            "Claude\\Claude.exe",
        ],
        ("claude_desktop", true) => &[
            "Claude\\Claude.exe",
            "Claude Desktop\\Claude.exe",
            "Anthropic\\Claude\\Claude.exe",
        ],
        ("codex_desktop", false) => &[
            "Programs\\Codex\\Codex.exe",
            "Programs\\OpenAI\\Codex\\Codex.exe",
            "Codex\\Codex.exe",
            "Programs\\ChatGPT\\ChatGPT.exe",
            "Programs\\OpenAI\\ChatGPT.exe",
            "Programs\\OpenAI\\ChatGPT\\ChatGPT.exe",
            "ChatGPT\\ChatGPT.exe",
        ],
        ("codex_desktop", true) => &[
            "Codex\\Codex.exe",
            "OpenAI\\Codex\\Codex.exe",
            "ChatGPT\\ChatGPT.exe",
            "OpenAI\\ChatGPT\\ChatGPT.exe",
        ],
        _ => &[],
    }
}

#[cfg(target_os = "windows")]
fn collect_windows_path_candidates(
    accepted: &mut HashMap<PathBuf, Candidate>,
    root: PathBuf,
    location_hint: LocationHint,
    relative_paths: &[&str],
) {
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

#[cfg(any(target_os = "windows", test))]
fn compact_windows_name(raw: &str) -> String {
    raw.chars()
        .filter(|character| character.is_ascii_alphanumeric())
        .map(|character| character.to_ascii_lowercase())
        .take(256)
        .collect()
}

#[cfg(any(target_os = "windows", test))]
fn windows_package_identity_matches(app_id: &str, identity: &str) -> bool {
    let compact = compact_windows_name(identity);
    match app_id {
        "claude_desktop" => {
            compact == "claude"
                || compact == "claudedesktop"
                || compact == "anthropicclaude"
                || compact == "anthropicclaudedesktop"
        }
        "codex_desktop" => {
            compact == "codex"
                || compact == "chatgpt"
                || compact == "openaicodex"
                || compact == "openaichatgpt"
        }
        _ => false,
    }
}

#[cfg(any(target_os = "windows", test))]
fn windows_registered_name_matches(app_id: &str, name: &str, publisher: &str) -> bool {
    let name = compact_windows_name(name);
    let publisher = compact_windows_name(publisher);
    match app_id {
        "claude_desktop" => {
            name == "claude"
                || name == "claudedesktop"
                || ((name == "anthropicclaude" || name == "anthropicclaudedesktop")
                    && publisher.contains("anthropic"))
        }
        "codex_desktop" => {
            name == "codex"
                || name == "codexdesktop"
                || ((name == "chatgpt" || name == "chatgptdesktop")
                    && (publisher.is_empty() || publisher.contains("openai")))
        }
        _ => false,
    }
}

#[cfg(any(target_os = "windows", test))]
fn parse_windows_package_identity(raw: &str) -> Option<(String, String)> {
    if raw.is_empty() || raw.len() > 512 {
        return None;
    }
    let parts: Vec<_> = raw.split('_').collect();
    let version_index = parts.iter().position(|part| {
        let components: Vec<_> = part.split('.').collect();
        components.len() >= 2
            && components.iter().all(|component| {
                !component.is_empty() && component.bytes().all(|b| b.is_ascii_digit())
            })
    });
    let (identity, version) = match version_index {
        Some(0) => return None,
        Some(index) => (parts[..index].join("_"), parts[index].to_owned()),
        None => (parts.first()?.to_string(), String::new()),
    };
    Some((identity, version))
}

#[cfg(any(target_os = "windows", test))]
fn windows_version_key(raw: &str) -> [u64; 4] {
    let mut key = [0; 4];
    for (index, component) in raw.split('.').take(4).enumerate() {
        key[index] = component.parse().unwrap_or(0);
    }
    key
}

#[cfg(target_os = "windows")]
fn discover_windows_package_candidates(spec: DesktopAppSpec) -> Vec<Candidate> {
    let repositories = [
        "Software\\Classes\\ActivatableClasses\\Package",
        "Software\\Classes\\Local Settings\\Software\\Microsoft\\Windows\\CurrentVersion\\AppModel\\Repository\\Packages",
        "Software\\Classes\\Local Settings\\Software\\Microsoft\\Windows\\CurrentVersion\\AppModel\\Repository\\Families",
    ];
    let mut accepted: HashMap<String, Candidate> = HashMap::new();
    for repository in repositories {
        let Ok(packages) = HKCU.open_subkey_with_flags(repository, KEY_READ) else {
            continue;
        };
        for package_full_name in packages.enum_keys().filter_map(Result::ok) {
            let Some((identity, version)) = parse_windows_package_identity(&package_full_name)
            else {
                continue;
            };
            if !windows_package_identity_matches(spec.id, &identity) {
                continue;
            }
            let key = compact_windows_name(&identity);
            let candidate = Candidate {
                path: PathBuf::from(format!("package-{key}")),
                location_hint: LocationHint::LocalAppData,
                bundle_identifier: String::new(),
                version,
            };
            let replace = accepted
                .get(&key)
                .map(|current| {
                    windows_version_key(&candidate.version) > windows_version_key(&current.version)
                })
                .unwrap_or(true);
            if replace {
                accepted.insert(key, candidate);
            }
        }
    }
    let mut candidates: Vec<_> = accepted.into_values().collect();
    candidates.sort_by(|left, right| left.path.cmp(&right.path));
    candidates.truncate(8);
    candidates
}

#[cfg(target_os = "windows")]
fn discover_windows_uninstall_candidates(spec: DesktopAppSpec) -> Vec<Candidate> {
    let mut accepted: HashMap<String, Candidate> = HashMap::new();
    for (hive_name, hive) in [("hkcu", &HKCU), ("hklm", &HKLM)] {
        for view in [KEY_WOW64_64KEY, KEY_WOW64_32KEY] {
            collect_windows_uninstall_candidates(&mut accepted, spec, hive_name, hive, view);
        }
    }
    let mut candidates: Vec<_> = accepted.into_values().collect();
    candidates.sort_by(|left, right| left.path.cmp(&right.path));
    candidates.truncate(8);
    candidates
}

#[cfg(target_os = "windows")]
fn collect_windows_uninstall_candidates(
    accepted: &mut HashMap<String, Candidate>,
    spec: DesktopAppSpec,
    hive_name: &str,
    hive: &RegKey,
    view: u32,
) {
    const UNINSTALL: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall";
    let Ok(parent) = hive.open_subkey_with_flags(UNINSTALL, KEY_READ | view) else {
        return;
    };
    for subkey_name in parent.enum_keys().filter_map(Result::ok) {
        let Ok(item) = parent.open_subkey_with_flags(&subkey_name, KEY_READ | view) else {
            continue;
        };
        let display_name: String = item.get_value("DisplayName").unwrap_or_default();
        let publisher: String = item.get_value("Publisher").unwrap_or_default();
        if !windows_registered_name_matches(spec.id, &display_name, &publisher) {
            continue;
        }
        let version: String = item.get_value("DisplayVersion").unwrap_or_default();
        let key = compact_windows_name(&display_name);
        let candidate = Candidate {
            path: PathBuf::from(format!("uninstall-{hive_name}-{key}")),
            location_hint: if hive_name == "hkcu" {
                LocationHint::LocalAppData
            } else {
                LocationHint::ProgramFiles
            },
            bundle_identifier: String::new(),
            version,
        };
        let replace = accepted
            .get(&key)
            .map(|current| {
                windows_version_key(&candidate.version) > windows_version_key(&current.version)
            })
            .unwrap_or(true);
        if replace {
            accepted.insert(key, candidate);
        }
    }
}

#[cfg(target_os = "windows")]
fn discover_windows_protocol_candidate(spec: DesktopAppSpec) -> Vec<Candidate> {
    let protocols: &[&str] = match spec.id {
        "claude_desktop" => &["claude"],
        "codex_desktop" => &["codex", "chatgpt"],
        _ => &[],
    };
    for protocol in protocols {
        let user_path = format!("Software\\Classes\\{protocol}\\shell\\open\\command");
        let registered = HKCU.open_subkey_with_flags(&user_path, KEY_READ).is_ok()
            || HKCR
                .open_subkey_with_flags(format!("{protocol}\\shell\\open\\command"), KEY_READ)
                .is_ok();
        if registered {
            return vec![Candidate {
                path: PathBuf::from(format!("protocol-{protocol}")),
                location_hint: LocationHint::LocalAppData,
                bundle_identifier: String::new(),
                version: String::new(),
            }];
        }
    }
    Vec::new()
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

    #[test]
    fn windows_package_registration_matches_current_desktop_apps() {
        let (claude, claude_version) =
            parse_windows_package_identity("Claude_1.4.2.0_x64__examplepublisher").unwrap();
        let (chatgpt, chatgpt_version) =
            parse_windows_package_identity("OPENAI.ChatGPT_2026.901.1200.0_x64__examplepublisher")
                .unwrap();
        let (codex_family, family_version) =
            parse_windows_package_identity("OpenAI.Codex_2p2nqsd0c76g0").unwrap();

        assert_eq!(claude, "Claude");
        assert_eq!(claude_version, "1.4.2.0");
        assert_eq!(chatgpt, "OPENAI.ChatGPT");
        assert_eq!(chatgpt_version, "2026.901.1200.0");
        assert_eq!(codex_family, "OpenAI.Codex");
        assert!(family_version.is_empty());
        assert!(windows_package_identity_matches("claude_desktop", &claude));
        assert!(windows_package_identity_matches("codex_desktop", &chatgpt));
        assert!(windows_package_identity_matches(
            "codex_desktop",
            &codex_family
        ));
        assert!(!windows_package_identity_matches(
            "claude_desktop",
            "ClaudeTheme"
        ));
        assert!(!windows_package_identity_matches(
            "codex_desktop",
            "OpenAI.API"
        ));
        assert_eq!(windows_version_key("26.825.51511.0"), [26, 825, 51511, 0]);
    }

    #[test]
    fn windows_registered_and_path_fallbacks_cover_both_installers() {
        assert!(windows_registered_name_matches(
            "claude_desktop",
            "Claude Desktop",
            "Anthropic PBC"
        ));
        assert!(windows_registered_name_matches(
            "codex_desktop",
            "ChatGPT",
            "OpenAI, L.L.C."
        ));
        assert!(!windows_registered_name_matches(
            "codex_desktop",
            "ChatGPT Helper",
            "Unknown"
        ));

        assert!(windows_relative_paths("claude_desktop", false)
            .contains(&"Programs\\Claude Desktop\\Claude.exe"));
        assert!(
            windows_relative_paths("codex_desktop", false).contains(&"Programs\\Codex\\Codex.exe")
        );
        assert!(
            windows_relative_paths("codex_desktop", true).contains(&"OpenAI\\ChatGPT\\ChatGPT.exe")
        );
    }
}

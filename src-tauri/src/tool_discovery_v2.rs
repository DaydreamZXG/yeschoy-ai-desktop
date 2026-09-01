use crate::tool_discovery::{
    common_binary_directories, executable_filenames, is_executable_candidate, platform_name,
    probe_version, unix_epoch_ms, validate_request_id, Candidate,
};
use crate::tool_discovery_core::{classify, LocationHint, ProbeObservation, ToolSpec, TOOL_SPECS};
use crate::tool_selection_core::{exact_version, Inventory, LIMIT};
use serde::{Deserialize, Serialize};
use std::{env, path::PathBuf};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ScanRequest {
    request_id: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanResponse {
    request_id: String,
    platform: &'static str,
    started_at_epoch_ms: u64,
    completed_at_epoch_ms: u64,
    tools: Vec<ToolProjection>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ToolProjection {
    tool_id: &'static str,
    display_name: &'static str,
    status: &'static str,
    version: String,
    candidate_count: usize,
    bundled_count: usize,
    selection: &'static str,
    location_hint: &'static str,
    compatibility: &'static str,
    reason_code: &'static str,
}

#[tauri::command]
pub async fn scan_tools_read_only_v2(request: ScanRequest) -> Result<ScanResponse, String> {
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
        tools: vec![claude, codex, opencode, pi, dsh],
    })
}

fn discover(executable: &str) -> Inventory {
    let mut directories: Vec<(PathBuf, Option<usize>)> = env::var_os("PATH")
        .map(|value| {
            env::split_paths(&value)
                .enumerate()
                .map(|(i, p)| (p, Some(i)))
                .collect()
        })
        .unwrap_or_default();
    directories.extend(common_binary_directories().into_iter().map(|p| (p, None)));
    discover_in(executable, directories)
}

fn discover_in(executable: &str, directories: Vec<(PathBuf, Option<usize>)>) -> Inventory {
    let mut inventory = Inventory::default();
    for (directory, rank) in directories {
        if !directory.is_absolute() {
            continue;
        }
        for name in executable_filenames(executable) {
            let entry = directory.join(name);
            if !is_executable_candidate(&entry) {
                continue;
            }
            // An unresolvable link cannot establish a safe distinct identity.
            if let Ok(canonical) = std::fs::canonicalize(&entry) {
                inventory.add(&entry, canonical, rank, cfg!(target_os = "macos"));
            }
        }
        if inventory.standalone.len() >= LIMIT {
            break;
        }
    }
    inventory
}

async fn scan_tool(spec: ToolSpec) -> ToolProjection {
    let inventory = discover(spec.executable_name);
    project_inventory(spec, inventory).await
}

async fn project_inventory(spec: ToolSpec, inventory: Inventory) -> ToolProjection {
    let (selection, selected) = inventory.selection();
    let observation = if let Some(selected) = selected {
        let candidate = Candidate {
            path: selected.path.clone(),
            location_hint: if selected.path_rank.is_some() {
                LocationHint::Path
            } else {
                LocationHint::CommonLocation
            },
        };
        match probe_version(&candidate).await {
            ProbeObservation::Found {
                version,
                location_hint,
            } if !exact_version(&version) => ProbeObservation::Failed { location_hint },
            result => result,
        }
    } else if inventory.standalone.is_empty() {
        ProbeObservation::NotFound
    } else {
        ProbeObservation::MultipleInstallations {
            candidate_count: inventory.standalone.len(),
        }
    };
    let result = classify(spec, observation);
    ToolProjection {
        tool_id: result.tool_id,
        display_name: result.display_name,
        status: result.status.as_str(),
        version: result.version,
        candidate_count: inventory.standalone.len(),
        bundled_count: inventory.bundled_count(),
        selection,
        location_hint: result.location_hint.as_str(),
        compatibility: result.compatibility.as_str(),
        reason_code: result.reason_code.as_str(),
    }
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::{
        fs,
        os::unix::fs::{symlink, PermissionsExt},
    };
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> Self {
            let path = env::temp_dir().join(format!(
                "yeschoy-discovery-{}-{}-{}",
                std::process::id(),
                unix_epoch_ms(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&path).unwrap();
            Self(path)
        }
        fn binary(&self, rel: &str, body: &str) -> PathBuf {
            let path = self.0.join(rel);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
            fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
            path
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[tokio::test]
    async fn real_symlink_and_bundle_probe_only_standalone() {
        let fixture = Fixture::new();
        let binary = fixture.binary("standalone/codex", "printf 'codex-cli 0.146.0\\n'");
        let bundled = fixture.binary("ChatGPT.app/Contents/Resources/codex", "exit 17");
        let alias_dir = fixture.0.join("alias");
        fs::create_dir(&alias_dir).unwrap();
        symlink(&binary, alias_dir.join("codex")).unwrap();
        let result = project_inventory(
            TOOL_SPECS[1],
            discover_in(
                "codex",
                vec![
                    (bundled.parent().unwrap().into(), Some(0)),
                    (alias_dir, Some(1)),
                    (binary.parent().unwrap().into(), None),
                ],
            ),
        )
        .await;
        assert_eq!(result.candidate_count, 1);
        assert_eq!(result.bundled_count, 1);
        assert_eq!(result.status, "detected_unverified");
        assert_eq!(result.version, "0.146.0");
        assert_eq!(result.selection, "single_installation");
    }

    #[tokio::test]
    async fn failed_primary_does_not_fall_back_to_secondary() {
        let fixture = Fixture::new();
        let primary = fixture.binary("primary/codex", "exit 3");
        let secondary = fixture.binary("secondary/codex", "printf 'codex-cli 9.9.9\\n'");
        let result = project_inventory(
            TOOL_SPECS[1],
            discover_in(
                "codex",
                vec![
                    (primary.parent().unwrap().into(), Some(0)),
                    (secondary.parent().unwrap().into(), Some(1)),
                ],
            ),
        )
        .await;
        assert_eq!(result.status, "probe_failed");
        assert_eq!(result.selection, "path_precedence");
        assert_eq!(result.version, "");
    }

    #[tokio::test]
    async fn invalid_output_and_timeout_stay_unverified() {
        let fixture = Fixture::new();
        for (name, body, expected) in [
            (
                "invalid",
                "printf 'secret-looking-error\\n'",
                "probe_failed",
            ),
            ("timeout", "exec /bin/sleep 4", "probe_timed_out"),
        ] {
            let binary = fixture.binary(&format!("{name}/codex"), body);
            let result = project_inventory(
                TOOL_SPECS[1],
                discover_in("codex", vec![(binary.parent().unwrap().into(), None)]),
            )
            .await;
            assert_eq!(result.status, expected);
            assert_eq!(result.version, "");
            assert_eq!(result.compatibility, "unverified_read_only");
        }
    }
}

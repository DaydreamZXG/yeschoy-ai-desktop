pub(crate) mod claude_code;
pub(crate) mod claude_desktop;
pub(crate) mod codex_desktop;
pub(crate) mod common;
pub(crate) mod desktop_launch;
pub(crate) mod desktop_lifecycle;
pub(crate) mod dsh_web;
pub(crate) mod pi;
pub(crate) mod terminal_launch;
pub(crate) mod workbuddy;

use std::path::{Path, PathBuf};

use futures::{stream, StreamExt};
use serde::Serialize;

use crate::{desktop_app_discovery, tool_discovery};

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
    LaunchError(&'static str),
}

#[derive(Clone, Debug)]
pub(crate) struct ResolvedInstallation {
    pub(crate) path: PathBuf,
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

const TARGETS: [(&str, &str, &str); 6] = [
    ("claude_code", "Claude Code", "命令行与编辑器工作区"),
    ("claude_desktop", "Claude Desktop", "Claude 桌面应用"),
    (
        "codex_desktop",
        "Codex Desktop",
        "ChatGPT 桌面应用中的 Codex",
    ),
    ("pi", "Pi", "Pi 编程助手"),
    ("dsh_web", "DSH web", "DeepSeek Harness 浏览器工作台"),
    ("workbuddy", "WorkBuddy", "腾讯 AI 办公与开发助手"),
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
        "workbuddy" => Some("workbuddy"),
        _ => None,
    }
}

/// Whether picking an installation is a question worth asking the user.
///
/// It never changes what gets configured: every adapter's `prepare_catalog`
/// takes `home`, so the files written are user-level and identical whichever
/// copy is selected. The choice only decides which binary or bundle the
/// assistant launches and lifecycle-checks.
///
/// That makes it the wrong question for a CLI. What `claude` runs in a terminal
/// is decided by the login shell's `PATH`, and `discover_candidates` already
/// orders candidates by that. Asking the user would let them pick a second copy
/// the "打开终端使用" button then launches while their own terminal keeps
/// running the first one — two different binaries, no way to tell from the two
/// identical labels they were offered.
///
/// Desktop bundles have no `PATH` to arbitrate, and discovery reads a real
/// version out of them, so there the question is both necessary and answerable.
fn asks_which_installation(tool_id: &str) -> bool {
    desktop_id_for(tool_id).is_some()
}

/// A path the user can recognise, with the parts they should not have to see
/// stripped out.
///
/// The home prefix collapses to `~`, which removes the account name — the only
/// genuinely sensitive part of a local path — while still telling a person
/// apart `~/Applications/Claude.app` from `/Applications/Claude.app`. A long
/// path keeps its last two components, because the tail is what differs.
/// Control and bidi characters are dropped: the renderer rejects them, and a
/// user with an oddly named folder should not silently lose the entry.
fn display_path(path: &Path, home: Option<&Path>) -> String {
    let shortened = home
        .and_then(|home| path.strip_prefix(home).ok())
        .map(|rest| format!("~/{}", rest.to_string_lossy()))
        .unwrap_or_else(|| path.to_string_lossy().into_owned());
    let chosen = if shortened.chars().count() <= 64 {
        shortened
    } else {
        let tail = path
            .components()
            .rev()
            .take(2)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .map(|part| part.as_os_str().to_string_lossy().into_owned())
            .collect::<Vec<_>>()
            .join("/");
        format!("…/{tail}")
    };
    // Filtering last, so a hostile name reached through the eliding branch is
    // cleaned too, and truncation cannot leave a half-written escape sequence.
    chosen
        .chars()
        .filter(|c| {
            !c.is_control() && !matches!(*c, '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
        })
        .take(64)
        .collect()
}

fn opaque_installation_id(path: &Path) -> String {
    // FNV-1a is used only as an opaque, local selection handle.
    //
    // What this protects is the *inbound* direction: the renderer hands back an
    // id, never a path, so it cannot steer the backend at an arbitrary file,
    // and collisions are rejected during resolution. Putting a readable path in
    // the outbound `label` does not weaken that — it is a caption for a choice
    // the user is already making about their own machine.
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
        "codex_desktop" | "claude_desktop" | "workbuddy" => installation.path.exists(),
        "claude_code" | "pi" | "dsh_web" => installation.path.is_file(),
        _ => false,
    }
}

async fn observe_cli(executable: &str) -> Vec<ObservedInstallation> {
    // Activation eligibility is based on the closed path inventory and atomic
    // configuration readback, never on `--version` output. Starting five
    // third-party CLIs here could add up to minutes of cold-start time before
    // the beginner could even choose an app, so this scan remains read-only
    // and process-free. Exact version diagnostics remain available through the
    // dedicated tool discovery command.
    tool_discovery::discover_candidates(executable)
        .into_iter()
        .take(8)
        .map(|candidate| ObservedInstallation {
            path: candidate.path,
            version: String::new(),
            location: candidate.location_hint.as_str(),
        })
        .collect()
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
    let preferred = observed
        .iter()
        .position(|installation| can_attempt(tool_id, installation));
    let home = user_home();
    observed
        .iter()
        .enumerate()
        .map(|(index, installation)| InstallationProjection {
            installation_id: opaque_installation_id(&installation.path),
            // "安装 1 · 系统 PATH" told the user nothing: CLI discovery leaves
            // `version` empty on purpose, so two entries could be completely
            // indistinguishable. The location stays as context; the path is
            // what actually answers the question.
            label: format!(
                "{} · {}",
                location_label(installation.location),
                display_path(&installation.path, home.as_deref())
            ),
            version: installation.version.clone(),
            supported: can_attempt(tool_id, installation),
            recommended: preferred == Some(index),
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
    stream::iter(0..TARGETS.len())
        .map(scan_target)
        .buffered(3)
        .collect()
        .await
}

async fn scan_target(index: usize) -> TargetProjection {
    let (tool_id, display_name, surface) = TARGETS[index];
    let observed = observe(tool_id).await;
    let supported_count = observed
        .iter()
        .filter(|item| can_attempt(tool_id, item))
        .count();
    // Only ask when there is a real choice, and only for the tools where the
    // choice is real at all (see `asks_which_installation`). `observed` also
    // carries entries that cannot be activated, so a single usable copy beside
    // a stale one is an answer, not a question.
    let status = if observed.is_empty() {
        "not_found"
    } else if supported_count == 0 {
        "missing_runtime"
    } else if supported_count > 1 && asks_which_installation(tool_id) {
        "selection_required"
    } else {
        "available"
    };
    TargetProjection {
        tool_id,
        display_name,
        surface,
        status,
        installations: projections(tool_id, &observed),
    }
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
        // Mirrors the `available` rule in `scan_target`, so the two can never
        // disagree: whenever the scan did not ask, resolution must not refuse.
        // `observed` is already ordered by login-shell PATH position, so the
        // first usable candidate is the one the user's own terminal resolves.
        let mut usable = observed
            .iter()
            .filter(|candidate| can_attempt(tool_id, candidate));
        match (usable.next(), usable.next()) {
            (Some(only), None) => only,
            (None, _) => return Err(AdapterFailure::MissingRuntime),
            (Some(first), Some(_)) if !asks_which_installation(tool_id) => first,
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
    })
}

/// User-level settings are shared by installations of the same product.
/// Daily Open uses the same native preference order as discovery, never a
/// renderer supplied path and never a version-number compatibility gate.
pub(crate) async fn resolve_preferred_installation(
    tool_id: &str,
) -> Result<ResolvedInstallation, AdapterFailure> {
    observe(tool_id)
        .await
        .into_iter()
        .find(|candidate| can_attempt(tool_id, candidate))
        .map(|candidate| ResolvedInstallation {
            path: candidate.path,
        })
        .ok_or(AdapterFailure::ToolNotFound)
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

    #[test]
    fn one_usable_copy_is_an_answer_even_when_discovery_found_others() {
        // Discovery keeps unusable entries (a stale directory where the binary
        // used to be, a bundle that no longer exists). With CLI versions left
        // empty on purpose, offering those alongside the real installation gave
        // the user two indistinguishable labels to choose between.
        let directory = common::temporary_working_directory("single-usable").unwrap();
        let real = directory.join("claude");
        std::fs::write(&real, b"fixture").unwrap();
        let observed = vec![
            ObservedInstallation {
                path: directory.join("gone"),
                version: String::new(),
                location: "common_location",
            },
            ObservedInstallation {
                path: real.clone(),
                version: String::new(),
                location: "path",
            },
        ];
        let supported = observed
            .iter()
            .filter(|item| can_attempt("claude_code", item))
            .count();
        assert_eq!(supported, 1);
        let projected = projections("claude_code", &observed);
        assert!(!projected[0].supported);
        assert!(projected[1].supported);
        assert!(projected[1].recommended);
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn only_desktop_bundles_ask_which_installation() {
        // The choice never changes what gets written — every adapter's
        // prepare_catalog takes `home`. For a CLI it would only decide which
        // binary the assistant launches, and the login shell already decides
        // that; offering a second one invites the "打开使用" button to run a
        // different copy than the user's own terminal does.
        for cli in ["claude_code", "pi", "dsh_web"] {
            assert!(!asks_which_installation(cli), "{cli}");
        }
        for desktop in ["claude_desktop", "codex_desktop", "workbuddy"] {
            assert!(asks_which_installation(desktop), "{desktop}");
        }
    }

    #[test]
    fn two_usable_bundles_still_require_a_choice() {
        let directory = common::temporary_working_directory("two-usable").unwrap();
        let first = directory.join("Claude.app");
        let second = directory.join("Claude-2.app");
        std::fs::create_dir(&first).unwrap();
        std::fs::create_dir(&second).unwrap();
        let observed = vec![
            ObservedInstallation {
                path: first,
                version: "1.0".into(),
                location: "applications",
            },
            ObservedInstallation {
                path: second,
                version: "0.9".into(),
                location: "user_applications",
            },
        ];
        assert_eq!(
            observed
                .iter()
                .filter(|item| can_attempt("claude_desktop", item))
                .count(),
            2
        );
        // Exactly one is recommended, so the picker has a default to land on.
        let projected = projections("claude_desktop", &observed);
        assert_eq!(
            projected
                .iter()
                .filter(|installation| installation.recommended)
                .count(),
            1
        );
        // And the two options are now told apart by something real.
        assert_ne!(projected[0].label, projected[1].label);
        assert!(projected[0].label.contains("Claude.app"));
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn labels_show_a_path_without_the_account_name() {
        let home = Path::new("/Users/zxg");
        assert_eq!(
            display_path(Path::new("/Users/zxg/Applications/Claude.app"), Some(home)),
            "~/Applications/Claude.app"
        );
        // Outside the home directory there is no account name to hide.
        assert_eq!(
            display_path(Path::new("/Applications/Claude.app"), Some(home)),
            "/Applications/Claude.app"
        );
        // A long path keeps the tail, which is the part that differs.
        let deep = Path::new(
            "/Users/zxg/Library/Application Support/some/very/deeply/nested/vendor/tree/claude",
        );
        let shown = display_path(deep, Some(home));
        assert!(shown.chars().count() <= 64, "{shown}");
        assert!(shown.ends_with("tree/claude"), "{shown}");
        // The renderer rejects control and bidi characters; dropping them here
        // keeps one oddly named folder from discarding the whole entry.
        let hostile = Path::new("/Applications/Cl\u{202e}aude.app");
        assert_eq!(
            display_path(hostile, Some(home)),
            "/Applications/Claude.app"
        );
        // The frontend bounds `label` at 100 code points; the location prefix
        // plus a 64-character path has to stay under that.
        assert!(
            location_label("user_applications").chars().count() + 3 + 64 <= 100,
            "label must fit the IPC contract"
        );
    }

    #[test]
    fn discovered_codex_desktop_does_not_require_an_unused_bundled_cli() {
        let directory = common::temporary_working_directory("codex-without-cli").unwrap();
        let app = directory.join("Codex.app");
        std::fs::create_dir(&app).unwrap();
        let installation = ObservedInstallation {
            path: app,
            version: String::new(),
            location: "applications",
        };
        assert!(can_attempt("codex_desktop", &installation));
        assert!(projections("codex_desktop", &[installation])[0].supported);
        assert!(!directory
            .join("Codex.app/Contents/Resources/codex")
            .exists());
        std::fs::remove_dir_all(directory).unwrap();
    }
}

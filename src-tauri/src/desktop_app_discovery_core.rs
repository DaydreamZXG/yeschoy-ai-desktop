//! Pure projection rules for the desktop-application-first discovery surface.

#[cfg(any(target_os = "windows", test))]
use std::path::Path;
use std::path::PathBuf;

/// Native-only launch information. It is never accepted from or sent to IPC.
#[derive(Clone, Debug, Eq, PartialEq)]
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
pub(crate) enum DesktopLaunchTarget {
    #[cfg_attr(not(any(target_os = "macos", test)), allow(dead_code))]
    MacBundle(PathBuf),
    WindowsExecutable(PathBuf),
    WindowsPackage(WindowsPackageApplication),
}

#[derive(Clone, Debug, Eq, PartialEq)]
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
pub(crate) struct WindowsPackageApplication {
    package_full_name: String,
    app_user_model_id: String,
}

#[cfg(any(target_os = "windows", test))]
impl WindowsPackageApplication {
    /// All three strings must originate from the installed-package APIs. This
    /// check only establishes their syntax and relationship, not installation.
    pub(crate) fn from_installed_package(
        app_id: &str,
        full_name: &str,
        family_name: &str,
        application_id: &str,
    ) -> Option<Self> {
        let parts = full_name.split('_').collect::<Vec<_>>();
        if parts.len() != 5
            || !windows_package_name_matches(app_id, parts[0])
            || !valid_identity_component(parts[0], 50, false)
            || !valid_package_version(parts[1])
            || !matches!(parts[2], "x86" | "x64" | "arm" | "arm64" | "neutral")
            || (!parts[3].is_empty() && !valid_identity_component(parts[3], 30, false))
            || parts[4].len() != 13
            || !parts[4].bytes().all(|byte| byte.is_ascii_alphanumeric())
            || family_name != format!("{}_{}", parts[0], parts[4])
            || application_id.len() >= 130
        {
            return None;
        }
        let (application_family, relative_id) = application_id.split_once('!')?;
        if application_family != family_name || !valid_relative_application_id(relative_id) {
            return None;
        }
        Some(Self {
            package_full_name: full_name.to_owned(),
            app_user_model_id: application_id.to_owned(),
        })
    }

    pub(crate) fn app_user_model_id(&self) -> &str {
        &self.app_user_model_id
    }
}

#[cfg(any(target_os = "windows", test))]
fn valid_identity_component(value: &str, maximum: usize, application: bool) -> bool {
    !value.is_empty()
        && value.len() <= maximum
        && value.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || byte == b'.' || (!application && byte == b'-')
        })
        && value.as_bytes()[0].is_ascii_alphanumeric()
        && !value.ends_with('.')
}

#[cfg(any(target_os = "windows", test))]
fn valid_relative_application_id(value: &str) -> bool {
    valid_identity_component(value, 64, true)
        && value.split('.').all(|field| {
            field
                .as_bytes()
                .first()
                .is_some_and(u8::is_ascii_alphabetic)
                && !matches!(
                    field.to_ascii_uppercase().as_str(),
                    "CON"
                        | "PRN"
                        | "AUX"
                        | "NUL"
                        | "COM1"
                        | "COM2"
                        | "COM3"
                        | "COM4"
                        | "COM5"
                        | "COM6"
                        | "COM7"
                        | "COM8"
                        | "COM9"
                        | "LPT1"
                        | "LPT2"
                        | "LPT3"
                        | "LPT4"
                        | "LPT5"
                        | "LPT6"
                        | "LPT7"
                        | "LPT8"
                        | "LPT9"
                )
        })
}

#[cfg(any(target_os = "windows", test))]
fn valid_package_version(value: &str) -> bool {
    let parts = value.split('.').collect::<Vec<_>>();
    parts.len() == 4
        && parts.iter().all(|part| {
            !part.is_empty()
                && part.bytes().all(|byte| byte.is_ascii_digit())
                && part.parse::<u16>().is_ok()
        })
}

#[cfg(any(target_os = "windows", test))]
pub(crate) fn windows_package_name_matches(app_id: &str, name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    match app_id {
        "claude_desktop" => matches!(
            name.as_str(),
            "claude"
                | "claudedesktop"
                | "anthropic.claude"
                | "anthropicclaude"
                | "anthropic.claudedesktop"
                | "anthropicclaudedesktop"
        ),
        "codex_desktop" => matches!(
            name.as_str(),
            "codex"
                | "chatgpt"
                | "openai.codex"
                | "openaicodex"
                | "openai.chatgpt"
                | "openaichatgpt"
        ),
        "workbuddy" => matches!(
            name.as_str(),
            "workbuddy" | "tencent.workbuddy" | "tencentworkbuddy"
        ),
        _ => false,
    }
}

#[cfg(any(target_os = "windows", test))]
pub(crate) fn windows_desktop_filename_matches(app_id: &str, path: &Path) -> bool {
    // Split both separators so the same fixtures run on macOS and Windows.
    let path = path.to_string_lossy();
    let name = path.rsplit(['/', '\\']).next().unwrap_or_default();
    match app_id {
        "claude_desktop" => name.eq_ignore_ascii_case("Claude.exe"),
        "codex_desktop" => {
            name.eq_ignore_ascii_case("Codex.exe") || name.eq_ignore_ascii_case("ChatGPT.exe")
        }
        "workbuddy" => name.eq_ignore_ascii_case("WorkBuddy.exe"),
        _ => false,
    }
}

#[cfg(any(target_os = "windows", test))]
pub(crate) fn windows_desktop_file_identity_matches(
    app_id: &str,
    path: &Path,
    product_name: &str,
    company_name: &str,
    gui_executable: bool,
) -> bool {
    let normalized = |value: &str| {
        value
            .chars()
            .filter(|character| character.is_ascii_alphanumeric())
            .map(|character| character.to_ascii_lowercase())
            .collect::<String>()
    };
    let product = normalized(product_name);
    let company = normalized(company_name);
    let identity_matches = match app_id {
        "claude_desktop" => {
            matches!(product.as_str(), "claude" | "claudedesktop")
                && matches!(company.as_str(), "anthropic" | "anthropicpbc")
        }
        "codex_desktop" => {
            matches!(
                product.as_str(),
                "codex" | "codexdesktop" | "chatgpt" | "chatgptdesktop"
            ) && matches!(company.as_str(), "openai" | "openaillc" | "openaiopcollc")
        }
        "workbuddy" => {
            matches!(product.as_str(), "workbuddy" | "tencentworkbuddy")
                && company.contains("tencent")
        }
        _ => false,
    };
    gui_executable && windows_desktop_filename_matches(app_id, path) && identity_matches
}

/// PE subsystem distinguishes a desktop GUI from a same-named bundled CLI.
#[cfg(any(target_os = "windows", test))]
pub(crate) fn is_windows_gui_pe(header: &[u8]) -> bool {
    let Some(offset) = header
        .get(0x3c..0x40)
        .and_then(|bytes| <[u8; 4]>::try_from(bytes).ok())
        .map(u32::from_le_bytes)
        .map(|value| value as usize)
    else {
        return false;
    };
    let Some(optional) = offset.checked_add(24) else {
        return false;
    };
    header.starts_with(b"MZ")
        && header.get(offset..offset.saturating_add(4)) == Some(b"PE\0\0")
        && matches!(
            header.get(optional..optional.saturating_add(2)),
            Some([0x0b, 0x01] | [0x0b, 0x02])
        )
        && header.get(optional.saturating_add(68)..optional.saturating_add(70)) == Some(&[2, 0])
}

/// Parse the installed manifest as data. DTDs, external entities, ambiguous
/// paths and executable wrappers are never interpreted as launch instructions.
#[cfg(any(target_os = "windows", test))]
pub(crate) fn windows_manifest_applications(
    app_id: &str,
    source: &[u8],
) -> Option<Vec<(String, String)>> {
    use quick_xml::{events::Event, Reader, XmlVersion};
    if source.len() > 1024 * 1024 {
        return None;
    }
    let source = std::str::from_utf8(source).ok()?;
    let mut reader = Reader::from_str(source);
    let mut elements = Vec::<String>::new();
    let mut applications = Vec::new();
    loop {
        let event = reader.read_event().ok()?;
        match event {
            Event::DocType(_) => return None,
            Event::Start(ref element) | Event::Empty(ref element) => {
                let name = element.local_name();
                if name.as_ref() == "Application"
                    && elements.as_slice() == ["Package", "Applications"]
                {
                    let mut id = None;
                    let mut executable = None;
                    for attribute in element.attributes() {
                        let attribute = attribute.ok()?;
                        match attribute.key.as_ref() {
                            "Id" => {
                                id = Some(
                                    attribute
                                        .normalized_value(XmlVersion::Implicit1_0)
                                        .ok()?
                                        .into_owned(),
                                )
                            }
                            "Executable" => {
                                executable = Some(
                                    attribute
                                        .normalized_value(XmlVersion::Implicit1_0)
                                        .ok()?
                                        .into_owned(),
                                )
                            }
                            _ => (),
                        }
                    }
                    if let (Some(id), Some(executable)) = (id, executable) {
                        if valid_relative_application_id(&id)
                            && valid_windows_relative_executable(&executable)
                            && windows_desktop_filename_matches(app_id, Path::new(&executable))
                        {
                            applications.push((id, executable));
                            if applications.len() > 64 {
                                return None;
                            }
                        }
                    }
                }
                if matches!(event, Event::Start(_)) {
                    elements.push(name.as_ref().to_owned());
                    if elements.len() > 64 {
                        return None;
                    }
                }
            }
            Event::End(_) => {
                elements.pop()?;
            }
            Event::Eof => return elements.is_empty().then_some(applications),
            _ => (),
        }
    }
}

#[cfg(any(target_os = "windows", test))]
fn valid_windows_relative_executable(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 32768
        && !value.chars().any(|character| {
            character.is_control() || matches!(character, ':' | '"' | '<' | '>' | '|' | '*' | '?')
        })
        && value.split(['/', '\\']).all(|part| {
            !part.is_empty() && !matches!(part, "." | "..") && !part.ends_with(['.', ' '])
        })
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DesktopAppSpec {
    pub id: &'static str,
    pub display_name: &'static str,
    pub expected_bundle_id: &'static str,
}

pub const PRIMARY_DESKTOP_APP_SPECS: [DesktopAppSpec; 2] = [
    DesktopAppSpec {
        id: "claude_desktop",
        display_name: "Claude Desktop",
        expected_bundle_id: "com.anthropic.claudefordesktop",
    },
    DesktopAppSpec {
        id: "codex_desktop",
        display_name: "Codex",
        expected_bundle_id: "com.openai.codex",
    },
];

pub const DESKTOP_APP_SPECS: [DesktopAppSpec; 3] = [
    PRIMARY_DESKTOP_APP_SPECS[0],
    PRIMARY_DESKTOP_APP_SPECS[1],
    DesktopAppSpec {
        id: "workbuddy",
        display_name: "WorkBuddy",
        expected_bundle_id: "com.tencent.workbuddy.mac",
    },
];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
pub enum LocationHint {
    None,
    #[cfg_attr(not(any(target_os = "macos", test)), allow(dead_code))]
    Applications,
    #[cfg_attr(not(any(target_os = "macos", test)), allow(dead_code))]
    UserApplications,
    LocalAppData,
    ProgramFiles,
    Multiple,
    Unsupported,
}

impl LocationHint {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Applications => "applications",
            Self::UserApplications => "user_applications",
            Self::LocalAppData => "local_app_data",
            Self::ProgramFiles => "program_files",
            Self::Multiple => "multiple",
            Self::Unsupported => "unsupported",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DesktopAppObservation {
    NotFound,
    Found {
        version: String,
        location_hint: LocationHint,
        bundle_identifier: String,
    },
    Multiple {
        candidate_count: usize,
    },
    #[allow(dead_code)] // Constructed on platforms other than Windows/macOS.
    Unsupported,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DesktopAppResult {
    pub app_id: &'static str,
    pub display_name: &'static str,
    pub status: &'static str,
    pub version: String,
    pub candidate_count: usize,
    pub location_hint: LocationHint,
    pub bundle_identifier: String,
    pub configuration_status: &'static str,
    pub reason_code: &'static str,
}

pub fn classify(spec: DesktopAppSpec, observation: DesktopAppObservation) -> DesktopAppResult {
    match observation {
        DesktopAppObservation::NotFound => DesktopAppResult {
            app_id: spec.id,
            display_name: spec.display_name,
            status: "not_found",
            version: String::new(),
            candidate_count: 0,
            location_hint: LocationHint::None,
            bundle_identifier: String::new(),
            configuration_status: "not_applicable",
            reason_code: "desktop_app_not_found",
        },
        DesktopAppObservation::Found {
            version,
            location_hint,
            bundle_identifier,
        } => DesktopAppResult {
            app_id: spec.id,
            display_name: spec.display_name,
            status: "detected_unverified",
            version: sanitize_version(&version),
            candidate_count: 1,
            location_hint,
            bundle_identifier,
            configuration_status: "documented_unverified",
            reason_code: "desktop_app_detected_adapter_unverified",
        },
        DesktopAppObservation::Multiple { candidate_count } => DesktopAppResult {
            app_id: spec.id,
            display_name: spec.display_name,
            status: "multiple_installations",
            version: String::new(),
            candidate_count: candidate_count.min(8),
            location_hint: LocationHint::Multiple,
            bundle_identifier: String::new(),
            configuration_status: "documented_unverified",
            reason_code: "multiple_desktop_apps_found",
        },
        DesktopAppObservation::Unsupported => DesktopAppResult {
            app_id: spec.id,
            display_name: spec.display_name,
            status: "unsupported_platform",
            version: String::new(),
            candidate_count: 0,
            location_hint: LocationHint::Unsupported,
            bundle_identifier: String::new(),
            configuration_status: "not_applicable",
            reason_code: "desktop_platform_not_supported",
        },
    }
}

pub fn sanitize_version(raw: &str) -> String {
    raw.chars()
        .filter(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '.' | '-' | '_' | '+')
        })
        .take(128)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn installed_package_identity_rejects_wrong_family_and_command_payloads() {
        let full = "OpenAI.Codex_1.2.3.4_x64__2p2nqsd0c76g0";
        let family = "OpenAI.Codex_2p2nqsd0c76g0";
        assert!(WindowsPackageApplication::from_installed_package(
            "codex_desktop",
            full,
            family,
            "OpenAI.Codex_2p2nqsd0c76g0!CodexDesktop"
        )
        .is_some());
        for application in [
            "OpenAI.Codex_otherpublishe!CodexDesktop",
            "OpenAI.Codex_2p2nqsd0c76g0!App & calc",
            "OpenAI.Codex_2p2nqsd0c76g0!App\0evil",
            "OpenAI.Codex_2p2nqsd0c76g0!../App",
            "OpenAI.Codex_2p2nqsd0c76g0!App!other",
            "OpenAI.Codex_2p2nqsd0c76g0!",
        ] {
            assert!(WindowsPackageApplication::from_installed_package(
                "codex_desktop",
                full,
                family,
                application
            )
            .is_none());
        }
        for full in [
            "OpenAI.Codex",
            "OpenAI.Codex_1.2_x64__2p2nqsd0c76g0",
            "OpenAI.Codex_1.2.70000.4_x64__2p2nqsd0c76g0",
            "OpenAI.Codex;calc_1.2.3.4_x64__2p2nqsd0c76g0",
        ] {
            assert!(WindowsPackageApplication::from_installed_package(
                "codex_desktop",
                full,
                family,
                "OpenAI.Codex_2p2nqsd0c76g0!CodexDesktop"
            )
            .is_none());
        }
        assert!(!windows_package_name_matches(
            "codex_desktop",
            "OpenAI;Codex"
        ));
    }

    fn pe_fixture(subsystem: u16) -> Vec<u8> {
        let mut bytes = vec![0u8; 256];
        bytes[..2].copy_from_slice(b"MZ");
        bytes[0x3c..0x40].copy_from_slice(&64u32.to_le_bytes());
        bytes[64..68].copy_from_slice(b"PE\0\0");
        bytes[88..90].copy_from_slice(&0x20bu16.to_le_bytes());
        bytes[156..158].copy_from_slice(&subsystem.to_le_bytes());
        bytes
    }

    #[test]
    fn codex_gui_is_not_a_cli_runtime_and_unrelated_executables_are_rejected() {
        let gui = pe_fixture(2);
        let cli = pe_fixture(3);
        assert!(is_windows_gui_pe(&gui));
        assert!(!is_windows_gui_pe(&cli));
        assert!(!is_windows_gui_pe(b"#!/bin/sh\n"));
        let mut malformed = gui.clone();
        malformed[0x3c..0x40].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(!is_windows_gui_pe(&malformed));
        assert!(windows_desktop_file_identity_matches(
            "codex_desktop",
            Path::new("C:\\Apps\\Codex.exe"),
            "Codex",
            "OpenAI, L.L.C.",
            is_windows_gui_pe(&gui)
        ));
        assert!(!windows_desktop_file_identity_matches(
            "codex_desktop",
            Path::new("C:\\Apps\\codex.exe"),
            "Codex",
            "OpenAI",
            is_windows_gui_pe(&cli)
        ));
        for (path, product, company) in [
            ("C:\\Apps\\Update.exe", "Codex", "OpenAI"),
            ("C:\\Apps\\Codex.exe", "Calculator", "OpenAI"),
            ("C:\\Apps\\Codex.exe", "Codex", "Unrelated OpenAI Helper"),
        ] {
            assert!(!windows_desktop_file_identity_matches(
                "codex_desktop",
                Path::new(path),
                product,
                company,
                true
            ));
        }
    }

    #[test]
    fn package_manifest_keeps_real_application_mapping_and_rejects_unsafe_paths() {
        let manifest = br#"<Package xmlns="http://schemas.microsoft.com/appx/manifest/foundation/windows10"><Applications>
          <Application Id="Updater" Executable="Update.exe"/>
          <Application Id="ActualCodex" Executable="app\Codex.exe"/>
        </Applications></Package>"#;
        assert_eq!(
            windows_manifest_applications("codex_desktop", manifest),
            Some(vec![("ActualCodex".into(), "app\\Codex.exe".into())])
        );
        for path in [
            "..\\Codex.exe",
            "C:\\Codex.exe",
            "\\\\server\\Codex.exe",
            "app\\.\\Codex.exe",
            "app\\..\\Codex.exe",
            "app\\Codex.exe --run",
        ] {
            let manifest = format!("<Package><Applications><Application Id=\"App\" Executable=\"{path}\"/></Applications></Package>");
            assert_eq!(
                windows_manifest_applications("codex_desktop", manifest.as_bytes()),
                Some(vec![])
            );
        }
        assert!(windows_manifest_applications(
            "codex_desktop",
            b"<!DOCTYPE Package [<!ENTITY external SYSTEM 'file:///secret'>]><Package/>"
        )
        .is_none());
        assert!(
            windows_manifest_applications("codex_desktop", b"<Package><Applications>").is_none()
        );
    }

    #[test]
    fn catalog_is_desktop_only_and_stable() {
        assert_eq!(
            PRIMARY_DESKTOP_APP_SPECS.map(|spec| spec.id),
            ["claude_desktop", "codex_desktop"]
        );
        assert_eq!(DESKTOP_APP_SPECS[2].id, "workbuddy");
        assert_eq!(
            DESKTOP_APP_SPECS[2].expected_bundle_id,
            "com.tencent.workbuddy.mac"
        );
        assert_eq!(
            PRIMARY_DESKTOP_APP_SPECS.map(|spec| spec.expected_bundle_id),
            ["com.anthropic.claudefordesktop", "com.openai.codex"]
        );
    }

    #[test]
    fn detection_never_claims_configuration() {
        let result = classify(
            DESKTOP_APP_SPECS[0],
            DesktopAppObservation::Found {
                version: "1.2.3<script>".into(),
                location_hint: LocationHint::Applications,
                bundle_identifier: "com.anthropic.claudefordesktop".into(),
            },
        );
        assert_eq!(result.status, "detected_unverified");
        assert_eq!(result.configuration_status, "documented_unverified");
        assert_eq!(result.version, "1.2.3script");
    }

    #[test]
    fn conflicts_and_unsupported_fail_closed() {
        let conflict = classify(
            DESKTOP_APP_SPECS[1],
            DesktopAppObservation::Multiple {
                candidate_count: 99,
            },
        );
        assert_eq!(conflict.status, "multiple_installations");
        assert_eq!(conflict.candidate_count, 8);
        assert!(conflict.bundle_identifier.is_empty());

        let unsupported = classify(DESKTOP_APP_SPECS[1], DesktopAppObservation::Unsupported);
        assert_eq!(unsupported.status, "unsupported_platform");
        assert_eq!(unsupported.configuration_status, "not_applicable");
    }

    #[test]
    fn workbuddy_windows_identity_is_exact_and_gui_only() {
        assert!(windows_package_name_matches(
            "workbuddy",
            "Tencent.WorkBuddy"
        ));
        assert!(windows_desktop_file_identity_matches(
            "workbuddy",
            Path::new("C:\\Apps\\WorkBuddy.exe"),
            "WorkBuddy",
            "Tencent Technology",
            true
        ));
        assert!(!windows_desktop_file_identity_matches(
            "workbuddy",
            Path::new("C:\\Apps\\WorkBuddy.exe"),
            "WorkBuddy Helper",
            "Tencent Technology",
            true
        ));
    }
}

//! Pure projection rules for the desktop-application-first discovery surface.

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DesktopAppSpec {
    pub id: &'static str,
    pub display_name: &'static str,
    pub expected_bundle_id: &'static str,
}

pub const DESKTOP_APP_SPECS: [DesktopAppSpec; 2] = [
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
pub enum LocationHint {
    None,
    Applications,
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
    fn catalog_is_desktop_only_and_stable() {
        assert_eq!(
            DESKTOP_APP_SPECS.map(|spec| spec.id),
            ["claude_desktop", "codex_desktop"]
        );
        assert_eq!(
            DESKTOP_APP_SPECS.map(|spec| spec.expected_bundle_id),
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
}

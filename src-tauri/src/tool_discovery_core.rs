//! Pure, dependency-free projection rules for RU-001 tool discovery.
//!
//! Keeping the compatibility decision here deliberately boring is a security
//! boundary: discovery can observe an executable, but RU-001 cannot turn that
//! observation into configuration authority.

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ToolSpec {
    pub id: &'static str,
    pub display_name: &'static str,
    pub executable_name: &'static str,
}

pub const TOOL_SPECS: [ToolSpec; 7] = [
    ToolSpec {
        id: "claude",
        display_name: "Claude Code",
        executable_name: "claude",
    },
    ToolSpec {
        id: "codex",
        display_name: "Codex",
        executable_name: "codex",
    },
    ToolSpec {
        id: "opencode",
        display_name: "OpenCode",
        executable_name: "opencode",
    },
    ToolSpec {
        id: "pi",
        display_name: "Pi",
        executable_name: "pi",
    },
    ToolSpec {
        id: "dsh",
        display_name: "DSH",
        executable_name: "dsh",
    },
    ToolSpec {
        id: "hermes",
        display_name: "Hermes",
        executable_name: "hermes",
    },
    ToolSpec {
        id: "openclaw",
        display_name: "OpenClaw",
        executable_name: "openclaw",
    },
];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LocationHint {
    None,
    Path,
    CommonLocation,
    Multiple,
}

impl LocationHint {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Path => "path",
            Self::CommonLocation => "common_location",
            Self::Multiple => "multiple",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ProbeObservation {
    NotFound,
    Found {
        version: String,
        location_hint: LocationHint,
    },
    Failed {
        location_hint: LocationHint,
    },
    TimedOut {
        location_hint: LocationHint,
    },
    MultipleInstallations {
        candidate_count: usize,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DiscoveryStatus {
    NotFound,
    DetectedUnverified,
    ProbeFailed,
    ProbeTimedOut,
    MultipleInstallations,
}

impl DiscoveryStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::NotFound => "not_found",
            Self::DetectedUnverified => "detected_unverified",
            Self::ProbeFailed => "probe_failed",
            Self::ProbeTimedOut => "probe_timed_out",
            Self::MultipleInstallations => "multiple_installations",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Compatibility {
    NotApplicable,
    UnverifiedReadOnly,
}

impl Compatibility {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::NotApplicable => "not_applicable",
            Self::UnverifiedReadOnly => "unverified_read_only",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReasonCode {
    ToolNotFound,
    ExactVersionNotAllowlisted,
    VersionCommandFailed,
    VersionCommandTimedOut,
    MultipleExecutablesFound,
}

impl ReasonCode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ToolNotFound => "tool_not_found",
            Self::ExactVersionNotAllowlisted => "exact_version_not_allowlisted",
            Self::VersionCommandFailed => "version_command_failed",
            Self::VersionCommandTimedOut => "version_command_timed_out",
            Self::MultipleExecutablesFound => "multiple_executables_found",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DiscoveryResult {
    pub tool_id: &'static str,
    pub display_name: &'static str,
    pub status: DiscoveryStatus,
    pub version: String,
    pub candidate_count: usize,
    pub location_hint: LocationHint,
    pub compatibility: Compatibility,
    pub reason_code: ReasonCode,
}

pub fn classify(spec: ToolSpec, observation: ProbeObservation) -> DiscoveryResult {
    let (status, version, candidate_count, location_hint, compatibility, reason_code) =
        match observation {
            ProbeObservation::NotFound => (
                DiscoveryStatus::NotFound,
                String::new(),
                0,
                LocationHint::None,
                Compatibility::NotApplicable,
                ReasonCode::ToolNotFound,
            ),
            ProbeObservation::Found {
                version,
                location_hint,
            } => (
                DiscoveryStatus::DetectedUnverified,
                version,
                1,
                location_hint,
                Compatibility::UnverifiedReadOnly,
                ReasonCode::ExactVersionNotAllowlisted,
            ),
            ProbeObservation::Failed { location_hint } => (
                DiscoveryStatus::ProbeFailed,
                String::new(),
                1,
                location_hint,
                Compatibility::UnverifiedReadOnly,
                ReasonCode::VersionCommandFailed,
            ),
            ProbeObservation::TimedOut { location_hint } => (
                DiscoveryStatus::ProbeTimedOut,
                String::new(),
                1,
                location_hint,
                Compatibility::UnverifiedReadOnly,
                ReasonCode::VersionCommandTimedOut,
            ),
            ProbeObservation::MultipleInstallations { candidate_count } => (
                DiscoveryStatus::MultipleInstallations,
                String::new(),
                candidate_count.min(32),
                LocationHint::Multiple,
                Compatibility::UnverifiedReadOnly,
                ReasonCode::MultipleExecutablesFound,
            ),
        };

    DiscoveryResult {
        tool_id: spec.id,
        display_name: spec.display_name,
        status,
        version,
        candidate_count,
        location_hint,
        compatibility,
        reason_code,
    }
}

pub fn normalize_version_output(raw: &str) -> Option<String> {
    let first_non_empty = raw.lines().map(str::trim).find(|line| !line.is_empty())?;

    for token in first_non_empty.split_whitespace() {
        let candidate = token.trim_matches(|character: char| {
            !character.is_ascii_alphanumeric() && !matches!(character, '.' | '-' | '+' | '_')
        });
        let candidate = candidate
            .strip_prefix('v')
            .filter(|rest| rest.starts_with(|character: char| character.is_ascii_digit()))
            .unwrap_or(candidate);
        if looks_like_version(candidate) {
            return Some(candidate.chars().take(256).collect());
        }
    }

    let sanitized: String = first_non_empty
        .chars()
        .filter(|character| !character.is_control())
        .take(256)
        .collect();
    (!sanitized.is_empty()).then_some(sanitized)
}

fn looks_like_version(candidate: &str) -> bool {
    candidate
        .chars()
        .next()
        .is_some_and(|character| character.is_ascii_digit())
        && candidate.matches('.').count() >= 2
        && candidate.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '.' | '-' | '+' | '_')
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixed_catalog_and_empty_state() {
        assert_eq!(
            TOOL_SPECS.map(|spec| spec.id),
            ["claude", "codex", "opencode", "pi", "dsh", "hermes", "openclaw"]
        );
        let results = TOOL_SPECS.map(|spec| classify(spec, ProbeObservation::NotFound));
        assert!(results.iter().all(|result| {
            result.status == DiscoveryStatus::NotFound
                && result.compatibility == Compatibility::NotApplicable
                && result.candidate_count == 0
                && result.version.is_empty()
        }));
    }

    #[test]
    fn detected_version_is_exact_and_read_only() {
        let version = normalize_version_output("codex-cli 0.151.0\n").unwrap();
        let result = classify(
            TOOL_SPECS[1],
            ProbeObservation::Found {
                version,
                location_hint: LocationHint::Path,
            },
        );
        assert_eq!(result.version, "0.151.0");
        assert_eq!(result.status, DiscoveryStatus::DetectedUnverified);
        assert_eq!(result.compatibility, Compatibility::UnverifiedReadOnly);
        assert_eq!(result.reason_code, ReasonCode::ExactVersionNotAllowlisted);
    }

    #[test]
    fn multiple_installations_fail_closed() {
        let result = classify(
            TOOL_SPECS[0],
            ProbeObservation::MultipleInstallations { candidate_count: 2 },
        );
        assert_eq!(result.status, DiscoveryStatus::MultipleInstallations);
        assert_eq!(result.candidate_count, 2);
        assert_eq!(result.location_hint, LocationHint::Multiple);
        assert_eq!(result.compatibility, Compatibility::UnverifiedReadOnly);
        assert!(result.version.is_empty());
    }

    #[test]
    fn probe_failure_and_timeout_are_isolated() {
        let results = [
            classify(
                TOOL_SPECS[0],
                ProbeObservation::Failed {
                    location_hint: LocationHint::CommonLocation,
                },
            ),
            classify(
                TOOL_SPECS[1],
                ProbeObservation::TimedOut {
                    location_hint: LocationHint::Path,
                },
            ),
            classify(TOOL_SPECS[2], ProbeObservation::NotFound),
            classify(TOOL_SPECS[3], ProbeObservation::NotFound),
            classify(TOOL_SPECS[4], ProbeObservation::NotFound),
            classify(TOOL_SPECS[5], ProbeObservation::NotFound),
            classify(TOOL_SPECS[6], ProbeObservation::NotFound),
        ];
        assert_eq!(results.len(), 7);
        assert_eq!(results[0].status, DiscoveryStatus::ProbeFailed);
        assert_eq!(results[1].status, DiscoveryStatus::ProbeTimedOut);
        assert!(results[0].version.is_empty() && results[1].version.is_empty());
        assert!(results[2..]
            .iter()
            .all(|result| result.status == DiscoveryStatus::NotFound));
    }
}

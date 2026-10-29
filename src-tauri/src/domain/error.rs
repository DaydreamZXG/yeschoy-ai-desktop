//! Closed failures contain reason codes, never native error text or secrets.
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ConfigFailure {
    Read,
    #[allow(dead_code)] // Kept for parsers and the closed legacy failure mapping.
    Parse,
    ExternalChange,
    Write,
    Readback,
    Rollback,
}

impl From<ConfigFailure> for AdapterFailure {
    fn from(error: ConfigFailure) -> Self {
        match error {
            ConfigFailure::ExternalChange => Self::ExternalOverride,
            ConfigFailure::Read => Self::ConfigurationFailed("configuration_read_failed"),
            ConfigFailure::Parse => Self::ConfigurationFailed("configuration_parse_failed"),
            ConfigFailure::Write => Self::ConfigurationFailed("configuration_write_failed"),
            ConfigFailure::Readback => Self::ConfigurationFailed("configuration_readback_failed"),
            ConfigFailure::Rollback => Self::ConfigurationFailed("configuration_rollback_failed"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn conflicts_and_failed_rollback_have_distinct_safe_results() {
        assert_eq!(
            AdapterFailure::from(ConfigFailure::ExternalChange),
            AdapterFailure::ExternalOverride
        );
        assert_eq!(
            AdapterFailure::from(ConfigFailure::Rollback),
            AdapterFailure::ConfigurationFailed("configuration_rollback_failed")
        );
    }
}
